# Calendar views

Standalone Rust library grouping arbitrary application data into timezone-aware
day/week views. No dependency on `schedules-core`, recurrence, item IDs, or a UI.
Uses Chrono's Gregorian dates and chrono-tz's IANA timezones.

1. Build a `CalendarWindow` from inclusive/exclusive local dates and a timezone.
2. Adapt each payload with `CalendarEntry::new(payload, TimeSpan)`; payloads may
   be owned values or borrowed references, without implementing a trait.
3. Construct `Calendar::new(window, entries)` from a finite iterator.
4. Iterate `calendar.days()` or `calendar.weeks(Weekday::Mon)`.

`TimeSpan::point` represents an instant. `TimeSpan::interval` validates a
nonempty half-open UTC interval. To adapt date-only all-day data, resolve its
dates using `CalendarWindow::new(start, end, viewing_zone)?.span()`.

The snapshot retains overlapping entries sorted by UTC start, preserving input
order on ties. Views borrow entries without cloning payloads. Multi-day entries
appear on every overlapping day. End-at-midnight intervals exclude the next day;
points belong to exactly one local date. Original spans remain unmodified.

Days include empty dates and follow actual local midnight boundaries, including
23/25-hour DST days. Ambiguous midnight selects the earlier instant; nonexistent
midnight returns a typed error. Outer boundaries resolve during window creation;
interior boundaries resolve during iteration. Iterators stop permanently after
an error; collecting into `Result<Vec<_>, _>` discards partial results.

Weeks contain seven `Option<DayView>` positions: `None` means outside the requested
window, while `Some` with no entries means a requested empty date. Call
`window.covering_weeks(weekday)` before fetching data for complete weeks. Week
alignment and padding outside supported dates return `Error::OutOfRange`.

Snapshot construction uses O(n log n) sorting and O(n) storage. Lazy day grouping
scans retained entries per requested day: O(d × n), without building a full grid.
The caller controls input size and how many views to consume. Week iteration
constructs at most seven days at a time.

```sh
cargo test -p calendar-views
```
