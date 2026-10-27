//! Availability and conflict analysis of bounded scheduler query results.

use crate::{DateRange, DateTime, Duration, Error, QueryWindow, ScheduledOccurrence, Utc};

/// A window's occupied intervals and maximal free gaps.
///
/// Occurrences retain original identity and effective placement; occupied ranges
/// are clipped to the query window. Points consume no time and never conflict.
/// Overlapping or touching intervals merge for availability and occupancy.
/// Construction uses O(n) storage after occurrence expansion and sorting.
#[derive(Debug)]
pub struct ScheduleAnalysis<'a, ItemId> {
    range: DateRange,
    occupied: Vec<OccupiedOccurrence<'a, ItemId>>,
    gaps: Vec<DateRange>,
    density: ScheduleDensity,
}

impl<'a, ItemId> ScheduleAnalysis<'a, ItemId> {
    pub(crate) fn new(
        window: QueryWindow,
        occurrences: Vec<ScheduledOccurrence<'a, ItemId>>,
    ) -> Result<Self, Error> {
        let mut occupied = Vec::new();
        let mut gaps = Vec::new();
        let mut cursor = window.range.start();
        let mut free = Duration::zero();
        for occurrence in occurrences {
            let (start, end) = occurrence
                .occurrence
                .placement()
                .bounds(window.all_day_zone)?;
            let Some(end) = end else { continue };
            let Some(range) = DateRange::new(start, end)?.intersection(window.range) else {
                continue;
            };
            if cursor < range.start() {
                let gap = DateRange::new(cursor, range.start())?;
                free += gap.duration();
                gaps.push(gap);
            }
            cursor = cursor.max(range.end());
            occupied.push(OccupiedOccurrence { occurrence, range });
        }
        if cursor < window.range.end() {
            let gap = DateRange::new(cursor, window.range.end())?;
            free += gap.duration();
            gaps.push(gap);
        }
        Ok(Self {
            range: window.range,
            occupied,
            gaps,
            density: ScheduleDensity {
                occupied: window.range.duration() - free,
                free,
            },
        })
    }

    /// Returns the analyzed UTC window.
    pub const fn range(&self) -> DateRange {
        self.range
    }

    /// Returns maximal free ranges in ascending start order, including window edges.
    pub fn gaps(&self) -> &[DateRange] {
        &self.gaps
    }

    /// Returns the longest free gap, preferring the earliest start on ties.
    pub fn longest_gap(&self) -> Option<DateRange> {
        self.gaps.iter().copied().max_by(|left, right| {
            left.duration()
                .cmp(&right.duration())
                .then_with(|| right.start().cmp(&left.start()))
        })
    }

    /// Returns maximal free gaps long enough to fit the requested elapsed duration.
    ///
    /// Results retain entire gaps, rather than subdividing them into appointments.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDuration`] for a nonpositive minimum duration.
    pub fn available_slots(
        &self,
        minimum: Duration,
    ) -> Result<impl Iterator<Item = DateRange> + '_, Error> {
        if minimum <= Duration::zero() {
            return Err(Error::InvalidDuration);
        }
        Ok(self
            .gaps
            .iter()
            .copied()
            .filter(move |gap| gap.duration() >= minimum))
    }

    /// Tests whether an entire slot lies inside one free gap of the analyzed window.
    ///
    /// Returns false for slots extending outside the window, even if no events exist.
    pub fn is_slot_available(&self, slot: DateRange) -> bool {
        self.gaps
            .iter()
            .any(|gap| gap.start() <= slot.start() && slot.end() <= gap.end())
    }

    /// Returns elapsed occupied and free time without double-counting overlaps.
    pub const fn density(&self) -> ScheduleDensity {
        self.density
    }

    /// Lazily yields every conflicting pair, including occurrences of the same item.
    ///
    /// Each pair appears once in occurrence order (UTC start, item ID, key), with
    /// its intersection clipped to the query window. Touching endpoints do not
    /// conflict. Expansion budgets do not limit pair enumeration: consuming all
    /// pairs takes O(n²) worst-case time, with no quadratic result allocation.
    /// Callers may use `take` to bound the number of pairs consumed.
    pub fn overlaps(&self) -> impl Iterator<Item = Overlap<'a, ItemId>> + '_ {
        self.occupied
            .iter()
            .enumerate()
            .flat_map(move |(index, first)| {
                self.occupied
                    .iter()
                    .skip(index + 1)
                    .take_while(move |second| second.range.start() < first.range.end())
                    .filter_map(move |second| {
                        first.range.intersection(second.range).map(|range| Overlap {
                            first: first.occurrence,
                            second: second.occurrence,
                            range,
                        })
                    })
            })
    }

    /// Suggests one duration-fitting appointment per free gap, nearest to a preferred start.
    ///
    /// Each suggestion clamps `preferred_start` to the gap's valid start range.
    /// Results sort by absolute UTC distance from that start, then earlier start.
    /// No time grid or business-hour policy is imposed. Existing occurrences stay
    /// occupied, including the event being rescheduled. Uses O(g log g) time and
    /// O(g) storage for g fitting gaps; results remain inside the analyzed window.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDuration`] for a nonpositive duration and
    /// [`Error::OutOfRange`] if date arithmetic exceeds supported bounds.
    pub fn suggest_alternatives(
        &self,
        preferred_start: DateTime<Utc>,
        duration: Duration,
    ) -> Result<Vec<DateRange>, Error> {
        let mut slots = Vec::new();
        for gap in self.available_slots(duration)? {
            let latest = gap
                .end()
                .checked_sub_signed(duration)
                .ok_or(Error::OutOfRange)?;
            let start = preferred_start.clamp(gap.start(), latest);
            let end = start
                .checked_add_signed(duration)
                .ok_or(Error::OutOfRange)?;
            slots.push(DateRange::new(start, end)?);
        }
        slots.sort_unstable_by_key(|slot| ((slot.start() - preferred_start).abs(), slot.start()));
        Ok(slots)
    }
}

/// Elapsed occupied and free time within a nonempty analysis window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleDensity {
    occupied: Duration,
    free: Duration,
}

impl ScheduleDensity {
    /// Returns the union duration of occupied intervals, clipped to the window.
    pub const fn occupied(self) -> Duration {
        self.occupied
    }

    /// Returns total free duration within the window.
    pub const fn free(self) -> Duration {
        self.free
    }

    /// Returns occupied fraction between zero and one, with floating-point rounding.
    pub fn occupancy_ratio(self) -> f64 {
        self.occupied.as_seconds_f64() / (self.occupied + self.free).as_seconds_f64()
    }
}

/// Two conflicting occurrences and their shared UTC range within the analysis window.
#[derive(Debug, PartialEq, Eq)]
pub struct Overlap<'a, ItemId> {
    first: ScheduledOccurrence<'a, ItemId>,
    second: ScheduledOccurrence<'a, ItemId>,
    range: DateRange,
}

impl<ItemId> Copy for Overlap<'_, ItemId> {}

impl<ItemId> Clone for Overlap<'_, ItemId> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, ItemId> Overlap<'a, ItemId> {
    /// Returns the earlier occurrence in scheduler query order.
    pub const fn first(&self) -> ScheduledOccurrence<'a, ItemId> {
        self.first
    }

    /// Returns the later occurrence in scheduler query order.
    pub const fn second(&self) -> ScheduledOccurrence<'a, ItemId> {
        self.second
    }

    /// Returns the nonempty intersection, clipped to the query window.
    pub const fn range(&self) -> DateRange {
        self.range
    }
}

#[derive(Debug)]
struct OccupiedOccurrence<'a, ItemId> {
    occurrence: ScheduledOccurrence<'a, ItemId>,
    range: DateRange,
}
