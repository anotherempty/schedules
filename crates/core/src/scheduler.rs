use std::{collections::BTreeMap, num::NonZeroUsize};

use chrono_tz::Tz;

use crate::query::{OccurrencePosition, SearchBudget};

use crate::{Error, Occurrence, OccurrenceKey, QueryWindow, Schedule};

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
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace an item's complete schedule, returning previous definition.
    pub fn insert(&mut self, item_id: ItemId, schedule: Schedule) -> Option<Schedule> {
        self.schedules.insert(item_id, schedule)
    }

    pub fn get(&self, item_id: &ItemId) -> Option<&Schedule> {
        self.schedules.get(item_id)
    }

    /// Mutable access supports validated per-occurrence overrides.
    pub fn get_mut(&mut self, item_id: &ItemId) -> Option<&mut Schedule> {
        self.schedules.get_mut(item_id)
    }

    pub fn remove(&mut self, item_id: &ItemId) -> Option<Schedule> {
        self.schedules.remove(item_id)
    }

    pub fn len(&self) -> usize {
        self.schedules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.schedules.is_empty()
    }

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&ItemId, &Schedule)> + ExactSizeIterator {
        self.schedules.iter()
    }

    /// Return the next occurrence after an item's occurrence across all schedules.
    ///
    /// Orders by UTC start, item ID, then original occurrence key, as in
    /// [`Self::occurrences`]. The successor can belong to the same item or share
    /// the anchor's start time. Overrides determine actual placement; cancelled
    /// anchors are invalid. All-day starts use `all_day_zone`.
    ///
    /// Searches without a date window and retains only one successor per schedule.
    /// `None` means all schedules were exhausted without a successor.
    /// The shared candidate budget covers anchor resolution, schedule inspections,
    /// overrides, and recurrence candidates (including skipped dates).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidOccurrenceKey`] for a missing item, invalid key,
    /// or cancelled anchor. Returns [`Error::CandidateLimit`] if the search cannot
    /// finish within budget; never returns a partial answer. Propagates temporal
    /// resolution and range errors. Elapsed recurrence seeks arithmetically; calendar
    /// recurrence seeks to the anchor date unless a count limit requires bounded
    /// historical expansion to count valid occurrences (excluding timezone gaps).
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

    /// Query all items, sorted by UTC start, item ID, and original occurrence key.
    /// Budgets apply across the whole query. Inspecting each schedule also costs one
    /// candidate unit, bounding queries over schedules without matching occurrences.
    /// Errors discard partial results. IDs are borrowed; neither IDs nor schedules are cloned.
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

/// Occurrence paired with its owning item. Identity is item ID plus occurrence key.
#[derive(Debug, PartialEq, Eq)]
pub struct ScheduledOccurrence<'a, ItemId> {
    pub item_id: &'a ItemId,
    pub occurrence: Occurrence,
}

impl<ItemId> Copy for ScheduledOccurrence<'_, ItemId> {}

impl<ItemId> Clone for ScheduledOccurrence<'_, ItemId> {
    fn clone(&self) -> Self {
        *self
    }
}
