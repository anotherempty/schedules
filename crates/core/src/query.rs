use std::num::NonZeroUsize;

use chrono_tz::Tz;

use crate::{DateRange, Error, Placement};

/// Half-open UTC window. Budgets bound both scanned candidates and returned occurrences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryWindow {
    pub range: DateRange,
    pub all_day_zone: Tz,
    pub candidate_limit: NonZeroUsize,
    pub occurrence_limit: NonZeroUsize,
}

impl QueryWindow {
    pub(crate) fn overlaps(self, placement: Placement) -> Result<bool, Error> {
        let (start, end) = placement.bounds(self.all_day_zone)?;
        Ok(if let Some(end) = end {
            self.range.overlaps(DateRange::new(start, end)?)
        } else {
            self.range.contains(start)
        })
    }
}
