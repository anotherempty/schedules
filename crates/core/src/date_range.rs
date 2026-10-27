use chrono::{DateTime, Utc};

use crate::Error;

/// UTC date-time range with inclusive start and exclusive end.
/// Construction rejects empty or reversed ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DateRange {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl DateRange {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, Error> {
        if end <= start {
            return Err(Error::InvalidRange);
        }
        Ok(Self { start, end })
    }

    pub const fn start(self) -> DateTime<Utc> {
        self.start
    }

    pub const fn end(self) -> DateTime<Utc> {
        self.end
    }

    pub fn contains(self, instant: DateTime<Utc>) -> bool {
        self.start <= instant && instant < self.end
    }

    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}
