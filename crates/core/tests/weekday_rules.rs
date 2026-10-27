use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::{
    CalendarRule, DateRange, DateTime, Duration, Error, Frequency, NaiveDateTime, NaiveTime,
    OccurrenceKey, OccurrenceOverride, Placement, QueryWindow, Recurrence, ResolutionPolicy,
    Schedule, Scheduler, Termination, Tz, Utc, Weekday,
};

const WORK_BUDGET: usize = 5_000;
const RESULT_BUDGET: usize = 100;
const ALTERNATING_PERIODS: u32 = 2;
const THREE_OCCURRENCES: u32 = 3;
const FOUR_OCCURRENCES: u32 = 4;
const FIVE_OCCURRENCES: u32 = 5;
const UNEVEN_CADENCE: Duration = Duration::hours(7);
const HOURLY_CADENCE: Duration = Duration::hours(1);

struct Fixture;

impl Fixture {
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn utc(text: &str) -> DateTime<Utc> {
        Self::local(text).and_utc()
    }

    fn point(text: &str, zone: Tz) -> Placement {
        Placement::point(Self::local(text), zone, ResolutionPolicy::default()).unwrap()
    }

    fn budget() -> NonZeroUsize {
        NonZeroUsize::new(WORK_BUDGET).unwrap()
    }

    fn count(count: u32) -> Termination {
        Termination::Count(NonZeroU32::new(count).unwrap())
    }

    fn window(start: &str, end: &str) -> QueryWindow {
        QueryWindow {
            range: DateRange::new(Self::utc(start), Self::utc(end)).unwrap(),
            all_day_zone: Tz::UTC,
            candidate_limit: Self::budget(),
            occurrence_limit: NonZeroUsize::new(RESULT_BUDGET).unwrap(),
        }
    }

    fn starts(schedule: &Schedule, window: QueryWindow) -> Vec<DateTime<Utc>> {
        schedule
            .occurrences(window)
            .unwrap()
            .into_iter()
            .map(|occurrence| {
                occurrence
                    .placement()
                    .bounds(window.all_day_zone)
                    .unwrap()
                    .0
            })
            .collect()
    }
}

#[test]
fn daily_filter_preserves_every_other_day_cadence_and_counts_only_matches() {
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::new(ALTERNATING_PERIODS).unwrap(),
        Fixture::count(THREE_OCCURRENCES),
    )
    .unwrap()
    .with_weekdays([Weekday::Fri, Weekday::Mon]);
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-02 09:00:00", Tz::UTC),
        Recurrence::calendar(rule),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-01 00:00:00", "2026-02-01 00:00:00")
        ),
        [
            "2026-01-02 09:00:00",
            "2026-01-12 09:00:00",
            "2026-01-16 09:00:00"
        ]
        .map(Fixture::utc)
    );
}

#[test]
fn monthly_and_yearly_expand_weekdays_inside_matching_periods() {
    let cases = [
        (
            Frequency::Monthly,
            Weekday::Thu,
            "2026-01-29 09:00:00",
            "2026-03-13 00:00:00",
            [
                "2026-01-29 09:00:00",
                "2026-03-05 09:00:00",
                "2026-03-12 09:00:00",
            ],
        ),
        (
            Frequency::Yearly,
            Weekday::Fri,
            "2026-12-25 09:00:00",
            "2028-01-15 00:00:00",
            [
                "2026-12-25 09:00:00",
                "2028-01-07 09:00:00",
                "2028-01-14 09:00:00",
            ],
        ),
    ];
    for (frequency, weekday, anchor, end, expected) in cases {
        let rule = CalendarRule::new(
            frequency,
            NonZeroU32::new(ALTERNATING_PERIODS).unwrap(),
            Termination::Until(Fixture::local(expected.last().unwrap())),
        )
        .unwrap()
        .with_weekdays([weekday, weekday]);
        let schedule =
            Schedule::repeating(Fixture::point(anchor, Tz::UTC), Recurrence::calendar(rule))
                .unwrap();
        assert_eq!(
            Fixture::starts(&schedule, Fixture::window(anchor, end)),
            expected.map(Fixture::utc),
            "{frequency:?}"
        );
    }
}

#[test]
fn monthly_selected_times_skip_dst_gaps_without_consuming_count() {
    let times =
        ["01:30:00", "03:30:00"].map(|time| NaiveTime::parse_from_str(time, "%H:%M:%S").unwrap());
    let rule = CalendarRule::new(
        Frequency::Monthly,
        NonZeroU32::MIN,
        Fixture::count(FIVE_OCCURRENCES),
    )
    .unwrap()
    .with_weekdays([Weekday::Sun])
    .with_times(times);
    let schedule = Schedule::repeating(
        Fixture::point("2026-03-22 01:30:00", Tz::Europe__London),
        Recurrence::calendar(rule),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-03-01 00:00:00", "2026-04-06 00:00:00")
        ),
        [
            "2026-03-22 01:30:00",
            "2026-03-22 03:30:00",
            "2026-03-29 02:30:00",
            "2026-04-05 00:30:00",
            "2026-04-05 02:30:00"
        ]
        .map(Fixture::utc)
    );
}

#[test]
fn empty_selection_restores_defaults_and_mismatched_anchors_are_rejected() {
    for frequency in [
        Frequency::Daily,
        Frequency::Weekly,
        Frequency::Monthly,
        Frequency::Yearly,
    ] {
        let default = CalendarRule::new(
            frequency,
            NonZeroU32::MIN,
            Fixture::count(THREE_OCCURRENCES),
        )
        .unwrap();
        assert_eq!(
            default
                .clone()
                .with_weekdays([Weekday::Mon])
                .with_weekdays([]),
            default
        );
        let recurrence = Recurrence::calendar(default).with_weekdays([Weekday::Mon]);
        assert_eq!(
            Schedule::repeating(Fixture::point("2026-01-02 09:00:00", Tz::UTC), recurrence),
            Err(Error::InvalidAnchor)
        );
    }
    let default = Recurrence::elapsed(HOURLY_CADENCE, Termination::Forever).unwrap();
    assert_eq!(
        default
            .clone()
            .with_weekdays([Weekday::Mon])
            .with_weekdays([]),
        default
    );
    assert_eq!(
        Schedule::repeating(
            Fixture::point("2026-01-02 09:00:00", Tz::UTC),
            default.with_weekdays([Weekday::Mon])
        ),
        Err(Error::InvalidAnchor)
    );
}

#[test]
fn elapsed_filter_uses_local_weekday_and_preserves_cadence_count_and_successors() {
    let recurrence = Recurrence::elapsed(UNEVEN_CADENCE, Fixture::count(FOUR_OCCURRENCES))
        .unwrap()
        .with_weekdays([Weekday::Fri, Weekday::Mon]);
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-02 23:00:00", Tz::Africa__Nairobi),
        recurrence,
    )
    .unwrap();
    let window = Fixture::window("2026-01-02 00:00:00", "2026-01-10 00:00:00");
    let expected = [
        "2026-01-02 20:00:00",
        "2026-01-04 21:00:00",
        "2026-01-05 04:00:00",
        "2026-01-05 11:00:00",
    ]
    .map(Fixture::utc);
    assert_eq!(Fixture::starts(&schedule, window), expected);
    let mut scheduler = Scheduler::new();
    scheduler.insert(Weekday::Fri.num_days_from_monday(), schedule);
    let item = Weekday::Fri.num_days_from_monday();
    for (anchor, next) in expected.iter().zip(expected.iter().skip(1)) {
        let actual = scheduler
            .next_after(
                &item,
                OccurrenceKey::Elapsed(*anchor),
                Tz::UTC,
                Fixture::budget(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(actual.occurrence.key(), OccurrenceKey::Elapsed(*next));
    }
    assert!(
        scheduler
            .next_after(
                &item,
                OccurrenceKey::Elapsed(*expected.last().unwrap()),
                Tz::UTC,
                Fixture::budget()
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        scheduler.next_after(
            &item,
            OccurrenceKey::Elapsed(Fixture::utc("2026-01-03 03:00:00")),
            Tz::UTC,
            Fixture::budget()
        ),
        Err(Error::InvalidOccurrenceKey)
    );
}

#[test]
fn elapsed_weekday_filters_keep_both_fold_instants_and_skip_spring_wall_gap() {
    for (anchor, end, expected) in [
        (
            "2026-03-29 00:30:00",
            "2026-03-30 00:00:00",
            [
                "2026-03-29 00:30:00",
                "2026-03-29 01:30:00",
                "2026-03-29 02:30:00",
            ],
        ),
        (
            "2026-10-25 00:30:00",
            "2026-10-26 00:00:00",
            [
                "2026-10-25 00:30:00",
                "2026-10-25 01:30:00",
                "2026-10-25 02:30:00",
            ],
        ),
    ] {
        let placement = Placement::instant(Fixture::utc(anchor), Tz::Europe__London).unwrap();
        let recurrence = Recurrence::elapsed(HOURLY_CADENCE, Fixture::count(THREE_OCCURRENCES))
            .unwrap()
            .with_weekdays([Weekday::Sun]);
        let schedule = Schedule::repeating(placement, recurrence).unwrap();
        assert_eq!(
            Fixture::starts(&schedule, Fixture::window(anchor, end)),
            expected.map(Fixture::utc)
        );
    }
}

#[test]
fn filtered_elapsed_overrides_preserve_count_and_reject_excluded_keys() {
    let recurrence = Recurrence::elapsed(UNEVEN_CADENCE, Fixture::count(THREE_OCCURRENCES))
        .unwrap()
        .with_weekdays([Weekday::Fri, Weekday::Mon]);
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-02 23:00:00", Tz::Africa__Nairobi),
        recurrence,
    )
    .unwrap();
    let anchor = OccurrenceKey::Elapsed(Fixture::utc("2026-01-02 20:00:00"));
    schedule
        .set_override(anchor, OccurrenceOverride::Cancel, Fixture::budget())
        .unwrap();
    let monday = OccurrenceKey::Elapsed(Fixture::utc("2026-01-04 21:00:00"));
    let replacement = Fixture::point("2026-01-03 10:00:00", Tz::UTC);
    schedule
        .set_override(
            monday,
            OccurrenceOverride::Replace(replacement),
            Fixture::budget(),
        )
        .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-02 00:00:00", "2026-01-10 00:00:00")
        ),
        ["2026-01-03 10:00:00", "2026-01-05 04:00:00"].map(Fixture::utc)
    );
    assert_eq!(
        schedule.set_override(
            OccurrenceKey::Elapsed(Fixture::utc("2026-01-03 03:00:00")),
            OccurrenceOverride::Cancel,
            Fixture::budget()
        ),
        Err(Error::InvalidOccurrenceKey)
    );
}

#[test]
fn filtered_candidates_consume_work_but_not_result_budget() {
    let recurrence = Recurrence::elapsed(
        UNEVEN_CADENCE,
        Termination::UntilInstant(Fixture::utc("2026-01-05 04:00:00")),
    )
    .unwrap()
    .with_weekdays([Weekday::Fri, Weekday::Mon]);
    let schedule = Schedule::repeating(
        Fixture::point("2026-01-02 23:00:00", Tz::Africa__Nairobi),
        recurrence,
    )
    .unwrap();
    let window = Fixture::window("2026-01-04 21:00:00", "2026-01-06 00:00:00");
    assert_eq!(
        schedule.occurrences(QueryWindow {
            candidate_limit: NonZeroUsize::MIN,
            ..window
        }),
        Err(Error::CandidateLimit)
    );
    assert_eq!(
        Fixture::starts(&schedule, window),
        ["2026-01-04 21:00:00", "2026-01-05 04:00:00"].map(Fixture::utc)
    );
}

#[test]
fn all_day_monthly_weekdays_expand_without_inheriting_anchor_day() {
    let start = Fixture::local("2026-01-29 00:00:00").date();
    let end = Fixture::local("2026-01-30 00:00:00").date();
    let rule = CalendarRule::new(
        Frequency::Monthly,
        NonZeroU32::MIN,
        Fixture::count(THREE_OCCURRENCES),
    )
    .unwrap()
    .with_weekdays([Weekday::Thu]);
    let schedule = Schedule::repeating(
        Placement::all_day(start, end).unwrap(),
        Recurrence::calendar(rule),
    )
    .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-01 00:00:00", "2026-03-01 00:00:00")
        ),
        [
            "2026-01-29 00:00:00",
            "2026-02-05 00:00:00",
            "2026-02-12 00:00:00"
        ]
        .map(Fixture::utc)
    );
}

#[test]
fn successor_seeking_matches_expansion_for_filtered_calendar_and_elapsed_rules() {
    let anchor = Fixture::point("2026-01-02 09:00:00", Tz::UTC);
    let cutoff = Fixture::local("2026-01-20 09:00:00");
    let mut recurrences = Vec::new();
    for frequency in [
        Frequency::Daily,
        Frequency::Weekly,
        Frequency::Monthly,
        Frequency::Yearly,
    ] {
        for termination in [
            Termination::Forever,
            Termination::Until(cutoff),
            Fixture::count(THREE_OCCURRENCES),
        ] {
            recurrences.push(
                Recurrence::calendar(
                    CalendarRule::new(
                        frequency,
                        NonZeroU32::new(ALTERNATING_PERIODS).unwrap(),
                        termination,
                    )
                    .unwrap(),
                )
                .with_weekdays([Weekday::Mon, Weekday::Fri]),
            );
        }
    }
    for termination in [
        Termination::Forever,
        Termination::UntilInstant(cutoff.and_utc()),
        Fixture::count(THREE_OCCURRENCES),
    ] {
        recurrences.push(
            Recurrence::elapsed(UNEVEN_CADENCE, termination)
                .unwrap()
                .with_weekdays([Weekday::Mon, Weekday::Fri]),
        );
    }
    for recurrence in recurrences {
        let schedule = Schedule::repeating(anchor, recurrence).unwrap();
        let mut scheduler = Scheduler::new();
        scheduler.insert((), schedule);
        let results = scheduler
            .occurrences(Fixture::window(
                "2026-01-02 00:00:00",
                "2026-02-01 00:00:00",
            ))
            .unwrap();
        assert!(!results.is_empty());
        for (current, expected) in results.iter().zip(results.iter().skip(1)) {
            assert_eq!(
                scheduler
                    .next_after(&(), current.occurrence.key(), Tz::UTC, Fixture::budget())
                    .unwrap(),
                Some(*expected)
            );
        }
    }
}
