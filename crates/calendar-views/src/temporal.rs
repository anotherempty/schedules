//! Validated UTC spans and local-date viewing windows.

use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

use crate::{DAYS_PER_WEEK, Error};

/// A UTC point or nonempty interval with an exclusive end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSpan {
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

impl TimeSpan {
    /// Creates a point that belongs to the day containing its instant.
    pub const fn point(instant: DateTime<Utc>) -> Self {
        Self {
            start: instant,
            end: None,
        }
    }

    /// Creates an interval with inclusive start and exclusive end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRange`] when `end` is not later than `start`.
    pub fn interval(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        Ok(Self {
            start,
            end: Some(end),
        })
    }

    /// Returns the inclusive start or point instant.
    pub const fn start(self) -> DateTime<Utc> {
        self.start
    }

    /// Returns the exclusive interval end, or `None` for a point.
    pub const fn end(self) -> Option<DateTime<Utc>> {
        self.end
    }

    pub(crate) fn overlaps(self, window: CalendarWindow) -> bool {
        match self.end {
            Some(end) => self.start < window.utc_end && window.utc_start < end,
            None => window.utc_start <= self.start && self.start < window.utc_end,
        }
    }
}

/// A nonempty local-date range in one viewing timezone, with exclusive end date.
///
/// Boundary midnights select the earlier instant when ambiguous and reject gaps.
/// UTC boundaries may be separated by 23 or 25 hours on DST transition days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarWindow {
    start: NaiveDate,
    end: NaiveDate,
    zone: Tz,
    utc_start: DateTime<Utc>,
    utc_end: DateTime<Utc>,
}

impl CalendarWindow {
    /// Creates a local-date window and resolves its outer UTC boundaries.
    ///
    /// Interior midnights are resolved lazily by day/week iterators.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRange`] for reversed/equal dates or nonpositive UTC
    /// bounds, and [`Error::NonexistentMidnight`] for an unresolvable outer boundary.
    pub fn new(start: NaiveDate, end: NaiveDate, zone: Tz) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        let utc_start = Self::midnight(start, zone)?;
        let utc_end = Self::midnight(end, zone)?;
        if utc_end <= utc_start {
            return Err(Error::InvalidRange);
        }
        Ok(Self {
            start,
            end,
            zone,
            utc_start,
            utc_end,
        })
    }

    /// Returns the inclusive first local date.
    pub const fn start(self) -> NaiveDate {
        self.start
    }

    /// Returns the exclusive final local date.
    pub const fn end(self) -> NaiveDate {
        self.end
    }

    /// Returns the viewing timezone.
    pub const fn timezone(self) -> Tz {
        self.zone
    }

    /// Returns the inclusive UTC boundary.
    pub const fn utc_start(self) -> DateTime<Utc> {
        self.utc_start
    }

    /// Returns the exclusive UTC boundary.
    pub const fn utc_end(self) -> DateTime<Utc> {
        self.utc_end
    }

    /// Returns a UTC interval suitable for adapting an all-day item or querying data.
    pub const fn span(self) -> TimeSpan {
        TimeSpan {
            start: self.utc_start,
            end: Some(self.utc_end),
        }
    }

    /// Expands the window to whole weeks beginning on the requested weekday.
    ///
    /// Use before fetching data to include items on padded calendar dates.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OutOfRange`] if padding exceeds supported dates, or the
    /// boundary-resolution errors documented by [`Self::new`].
    pub fn covering_weeks(self, week_start: Weekday) -> Result<Self, Error> {
        let start = self.week_start(week_start)?;
        let offset = Self::weekday_offset(self.end.weekday(), week_start);
        let padding = (DAYS_PER_WEEK as u64 - offset) % DAYS_PER_WEEK as u64;
        let end = self
            .end
            .checked_add_days(Days::new(padding))
            .ok_or(Error::OutOfRange)?;
        Self::new(start, end, self.zone)
    }

    pub(crate) fn week_start(self, week_start: Weekday) -> Result<NaiveDate, Error> {
        self.start
            .checked_sub_days(Days::new(Self::weekday_offset(
                self.start.weekday(),
                week_start,
            )))
            .ok_or(Error::OutOfRange)
    }

    pub(crate) fn weekday_offset(day: Weekday, week_start: Weekday) -> u64 {
        (u64::from(day.num_days_from_monday()) + DAYS_PER_WEEK as u64
            - u64::from(week_start.num_days_from_monday()))
            % DAYS_PER_WEEK as u64
    }

    fn midnight(date: NaiveDate, zone: Tz) -> Result<DateTime<Utc>, Error> {
        zone.from_local_datetime(&date.and_time(NaiveTime::MIN))
            .earliest()
            .map(|instant| instant.with_timezone(&Utc))
            .ok_or(Error::NonexistentMidnight { date, zone })
    }
}
