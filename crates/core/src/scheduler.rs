//! Application-owned schedule collections and queries across their occurrences.

use std::{collections::BTreeMap, num::NonZeroUsize};

use chrono_tz::Tz;

use crate::query::{OccurrencePosition, SearchBudget};

use crate::{Error, Occurrence, OccurrenceKey, QueryWindow, Schedule, ScheduleAnalysis};

/// In-memory schedules keyed by application-owned item identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduler<ItemId> {
    schedules: BTreeMap<ItemId, Schedule>,
}

impl<ItemId> Default for Scheduler<ItemId> {
    fn default() -> Self {
        Self {
            schedules: BTreeMap::new(),
        }
    }
}

impl<ItemId: Ord> Scheduler<ItemId> {
    /// Creates an empty scheduler keyed by ordered application identifiers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an item's complete schedule, returning previous definition.
    pub fn insert(&mut self, item_id: ItemId, schedule: Schedule) -> Option<Schedule> {
        self.schedules.insert(item_id, schedule)
    }

    /// Borrows an item schedule, or returns `None` when the item is absent.
    pub fn get(&self, item_id: &ItemId) -> Option<&Schedule> {
        self.schedules.get(item_id)
    }

    /// Mutably borrows an item schedule, or returns `None` when the item is absent.
    pub fn get_mut(&mut self, item_id: &ItemId) -> Option<&mut Schedule> {
        self.schedules.get_mut(item_id)
    }

    /// Removes and returns an item schedule, or `None` when the item is absent.
    pub fn remove(&mut self, item_id: &ItemId) -> Option<Schedule> {
        self.schedules.remove(item_id)
    }

    /// Returns the number of stored schedules, including those with no query matches.
    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    /// Returns whether no schedules are stored.
    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }

    /// Yields borrowed identifiers and schedules in ascending identifier order.
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&ItemId, &Schedule)> + ExactSizeIterator {
        self.schedules.iter()
    }

    /// Analyzes occupied time, free gaps, and conflicts within a query window.
    ///
    /// Expands once with the same shared budgets as [`Self::occurrences`].
    /// Intervals are clipped to the window; points consume no time and do not
    /// conflict. All-day placements use the window's viewing timezone.
    /// Analysis borrows item IDs without requiring `Clone` or owning payloads.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use schedules_core::{DateRange, DateTime, Duration, Error, QueryWindow, Scheduler, Tz};
    /// const WINDOW_LENGTH: Duration = Duration::hours(8);
    /// let start = DateTime::UNIX_EPOCH;
    /// let end = start.checked_add_signed(WINDOW_LENGTH).ok_or(Error::OutOfRange)?;
    /// let window = QueryWindow {
    ///     range: DateRange::new(start, end)?,
    ///     all_day_zone: Tz::UTC,
    ///     candidate_limit: NonZeroUsize::MIN,
    ///     occurrence_limit: NonZeroUsize::MIN,
    /// };
    /// let scheduler: Scheduler<u64> = Scheduler::new();
    /// let analysis = scheduler.analyze(window)?;
    /// assert_eq!(analysis.longest_gap(), Some(window.range));
    /// assert!(analysis.is_slot_available(window.range));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Propagates expansion, budget, and timezone errors from [`Self::occurrences`].
    /// No partial analysis is returned.
    pub fn analyze(&self, window: QueryWindow) -> Result<ScheduleAnalysis<'_, ItemId>, Error> {
        ScheduleAnalysis::new(window, self.occurrences(window)?)
    }

    /// Returns the next occurrence after an item occurrence across all schedules.
    ///
    /// Orders by UTC start, item ID, then original occurrence key, as in
    /// [`Self::occurrences`]. The successor may belong to the same item or share
    /// the anchor start time. Overrides determine effective placement; cancelled
    /// anchors are invalid. All-day starts use `all_day_zone`.
    ///
    /// No date window is required. `None` means every schedule was exhausted without
    /// a successor. The shared candidate budget covers anchor resolution, schedule
    /// inspections, overrides, and recurrence candidates, including skipped dates.
    /// Count-limited calendar rules may require scanning historical candidates to
    /// determine membership, so distant anchors can require larger budgets.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use schedules_core::{DateTime, Duration, Error, OccurrenceKey, Placement, Recurrence,
    ///     Schedule, Scheduler, Termination, Tz};
    /// #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    /// enum ItemId { Poll }
    /// const POLL_INTERVAL: Duration = Duration::hours(1);
    /// const SEARCH_WORK_UNITS: usize = 16;
    /// let start = DateTime::UNIX_EPOCH;
    /// let schedule = Schedule::repeating(
    ///     Placement::instant(start, Tz::UTC)?,
    ///     Recurrence::elapsed(POLL_INTERVAL, Termination::Forever)?,
    /// )?;
    /// let mut scheduler = Scheduler::new();
    /// scheduler.insert(ItemId::Poll, schedule);
    /// let next = scheduler.next_after(
    ///     &ItemId::Poll, OccurrenceKey::Elapsed(start), Tz::UTC,
    ///     NonZeroUsize::new(SEARCH_WORK_UNITS).ok_or(Error::OutOfRange)?,
    /// )?;
    /// assert_eq!(next.map(|entry| entry.occurrence.key()),
    ///     Some(OccurrenceKey::Elapsed(start.checked_add_signed(POLL_INTERVAL).ok_or(Error::OutOfRange)?)));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidOccurrenceKey`] for a missing item, invalid key,
    /// or cancelled anchor. Returns [`Error::CandidateLimit`] if the search cannot
    /// finish within budget; no partial answer is returned. Propagates
    /// [`Error::AmbiguousTime`] for rejected ambiguous starts,
    /// [`Error::NonexistentTime`] for unresolvable all-day boundaries, and
    /// [`Error::OutOfRange`] for dates, durations, or counts outside supported ranges.
    pub fn next_after(
        &self,
        item_id: &ItemId,
        key: OccurrenceKey,
        all_day_zone: Tz,
        candidate_limit: NonZeroUsize,
    ) -> Result<Option<ScheduledOccurrence<'_, ItemId>>, Error> {
        let mut budget = SearchBudget::new(candidate_limit);
        let schedule = self.get(item_id).ok_or(Error::InvalidOccurrenceKey)?;
        let anchor =
            OccurrencePosition::new(schedule.occurrence_at(key, &mut budget)?, all_day_zone)?;
        let mut best: Option<(OccurrencePosition, ScheduledOccurrence<'_, ItemId>)> = None;
        for (candidate_id, schedule) in &self.schedules {
            if let Some(occurrence) =
                schedule.successor(anchor, candidate_id.cmp(item_id), all_day_zone, &mut budget)?
            {
                let position = OccurrencePosition::new(occurrence, all_day_zone)?;
                if best.as_ref().is_none_or(|(current, entry)| {
                    current.follows(position, entry.item_id.cmp(candidate_id))
                }) {
                    best = Some((
                        position,
                        ScheduledOccurrence {
                            item_id: candidate_id,
                            occurrence,
                        },
                    ));
                }
            }
        }
        Ok(best.map(|(_, entry)| entry))
    }

    /// Returns overlapping occurrences sorted by UTC start, item ID, then original key.
    ///
    /// Budgets apply across the whole query. Inspecting each schedule costs one
    /// candidate unit in addition to the work described by [`Schedule::iter`].
    /// Returned item IDs borrow from this scheduler. Errors discard partial results.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use schedules_core::{DateRange, DateTime, Duration, Error, Placement, QueryWindow, Schedule, Tz};
    /// const WINDOW_LENGTH: Duration = Duration::days(1);
    /// let start = DateTime::UNIX_EPOCH;
    /// let end = start.checked_add_signed(WINDOW_LENGTH).ok_or(Error::OutOfRange)?;
    /// let schedule = Schedule::once(Placement::instant(start, Tz::UTC)?);
    /// let window = QueryWindow {
    ///     range: DateRange::new(start, end)?,
    ///     all_day_zone: Tz::UTC,
    ///     candidate_limit: NonZeroUsize::MIN,
    ///     occurrence_limit: NonZeroUsize::MIN,
    /// };
    /// use schedules_core::Scheduler;
    /// #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    /// enum ItemId { Meeting }
    /// const QUERY_WORK_UNITS: usize = 2; // One schedule inspection and one candidate.
    /// let mut scheduler = Scheduler::new();
    /// scheduler.insert(ItemId::Meeting, schedule);
    /// let window = QueryWindow {
    ///     candidate_limit: NonZeroUsize::new(QUERY_WORK_UNITS).ok_or(Error::OutOfRange)?,
    ///     ..window
    /// };
    /// let results = scheduler.occurrences(window)?;
    /// assert_eq!(results.first().map(|entry| entry.item_id), Some(&ItemId::Meeting));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::CandidateLimit`] or [`Error::OccurrenceLimit`] if the shared
    /// query budgets are exceeded. Propagates the temporal resolution and expansion
    /// errors described by [`Schedule::occurrences`].
    pub fn occurrences(
        &self,
        window: QueryWindow,
    ) -> Result<Vec<ScheduledOccurrence<'_, ItemId>>, Error> {
        let mut work = 0;
        let mut results = Vec::new();
        for (item_id, schedule) in &self.schedules {
            if work >= window.candidate_limit.get() {
                return Err(Error::CandidateLimit);
            }
            work += 1;
            let remaining = window.candidate_limit.get() - work;
            let mut occurrences = schedule.iter_with_budget(window, remaining)?;
            for occurrence in occurrences.by_ref() {
                let occurrence = occurrence?;
                if results.len() >= window.occurrence_limit.get() {
                    return Err(Error::OccurrenceLimit);
                }
                let start = occurrence.placement().bounds(window.all_day_zone)?.0;
                results.push((
                    start,
                    ScheduledOccurrence {
                        item_id,
                        occurrence,
                    },
                ));
            }
            work += occurrences.work_used();
        }
        // Item ID and occurrence key uniquely break ties; stability is unnecessary.
        results.sort_unstable_by(|(left_start, left), (right_start, right)| {
            left_start
                .cmp(right_start)
                .then_with(|| left.item_id.cmp(right.item_id))
                .then_with(|| left.occurrence.key().cmp(&right.occurrence.key()))
        });
        let mut results: Vec<_> = results
            .into_iter()
            .map(|(_, occurrence)| occurrence)
            .collect();
        // Removing cached sort keys can leave substantial unused capacity.
        results.shrink_to_fit();
        Ok(results)
    }
}

/// An occurrence paired with its owning item identifier.
///
/// Identity is the item identifier together with the original occurrence key.
#[derive(Debug, PartialEq, Eq)]
pub struct ScheduledOccurrence<'a, ItemId> {
    /// Borrowed identifier of the schedule that owns this occurrence.
    pub item_id: &'a ItemId,
    /// Occurrence with its original key and effective placement.
    pub occurrence: Occurrence,
}

impl<ItemId> Copy for ScheduledOccurrence<'_, ItemId> {}

impl<ItemId> Clone for ScheduledOccurrence<'_, ItemId> {
    fn clone(&self) -> Self {
        *self
    }
}
