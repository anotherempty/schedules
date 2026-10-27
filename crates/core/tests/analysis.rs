use std::num::NonZeroUsize;

use schedules_core::{
    DateRange, DateTime, Duration, Error, NaiveDate, OccurrenceKey, OccurrenceOverride, Placement,
    QueryWindow, Recurrence, ResolutionPolicy, Schedule, Scheduler, Termination, Tz, Utc,
};

const WORK_BUDGET: usize = 100;
const RESULT_BUDGET: usize = 20;
const APPOINTMENT: Duration = Duration::hours(1);
const LONG_APPOINTMENT: Duration = Duration::hours(2);
const BUSY_UNION: Duration = Duration::hours(5);
const FREE_TOTAL: Duration = Duration::hours(3);
const SPRING_DAY: Duration = Duration::hours(23);
const FULL_OCCUPANCY: f64 = 1.0;
const NO_OCCUPANCY: f64 = 0.0;
const SUBSECOND_SLOT: Duration = Duration::nanoseconds(1);

// Intentionally non-Clone: analysis must borrow identifiers.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Item {
    Early,
    Nested,
    Crossing,
    Late,
    Reminder,
}

struct Fixture;

impl Fixture {
    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn range(start: &str, end: &str) -> DateRange {
        DateRange::new(Self::instant(start), Self::instant(end)).unwrap()
    }

    fn window() -> QueryWindow {
        QueryWindow {
            range: Self::range("2026-01-01T09:00:00Z", "2026-01-01T17:00:00Z"),
            all_day_zone: Tz::UTC,
            candidate_limit: NonZeroUsize::new(WORK_BUDGET).unwrap(),
            occurrence_limit: NonZeroUsize::new(RESULT_BUDGET).unwrap(),
        }
    }

    fn placement(start: &str, end: &str) -> Placement {
        let range = Self::range(start, end);
        Placement::timed(
            range.start().naive_utc(),
            Tz::UTC,
            range.duration(),
            ResolutionPolicy::default(),
        )
        .unwrap()
    }

    fn insert(scheduler: &mut Scheduler<Item>, item: Item, start: &str, end: &str) {
        scheduler.insert(item, Schedule::once(Self::placement(start, end)));
    }
}

#[test]
fn union_clips_nested_and_touching_intervals_without_double_counting() {
    let mut scheduler = Scheduler::new();
    Fixture::insert(
        &mut scheduler,
        Item::Early,
        "2026-01-01T08:00:00Z",
        "2026-01-01T12:00:00Z",
    );
    Fixture::insert(
        &mut scheduler,
        Item::Nested,
        "2026-01-01T10:00:00Z",
        "2026-01-01T11:00:00Z",
    );
    Fixture::insert(
        &mut scheduler,
        Item::Crossing,
        "2026-01-01T12:00:00Z",
        "2026-01-01T13:00:00Z",
    );
    Fixture::insert(
        &mut scheduler,
        Item::Late,
        "2026-01-01T16:00:00Z",
        "2026-01-01T18:00:00Z",
    );
    let analysis = scheduler.analyze(Fixture::window()).unwrap();
    assert_eq!(
        analysis.gaps(),
        &[Fixture::range(
            "2026-01-01T13:00:00Z",
            "2026-01-01T16:00:00Z"
        )]
    );
    assert_eq!(analysis.density().occupied(), BUSY_UNION);
    assert_eq!(analysis.density().free(), FREE_TOTAL);
    let pairs: Vec<_> = analysis
        .overlaps()
        .map(|pair| {
            (
                *pair.first().item_id == Item::Early,
                *pair.second().item_id == Item::Nested,
                pair.range(),
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![(
            true,
            true,
            Fixture::range("2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z")
        )]
    );
}

#[test]
fn empty_and_point_only_schedules_leave_entire_window_free() {
    let mut scheduler = Scheduler::<Item>::new();
    let window = Fixture::window();
    assert_eq!(scheduler.analyze(window).unwrap().gaps(), &[window.range]);
    scheduler.insert(
        Item::Reminder,
        Schedule::once(Placement::instant(window.range.start(), Tz::UTC).unwrap()),
    );
    let analysis = scheduler.analyze(window).unwrap();
    assert_eq!(analysis.gaps(), &[window.range]);
    assert_eq!(analysis.longest_gap(), Some(window.range));
    assert_eq!(analysis.density().occupancy_ratio(), NO_OCCUPANCY);
    assert!(analysis.overlaps().next().is_none());
    assert!(analysis.is_slot_available(window.range));
    assert!(!analysis.is_slot_available(Fixture::range(
        "2026-01-01T08:00:00Z",
        "2026-01-01T10:00:00Z"
    )));
}

#[test]
fn alternatives_fit_gaps_and_rank_nearest_start_with_earlier_ties() {
    let mut scheduler = Scheduler::new();
    Fixture::insert(
        &mut scheduler,
        Item::Early,
        "2026-01-01T11:00:00Z",
        "2026-01-01T12:00:00Z",
    );
    Fixture::insert(
        &mut scheduler,
        Item::Late,
        "2026-01-01T14:00:00Z",
        "2026-01-01T15:00:00Z",
    );
    let analysis = scheduler.analyze(Fixture::window()).unwrap();
    assert_eq!(
        analysis.longest_gap(),
        Some(Fixture::range(
            "2026-01-01T09:00:00Z",
            "2026-01-01T11:00:00Z"
        ))
    );
    let slots = analysis
        .suggest_alternatives(Fixture::instant("2026-01-01T11:00:00Z"), APPOINTMENT)
        .unwrap();
    assert_eq!(
        slots,
        vec![
            Fixture::range("2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z"),
            Fixture::range("2026-01-01T12:00:00Z", "2026-01-01T13:00:00Z"),
            Fixture::range("2026-01-01T15:00:00Z", "2026-01-01T16:00:00Z"),
        ]
    );
    assert!(slots.iter().all(|slot| analysis.is_slot_available(*slot)));
    assert_eq!(
        analysis
            .available_slots(LONG_APPOINTMENT)
            .unwrap()
            .collect::<Vec<_>>(),
        analysis.gaps()
    );
    assert!(
        analysis
            .available_slots(BUSY_UNION)
            .unwrap()
            .next()
            .is_none()
    );
    assert!(matches!(
        analysis.available_slots(Duration::zero()),
        Err(Error::InvalidDuration)
    ));
    assert_eq!(
        analysis.suggest_alternatives(Fixture::window().range.start(), -APPOINTMENT),
        Err(Error::InvalidDuration)
    );
}

#[test]
fn all_day_occupancy_uses_viewing_zone_and_actual_dst_duration() {
    let mut scheduler = Scheduler::new();
    let start: NaiveDate = "2026-03-29".parse().unwrap();
    let end: NaiveDate = "2026-03-30".parse().unwrap();
    scheduler.insert(
        Item::Early,
        Schedule::once(Placement::all_day(start, end).unwrap()),
    );
    let window = QueryWindow {
        range: Fixture::range("2026-03-29T00:00:00Z", "2026-03-29T23:00:00Z"),
        all_day_zone: Tz::Europe__London,
        ..Fixture::window()
    };
    let analysis = scheduler.analyze(window).unwrap();
    assert_eq!(analysis.density().occupied(), SPRING_DAY);
    assert_eq!(analysis.density().occupancy_ratio(), FULL_OCCUPANCY);
    assert!(analysis.gaps().is_empty());
    assert_eq!(analysis.longest_gap(), None);
    assert!(!analysis.is_slot_available(window.range));
}

#[test]
fn overlapping_recurrences_keep_same_item_identity() {
    let mut scheduler = Scheduler::new();
    let placement = Fixture::placement("2026-01-01T09:00:00Z", "2026-01-01T11:00:00Z");
    let recurrence = Recurrence::elapsed(APPOINTMENT, Termination::Forever).unwrap();
    scheduler.insert(
        Item::Early,
        Schedule::repeating(placement, recurrence).unwrap(),
    );
    let window = QueryWindow {
        range: Fixture::range("2026-01-01T09:00:00Z", "2026-01-01T11:00:00Z"),
        ..Fixture::window()
    };
    let analysis = scheduler.analyze(window).unwrap();
    let mut pairs = analysis.overlaps();
    let pair = pairs.next().unwrap();
    assert_eq!(pair.first().item_id, pair.second().item_id);
    assert_ne!(
        pair.first().occurrence.key(),
        pair.second().occurrence.key()
    );
    assert_eq!(
        pair.range(),
        Fixture::range("2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z")
    );
    assert!(pairs.next().is_none());
}

#[test]
fn cancellations_and_moves_control_effective_occupancy() {
    let mut scheduler = Scheduler::new();
    let original = Fixture::placement("2026-01-01T08:00:00Z", "2026-01-01T09:00:00Z");
    let key = OccurrenceKey::Calendar(original.local_start());
    let mut schedule = Schedule::once(original);
    schedule
        .set_override(
            key,
            OccurrenceOverride::Replace(Fixture::placement(
                "2026-01-01T10:00:00Z",
                "2026-01-01T11:00:00Z",
            )),
            NonZeroUsize::new(WORK_BUDGET).unwrap(),
        )
        .unwrap();
    scheduler.insert(Item::Early, schedule);
    Fixture::insert(
        &mut scheduler,
        Item::Nested,
        "2026-01-01T10:00:00Z",
        "2026-01-01T11:00:00Z",
    );
    let analysis = scheduler.analyze(Fixture::window()).unwrap();
    assert_eq!(
        analysis.overlaps().next().unwrap().first().occurrence.key(),
        key
    );
    assert_eq!(analysis.density().occupied(), APPOINTMENT);
    scheduler
        .get_mut(&Item::Early)
        .unwrap()
        .set_override(
            key,
            OccurrenceOverride::Cancel,
            NonZeroUsize::new(WORK_BUDGET).unwrap(),
        )
        .unwrap();
    assert!(
        scheduler
            .analyze(Fixture::window())
            .unwrap()
            .overlaps()
            .next()
            .is_none()
    );
}

#[test]
fn analysis_propagates_shared_query_budget_failures() {
    let mut scheduler = Scheduler::new();
    Fixture::insert(
        &mut scheduler,
        Item::Early,
        "2026-01-01T10:00:00Z",
        "2026-01-01T11:00:00Z",
    );
    Fixture::insert(
        &mut scheduler,
        Item::Late,
        "2026-01-01T12:00:00Z",
        "2026-01-01T13:00:00Z",
    );
    assert!(matches!(
        scheduler.analyze(QueryWindow {
            candidate_limit: NonZeroUsize::MIN,
            ..Fixture::window()
        }),
        Err(Error::CandidateLimit)
    ));
    assert!(matches!(
        scheduler.analyze(QueryWindow {
            occurrence_limit: NonZeroUsize::MIN,
            ..Fixture::window()
        }),
        Err(Error::OccurrenceLimit)
    ));
}

#[test]
fn conflict_pairs_are_complete_and_clipped_with_equal_starts() {
    let mut scheduler = Scheduler::new();
    for item in [Item::Early, Item::Nested, Item::Crossing] {
        Fixture::insert(
            &mut scheduler,
            item,
            "2026-01-01T08:00:00Z",
            "2026-01-01T18:00:00Z",
        );
    }
    let window = Fixture::window();
    let analysis = scheduler.analyze(window).unwrap();
    let pairs: Vec<_> = analysis
        .overlaps()
        .map(|pair| (pair.first().item_id, pair.second().item_id, pair.range()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (&Item::Early, &Item::Nested, window.range),
            (&Item::Early, &Item::Crossing, window.range),
            (&Item::Nested, &Item::Crossing, window.range),
        ]
    );
    assert!(
        analysis
            .suggest_alternatives(window.range.start(), APPOINTMENT)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn extreme_and_subsecond_windows_preserve_duration_and_suggestion_bounds() {
    let scheduler = Scheduler::<Item>::new();
    let full_range = DateRange::new(DateTime::<Utc>::MIN_UTC, DateTime::<Utc>::MAX_UTC).unwrap();
    let analysis = scheduler
        .analyze(QueryWindow {
            range: full_range,
            ..Fixture::window()
        })
        .unwrap();
    let last_start = full_range.end().checked_sub_signed(SUBSECOND_SLOT).unwrap();
    let last_slot = DateRange::new(last_start, full_range.end()).unwrap();
    assert_eq!(
        analysis
            .suggest_alternatives(full_range.end(), SUBSECOND_SLOT)
            .unwrap(),
        vec![last_slot]
    );
    assert_eq!(analysis.density().free(), full_range.duration());
    let tiny = scheduler
        .analyze(QueryWindow {
            range: last_slot,
            ..Fixture::window()
        })
        .unwrap();
    assert_eq!(
        tiny.available_slots(SUBSECOND_SLOT)
            .unwrap()
            .collect::<Vec<_>>(),
        vec![last_slot]
    );
    assert_eq!(
        tiny.suggest_alternatives(full_range.start(), SUBSECOND_SLOT)
            .unwrap(),
        vec![last_slot]
    );
    assert_eq!(last_slot.intersection(last_slot), Some(last_slot));
    assert_eq!(
        last_slot.intersection(DateRange::new(full_range.start(), last_start).unwrap()),
        None
    );
}
