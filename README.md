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

1. `Placement`: timezone-aware point, timed interval, or date-only all-day range.
2. `Schedule`: placement plus optional recurrence and per-occurrence overrides.
3. `CalendarRule`: Gregorian daily, weekly, monthly, or yearly frequency, positive
   interval, optional weekly weekday set, arbitrary selected local times, and termination.
4. `Recurrence::elapsed`: fixed positive duration between occurrence starts.
5. `QueryWindow`: half-open UTC window, all-day viewing timezone, work budget,
   and result budget.
6. `Schedule::iter`: lazy fallible expansion. Overrides appear first, followed by
   regular occurrences in recurrence order. First error terminates iteration.
7. `Schedule::occurrences`: collect and sort overlapping occurrences by UTC start,
   then original occurrence key. Errors discard partial results.

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
Monthly/yearly rules retain anchor day rather than clamping to month end.

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
