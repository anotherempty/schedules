use chrono::{
    DateTime, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Offset, TimeZone, Utc,
};
use chrono_tz::Tz;

use crate::Error;

/// Choice when a local clock time occurs twice during an offset transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AmbiguityPolicy {
    #[default]
    Earlier,
    Later,
    Reject,
}

/// Nonexistent local times are rejected for explicit placements and skipped in recurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResolutionPolicy {
    ambiguity: AmbiguityPolicy,
}

impl ResolutionPolicy {
    pub const fn new(ambiguity: AmbiguityPolicy) -> Self {
        Self { ambiguity }
    }

    pub const fn ambiguity(self) -> AmbiguityPolicy {
        self.ambiguity
    }

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

/// Placement representation is private so constructors enforce its invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    kind: PlacementKind,
}

impl Placement {
    /// Construct a point from an absolute instant while retaining a display timezone.
    pub fn instant(instant: DateTime<Utc>, zone: Tz) -> Result<Self, Error> {
        let local = Self::local_at(instant, zone)?;
        Self::point(local, zone, ResolutionPolicy::default())?.at_instant(instant)
    }

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

    /// Timed intervals have an exact elapsed duration, including across DST transitions.
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

    /// Date-only interval; end date is exclusive. No timezone is stored.
    pub fn all_day(start: NaiveDate, end: NaiveDate) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        Ok(Self {
            kind: PlacementKind::AllDay { start, end },
        })
    }

    pub fn local_start(self) -> NaiveDateTime {
        match self.kind {
            PlacementKind::Timed { local, .. } => local,
            PlacementKind::AllDay { start, .. } => start.and_time(NaiveTime::MIN),
        }
    }

    pub fn timezone(self) -> Option<Tz> {
        match self.kind {
            PlacementKind::Timed { zone, .. } => Some(zone),
            PlacementKind::AllDay { .. } => None,
        }
    }

    pub fn duration(self) -> Option<Duration> {
        match self.kind {
            PlacementKind::Timed { duration, .. } => duration,
            PlacementKind::AllDay { .. } => None,
        }
    }

    pub fn dates(self) -> Option<(NaiveDate, NaiveDate)> {
        match self.kind {
            PlacementKind::AllDay { start, end } => Some((start, end)),
            PlacementKind::Timed { .. } => None,
        }
    }

    /// Resolve all-day dates using caller's viewing timezone.
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
