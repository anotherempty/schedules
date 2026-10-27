//! UTC query windows and budgets for bounded occurrence expansion.

use std::{cmp::Ordering, num::NonZeroUsize};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::{DateRange, Error, Occurrence, OccurrenceKey, Placement};

/// A half-open UTC query window with explicit work and result limits.
///
/// Points match when contained in `range`; intervals match when overlapping it.
/// Limits produce errors rather than silently truncating results. See
/// [`crate::Schedule::iter`] and [`crate::ScheduleStore::occurrences`] for budget scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryWindow {
    /// UTC interval to query, including its start and excluding its end.
    pub range: DateRange,
    /// Timezone used to resolve all-day midnight boundaries.
    ///
    /// Timed placements retain their own timezone. Ambiguous all-day midnights
    /// use the earlier instant; nonexistent midnights produce an error.
    pub all_day_zone: Tz,
    /// Maximum work units for candidate expansion and override inspection.
    ///
    /// Skipped dates and candidates outside the window also consume budget.
    /// Schedule store queries additionally charge for inspecting each schedule.
    pub candidate_limit: NonZeroUsize,
    /// Maximum accepted occurrences returned before reporting [`Error::OccurrenceLimit`].
    ///
    /// Stored date exclusions and custom query predicates run before this limit.
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

pub(crate) struct SearchBudget {
    remaining: usize,
}

impl SearchBudget {
    pub(crate) fn new(limit: NonZeroUsize) -> Self {
        Self {
            remaining: limit.get(),
        }
    }

    pub(crate) fn charge(&mut self) -> Result<(), Error> {
        self.remaining = self.remaining.checked_sub(1).ok_or(Error::CandidateLimit)?;
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OccurrencePosition {
    start: DateTime<Utc>,
    key: OccurrenceKey,
}

impl OccurrencePosition {
    pub(crate) const fn start(self) -> DateTime<Utc> {
        self.start
    }

    pub(crate) fn new(occurrence: Occurrence, zone: Tz) -> Result<Self, Error> {
        Ok(Self {
            start: occurrence.placement().bounds(zone)?.0,
            key: occurrence.key(),
        })
    }

    pub(crate) fn follows(self, anchor: Self, item_order: Ordering) -> bool {
        self.start
            .cmp(&anchor.start)
            .then(item_order)
            .then_with(|| self.key.cmp(&anchor.key))
            .is_gt()
    }
}
