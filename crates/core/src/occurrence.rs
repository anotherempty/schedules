use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use crate::Placement;

/// Original position, independent of overrides. Keys are scoped to their owning schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OccurrenceKey {
    Calendar(NaiveDateTime),
    Elapsed(DateTime<Utc>),
    Date(NaiveDate),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceOverride {
    Cancel,
    Replace(Placement),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    key: OccurrenceKey,
    placement: Placement,
}

impl Occurrence {
    pub(crate) const fn new(key: OccurrenceKey, placement: Placement) -> Self {
        Self { key, placement }
    }
    pub const fn key(self) -> OccurrenceKey {
        self.key
    }
    pub const fn placement(self) -> Placement {
        self.placement
    }
}
