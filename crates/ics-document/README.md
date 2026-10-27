# ics-document

Scheduling-agnostic iCalendar document parsing and serialization. No dependency
on `schedules-core`, Chrono, timezone databases, or an application event model.

```rust
use ics_document::IcsDocument;

let input = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";
let document = IcsDocument::deserialize(input)?;
let output = document.serialize();
# Ok::<(), ics_document::Error>(())
```

`properties()` and `events()` expose read-only wire data. Components expose their
properties and nested alarms; properties expose names, parameter values, raw
values, and original physical line ranges. Metadata remains uninterpreted.

Deserialization checks content-line syntax and nesting for VCALENDAR, VEVENT,
and nested VALARM. Other component kinds currently return a typed error. Timing
values, recurrence rules, UIDs, and required semantic fields are left to consumers.
Serialization normalizes component/property ordering, parameter quoting, UTF-8
line folding, and CRLF endings. It does not preserve original bytes.

`Error::location()` reports one-based physical line ranges, including folded
continuations. `kind()` exposes the typed failure; underlying causes remain in
the source chain. Callers must bound input size before deserialization.

Core's optional `ics` feature provides `IcsScheduleExt` for conversion between
documents and schedules. This crate does not depend on that integration.

```sh
cargo test -p ics-document
cargo bench -p ics-document --bench ics_deserialize
```

The benchmark covers 10, 100, and 1,000 events, with basic and metadata-heavy
fixtures. It measures parsing, source-location tracking, allocation, and document
destruction. Fixture generation and validation run outside measurement.
