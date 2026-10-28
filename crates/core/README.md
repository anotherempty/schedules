# schedules-core

Timezone-aware schedule definitions, recurrence, occurrence queries, and
availability analysis. Supports one-off and recurring points, timed intervals,
all-day ranges, exclusions, and per-occurrence overrides.

Add the dependency:

```toml
[dependencies]
schedules-core = "0.1"
```

Optional features:

1. `calendar-views`: group occurrences into local days and weeks.
2. `ics`: convert supported iCalendar documents to and from schedules.

Build a `Placement`, wrap it in a `Schedule`, and query a `QueryWindow`.
Use `ScheduleStore` to associate schedules with application IDs and inspect
conflicts, free time, and alternative appointment slots.

The application owns persistence and job execution. Queries require explicit
work and result budgets; exhaustion returns an error rather than partial success.

See the [repository guide](https://github.com/anotherempty/schedules#usage)
for examples and supported timing rules.

Licensed under MIT OR Apache-2.0.
