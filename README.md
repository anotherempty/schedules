# Schedules

## Description

Schedules is a Rust library for calendars, recurring appointments, and availability
tools. Define when items happen, query occurrences, and find conflicts or free time
without running a background service.

1. Schedule one-off or recurring points, timed intervals, and all-day ranges.
2. Exclude dates, cancel occurrences, or move individual events.
3. Find conflicts, free slots, occupancy, and alternative appointment times.
4. Group occurrences into local days and weeks.
5. Parse and serialize ICS documents, with optional conversion to schedules.

Your application owns item IDs, event content, persistence, and job execution.
Schedules computes occurrences in memory; it does not execute tasks or read the
system clock.

Version 0.1: public APIs may change before 1.0. Project code is available under
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Vendored skills retain their [upstream licenses](THIRD-PARTY-NOTICES.md).

| Crate | Purpose |
| --- | --- |
| `schedules-core` | Schedule definitions, recurrence, queries, and availability |
| [`calendar-views`](crates/calendar-views/README.md) | Standalone day/week grouping for arbitrary payloads |
| [`ics-document`](crates/ics-document/README.md) | Standalone ICS parsing, serialization, metadata, and source locations |

Standalone crates do not depend on core. Core integrates them through optional
features, both disabled by default.

## Usage

### Add the dependency

Add the dependency:

```toml
[dependencies]
schedules-core = "0.1"
```

Enable either or both integrations when needed:

```toml
[dependencies]
schedules-core = { version = "0.1", features = ["calendar-views", "ics"] }
```

### Query a recurring schedule

This complete example creates a daily reminder and queries one week. The Unix
epoch is a fixed example start; supply your application's timestamp in practice.

```rust
use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::{
    CalendarRule, DateRange, DateTime, Duration, Error, Frequency, Placement,
    QueryWindow, Recurrence, Schedule, Termination, Tz,
};

const QUERY_SPAN: Duration = Duration::weeks(1);
const CANDIDATE_BUDGET: usize = 1_000;
const OCCURRENCE_BUDGET: usize = 100;

fn main() -> Result<(), Error> {
    let start = DateTime::UNIX_EPOCH;
    let end = start.checked_add_signed(QUERY_SPAN).ok_or(Error::OutOfRange)?;
    let placement = Placement::instant(start, Tz::UTC)?;
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Forever,
    )?;
    let schedule = Schedule::repeating(placement, Recurrence::calendar(rule))?;
    let window = QueryWindow {
        range: DateRange::new(start, end)?,
        all_day_zone: Tz::UTC,
        candidate_limit: NonZeroUsize::new(CANDIDATE_BUDGET).ok_or(Error::OutOfRange)?,
        occurrence_limit: NonZeroUsize::new(OCCURRENCE_BUDGET).ok_or(Error::OutOfRange)?,
    };

    let occurrences = schedule.occurrences(window)?;
    // Seven daily points, sorted by UTC start. Pass these to your application.
    for occurrence in occurrences {
        let _placement = occurrence.placement();
        let _identity = occurrence.key();
    }
    Ok(())
}
```

Use `Schedule::once(placement)` for a one-off event, `Placement::timed` for an
interval with an elapsed duration, or `Placement::all_day` for a date range.
`CalendarRule::with_weekdays` selects weekdays; `with_times` selects multiple local
start times per matching date. The initial placement must match the rule.

### Manage items and find availability

`ScheduleStore<ItemId>` associates your IDs with schedules. Create one with
`ScheduleStore::new()`, add a schedule with `store.insert(item_id, schedule)`,
then query `store.occurrences(window)`. Results borrow item IDs; payloads remain
in your application. Inserting an existing ID replaces its schedule and returns
the previous value.

| Task | API |
| --- | --- |
| Find the next occurrence across stored schedules after an existing occurrence | `store.next_after(&item_id, key, zone, candidate_limit)` |
| Exclude or restore a local date | `schedule.exclude_date(date)` / `schedule.include_date(date)` |
| Apply holiday calendars or business rules | `occurrences_filtered(window, predicate)` |
| Analyze a window | `store.analyze(window)` |
| Inspect free time | `analysis.gaps()` / `analysis.longest_gap()` |
| Find gaps fitting a duration | `analysis.available_slots(minimum)` |
| Check a proposed slot | `analysis.is_slot_available(range)` |
| Inspect conflicts or occupancy | `analysis.overlaps()` / `analysis.density()` |
| Suggest appointment times | `analysis.suggest_alternatives(preferred_start, duration)` |

Points consume no time and do not block availability. Timed and all-day intervals
do. Touching intervals do not conflict. Suggestions stay inside the query window
and keep existing events occupied, including an event being rescheduled.

### Calendar views and ICS

With `calendar-views`, call
`store.calendar(window, candidate_limit, occurrence_limit)` using a
`schedules_core::calendar::CalendarWindow`. The snapshot exposes `days()` and
`weeks(weekday)`. For complete boundary weeks, expand the window with
`covering_weeks(weekday)` before querying. See the
[calendar views guide](crates/calendar-views/README.md) for standalone usage.

With `ics`, import `schedules_core::IcsScheduleExt` alongside `IcsDocument`:

1. `IcsDocument::deserialize(text)` parses an ICS document.
2. `document.to_schedules(candidate_limit)` converts supported events into schedules.
3. `IcsDocument::from_schedules(&events, dtstamp)` builds a document from schedules.
4. `document.serialize()` produces ICS text.

Document parsing and schedule conversion are separate. Documents preserve metadata
properties and nested alarms; schedule conversion ignores metadata. Serialization
normalizes formatting rather than preserving original bytes. See the
[ICS document guide](crates/ics-document/README.md) for standalone usage.

## How it works

### Definitions and queries

1. `Placement` defines when an item happens; `Schedule` adds recurrence and exceptions.
   `QueryWindow` sets the UTC range, all-day timezone, and work/result limits.
2. Queries expand schedules, apply overrides, then filter by exclusions and window.
   Occurrence keys remain stable when events move; store identity pairs the key
   with an item ID.
3. `occurrences` returns sorted results: UTC start, item ID for store queries,
   then occurrence key. `iter` yields lazily, overrides first, and stops on error.

### Time and recurrence

1. **Calendar recurrence** preserves local wall time in a named IANA timezone.
   Daily, weekly, monthly, and yearly rules support positive intervals, weekdays,
   multiple local times, and count or inclusive local cutoffs.
2. **Elapsed recurrence** preserves an exact UTC cadence, with optional local
   weekday filtering and count or inclusive UTC cutoffs. Repeating every 24 hours
   can differ from repeating daily across daylight-saving changes.
3. **Ranges** include their start and exclude their end. Timed durations measure
   elapsed time. All-day dates resolve in the query's viewing timezone.
4. **DST and invalid dates:** explicit nonexistent local placements fail;
   recurrence skips nonexistent times and invalid dates without consuming count.
   Ambiguous times use `ResolutionPolicy`, defaulting to the earlier instant.
   Nonexistent all-day midnight boundaries return errors.
5. **Calendar selection:** weeks begin Monday. Without weekday selection, monthly
   and yearly rules keep the anchor day and skip invalid dates instead of clamping.
   With weekday selection, they expand every matching weekday in matching periods.
6. **Exceptions:** cancellations, exclusions, and predicate rejection do not extend
   recurrence counts. Date exclusions use the effective occurrence's local start
   date, so an overnight event starting the previous day remains visible.

Use deterministic predicates consistently across occurrence, analysis, and
successor queries; traversal order can differ. Timezone rules come from
`chrono-tz`, so dependency updates deliver timezone database changes.

### Limits and errors

1. **Query limits:** positive work and result budgets apply across the whole query.
   Exhaustion returns a typed error, not truncated success. Collecting queries
   discard partial results; lazy iteration can fail after yielding items.
   `next_after` returns `None` when exhausted and errors for invalid anchors.
2. **Cost:** long histories and filtered recurrences can require large budgets.
   Budgets exclude predicate runtime and conflict enumeration, which can be quadratic.
3. **Recurrence scope:** core does not implement full RFC 5545, floating times,
   calendar-duration event lengths, or Google adapters.
4. **ICS:** conversion supports a subset of UTC and date-only schedules; unsupported
   timing returns errors. Parsing accepts `VCALENDAR`, `VEVENT`, and nested `VALARM`,
   but not `VTIMEZONE`. Bound input size and event count separately; conversion
   budgets apply per exception. See the [ICS guide](crates/ics-document/README.md).

## Development

Requires Rust 1.91 or newer. Run commands from the repository root.

### Checks

```sh
cargo test --workspace
cargo test --workspace --all-features
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-features --all-targets -- -D warnings
```

Generate API documentation, including optional integrations:

```sh
cargo doc --workspace --all-features --no-deps --open
```

### Benchmarks

```sh
cargo bench -p schedules-core --bench schedule_store
cargo bench -p ics-document --bench ics_deserialize
```

Benchmarks focus on how costs grow with more items, older recurring schedules,
and richer calendar documents:

1. **Occurrence queries:** measure expansion and sorting across store sizes.
   Compare recent and year-old schedules to expose the cost of scanning history.
2. **Successor searches:** measure finding one next occurrence without collecting
   a whole window. Older schedules help assess whether seeking avoids historical
   expansion.
3. **Availability analysis:** measure building an analysis from recurring events.
   Separate benchmarks measure enumerating all conflict pairs in dense calendars
   and ranking alternative appointments from an already-built analysis.
4. **ICS parsing:** compare basic and metadata-heavy documents as event counts
   grow. Measure document parsing and allocation, excluding schedule conversion
   and serialization.

Fixture setup and correctness checks run outside timing.

#### Benchmarking new functionality

When adding functionality, benchmark the user-visible operation and the factors
that make its cost grow:

1. **Representative workloads:** cover typical use and expensive supported cases.
   For scheduling features, consider long recurrence histories, dense overlaps,
   exclusions, overrides, and timezone transitions where relevant.
2. **Scaling:** vary the relevant input size, such as schedules, occurrences,
   exceptions, free gaps, or document bytes. Start with 10, 100, and 1,000 items
   when meaningful; keep other factors fixed to isolate the cost being measured.
3. **Complete operations:** measure what callers pay, including expansion,
   allocation, sorting, and result destruction where applicable. If a feature
   reuses precomputed data, measure preparation and reuse separately.
4. **Correctness and limits:** validate expected results outside timing. For
   bounded operations, include workloads near their budgets; benchmark rejection
   paths separately when handling oversized or invalid input is a feature concern.
5. **Comparisons:** run existing benchmarks before and after changes under the
   same conditions. Record input sizes, units, CPU, thread count, toolchain, and
   build profile alongside results. Compare against measurement variability
   before claiming an improvement or regression.

Keep fixture construction outside timing unless construction itself is the
operation being measured. Add benchmarks for meaningful costs rather than every
method, and update the results below when benchmarked behavior changes.

#### Measured results

Benchmarks used one workload thread (reported clock: 3.8 GHz).
Times are per complete operation, not per item.

| Core operation | 10 schedules | 100 schedules | 1,000 schedules |
| --- | ---: | ---: | ---: |
| Occurrence query, recent anchors | 75.14 µs | 830.69 µs | 9.19 ms |
| Occurrence query, year-old anchors | 1.48 ms | 14.89 ms | 149.45 ms |
| Next occurrence, recent anchors | 3.50 µs | 29.37 µs | 284.29 µs |
| Next occurrence, year-old anchors | 3.08 µs | 28.46 µs | 286.08 µs |
| Availability analysis, recent anchors | 110.07 µs | 1.13 ms | 12.80 ms |
| Availability analysis, year-old anchors | 1.82 ms | 17.63 ms | 180.46 ms |
| Enumerate all conflict pairs | 10.62 µs | 1.20 ms | 122.34 ms |
| Rank alternative appointments | 2.34 µs | 2.38 µs | 2.43 µs |

Core fixtures use recurring schedules across London, New York, and Nairobi,
with 24 occurrences per schedule in a two-week window crossing London's DST
change. Analysis uses one-hour events.

| ICS document parsing | 10 events | 100 events | 1,000 events |
| --- | ---: | ---: | ---: |
| Basic UTC events | 22.67 µs | 259.76 µs | 2.35 ms |
| Events with metadata and alarms | 66.64 µs | 710.09 µs | 7.34 ms |

ICS timings include
parsing, source-location tracking, allocation, and document destruction.
