# Schedules

Rust library for calendar-aware placement and recurrence. Workspace library:
`crates/core` (`schedules-core`). No runtime or persistence required.

`Scheduler<ItemId>` owns an in-memory map of item IDs to schedules. `insert` adds
or replaces a complete schedule and returns the previous value. `get`, `get_mut`,
`remove`, and `iter` provide access without cloning IDs or schedules. Application
continues to own item payloads and persistence.

`Scheduler::occurrences` queries all items and returns `ScheduledOccurrence` values
pairing borrowed item IDs with occurrences. Results sort by UTC start, item ID,
then occurrence key. Identity across items is `(item ID, occurrence key)`.
Work and result budgets apply across the entire query. Inspecting a schedule costs
one additional work unit, including schedules without matching occurrences.
Any expansion failure returns an error and discards partial results.

`Scheduler::next_after(&item_id, occurrence_key, all_day_zone, candidate_limit)`
searches directly for one successor, without a date window or result limit.
Ordering matches `occurrences`, including ties and repeated occurrences of the
same item. Overrides determine actual placement. Missing or cancelled anchors
return `Error::InvalidOccurrenceKey`; `None` means no successor exists across
exhausted schedules. Candidate budget covers anchor lookup, schedule inspections,
overrides, and recurrence expansion. Exhaustion returns `Error::CandidateLimit`,
never an incomplete answer. IDs remain borrowed; no result list is collected.
Unfiltered elapsed recurrence seeks by interval arithmetic, including count-limited
series. Calendar recurrence seeks to the anchor date for forever/until rules.
Calendar and weekday-filtered elapsed count limits use bounded historical expansion
because skipped dates, timezone gaps, and excluded weekdays do not count as occurrences.

1. `Placement`: timezone-aware point, timed interval, or date-only all-day range.
2. `Schedule`: placement plus optional recurrence and per-occurrence overrides.
3. `CalendarRule`: Gregorian daily, weekly, monthly, or yearly frequency, positive
   interval, optional weekday set, arbitrary selected local times, and termination.
4. `Recurrence::elapsed`: fixed positive UTC cadence with optional local weekday filtering.
5. `QueryWindow`: half-open UTC window, all-day viewing timezone, work budget,
   and result budget.
6. `Schedule::iter`: lazy fallible expansion. Overrides appear first, followed by
   regular occurrences in recurrence order. First error terminates iteration.
7. `Schedule::occurrences`: collect and sort overlapping occurrences by UTC start,
   then original occurrence key. Errors discard partial results.

## Availability and conflicts

`Scheduler::analyze(window)` expands occurrences once and returns a
`ScheduleAnalysis` borrowing item identifiers. Query work and occurrence budgets
retain their existing scope; any expansion error discards the analysis.

1. `gaps()` returns maximal free UTC ranges, including window edges.
2. `available_slots(minimum)` yields free gaps fitting a positive elapsed duration.
3. `longest_gap()` returns the longest gap, preferring the earliest on ties.
4. `is_slot_available(range)` requires the entire slot to lie inside a free gap.
5. `overlaps()` lazily yields each conflicting occurrence pair once, including
   occurrences of the same item, with original identities and clipped intersection.
6. `density()` reports occupied and free elapsed durations and occupancy ratio
   (zero to one). Overlapping time counts once.
7. `suggest_alternatives(preferred_start, duration)` returns one fitting appointment
   per free gap, ranked by distance from the preferred UTC start, then earlier start.
   Starts are clamped to each gap's valid start range; no time grid is imposed.

Analysis clips intervals to the window and resolves all-day boundaries in its
viewing timezone. Touching intervals do not conflict. Points consume no time and
do not block slots. Cancelled occurrences are absent; moved occurrences use their
replacement placement. Suggestions keep existing events occupied, including an
event being rescheduled, and cannot extend outside the analyzed window.

Analysis stores O(n) data after expansion and sorting. Conflict pair enumeration
can take O(n²) time; query budgets bound expansion, not pair enumeration. Use
`overlaps().take(limit)` to limit returned pairs without allocating all conflicts.
Suggestions use O(g log g) time and O(g) storage for g fitting gaps.

## Time semantics

Calendar recurrence preserves local wall time and named IANA timezone. Elapsed
recurrence preserves exact elapsed duration. Daily local recurrence therefore
differs from repeating every 24 hours across daylight-saving transitions.

Timed intervals use exact elapsed durations. All-day ranges use calendar dates,
with exclusive end dates; querying resolves boundaries in the supplied viewing
timezone. Nonexistent all-day midnight boundaries return an error.

Explicit nonexistent local placements are rejected. Calendar expansion skips
nonexistent local times and invalid dates without consuming recurrence count.
Ambiguous local times select earlier/later instant or return an error, according
to `ResolutionPolicy`. Default selects earlier instant. Weeks begin Monday.
Without weekday selection, monthly/yearly rules retain anchor day rather than
clamping to month end.

`CalendarRule::with_weekdays` and `Recurrence::with_weekdays` replace weekday
selection, removing duplicates:

1. Daily rules filter the original every-N-days cadence.
2. Weekly rules expand selected weekdays within matching Monday-based weeks.
3. Monthly rules expand every selected weekday within matching months.
4. Yearly rules expand every selected weekday throughout matching years.
5. Elapsed rules filter starts by local weekday in the placement's timezone,
   preserving original UTC cadence across excluded days and DST changes.

The initial placement must match the selection; earlier dates in its month/year
are not emitted. Empty selection restores default behavior: anchor weekday for
weekly, anchor day for monthly, anchor month/day for yearly, no weekday filter
for daily or elapsed. Excluded weekdays do not consume occurrence count, but
each scanned date or elapsed cadence step consumes query work budget. Filtered
elapsed rules currently scan skipped cadence steps; subsecond rules spanning
excluded days can require large budgets.

`CalendarRule::with_weekdays` now returns `Self`; remove trailing `?` or `unwrap()`
from existing calls. The obsolete `Error::InvalidWeekdayFilter` variant is removed.

`CalendarRule::with_times` selects any number of local times on each matching date.
`TimeSet` stores times uniquely in sorted order, retaining subsecond precision.
Empty selection inherits initial placement's time. For example, weekly recurrence
on Monday through Thursday with selected times 09:00 and 17:00 produces eight
occurrences per week. Three or more selected times work the same way.

Initial timed placement must belong to the selected time set. Earlier selected
times on its date are excluded; later matching dates include all selected times.
Each date/time pair has its own occurrence key and consumes recurrence count only
when successfully resolved. DST gaps skip only the affected time; ambiguity policy
applies independently to each time. Explicit time selection is rejected for all-day
schedules. Elapsed recurrence remains independent of calendar time selection.

Termination supports forever, count, inclusive local cutoff for calendar rules,
or inclusive UTC cutoff for elapsed rules. Initial placement must match rule and
not exceed cutoff. Count includes original occurrence and cancelled occurrences;
moving or cancelling instances does not regenerate the series.

Overrides retain original `OccurrenceKey`, scoped to their schedule. Replacement
can enter a query window even when original occurrence lies outside it. Membership
is validated with an explicit candidate budget before storing an override.

## Scope and limits

Application owns item identity and payload. Library computes placements without
executing jobs or reading system clock. Chrono and chrono-tz supply temporal types
and bundled timezone rules; dependency updates deliver timezone database changes.

Expansion scans from anchor to preserve count semantics. Calendar candidates are
scanned by date, then by selected time on matching dates. Each attempted time and
each skipped nonmatching date consumes work. Both overrides inspected and candidates scanned consume work
budget. Exhaustion returns a typed error rather than successful truncation.
Lazy consumers must handle errors encountered after earlier successful items.

Current API implements a recurrence subset, not full RFC 5545. Arbitrary month/day
filters, additional recurrence dates, floating local times, calendar-duration
event lengths, and iCalendar/Google adapters are not implemented.

## Validation

```sh
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Benchmark

```sh
cargo bench -p schedules-core --bench scheduler
```

Criterion measures complete sorted scheduler queries with 10, 100, and 1,000 items.
Each item repeats on four weekdays at three local times across London, New York,
and Nairobi timezones. Two-week query crosses London's spring DST transition and
returns 24 occurrences per item. Recent and year-old anchors expose expansion's
history-scanning cost. Setup and successful result-count verification occur outside
timed iterations; query expansion, timezone resolution, allocation, sorting, and
result destruction are measured. Criterion stores reports under `target/criterion`.

`scheduler_next_after` measures one-successor searches using the same item counts,
timezones, and recent/year-old schedules. Anchor occurrence stays in March 2026,
so year-old cases verify that seeking avoids historical expansion. Setup validates the successor
against sorted query results outside timed iterations. Throughput counts one
successor per query. Run only this group with:

```sh
cargo bench -p schedules-core --bench scheduler -- scheduler_next_after
```

Analysis benchmarks use one-hour timed events with the same item counts,
recurrence rules, timezones, and DST-crossing window:

1. `scheduler_analysis` measures expansion, sorting, analysis construction, and
   result destruction for recent and year-old anchors.
2. `scheduler_analysis_overlaps` consumes every conflict pair from a precomputed
   analysis; throughput counts pairs. Dense same-zone events expose quadratic
   output growth without allocating a conflict list.
3. `scheduler_analysis_alternatives` generates and drops ranked one-hour
   appointments from a precomputed analysis. Gap count remains fixed as item
   count grows, isolating suggestion cost from occurrence expansion.

Setup validates occurrence counts, 64 occupied hours, 49 free gaps, independently
calculated conflict counts, and suggested appointment validity outside timing.
Run all analysis groups with:

```sh
cargo bench -p schedules-core --bench scheduler -- scheduler_analysis
```
