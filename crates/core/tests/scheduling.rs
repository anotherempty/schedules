use std::num::{NonZeroU32, NonZeroUsize};

use scheduler_core::{
    AmbiguityPolicy, CalendarRule, DateRange, DateTime, Duration, Error, Frequency, NaiveDate,
    NaiveDateTime, Occurrence, OccurrenceKey, OccurrenceOverride, Placement, QueryWindow,
    Recurrence, ResolutionPolicy, Schedule, Termination, Tz, Utc, Weekday,
};

const LOCAL_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
const DATE_FORMAT: &str = "%Y-%m-%d";
const CANDIDATE_BUDGET: usize = 10_000;
const OCCURRENCE_BUDGET: usize = 100;
const THREE_OCCURRENCES: u32 = 3;
const TWO_OCCURRENCES: u32 = 2;
const FORTNIGHT_WEEKS: u32 = 2;
const EXACT_DAY_HOURS: i64 = 24;
const MEETING_HOURS: i64 = 2;
const ONE_HOUR_SECONDS: i64 = 3_600;
const ONE_OCCURRENCE: u32 = 1;

#[test]
fn query_stops_before_future_ambiguity() {
    let anchor = Placement::point(
        Fixture::local("2026-10-24 01:30:00"),
        chrono_tz::Europe::London,
        ResolutionPolicy::new(AmbiguityPolicy::Reject),
    )
    .unwrap();
    let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Termination::Forever).unwrap();
    let schedule = Schedule::repeating(anchor, Recurrence::calendar(rule)).unwrap();
    for end in ["2026-10-24T23:00:00Z", "2026-10-25T00:30:00Z"] {
        let items = schedule
            .occurrences(Fixture::window("2026-10-24T00:00:00Z", end))
            .unwrap();
        assert_eq!(
            Fixture::starts(&items),
            [Fixture::instant("2026-10-24T00:30:00Z")]
        );
    }
    assert!(matches!(
        schedule.occurrences(Fixture::window(
            "2026-10-24T00:00:00Z",
            "2026-10-25T01:00:00Z"
        )),
        Err(Error::AmbiguousTime { .. })
    ));
}

#[test]
fn elapsed_cutoff_prevents_overflowing_advance() {
    let anchor = Fixture::point("2026-01-01 00:00:00", chrono_tz::UTC);
    for cutoff in ["2026-01-01T00:00:00Z", "2026-01-01T12:00:00Z"] {
        let recurrence = Recurrence::elapsed(
            Duration::MAX,
            Termination::UntilInstant(Fixture::instant(cutoff)),
        )
        .unwrap();
        let schedule = Schedule::repeating(anchor, recurrence).unwrap();
        let items = schedule
            .occurrences(Fixture::window(
                "2026-01-01T00:00:00Z",
                "2026-01-02T00:00:00Z",
            ))
            .unwrap();
        assert_eq!(
            Fixture::starts(&items),
            [Fixture::instant("2026-01-01T00:00:00Z")]
        );
    }
}

struct Fixture;

impl Fixture {
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, LOCAL_FORMAT).expect("valid fixture local time")
    }

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, DATE_FORMAT).expect("valid fixture date")
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("valid fixture instant")
            .with_timezone(&Utc)
    }

    fn budget() -> NonZeroUsize {
        NonZeroUsize::new(CANDIDATE_BUDGET).expect("positive budget")
    }

    fn window(start: &str, end: &str) -> QueryWindow {
        QueryWindow {
            range: DateRange::new(Self::instant(start), Self::instant(end)).unwrap(),
            all_day_zone: chrono_tz::UTC,
            candidate_limit: Self::budget(),
            occurrence_limit: NonZeroUsize::new(OCCURRENCE_BUDGET).expect("positive result limit"),
        }
    }

    fn point(local: &str, zone: Tz) -> Placement {
        Placement::point(Self::local(local), zone, ResolutionPolicy::default())
            .expect("valid point")
    }

    fn rule(frequency: Frequency, count: u32) -> CalendarRule {
        CalendarRule::new(
            frequency,
            NonZeroU32::MIN,
            Termination::Count(NonZeroU32::new(count).expect("positive count")),
        )
        .expect("valid rule")
    }

    fn starts(items: &[Occurrence]) -> Vec<DateTime<Utc>> {
        items
            .iter()
            .map(|item| {
                item.placement()
                    .bounds(chrono_tz::UTC)
                    .expect("valid bounds")
                    .0
            })
            .collect()
    }
}

#[test]
fn calendar_preserves_wall_time_while_elapsed_preserves_duration() {
    let anchor = Fixture::point("2026-03-28 09:00:00", chrono_tz::Europe::London);
    let count = Termination::Count(NonZeroU32::new(THREE_OCCURRENCES).unwrap());
    let calendar = Schedule::repeating(
        anchor,
        Recurrence::calendar(Fixture::rule(Frequency::Daily, THREE_OCCURRENCES)),
    )
    .unwrap();
    let elapsed = Schedule::repeating(
        anchor,
        Recurrence::elapsed(Duration::hours(EXACT_DAY_HOURS), count).unwrap(),
    )
    .unwrap();
    let window = Fixture::window("2026-03-28T00:00:00Z", "2026-04-01T00:00:00Z");
    assert_eq!(
        Fixture::starts(&calendar.occurrences(window).unwrap()),
        [
            Fixture::instant("2026-03-28T09:00:00Z"),
            Fixture::instant("2026-03-29T08:00:00Z"),
            Fixture::instant("2026-03-30T08:00:00Z")
        ]
    );
    assert_eq!(
        Fixture::starts(&elapsed.occurrences(window).unwrap()),
        [
            Fixture::instant("2026-03-28T09:00:00Z"),
            Fixture::instant("2026-03-29T09:00:00Z"),
            Fixture::instant("2026-03-30T09:00:00Z")
        ]
    );
}

#[test]
fn spring_gap_is_skipped_without_consuming_count() {
    let zone = chrono_tz::Europe::London;
    let missing = Fixture::local("2026-03-29 01:30:00");
    assert!(matches!(
        Placement::point(missing, zone, ResolutionPolicy::default()),
        Err(Error::NonexistentTime { .. })
    ));
    let schedule = Schedule::repeating(
        Fixture::point("2026-03-28 01:30:00", zone),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, TWO_OCCURRENCES)),
    )
    .unwrap();
    let items = schedule
        .occurrences(Fixture::window(
            "2026-03-28T00:00:00Z",
            "2026-04-01T00:00:00Z",
        ))
        .unwrap();
    assert_eq!(
        Fixture::starts(&items),
        [
            Fixture::instant("2026-03-28T01:30:00Z"),
            Fixture::instant("2026-03-30T00:30:00Z")
        ]
    );
}

#[test]
fn repeated_local_time_supports_all_ambiguity_policies() {
    let local = Fixture::local("2026-10-25 01:30:00");
    let zone = chrono_tz::Europe::London;
    let early = ResolutionPolicy::new(AmbiguityPolicy::Earlier)
        .resolve(local, zone)
        .unwrap();
    let late = ResolutionPolicy::new(AmbiguityPolicy::Later)
        .resolve(local, zone)
        .unwrap();
    assert_eq!((late - early).num_seconds(), ONE_HOUR_SECONDS);
    assert!(matches!(
        ResolutionPolicy::new(AmbiguityPolicy::Reject).resolve(local, zone),
        Err(Error::AmbiguousTime { .. })
    ));
    assert_eq!(
        Placement::instant(late, zone)
            .unwrap()
            .bounds(zone)
            .unwrap()
            .0,
        late
    );
}

#[test]
fn elapsed_expansion_retains_both_sides_of_clock_fold() {
    let schedule = Schedule::repeating(
        Fixture::point("2026-10-25 01:30:00", chrono_tz::Europe::London),
        Recurrence::elapsed(
            Duration::seconds(ONE_HOUR_SECONDS),
            Termination::UntilInstant(Fixture::instant("2026-10-25T01:30:00Z")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule
                .occurrences(Fixture::window(
                    "2026-10-25T00:00:00Z",
                    "2026-10-26T00:00:00Z"
                ))
                .unwrap()
        ),
        [
            Fixture::instant("2026-10-25T00:30:00Z"),
            Fixture::instant("2026-10-25T01:30:00Z")
        ]
    );
}

#[test]
fn monthly_skips_missing_dates_and_yearly_skips_non_leap_years() {
    let monthly = Schedule::repeating(
        Fixture::point("2026-01-31 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Monthly, THREE_OCCURRENCES)),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &monthly
                .occurrences(Fixture::window(
                    "2026-01-01T00:00:00Z",
                    "2026-06-01T00:00:00Z"
                ))
                .unwrap()
        ),
        [
            Fixture::instant("2026-01-31T09:00:00Z"),
            Fixture::instant("2026-03-31T09:00:00Z"),
            Fixture::instant("2026-05-31T09:00:00Z")
        ]
    );
    let yearly = Schedule::repeating(
        Fixture::point("2024-02-29 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Yearly, TWO_OCCURRENCES)),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &yearly
                .occurrences(Fixture::window(
                    "2024-01-01T00:00:00Z",
                    "2029-01-01T00:00:00Z"
                ))
                .unwrap()
        ),
        [
            Fixture::instant("2024-02-29T09:00:00Z"),
            Fixture::instant("2028-02-29T09:00:00Z")
        ]
    );
}

#[test]
fn weekly_filters_use_monday_based_interval_weeks() {
    let rule = CalendarRule::new(
        Frequency::Weekly,
        NonZeroU32::new(FORTNIGHT_WEEKS).unwrap(),
        Termination::Count(NonZeroU32::new(THREE_OCCURRENCES).unwrap()),
    )
    .unwrap()
    .with_weekdays([Weekday::Fri, Weekday::Mon, Weekday::Fri])
    .unwrap();
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-02 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(rule),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule
                .occurrences(Fixture::window(
                    "2026-01-01T00:00:00Z",
                    "2026-02-01T00:00:00Z"
                ))
                .unwrap()
        ),
        [
            Fixture::instant("2026-01-02T09:00:00Z"),
            Fixture::instant("2026-01-12T09:00:00Z"),
            Fixture::instant("2026-01-16T09:00:00Z")
        ]
    );
}

#[test]
fn overrides_move_into_window_cancel_and_preserve_original_identity() {
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, THREE_OCCURRENCES)),
    )
    .unwrap();
    let moved_key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    let cancel_key = OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"));
    schedule
        .set_override(
            moved_key,
            OccurrenceOverride::Replace(Fixture::point("2026-02-01 12:00:00", chrono_tz::UTC)),
            Fixture::budget(),
        )
        .unwrap();
    schedule
        .set_override(cancel_key, OccurrenceOverride::Cancel, Fixture::budget())
        .unwrap();
    let items = schedule
        .occurrences(Fixture::window(
            "2026-02-01T00:00:00Z",
            "2026-02-02T00:00:00Z",
        ))
        .unwrap();
    assert_eq!(items.first().unwrap().key(), moved_key);
    assert_eq!(
        Fixture::starts(&items),
        [Fixture::instant("2026-02-01T12:00:00Z")]
    );
    assert_eq!(
        Fixture::starts(
            &schedule
                .occurrences(Fixture::window(
                    "2026-01-01T00:00:00Z",
                    "2026-01-04T00:00:00Z"
                ))
                .unwrap()
        ),
        [Fixture::instant("2026-01-03T09:00:00Z")]
    );
    assert_eq!(
        schedule.set_override(
            OccurrenceKey::Calendar(Fixture::local("2026-01-04 09:00:00")),
            OccurrenceOverride::Cancel,
            Fixture::budget()
        ),
        Err(Error::InvalidOccurrenceKey)
    );
    assert_eq!(
        schedule.remove_override(cancel_key),
        Some(OccurrenceOverride::Cancel)
    );
}

#[test]
fn all_day_uses_viewing_zone_and_exclusive_end_dates() {
    let placement =
        Placement::all_day(Fixture::date("2026-03-29"), Fixture::date("2026-03-30")).unwrap();
    let (start, end) = placement.bounds(chrono_tz::Europe::London).unwrap();
    assert_eq!(start, Fixture::instant("2026-03-29T00:00:00Z"));
    assert_eq!(end, Some(Fixture::instant("2026-03-29T23:00:00Z")));
    let query = QueryWindow {
        range: DateRange::new(
            Fixture::instant("2026-03-29T23:00:00Z"),
            Fixture::instant("2026-03-30T00:00:00Z"),
        )
        .unwrap(),
        all_day_zone: chrono_tz::Europe::London,
        candidate_limit: Fixture::budget(),
        occurrence_limit: NonZeroUsize::MIN,
    };
    assert!(
        Schedule::once(placement)
            .occurrences(query)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        Schedule::repeating(
            placement,
            Recurrence::elapsed(Duration::hours(EXACT_DAY_HOURS), Termination::Forever).unwrap()
        ),
        Err(Error::ElapsedAllDay)
    );
}

#[test]
fn overlap_includes_intervals_started_before_window_and_excludes_points_at_end() {
    let window = Fixture::window("2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z");
    let interval = Placement::timed(
        Fixture::local("2026-01-01 09:00:00"),
        chrono_tz::UTC,
        Duration::hours(MEETING_HOURS),
        ResolutionPolicy::default(),
    )
    .unwrap();
    assert!(
        !Schedule::once(interval)
            .occurrences(window)
            .unwrap()
            .is_empty()
    );
    assert!(
        Schedule::once(Fixture::point("2026-01-01 11:00:00", chrono_tz::UTC))
            .occurrences(window)
            .unwrap()
            .is_empty()
    );
    assert!(
        !Schedule::once(Fixture::point("2026-01-01 10:00:00", chrono_tz::UTC))
            .occurrences(window)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn limits_return_errors_instead_of_partial_results() {
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, THREE_OCCURRENCES)),
    )
    .unwrap();
    let start = Fixture::instant("2026-01-01T00:00:00Z");
    let end = Fixture::instant("2026-01-05T00:00:00Z");
    let work_limited = QueryWindow {
        range: DateRange::new(start, end).unwrap(),
        all_day_zone: chrono_tz::UTC,
        candidate_limit: NonZeroUsize::MIN,
        occurrence_limit: NonZeroUsize::MIN,
    };
    assert_eq!(
        schedule.occurrences(work_limited),
        Err(Error::CandidateLimit)
    );
    let result_limited = QueryWindow {
        range: DateRange::new(start, end).unwrap(),
        all_day_zone: chrono_tz::UTC,
        candidate_limit: Fixture::budget(),
        occurrence_limit: NonZeroUsize::MIN,
    };
    assert_eq!(
        schedule.occurrences(result_limited),
        Err(Error::OccurrenceLimit)
    );
}

#[test]
fn inclusive_until_and_validation() {
    let anchor = Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC);
    let cutoff = Fixture::local("2026-01-02 09:00:00");
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Until(cutoff),
    )
    .unwrap();
    let schedule = Schedule::repeating(anchor, Recurrence::calendar(rule)).unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule
                .occurrences(Fixture::window(
                    "2026-01-01T00:00:00Z",
                    "2026-01-05T00:00:00Z"
                ))
                .unwrap()
        ),
        [
            Fixture::instant("2026-01-01T09:00:00Z"),
            Fixture::instant("2026-01-02T09:00:00Z")
        ]
    );
    assert_eq!(
        Recurrence::elapsed(Duration::zero(), Termination::Forever),
        Err(Error::InvalidDuration)
    );
    assert_eq!(
        Recurrence::elapsed(Duration::hours(EXACT_DAY_HOURS), Termination::Until(cutoff)),
        Err(Error::InvalidTermination)
    );
    assert_eq!(
        Placement::all_day(cutoff.date(), cutoff.date()),
        Err(Error::InvalidRange)
    );
    let wrong_days = Fixture::rule(Frequency::Weekly, TWO_OCCURRENCES)
        .with_weekdays([Weekday::Fri])
        .unwrap();
    assert_eq!(
        Schedule::repeating(anchor, Recurrence::calendar(wrong_days)),
        Err(Error::InvalidAnchor)
    );
    assert_eq!(
        Fixture::rule(Frequency::Daily, TWO_OCCURRENCES).with_weekdays([Weekday::Fri]),
        Err(Error::InvalidWeekdayFilter)
    );
}

#[test]
fn extreme_instant_conversion_returns_error_without_panicking() {
    assert_eq!(
        Placement::instant(DateTime::<Utc>::MAX_UTC, chrono_tz::Asia::Tokyo),
        Err(Error::OutOfRange)
    );
}

#[test]
fn lazy_expansion_returns_first_item_before_budget_error_then_stays_exhausted() {
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, THREE_OCCURRENCES)),
    )
    .unwrap();
    let window = QueryWindow {
        range: DateRange::new(
            Fixture::instant("2026-01-01T00:00:00Z"),
            Fixture::instant("2026-02-01T00:00:00Z"),
        )
        .unwrap(),
        all_day_zone: chrono_tz::UTC,
        candidate_limit: NonZeroUsize::MIN,
        occurrence_limit: NonZeroUsize::MIN,
    };
    let mut iter = schedule.iter(window).unwrap();
    assert_eq!(
        iter.next().unwrap().unwrap().key(),
        OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"))
    );
    assert_eq!(iter.next(), Some(Err(Error::CandidateLimit)));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next(), None);
}

#[test]
fn finite_schedule_succeeds_with_exact_candidate_budget() {
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, ONE_OCCURRENCE)),
    )
    .unwrap();
    let window = QueryWindow {
        range: DateRange::new(
            Fixture::instant("2026-01-01T00:00:00Z"),
            Fixture::instant("2026-02-01T00:00:00Z"),
        )
        .unwrap(),
        all_day_zone: chrono_tz::UTC,
        candidate_limit: NonZeroUsize::MIN,
        occurrence_limit: NonZeroUsize::MIN,
    };
    assert_eq!(
        Fixture::starts(&schedule.occurrences(window).unwrap()),
        [Fixture::instant("2026-01-01T09:00:00Z")]
    );
}

#[test]
fn all_day_recurrence_keeps_date_range_length_and_date_keys() {
    let anchor =
        Placement::all_day(Fixture::date("2026-01-01"), Fixture::date("2026-01-03")).unwrap();
    let mut schedule = Schedule::repeating(
        anchor,
        Recurrence::calendar(Fixture::rule(Frequency::Daily, TWO_OCCURRENCES)),
    )
    .unwrap();
    let key = OccurrenceKey::Date(Fixture::date("2026-01-01"));
    schedule
        .set_override(key, OccurrenceOverride::Cancel, Fixture::budget())
        .unwrap();
    let items = schedule
        .occurrences(Fixture::window(
            "2026-01-01T00:00:00Z",
            "2026-01-05T00:00:00Z",
        ))
        .unwrap();
    assert_eq!(
        items.first().unwrap().placement().dates(),
        Some((Fixture::date("2026-01-02"), Fixture::date("2026-01-04")))
    );
    assert_eq!(
        schedule.set_override(
            OccurrenceKey::Calendar(Fixture::local("2026-01-02 00:00:00")),
            OccurrenceOverride::Cancel,
            NonZeroUsize::MIN
        ),
        Err(Error::InvalidOccurrenceKey)
    );
}

#[test]
fn timed_duration_remains_exact_across_spring_transition() {
    let placement = Placement::timed(
        Fixture::local("2026-03-29 00:30:00"),
        chrono_tz::Europe::London,
        Duration::hours(MEETING_HOURS),
        ResolutionPolicy::default(),
    )
    .unwrap();
    let (start, end) = placement.bounds(chrono_tz::UTC).unwrap();
    assert_eq!(end.unwrap() - start, Duration::hours(MEETING_HOURS));
    assert_eq!(end, Some(Fixture::instant("2026-03-29T02:30:00Z")));
}

#[test]
fn cancellation_does_not_extend_count_and_query_does_not_restart_count() {
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", chrono_tz::UTC),
        Recurrence::calendar(Fixture::rule(Frequency::Daily, TWO_OCCURRENCES)),
    )
    .unwrap();
    schedule
        .set_override(
            OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00")),
            OccurrenceOverride::Cancel,
            Fixture::budget(),
        )
        .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule
                .occurrences(Fixture::window(
                    "2026-01-02T00:00:00Z",
                    "2026-01-05T00:00:00Z"
                ))
                .unwrap()
        ),
        [Fixture::instant("2026-01-02T09:00:00Z")]
    );
}
