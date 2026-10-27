use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::calendar::{CalendarWindow, DAYS_PER_WEEK};
use schedules_core::{
    CalendarRule, Duration, Error, Frequency, NaiveDate, NaiveDateTime, OccurrenceKey,
    OccurrenceOverride, Placement, Recurrence, ResolutionPolicy, Schedule, ScheduleStore,
    Termination, Tz, Weekday,
};

const WORK_BUDGET: usize = 100;
const RESULT_BUDGET: usize = 10;
const THREE_OCCURRENCES: u32 = 3;
const THREE_ITEMS: usize = 3;
const OVERNIGHT_LENGTH: Duration = Duration::hours(2);
const SPRING_DAY: Duration = Duration::hours(23);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    AllDay,
    Meeting,
    Reminder,
}

struct Fixture;

impl Fixture {
    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }
    fn budget() -> NonZeroUsize {
        NonZeroUsize::new(WORK_BUDGET).unwrap()
    }
    fn results() -> NonZeroUsize {
        NonZeroUsize::new(RESULT_BUDGET).unwrap()
    }
    fn point(text: &str) -> Placement {
        Placement::point(Self::local(text), Tz::UTC, ResolutionPolicy::default()).unwrap()
    }
    fn window(start: &str, end: &str, zone: Tz) -> CalendarWindow {
        CalendarWindow::new(Self::date(start), Self::date(end), zone).unwrap()
    }
}

#[test]
fn core_adapts_timed_all_day_and_point_occurrences_without_cloning_ids() {
    let zone = Tz::Europe__London;
    let window = Fixture::window("2026-03-28", "2026-03-31", zone);
    let mut store = ScheduleStore::new();
    store.insert(
        Item::AllDay,
        Schedule::once(
            Placement::all_day(Fixture::date("2026-03-28"), Fixture::date("2026-03-30")).unwrap(),
        ),
    );
    store.insert(
        Item::Meeting,
        Schedule::once(
            Placement::timed(
                Fixture::local("2026-03-28 23:30:00"),
                zone,
                OVERNIGHT_LENGTH,
                ResolutionPolicy::default(),
            )
            .unwrap(),
        ),
    );
    store.insert(
        Item::Reminder,
        Schedule::once(Fixture::point("2026-03-29 23:00:00")),
    );
    let snapshot = store
        .calendar(
            window,
            Fixture::budget(),
            NonZeroUsize::new(THREE_ITEMS).unwrap(),
        )
        .unwrap();
    assert_eq!(snapshot.entries().len(), THREE_ITEMS);
    let days = snapshot.days().collect::<Result<Vec<_>, _>>().unwrap();
    let ids: Vec<Vec<_>> = days
        .iter()
        .map(|day| {
            day.entries()
                .iter()
                .map(|entry| entry.item().item_id)
                .collect()
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            vec![&Item::AllDay, &Item::Meeting],
            vec![&Item::AllDay, &Item::Meeting],
            vec![&Item::Reminder]
        ]
    );
    let sunday = days
        .iter()
        .find(|day| day.date() == Fixture::date("2026-03-29"))
        .unwrap();
    assert_eq!(
        sunday.window().utc_end() - sunday.window().utc_start(),
        SPRING_DAY
    );
    let all_day = snapshot
        .entries()
        .iter()
        .find(|entry| *entry.item().item_id == Item::AllDay)
        .unwrap();
    assert_eq!(
        all_day.item().occurrence.placement().dates(),
        Some((Fixture::date("2026-03-28"), Fixture::date("2026-03-30")))
    );
}

#[test]
fn calendar_preserves_moved_keys_and_applies_exclusions_and_predicates_before_limits() {
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Count(NonZeroU32::new(THREE_OCCURRENCES).unwrap()),
    )
    .unwrap();
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00"),
        Recurrence::calendar(rule),
    )
    .unwrap();
    let key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    schedule
        .set_override(
            key,
            OccurrenceOverride::Replace(Fixture::point("2026-01-03 10:00:00")),
            Fixture::budget(),
        )
        .unwrap();
    schedule.exclude_date(Fixture::date("2026-01-02"));
    let mut store = ScheduleStore::new();
    store.insert(Item::Meeting, schedule);
    let window = Fixture::window("2026-01-01", "2026-01-04", Tz::UTC);
    let snapshot = store
        .calendar_filtered(
            window,
            Fixture::budget(),
            NonZeroUsize::MIN,
            |item, occurrence| *item == Item::Meeting && occurrence.key() == key,
        )
        .unwrap();
    let days = snapshot.days().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(
        days.iter()
            .take_while(|day| day.date() < Fixture::date("2026-01-03"))
            .all(|day| day.entries().is_empty())
    );
    assert_eq!(
        days.last()
            .unwrap()
            .entries()
            .first()
            .unwrap()
            .item()
            .occurrence
            .key(),
        key
    );
    assert!(matches!(
        store.calendar(window, Fixture::budget(), NonZeroUsize::MIN),
        Err(Error::OccurrenceLimit)
    ));
    let all = store
        .calendar(window, Fixture::budget(), Fixture::results())
        .unwrap();
    assert!(
        all.days()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .iter()
            .find(|day| day.date() == Fixture::date("2026-01-02"))
            .unwrap()
            .entries()
            .is_empty()
    );
}

#[test]
fn shared_expansion_budget_applies_before_any_snapshot_is_returned() {
    let mut store = ScheduleStore::new();
    store.insert(
        Item::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    let window = Fixture::window("2026-01-01", "2026-01-02", Tz::UTC);
    assert!(matches!(
        store.calendar(window, NonZeroUsize::MIN, Fixture::results()),
        Err(Error::CandidateLimit)
    ));
    assert!(matches!(
        store.calendar_filtered(window, NonZeroUsize::MIN, Fixture::results(), |_, _| false),
        Err(Error::CandidateLimit)
    ));
}

#[test]
fn expanded_week_window_fetches_entries_on_padding_dates() {
    let mut store = ScheduleStore::new();
    store.insert(
        Item::Meeting,
        Schedule::once(Fixture::point("2025-12-30 09:00:00")),
    );
    let window = Fixture::window("2026-01-01", "2026-01-02", Tz::UTC);
    assert!(
        store
            .calendar(window, Fixture::budget(), Fixture::results())
            .unwrap()
            .entries()
            .is_empty()
    );
    let snapshot = store
        .calendar(
            window.covering_weeks(Weekday::Mon).unwrap(),
            Fixture::budget(),
            Fixture::results(),
        )
        .unwrap();
    let week = snapshot
        .weeks(Weekday::Mon)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(week.days().iter().flatten().count(), DAYS_PER_WEEK);
    let tuesday = week
        .days()
        .iter()
        .flatten()
        .find(|day| day.date() == Fixture::date("2025-12-30"))
        .unwrap();
    assert_eq!(
        tuesday.entries().first().unwrap().item().item_id,
        &Item::Meeting
    );
}
