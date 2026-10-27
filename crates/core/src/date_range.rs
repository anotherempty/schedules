//! Nonempty UTC ranges for containment and overlap queries.

use chrono::{DateTime, Utc};

use crate::Error;

/// A nonempty UTC range with an inclusive start and exclusive end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl DateRange {
    /// Creates a half-open UTC range.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{DateRange, DateTime, Duration, Error};
    /// const WINDOW_LENGTH: Duration = Duration::days(1);
    /// let start = DateTime::UNIX_EPOCH;
    /// let end = start.checked_add_signed(WINDOW_LENGTH).ok_or(Error::OutOfRange)?;
    /// let range = DateRange::new(start, end)?;
    /// assert!(range.contains(start));
    /// assert!(!range.contains(end));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRange`] when `end` is not later than `start`.
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        Ok(Self { start, end })
    }

    /// Returns the inclusive start.
    pub const fn start(self) -> DateTime<Utc> {
        self.start
    }

    /// Returns the exclusive end.
    pub const fn end(self) -> DateTime<Utc> {
        self.end
    }

    /// Tests whether an instant is at or after the start and before the end.
    pub fn contains(self, instant: DateTime<Utc>) -> bool {
        self.start <= instant && instant < self.end
    }

    /// Tests whether ranges share time; touching endpoints do not overlap.
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}
