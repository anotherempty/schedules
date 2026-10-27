use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::{
    CalendarRule, DateRange, DateTime, Duration, Error, Frequency, NaiveDate, NaiveDateTime,
    NaiveTime, Occurrence, OccurrenceKey, OccurrenceOverride, Placement, QueryWindow, Recurrence,
    ResolutionPolicy, Schedule, ScheduleStore, Termination, Tz, Utc,
};

const WORK_BUDGET: usize = 100;
const RESULT_BUDGET: usize = 20;
const THREE_OCCURRENCES: u32 = 3;
const FOUR_OCCURRENCES: u32 = 4;
const TWO_ACCEPTED_RESULTS: usize = 2;
const FIRST_CANDIDATE_WORK: usize = 1;
const APPOINTMENT: Duration = Duration::hours(1);
const OVERNIGHT: Duration = Duration::hours(3);
const DAILY_CADENCE: Duration = Duration::days(1);

// Filters and results borrow identity; no Clone requirement.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Meeting,
    Reminder,
}

struct Fixture;

impl Fixture {
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn utc(text: &str) -> DateTime<Utc> {
        Self::local(text).and_utc()
    }

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn point(text: &str, zone: Tz) -> Placement {
        Placement::point(Self::local(text), zone, ResolutionPolicy::default()).unwrap()
    }

    fn timed(text: &str, duration: Duration, zone: Tz) -> Placement {
        Placement::timed(
            Self::local(text),
            zone,
            duration,
            ResolutionPolicy::default(),
        )
        .unwrap()
    }

    fn budget() -> NonZeroUsize {
        NonZeroUsize::new(WORK_BUDGET).unwrap()
    }

    fn count(count: u32) -> Termination {
        Termination::Count(NonZeroU32::new(count).unwrap())
    }

    fn calendar(count: u32) -> Schedule {
        let rule =
            CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Self::count(count)).unwrap();
        Schedule::repeating(
            Self::point("2026-01-01 09:00:00", Tz::UTC),
            Recurrence::calendar(rule),
        )
        .unwrap()
    }

    fn window() -> QueryWindow {
        QueryWindow {
            range: DateRange::new(
                Self::utc("2026-01-01 00:00:00"),
                Self::utc("2026-01-06 00:00:00"),
            )
            .unwrap(),
            all_day_zone: Tz::UTC,
            candidate_limit: Self::budget(),
            occurrence_limit: NonZeroUsize::new(RESULT_BUDGET).unwrap(),
        }
    }

    fn dates(occurrences: &[Occurrence]) -> Vec<NaiveDate> {
        occurrences
            .iter()
            .map(|occurrence| occurrence.placement().local_start().date())
            .collect()
    }
}

#[test]
fn holiday_exclusions_preserve_count_and_can_be_removed_without_shared_clone_state() {
    let original = Fixture::calendar(THREE_OCCURRENCES);
    let mut schedule = original.clone();
    let holiday = Fixture::date("2026-01-02");
    assert!(schedule.exclude_date(holiday));
    assert!(!schedule.exclude_date(holiday));
    assert_ne!(schedule, original);
    assert_eq!(
        Fixture::dates(&schedule.occurrences(Fixture::window()).unwrap()),
        ["2026-01-01", "2026-01-03"].map(Fixture::date)
    );
    assert!(original.excluded_dates().is_empty());
    assert_eq!(
        schedule
            .excluded_dates()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![holiday]
    );
    assert!(schedule.include_date(holiday));
    assert!(!schedule.include_date(holiday));
    assert_eq!(schedule, original);
}

#[test]
fn date_exclusion_removes_every_selected_time_without_extending_count() {
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Fixture::count(FOUR_OCCURRENCES),
    )
    .unwrap()
    .with_times(
        ["09:00:00", "17:00:00"].map(|text| NaiveTime::parse_from_str(text, "%H:%M:%S").unwrap()),
    );
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00", Tz::UTC),
        Recurrence::calendar(rule),
    )
    .unwrap();
    schedule.exclude_date(Fixture::date("2026-01-01"));
    let results = schedule.occurrences(Fixture::window()).unwrap();
    assert_eq!(
        results
            .iter()
            .map(|entry| entry.placement().local_start())
            .collect::<Vec<_>>(),
        ["2026-01-02 09:00:00", "2026-01-02 17:00:00"].map(Fixture::local)
    );
}

#[test]
fn exclusions_use_local_start_date_not_utc_date_or_days_spanned() {
    let holiday = Fixture::date("2026-01-02");
    let mut on_holiday = Schedule::once(Fixture::point("2026-01-02 01:00:00", Tz::Africa__Nairobi));
    on_holiday.exclude_date(holiday);
    assert!(
        on_holiday
            .occurrences(Fixture::window())
            .unwrap()
            .is_empty()
    );
    let mut overnight = Schedule::once(Fixture::timed(
        "2026-01-01 23:00:00",
        OVERNIGHT,
        Tz::Africa__Nairobi,
    ));
    overnight.exclude_date(holiday);
    assert_eq!(
        Fixture::dates(&overnight.occurrences(Fixture::window()).unwrap()),
        vec![Fixture::date("2026-01-01")]
    );
    let mut all_day =
        Schedule::once(Placement::all_day(holiday, Fixture::date("2026-01-03")).unwrap());
    all_day.exclude_date(holiday);
    for zone in [Tz::America__New_York, Tz::Africa__Nairobi] {
        assert!(
            all_day
                .occurrences(QueryWindow {
                    all_day_zone: zone,
                    ..Fixture::window()
                })
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn exclusions_apply_after_moves_and_do_not_prevent_override_membership() {
    let mut schedule = Fixture::calendar(THREE_OCCURRENCES);
    let holiday = Fixture::date("2026-01-02");
    schedule.exclude_date(holiday);
    let holiday_key = OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"));
    let first_key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    let replacement = Fixture::point("2026-01-04 10:00:00", Tz::UTC);
    schedule
        .set_override(
            holiday_key,
            OccurrenceOverride::Replace(replacement),
            Fixture::budget(),
        )
        .unwrap();
    schedule
        .set_override(
            first_key,
            OccurrenceOverride::Replace(Fixture::point("2026-01-02 11:00:00", Tz::UTC)),
            Fixture::budget(),
        )
        .unwrap();
    let results = schedule.occurrences(Fixture::window()).unwrap();
    assert_eq!(
        Fixture::dates(&results),
        ["2026-01-03", "2026-01-04"].map(Fixture::date)
    );
    assert_eq!(results.last().unwrap().key(), holiday_key);
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, schedule.clone());
    assert_eq!(
        store.next_after(&Item::Meeting, first_key, Tz::UTC, Fixture::budget()),
        Err(Error::InvalidOccurrenceKey)
    );
    assert!(
        store
            .next_after(&Item::Meeting, holiday_key, Tz::UTC, Fixture::budget())
            .unwrap()
            .is_none()
    );
    schedule.remove_override(holiday_key);
    assert_eq!(
        Fixture::dates(&schedule.occurrences(Fixture::window()).unwrap()),
        vec![Fixture::date("2026-01-03")]
    );
    schedule.include_date(holiday);
    assert_eq!(
        Fixture::dates(&schedule.occurrences(Fixture::window()).unwrap()),
        ["2026-01-02", "2026-01-02", "2026-01-03"].map(Fixture::date)
    );
}

#[test]
fn predicates_run_after_window_and_exclusions_before_result_limits() {
    let mut schedule = Fixture::calendar(FOUR_OCCURRENCES);
    schedule.exclude_date(Fixture::date("2026-01-02"));
    let window = QueryWindow {
        range: DateRange::new(
            Fixture::utc("2026-01-02 00:00:00"),
            Fixture::utc("2026-01-06 00:00:00"),
        )
        .unwrap(),
        occurrence_limit: NonZeroUsize::MIN,
        ..Fixture::window()
    };
    let mut inspected = Vec::new();
    let results = schedule
        .occurrences_filtered(window, |occurrence| {
            let date = occurrence.placement().local_start().date();
            inspected.push(date);
            date == Fixture::date("2026-01-04")
        })
        .unwrap();
    assert_eq!(Fixture::dates(&results), vec![Fixture::date("2026-01-04")]);
    assert_eq!(inspected, ["2026-01-03", "2026-01-04"].map(Fixture::date));
    let holidays = [Fixture::date("2026-01-03")];
    let lazy = schedule
        .iter_filtered(window, |entry| {
            !holidays.contains(&entry.placement().local_start().date())
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(lazy, results);
}

#[test]
fn rejected_candidates_still_exhaust_work_and_iterator_stays_finished() {
    let schedule = Fixture::calendar(THREE_OCCURRENCES);
    let window = QueryWindow {
        candidate_limit: NonZeroUsize::new(FIRST_CANDIDATE_WORK).unwrap(),
        ..Fixture::window()
    };
    let mut iter = schedule.iter_filtered(window, |_| false).unwrap();
    assert_eq!(iter.next(), Some(Err(Error::CandidateLimit)));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next(), None);
    let mut excluded = schedule;
    excluded.exclude_date(Fixture::date("2026-01-01"));
    assert_eq!(excluded.occurrences(window), Err(Error::CandidateLimit));
}

#[test]
fn store_filters_apply_before_per_schedule_and_shared_result_limits() {
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, Fixture::calendar(FOUR_OCCURRENCES));
    store.insert(
        Item::Reminder,
        Schedule::once(Fixture::point("2026-01-04 10:00:00", Tz::UTC)),
    );
    let window = QueryWindow {
        occurrence_limit: NonZeroUsize::MIN,
        ..Fixture::window()
    };
    let results = store
        .occurrences_filtered(window, |item, _| *item == Item::Reminder)
        .unwrap();
    assert_eq!(
        results
            .iter()
            .map(|entry| entry.item_id)
            .collect::<Vec<_>>(),
        vec![&Item::Reminder]
    );
    assert_eq!(
        store.occurrences_filtered(window, |_, _| true),
        Err(Error::OccurrenceLimit)
    );
    assert_eq!(
        store.occurrences_filtered(
            QueryWindow {
                candidate_limit: NonZeroUsize::MIN,
                ..window
            },
            |_, _| false
        ),
        Err(Error::CandidateLimit)
    );
}

#[test]
fn analysis_ignores_stored_exclusions_and_predicate_rejections() {
    let mut store = ScheduleStore::new();
    let mut meeting = Schedule::once(Fixture::timed("2026-01-02 09:00:00", APPOINTMENT, Tz::UTC));
    meeting.exclude_date(Fixture::date("2026-01-02"));
    store.insert(Item::Meeting, meeting);
    store.insert(
        Item::Reminder,
        Schedule::once(Fixture::timed("2026-01-02 09:00:00", APPOINTMENT, Tz::UTC)),
    );
    let window = Fixture::window();
    let analysis = store.analyze(window).unwrap();
    assert_eq!(analysis.density().occupied(), APPOINTMENT);
    assert!(analysis.overlaps().next().is_none());
    let filtered = store
        .analyze_filtered(window, |item, _| *item == Item::Meeting)
        .unwrap();
    assert_eq!(filtered.gaps(), &[window.range]);
    assert_eq!(filtered.density().occupied(), Duration::zero());
}

#[test]
fn successors_honor_exclusions_predicates_and_count_without_resurrecting_anchors() {
    let mut schedule = Fixture::calendar(FOUR_OCCURRENCES);
    schedule.exclude_date(Fixture::date("2026-01-02"));
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, schedule);
    let key = |text| OccurrenceKey::Calendar(Fixture::local(text));
    let first = key("2026-01-01 09:00:00");
    let excluded = key("2026-01-02 09:00:00");
    let last = key("2026-01-04 09:00:00");
    assert_eq!(
        store
            .next_after(&Item::Meeting, first, Tz::UTC, Fixture::budget())
            .unwrap()
            .unwrap()
            .occurrence
            .key(),
        key("2026-01-03 09:00:00")
    );
    assert_eq!(
        store.next_after(&Item::Meeting, excluded, Tz::UTC, Fixture::budget()),
        Err(Error::InvalidOccurrenceKey)
    );
    let next = store
        .next_after_filtered(
            &Item::Meeting,
            first,
            Tz::UTC,
            Fixture::budget(),
            |_, occurrence| {
                occurrence.placement().local_start().date() != Fixture::date("2026-01-03")
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.occurrence.key(), last);
    assert_eq!(
        store.next_after_filtered(&Item::Meeting, first, Tz::UTC, Fixture::budget(), |_, _| {
            false
        }),
        Err(Error::InvalidOccurrenceKey)
    );
    assert!(
        store
            .next_after(&Item::Meeting, last, Tz::UTC, Fixture::budget())
            .unwrap()
            .is_none()
    );
    let filtered = store
        .occurrences_filtered(Fixture::window(), |_, occurrence| {
            occurrence.placement().local_start().date() != Fixture::date("2026-01-03")
        })
        .unwrap();
    assert_eq!(filtered.len(), TWO_ACCEPTED_RESULTS);
    assert_eq!(filtered.last().unwrap(), &next);
}

#[test]
fn elapsed_exclusions_preserve_utc_keys_and_original_count_when_seeking() {
    let start = Fixture::utc("2026-01-01 09:00:00");
    let recurrence = Recurrence::elapsed(DAILY_CADENCE, Fixture::count(THREE_OCCURRENCES)).unwrap();
    let mut schedule =
        Schedule::repeating(Placement::instant(start, Tz::UTC).unwrap(), recurrence).unwrap();
    schedule.exclude_date(Fixture::date("2026-01-02"));
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, schedule);
    let next = store
        .next_after(
            &Item::Meeting,
            OccurrenceKey::Elapsed(start),
            Tz::UTC,
            Fixture::budget(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        next.occurrence.key(),
        OccurrenceKey::Elapsed(Fixture::utc("2026-01-03 09:00:00"))
    );
    assert!(
        store
            .next_after(
                &Item::Meeting,
                next.occurrence.key(),
                Tz::UTC,
                Fixture::budget()
            )
            .unwrap()
            .is_none()
    );
}

#[test]
fn endless_rejected_successors_return_budget_error_not_false_exhaustion() {
    let start = Fixture::utc("2026-01-01 09:00:00");
    let key = OccurrenceKey::Elapsed(start);
    let recurrence = Recurrence::elapsed(DAILY_CADENCE, Termination::Forever).unwrap();
    let schedule =
        Schedule::repeating(Placement::instant(start, Tz::UTC).unwrap(), recurrence).unwrap();
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, schedule);
    assert_eq!(
        store.next_after_filtered(
            &Item::Meeting,
            key,
            Tz::UTC,
            Fixture::budget(),
            |_, occurrence| occurrence.key() == key
        ),
        Err(Error::CandidateLimit)
    );
}
