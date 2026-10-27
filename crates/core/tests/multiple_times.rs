use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::{
    AmbiguityPolicy, CalendarRule, DateRange, DateTime, Error, Frequency, NaiveDateTime, NaiveTime,
    OccurrenceKey, OccurrenceOverride, Placement, QueryWindow, Recurrence, ResolutionPolicy,
    Schedule, Termination, TimeSet, Tz, Utc, Weekday,
};

const CANDIDATE_BUDGET: usize = 1_000;
const RESULT_BUDGET: usize = 100;
const EIGHT_OCCURRENCES: u32 = 8;
const THREE_OCCURRENCES: u32 = 3;
const FOUR_OCCURRENCES: u32 = 4;
const FORTNIGHT_WEEKS: u32 = 2;

struct Fixture;

impl Fixture {
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn time(text: &str) -> NaiveTime {
        NaiveTime::parse_from_str(text, "%H:%M:%S").unwrap()
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn budget() -> NonZeroUsize {
        NonZeroUsize::new(CANDIDATE_BUDGET).unwrap()
    }

    fn window(start: &str, end: &str) -> QueryWindow {
        QueryWindow {
            range: DateRange::new(Self::instant(start), Self::instant(end)).unwrap(),
            all_day_zone: chrono_tz::UTC,
            candidate_limit: Self::budget(),
            occurrence_limit: NonZeroUsize::new(RESULT_BUDGET).unwrap(),
        }
    }

    fn rule(frequency: Frequency, count: u32, times: &[&str]) -> CalendarRule {
        CalendarRule::new(
            frequency,
            NonZeroU32::MIN,
            Termination::Count(NonZeroU32::new(count).unwrap()),
        )
        .unwrap()
        .with_times(times.iter().map(|time| Self::time(time)))
    }

    fn schedule(anchor: &str, zone: Tz, rule: CalendarRule) -> Schedule {
        let placement =
            Placement::point(Self::local(anchor), zone, ResolutionPolicy::default()).unwrap();
        Schedule::repeating(placement, Recurrence::calendar(rule)).unwrap()
    }

    fn starts(schedule: &Schedule, window: QueryWindow) -> Vec<DateTime<Utc>> {
        schedule
            .occurrences(window)
            .unwrap()
            .iter()
            .map(|item| item.placement().bounds(chrono_tz::UTC).unwrap().0)
            .collect()
    }
}

#[test]
fn arbitrary_times_on_four_weekdays_repeat_every_week() {
    let rule = Fixture::rule(
        Frequency::Weekly,
        EIGHT_OCCURRENCES,
        &["17:00:00", "09:00:00", "09:00:00"],
    )
    .with_weekdays([Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu])
    .unwrap();
    assert_eq!(
        rule.times().iter().collect::<Vec<_>>(),
        [Fixture::time("09:00:00"), Fixture::time("17:00:00")]
    );
    let schedule = Fixture::schedule("2026-01-05 09:00:00", chrono_tz::UTC, rule);
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-05T00:00:00Z", "2026-01-12T00:00:00Z")
        ),
        [
            "2026-01-05T09:00:00Z",
            "2026-01-05T17:00:00Z",
            "2026-01-06T09:00:00Z",
            "2026-01-06T17:00:00Z",
            "2026-01-07T09:00:00Z",
            "2026-01-07T17:00:00Z",
            "2026-01-08T09:00:00Z",
            "2026-01-08T17:00:00Z",
        ]
        .map(Fixture::instant)
    );
    let rule = Fixture::rule(
        Frequency::Weekly,
        THREE_OCCURRENCES,
        &["08:00:00", "12:30:00", "18:00:00"],
    )
    .with_weekdays([Weekday::Mon])
    .unwrap();
    let schedule = Fixture::schedule("2026-01-05 08:00:00", chrono_tz::UTC, rule);
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-05T00:00:00Z", "2026-01-06T00:00:00Z")
        ),
        [
            "2026-01-05T08:00:00Z",
            "2026-01-05T12:30:00Z",
            "2026-01-05T18:00:00Z"
        ]
        .map(Fixture::instant)
    );
}

#[test]
fn interval_weeks_and_anchor_cutoff_apply_to_each_time() {
    let rule = CalendarRule::new(
        Frequency::Weekly,
        NonZeroU32::new(FORTNIGHT_WEEKS).unwrap(),
        Termination::Count(NonZeroU32::new(THREE_OCCURRENCES).unwrap()),
    )
    .unwrap()
    .with_times([Fixture::time("09:00:00"), Fixture::time("17:00:00")])
    .with_weekdays([Weekday::Mon])
    .unwrap();
    let schedule = Fixture::schedule("2026-01-05 17:00:00", chrono_tz::UTC, rule);
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-05T00:00:00Z", "2026-02-01T00:00:00Z")
        ),
        [
            "2026-01-05T17:00:00Z",
            "2026-01-19T09:00:00Z",
            "2026-01-19T17:00:00Z"
        ]
        .map(Fixture::instant)
    );
}

#[test]
fn spring_gap_skips_one_time_without_skipping_other_times_or_consuming_count() {
    let schedule = Fixture::schedule(
        "2026-03-28 01:30:00",
        chrono_tz::Europe::London,
        Fixture::rule(
            Frequency::Daily,
            FOUR_OCCURRENCES,
            &["01:30:00", "03:00:00"],
        ),
    );
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-03-28T00:00:00Z", "2026-04-01T00:00:00Z")
        ),
        [
            "2026-03-28T01:30:00Z",
            "2026-03-28T03:00:00Z",
            "2026-03-29T02:00:00Z",
            "2026-03-30T00:30:00Z"
        ]
        .map(Fixture::instant)
    );
}

#[test]
fn ambiguity_policy_resolves_each_selected_time_and_until_is_inclusive() {
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Until(Fixture::local("2026-10-25 01:45:00")),
    )
    .unwrap()
    .with_times([
        Fixture::time("01:30:00"),
        Fixture::time("01:45:00"),
        Fixture::time("03:00:00"),
    ]);
    let anchor = Placement::point(
        Fixture::local("2026-10-24 03:00:00"),
        chrono_tz::Europe::London,
        ResolutionPolicy::new(AmbiguityPolicy::Later),
    )
    .unwrap();
    let schedule = Schedule::repeating(anchor, Recurrence::calendar(rule)).unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-10-24T00:00:00Z", "2026-10-26T00:00:00Z")
        ),
        [
            "2026-10-24T02:00:00Z",
            "2026-10-25T01:30:00Z",
            "2026-10-25T01:45:00Z"
        ]
        .map(Fixture::instant)
    );
}

#[test]
fn individual_times_can_be_cancelled_or_moved_and_limits_count_each_item() {
    let mut schedule = Fixture::schedule(
        "2026-01-05 09:00:00",
        chrono_tz::UTC,
        Fixture::rule(
            Frequency::Daily,
            THREE_OCCURRENCES,
            &["09:00:00", "12:00:00", "17:00:00"],
        ),
    );
    let noon = OccurrenceKey::Calendar(Fixture::local("2026-01-05 12:00:00"));
    schedule
        .set_override(noon, OccurrenceOverride::Cancel, Fixture::budget())
        .unwrap();
    let evening = OccurrenceKey::Calendar(Fixture::local("2026-01-05 17:00:00"));
    let moved = Placement::point(
        Fixture::local("2026-02-01 10:00:00"),
        chrono_tz::UTC,
        ResolutionPolicy::default(),
    )
    .unwrap();
    schedule
        .set_override(
            evening,
            OccurrenceOverride::Replace(moved),
            Fixture::budget(),
        )
        .unwrap();
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-05T00:00:00Z", "2026-01-07T00:00:00Z")
        ),
        [Fixture::instant("2026-01-05T09:00:00Z")]
    );
    let items = schedule
        .occurrences(Fixture::window(
            "2026-02-01T00:00:00Z",
            "2026-02-02T00:00:00Z",
        ))
        .unwrap();
    assert_eq!(items.first().unwrap().key(), evening);
    let regular = Fixture::schedule(
        "2026-01-05 09:00:00",
        chrono_tz::UTC,
        Fixture::rule(
            Frequency::Daily,
            THREE_OCCURRENCES,
            &["09:00:00", "12:00:00", "17:00:00"],
        ),
    );
    let mut window = Fixture::window("2026-01-05T00:00:00Z", "2026-01-06T00:00:00Z");
    window.occurrence_limit = NonZeroUsize::MIN;
    assert_eq!(regular.occurrences(window), Err(Error::OccurrenceLimit));
    window.occurrence_limit = NonZeroUsize::new(RESULT_BUDGET).unwrap();
    window.candidate_limit = NonZeroUsize::MIN;
    let mut iter = regular.iter(window).unwrap();
    assert!(iter.next().unwrap().is_ok());
    assert_eq!(iter.next(), Some(Err(Error::CandidateLimit)));
    assert_eq!(iter.next(), None);
}

#[test]
fn explicit_times_reject_all_day_and_mismatched_anchor_but_empty_inherits_anchor() {
    let anchor = Fixture::local("2026-01-05 09:00:00");
    let rule = Fixture::rule(Frequency::Daily, THREE_OCCURRENCES, &["12:00:00"]);
    let point = Placement::point(anchor, chrono_tz::UTC, ResolutionPolicy::default()).unwrap();
    assert_eq!(
        Schedule::repeating(point, Recurrence::calendar(rule.clone())),
        Err(Error::InvalidAnchor)
    );
    let all_day = Placement::all_day(anchor.date(), anchor.date().succ_opt().unwrap()).unwrap();
    assert_eq!(
        Schedule::repeating(all_day, Recurrence::calendar(rule)),
        Err(Error::AllDayTimes)
    );
    let schedule = Fixture::schedule(
        "2026-01-05 09:00:00",
        chrono_tz::UTC,
        Fixture::rule(Frequency::Daily, THREE_OCCURRENCES, &[]),
    );
    assert_eq!(
        Fixture::starts(
            &schedule,
            Fixture::window("2026-01-05T00:00:00Z", "2026-01-08T00:00:00Z")
        ),
        [
            "2026-01-05T09:00:00Z",
            "2026-01-06T09:00:00Z",
            "2026-01-07T09:00:00Z"
        ]
        .map(Fixture::instant)
    );
}

#[test]
fn time_set_insertion_is_unique_sorted_and_preserves_subsecond_precision() {
    let mut times = TimeSet::default();
    let morning = Fixture::time("09:00:00");
    let fractional = NaiveTime::parse_from_str("09:00:00.123456789", "%H:%M:%S%.f").unwrap();
    assert!(times.insert(fractional));
    assert!(times.insert(morning));
    assert!(!times.insert(morning));
    assert!(times.contains(fractional));
    assert_eq!(times.iter().collect::<Vec<_>>(), [morning, fractional]);
}

#[test]
fn time_set_order_and_equality_ignore_insertion_history_and_clones_are_independent() {
    let ordered = ["00:00:00", "09:00:00", "12:00:00", "17:00:00", "23:59:59"].map(Fixture::time);
    let mut ascending = TimeSet::default();
    let mut descending = TimeSet::default();
    assert!(ascending.is_empty());
    for time in ordered {
        assert!(ascending.insert(time));
    }
    for time in ordered.into_iter().rev() {
        assert!(descending.insert(time));
    }
    assert_eq!(ascending, descending);
    assert_eq!(ascending.iter().collect::<Vec<_>>(), ordered);
    assert_eq!(
        ascending.iter().rev().collect::<Vec<_>>(),
        ordered.into_iter().rev().collect::<Vec<_>>()
    );
    for time in ordered {
        assert!(!descending.insert(time));
        assert!(descending.contains(time));
    }
    let between = Fixture::time("10:30:00");
    let mut independent = ascending.clone();
    assert!(independent.insert(between));
    assert!(!ascending.contains(between));
    assert_ne!(ascending, independent);
}
