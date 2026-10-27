//! Timed and all-day placements with explicit local-time ambiguity handling.

use chrono::{
    DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc,
};
use chrono_tz::Tz;

use crate::Error;

/// Choice when a local clock time occurs twice during an offset transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbiguityPolicy {
    /// Chooses the earlier UTC instant; this is the default.
    #[default]
    Earlier,
    /// Chooses the later UTC instant.
    Later,
    /// Rejects ambiguous local times with [`Error::AmbiguousTime`].
    Reject,
}

/// Policy for resolving local times into UTC instants.
///
/// Defaults to [`AmbiguityPolicy::Earlier`]. Nonexistent local times are always
/// rejected by [`Self::resolve`]; calendar recurrence expansion skips them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResolutionPolicy {
    ambiguity: AmbiguityPolicy,
}

impl ResolutionPolicy {
    /// Creates a policy with the chosen handling of ambiguous local times.
    pub const fn new(ambiguity: AmbiguityPolicy) -> Self {
        Self { ambiguity }
    }

    /// Returns the policy for local times that occur twice.
    pub const fn ambiguity(self) -> AmbiguityPolicy {
        self.ambiguity
    }

    /// Resolves a local date and time in a timezone to a UTC instant.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{AmbiguityPolicy, DateTime, Error, ResolutionPolicy, Tz};
    /// let local = DateTime::UNIX_EPOCH.naive_utc();
    /// let policy = ResolutionPolicy::new(AmbiguityPolicy::Reject);
    /// assert_eq!(policy.resolve(local, Tz::UTC)?, DateTime::UNIX_EPOCH);
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::AmbiguousTime`] when the time occurs twice and the policy
    /// is [`AmbiguityPolicy::Reject`]. Returns [`Error::NonexistentTime`] when
    /// the local time has no corresponding instant.
    pub fn resolve(self, local: NaiveDateTime, zone: Tz) -> Result<DateTime<Utc>, Error> {
        match zone.from_local_datetime(&local) {
            LocalResult::Single(time) => Ok(time.with_timezone(&Utc)),
            LocalResult::Ambiguous(earlier, later) => match self.ambiguity {
                AmbiguityPolicy::Earlier => Ok(earlier.with_timezone(&Utc)),
                AmbiguityPolicy::Later => Ok(later.with_timezone(&Utc)),
                AmbiguityPolicy::Reject => Err(Error::AmbiguousTime { local, zone }),
            },
            LocalResult::None => Err(Error::NonexistentTime { local, zone }),
        }
    }
}

/// A timed point, positive elapsed interval, or nonempty all-day date interval.
///
/// Timed placements retain a timezone and ambiguity policy. All-day placements
/// store dates without a timezone; [`Self::bounds`] resolves them in the viewing
/// timezone. Interval ends are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    kind: PlacementKind,
}

impl Placement {
    /// Creates a point at an absolute instant, retaining the requested display timezone.
    ///
    /// The selected instant is preserved even during an ambiguous local time.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateTime, Error, Placement, Tz};
    /// let start = DateTime::UNIX_EPOCH;
    /// let point = Placement::instant(start, Tz::America__New_York)?;
    /// assert_eq!(point.bounds(Tz::UTC)?, (start, None));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::OutOfRange`] if the timezone offset moves the local date
    /// outside the supported range. Propagates [`Error::NonexistentTime`] if
    /// the derived local time cannot be resolved.
    pub fn instant(instant: DateTime<Utc>, zone: Tz) -> Result<Self, Error> {
        let local = Self::local_at(instant, zone)?;
        Self::point(local, zone, ResolutionPolicy::default())?.at_instant(instant)
    }

    /// Creates a point at a local time resolved using the supplied policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateTime, Error, Placement, Tz};
    /// use schedules_core::ResolutionPolicy;
    /// let start = DateTime::UNIX_EPOCH;
    /// let point = Placement::point(start.naive_utc(), Tz::UTC, ResolutionPolicy::default())?;
    /// assert_eq!(point.bounds(Tz::UTC)?, (start, None));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::AmbiguousTime`] for a rejected ambiguous time or
    /// [`Error::NonexistentTime`] for a local time that does not exist.
    pub fn point(local: NaiveDateTime, zone: Tz, policy: ResolutionPolicy) -> Result<Self, Error> {
        policy.resolve(local, zone)?;
        Ok(Self {
            kind: PlacementKind::Timed {
                local,
                zone,
                duration: None,
                policy,
            },
        })
    }

    /// Creates an interval with positive elapsed duration, including across timezone transitions.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateTime, Duration, Error, Placement, Tz};
    /// use schedules_core::ResolutionPolicy;
    /// let start = DateTime::UNIX_EPOCH;
    /// const MEETING_LENGTH: Duration = Duration::minutes(30);
    /// let meeting = Placement::timed(
    ///     start.naive_utc(), Tz::UTC, MEETING_LENGTH, ResolutionPolicy::default(),
    /// )?;
    /// let (begin, end) = meeting.bounds(Tz::UTC)?;
    /// assert_eq!(end, begin.checked_add_signed(MEETING_LENGTH));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDuration`] for a nonpositive duration,
    /// [`Error::AmbiguousTime`] for a rejected ambiguous start,
    /// [`Error::NonexistentTime`] for a nonexistent start, or
    /// [`Error::OutOfRange`] if the end exceeds the supported range.
    pub fn timed(
        local: NaiveDateTime,
        zone: Tz,
        duration: Duration,
        policy: ResolutionPolicy,
    ) -> Result<Self, Error> {
        if duration <= Duration::zero() {
            return Err(Error::InvalidDuration);
        }
        let value = Self {
            kind: PlacementKind::Timed {
                local,
                zone,
                duration: Some(duration),
                policy,
            },
        };
        value.bounds(zone)?;
        Ok(value)
    }

    /// Creates a date-only interval with an inclusive start and exclusive end.
    ///
    /// No timezone is stored or resolved until [`Self::bounds`] is called.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateTime, Error, Placement};
    /// let start = DateTime::UNIX_EPOCH;
    /// let date = start.date_naive();
    /// let next_date = date.succ_opt().ok_or(Error::OutOfRange)?;
    /// let day = Placement::all_day(date, next_date)?;
    /// assert_eq!(day.dates(), Some((date, next_date)));
    /// assert_eq!(day.timezone(), None);
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRange`] when `end` is not later than `start`.
    pub fn all_day(start: NaiveDate, end: NaiveDate) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        Ok(Self {
            kind: PlacementKind::AllDay { start, end },
        })
    }

    /// Returns the local start, or midnight on the start date for all-day placements.
    pub fn local_start(self) -> NaiveDateTime {
        match self.kind {
            PlacementKind::Timed { local, .. } => local,
            PlacementKind::AllDay { start, .. } => start.and_time(NaiveTime::MIN),
        }
    }

    /// Returns the timed placement's timezone, or `None` for an all-day placement.
    pub fn timezone(self) -> Option<Tz> {
        match self.kind {
            PlacementKind::Timed { zone, .. } => Some(zone),
            PlacementKind::AllDay { .. } => None,
        }
    }

    /// Returns elapsed interval duration, or `None` for points and all-day placements.
    pub fn duration(self) -> Option<Duration> {
        match self.kind {
            PlacementKind::Timed { duration, .. } => duration,
            PlacementKind::AllDay { .. } => None,
        }
    }

    /// Returns inclusive start and exclusive end dates for all-day placements, otherwise `None`.
    pub fn dates(self) -> Option<(NaiveDate, NaiveDate)> {
        match self.kind {
            PlacementKind::AllDay { start, end } => Some((start, end)),
            PlacementKind::Timed { .. } => None,
        }
    }

    /// Returns the UTC start and optional exclusive end.
    ///
    /// Points have no end. Timed placements use their stored timezone and policy;
    /// `all_day_zone` applies only to all-day placements. All-day boundaries resolve
    /// at local midnight, selecting the earlier instant when midnight is ambiguous.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateTime, Duration, Error, Placement, Tz};
    /// let start = DateTime::UNIX_EPOCH;
    /// let date = start.date_naive();
    /// let day = Placement::all_day(date, date.succ_opt().ok_or(Error::OutOfRange)?)?;
    /// let (begin, end) = day.bounds(Tz::UTC)?;
    /// const UTC_DAY: Duration = Duration::days(1);
    /// assert_eq!(begin, start);
    /// assert_eq!(end, start.checked_add_signed(UTC_DAY));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::NonexistentTime`] for an unresolvable local boundary,
    /// [`Error::AmbiguousTime`] for a timed boundary rejected by its policy, or
    /// [`Error::OutOfRange`] if adding the duration exceeds the supported range.
    pub fn bounds(self, all_day_zone: Tz) -> Result<(DateTime<Utc>, Option<DateTime<Utc>>), Error> {
        match self.kind {
            PlacementKind::Timed {
                local,
                zone,
                duration,
                policy,
            } => {
                let start = policy.resolve(local, zone)?;
                let end = duration
                    .map(|duration| start.checked_add_signed(duration).ok_or(Error::OutOfRange))
                    .transpose()?;
                Ok((start, end))
            }
            PlacementKind::AllDay { start, end } => {
                let policy = ResolutionPolicy::default();
                Ok((
                    policy.resolve(start.and_time(NaiveTime::MIN), all_day_zone)?,
                    Some(policy.resolve(end.and_time(NaiveTime::MIN), all_day_zone)?),
                ))
            }
        }
    }

    pub(crate) fn at_local(self, local: NaiveDateTime) -> Result<Self, Error> {
        match self.kind {
            PlacementKind::Timed {
                zone,
                duration,
                policy,
                ..
            } => match duration {
                Some(duration) => Self::timed(local, zone, duration, policy),
                None => Self::point(local, zone, policy),
            },
            PlacementKind::AllDay { start, end } => {
                let end = local
                    .date()
                    .checked_add_signed(end.signed_duration_since(start))
                    .ok_or(Error::OutOfRange)?;
                Self::all_day(local.date(), end)
            }
        }
    }

    pub(crate) fn at_instant(self, instant: DateTime<Utc>) -> Result<Self, Error> {
        match self.kind {
            PlacementKind::Timed { zone, duration, .. } => {
                let local = Self::local_at(instant, zone)?;
                // Retain the selected side of an ambiguous time for elapsed recurrence.
                let earlier = ResolutionPolicy::default();
                let policy = if earlier.resolve(local, zone)? == instant {
                    earlier
                } else {
                    ResolutionPolicy::new(AmbiguityPolicy::Later)
                };
                match duration {
                    Some(duration) => Self::timed(local, zone, duration, policy),
                    None => Self::point(local, zone, policy),
                }
            }
            PlacementKind::AllDay { .. } => Err(Error::ElapsedAllDay),
        }
    }

    pub(crate) fn local_at(instant: DateTime<Utc>, zone: Tz) -> Result<NaiveDateTime, Error> {
        instant
            .naive_utc()
            .checked_add_offset(zone.offset_from_utc_datetime(&instant.naive_utc()).fix())
            .ok_or(Error::OutOfRange)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementKind {
    Timed {
        local: NaiveDateTime,
        zone: Tz,
        duration: Option<Duration>,
        policy: ResolutionPolicy,
    },
    AllDay {
        start: NaiveDate,
        end: NaiveDate,
    },
}
