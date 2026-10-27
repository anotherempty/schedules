use std::num::{NonZeroU32, NonZeroUsize};

use schedules_core::{
    CalendarRule, DateRange, DateTime, Error, Frequency, NaiveDateTime, OccurrenceKey,
    OccurrenceOverride, Placement, QueryWindow, Recurrence, ResolutionPolicy, Schedule, Scheduler,
    Termination, Utc,
};

const CANDIDATE_BUDGET: usize = 100;
const RESULT_BUDGET: usize = 10;
const TWO_OCCURRENCES: u32 = 2;
const ONE_SCHEDULE_WORK: usize = 2;
const TWO_SCHEDULES_WORK: usize = 4;
const ONE_ITEM: usize = 1;

#[test]
fn exact_inspection_budget_allows_future_schedule_without_candidates() {
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-02-01 09:00:00")),
    );
    let window = QueryWindow {
        candidate_limit: NonZeroUsize::MIN,
        ..Fixture::window()
    };
    assert!(scheduler.occurrences(window).unwrap().is_empty());
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    assert_eq!(scheduler.occurrences(window), Err(Error::CandidateLimit));
}

// Deliberately non-Clone: scheduler queries must borrow identity.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ItemId {
    Meeting,
    Reminder,
}

struct Fixture;

impl Fixture {
    fn local(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn instant(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn point(text: &str) -> Placement {
        Placement::point(
            Self::local(text),
            chrono_tz::UTC,
            ResolutionPolicy::default(),
        )
        .unwrap()
    }

    fn window() -> QueryWindow {
        QueryWindow {
            range: DateRange::new(
                Self::instant("2026-01-01T00:00:00Z"),
                Self::instant("2026-01-04T00:00:00Z"),
            )
            .unwrap(),
            all_day_zone: chrono_tz::UTC,
            candidate_limit: NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
            occurrence_limit: NonZeroUsize::new(RESULT_BUDGET).unwrap(),
        }
    }
}

#[test]
fn insert_replace_lookup_and_remove_transfer_schedule_ownership() {
    let mut scheduler = Scheduler::new();
    assert!(scheduler.is_empty());
    assert!(
        scheduler
            .insert(
                ItemId::Meeting,
                Schedule::once(Fixture::point("2026-01-01 09:00:00"))
            )
            .is_none()
    );
    assert_eq!(scheduler.len(), ONE_ITEM);
    let replacement = Schedule::once(Fixture::point("2026-01-01 12:00:00"));
    let previous = scheduler
        .insert(ItemId::Meeting, replacement.clone())
        .unwrap();
    assert_eq!(previous.placement(), Fixture::point("2026-01-01 09:00:00"));
    assert_eq!(scheduler.get(&ItemId::Meeting), Some(&replacement));
    assert_eq!(scheduler.iter().next().unwrap().0, &ItemId::Meeting);
    assert!(scheduler.get(&ItemId::Reminder).is_none());
    assert!(scheduler.get_mut(&ItemId::Reminder).is_none());
    assert_eq!(scheduler.remove(&ItemId::Meeting), Some(replacement));
    assert!(scheduler.remove(&ItemId::Meeting).is_none());
    assert!(scheduler.is_empty());
}

#[test]
fn cross_item_results_sort_by_start_then_id_and_keep_identity_scoped() {
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Count(NonZeroU32::new(TWO_OCCURRENCES).unwrap()),
    )
    .unwrap();
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Meeting,
        Schedule::repeating(
            Fixture::point("2026-01-01 09:00:00"),
            Recurrence::calendar(rule),
        )
        .unwrap(),
    );
    let results = scheduler.occurrences(Fixture::window()).unwrap();
    assert_eq!(
        results.iter().map(|item| item.item_id).collect::<Vec<_>>(),
        [&ItemId::Meeting, &ItemId::Reminder, &ItemId::Meeting]
    );
    let mut results_iter = results.iter();
    let meeting = results_iter.next().unwrap();
    let reminder = results_iter.next().unwrap();
    assert_eq!(meeting.occurrence.key(), reminder.occurrence.key());
    assert!(std::ptr::eq(
        meeting.item_id,
        scheduler.iter().next().unwrap().0
    ));
    let copied = *meeting;
    assert_eq!(copied, *meeting);
}

#[test]
fn per_item_mutation_applies_overrides_without_affecting_other_items() {
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler
        .get_mut(&ItemId::Meeting)
        .unwrap()
        .set_override(
            OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00")),
            OccurrenceOverride::Replace(Fixture::point("2026-01-02 12:00:00")),
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap();
    assert_eq!(
        scheduler
            .occurrences(Fixture::window())
            .unwrap()
            .iter()
            .map(|item| item.item_id)
            .collect::<Vec<_>>(),
        [&ItemId::Reminder, &ItemId::Meeting]
    );
}

#[test]
fn budgets_apply_globally_including_schedules_with_no_matching_results() {
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-01 10:00:00")),
    );
    let mut window = Fixture::window();
    window.occurrence_limit = NonZeroUsize::MIN;
    assert_eq!(scheduler.occurrences(window), Err(Error::OccurrenceLimit));
    window.occurrence_limit = NonZeroUsize::new(RESULT_BUDGET).unwrap();
    window.candidate_limit = NonZeroUsize::new(ONE_SCHEDULE_WORK).unwrap();
    assert_eq!(scheduler.occurrences(window), Err(Error::CandidateLimit));
    window.candidate_limit = NonZeroUsize::new(TWO_SCHEDULES_WORK).unwrap();
    assert!(scheduler.occurrences(window).is_ok());
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2025-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2025-01-01 10:00:00")),
    );
    window.candidate_limit = NonZeroUsize::new(ONE_SCHEDULE_WORK).unwrap();
    assert_eq!(scheduler.occurrences(window), Err(Error::CandidateLimit));
}

#[test]
fn empty_scheduler_returns_no_results() {
    assert!(
        Scheduler::<ItemId>::default()
            .occurrences(Fixture::window())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn next_after_orders_ties_and_recurring_items() {
    let first_key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    let second_key = OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"));
    let rule = CalendarRule::new(
        Frequency::Daily,
        NonZeroU32::MIN,
        Termination::Count(NonZeroU32::new(TWO_OCCURRENCES).unwrap()),
    )
    .unwrap();
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::repeating(
            Fixture::point("2026-01-01 09:00:00"),
            Recurrence::calendar(rule),
        )
        .unwrap(),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    let cases = [
        (
            &ItemId::Meeting,
            first_key,
            Some((&ItemId::Reminder, first_key)),
        ),
        (
            &ItemId::Reminder,
            first_key,
            Some((&ItemId::Meeting, second_key)),
        ),
        (&ItemId::Meeting, second_key, None),
    ];
    for (item_id, key, expected) in cases {
        let next = scheduler
            .next_after(
                item_id,
                key,
                chrono_tz::UTC,
                NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
            )
            .unwrap();
        assert_eq!(
            next.map(|entry| (entry.item_id, entry.occurrence.key())),
            expected,
            "anchor {item_id:?} {key:?}"
        );
    }
    scheduler.remove(&ItemId::Reminder);
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            first_key,
            chrono_tz::UTC,
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.occurrence.key(), second_key);
    assert_eq!(next.item_id, &ItemId::Meeting);
}

#[test]
fn next_after_resolves_moved_anchors_and_rejects_missing_anchors() {
    let key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    let mut scheduler = Scheduler::new();
    assert_eq!(
        scheduler.next_after(
            &ItemId::Meeting,
            key,
            chrono_tz::UTC,
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap()
        ),
        Err(Error::InvalidOccurrenceKey)
    );
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-02 09:00:00")),
    );
    let changes = [
        (
            OccurrenceOverride::Replace(Fixture::point("2026-01-03 09:00:00")),
            Ok(None),
        ),
        (
            OccurrenceOverride::Replace(Fixture::point("2026-02-01 09:00:00")),
            Ok(None),
        ),
        (OccurrenceOverride::Cancel, Err(Error::InvalidOccurrenceKey)),
    ];
    for (change, expected) in changes {
        scheduler
            .get_mut(&ItemId::Meeting)
            .unwrap()
            .set_override(key, change, NonZeroUsize::new(CANDIDATE_BUDGET).unwrap())
            .unwrap();
        assert_eq!(
            scheduler.next_after(
                &ItemId::Meeting,
                key,
                chrono_tz::UTC,
                NonZeroUsize::new(CANDIDATE_BUDGET).unwrap()
            ),
            expected,
            "override {change:?}"
        );
    }
}

#[test]
fn next_after_propagates_candidate_budget_errors() {
    let key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2026-01-02 09:00:00")),
    );
    assert_eq!(
        scheduler.next_after(&ItemId::Meeting, key, chrono_tz::UTC, NonZeroUsize::MIN),
        Err(Error::CandidateLimit)
    );
}

#[test]
fn next_after_stops_at_first_successor_of_endless_recurrence() {
    // Anchor lookup: inspection + candidate. Successor: inspection + two candidates.
    const NEXT_SEARCH_WORK: usize = 5;
    let mut scheduler = Scheduler::new();
    let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Termination::Forever).unwrap();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::repeating(
            Fixture::point("2026-01-01 09:00:00"),
            Recurrence::calendar(rule),
        )
        .unwrap(),
    );
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00")),
            chrono_tz::UTC,
            NonZeroUsize::new(NEXT_SEARCH_WORK).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        next.occurrence.key(),
        OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"))
    );
}

#[test]
fn next_after_finds_distant_successor_without_date_window() {
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 09:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Fixture::point("2036-01-01 09:00:00")),
    );
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00")),
            chrono_tz::UTC,
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.item_id, &ItemId::Reminder);
    assert_eq!(
        next.occurrence.placement(),
        Fixture::point("2036-01-01 09:00:00")
    );
}

#[test]
fn next_after_selects_moved_occurrence_before_regular_successor() {
    let mut scheduler = Scheduler::new();
    let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Termination::Forever).unwrap();
    let mut schedule = Schedule::repeating(
        Fixture::point("2026-01-01 09:00:00"),
        Recurrence::calendar(rule),
    )
    .unwrap();
    let moved_key = OccurrenceKey::Calendar(Fixture::local("2026-01-03 09:00:00"));
    let anchor_key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    schedule
        .set_override(
            moved_key,
            OccurrenceOverride::Replace(Fixture::point("2026-01-01 09:00:00")),
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap();
    scheduler.insert(ItemId::Meeting, schedule);
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            anchor_key,
            chrono_tz::UTC,
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.occurrence.key(), moved_key);
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            moved_key,
            chrono_tz::UTC,
            NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        next.occurrence.key(),
        OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"))
    );
}

#[test]
fn next_after_resolves_all_day_order_in_viewing_timezone() {
    let mut scheduler = Scheduler::new();
    let date = Fixture::local("2026-01-02 00:00:00").date();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::once(Fixture::point("2026-01-01 22:00:00")),
    );
    scheduler.insert(
        ItemId::Reminder,
        Schedule::once(Placement::all_day(date, date.succ_opt().unwrap()).unwrap()),
    );
    let key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 22:00:00"));
    for (zone, expected) in [
        (chrono_tz::UTC, Some(&ItemId::Reminder)),
        (chrono_tz::Africa::Nairobi, None),
    ] {
        let next = scheduler
            .next_after(
                &ItemId::Meeting,
                key,
                zone,
                NonZeroUsize::new(CANDIDATE_BUDGET).unwrap(),
            )
            .unwrap();
        assert_eq!(next.map(|entry| entry.item_id), expected, "zone {zone}");
    }
}

#[test]
fn next_after_seeks_old_calendar_rules_with_small_budget() {
    const SEEK_BUDGET: usize = 8;
    let old_start = Fixture::point("2000-01-01 09:00:00");
    let anchor_key = OccurrenceKey::Calendar(Fixture::local("2026-01-01 09:00:00"));
    for termination in [
        Termination::Forever,
        Termination::Until(Fixture::local("2026-01-02 09:00:00")),
    ] {
        let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, termination).unwrap();
        let mut scheduler = Scheduler::new();
        scheduler.insert(
            ItemId::Meeting,
            Schedule::repeating(old_start, Recurrence::calendar(rule)).unwrap(),
        );
        let next = scheduler
            .next_after(
                &ItemId::Meeting,
                anchor_key,
                chrono_tz::UTC,
                NonZeroUsize::new(SEEK_BUDGET).unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            next.occurrence.key(),
            OccurrenceKey::Calendar(Fixture::local("2026-01-02 09:00:00"))
        );
        if matches!(termination, Termination::Until(_)) {
            assert_eq!(
                scheduler.next_after(
                    &ItemId::Meeting,
                    next.occurrence.key(),
                    chrono_tz::UTC,
                    NonZeroUsize::new(SEEK_BUDGET).unwrap()
                ),
                Ok(None)
            );
        }
    }
}

#[test]
fn next_after_seeks_elapsed_count_with_nanosecond_precision() {
    const SERIES_COUNT: u32 = 1_000_000;
    const LAST_INDEX: i32 = 999_999;
    const INTERVAL_NANOSECONDS: i64 = 7;
    const OFF_GRID_NANOSECONDS: i64 = 1;
    const SEEK_BUDGET: usize = 8;
    let start = Fixture::instant("2026-01-01T09:00:00Z");
    let interval = schedules_core::Duration::nanoseconds(INTERVAL_NANOSECONDS);
    let last = start + interval * LAST_INDEX;
    let recurrence = Recurrence::elapsed(
        interval,
        Termination::Count(NonZeroU32::new(SERIES_COUNT).unwrap()),
    )
    .unwrap();
    let mut scheduler = Scheduler::new();
    scheduler.insert(
        ItemId::Meeting,
        Schedule::repeating(
            Placement::instant(start, chrono_tz::UTC).unwrap(),
            recurrence,
        )
        .unwrap(),
    );
    let next = scheduler
        .next_after(
            &ItemId::Meeting,
            OccurrenceKey::Elapsed(last - interval),
            chrono_tz::UTC,
            NonZeroUsize::new(SEEK_BUDGET).unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(next.occurrence.key(), OccurrenceKey::Elapsed(last));
    assert_eq!(
        scheduler.next_after(
            &ItemId::Meeting,
            OccurrenceKey::Elapsed(last),
            chrono_tz::UTC,
            NonZeroUsize::new(SEEK_BUDGET).unwrap()
        ),
        Ok(None)
    );
    for invalid in [
        last + interval,
        last - schedules_core::Duration::nanoseconds(OFF_GRID_NANOSECONDS),
    ] {
        assert_eq!(
            scheduler.next_after(
                &ItemId::Meeting,
                OccurrenceKey::Elapsed(invalid),
                chrono_tz::UTC,
                NonZeroUsize::new(SEEK_BUDGET).unwrap()
            ),
            Err(Error::InvalidOccurrenceKey)
        );
    }
}

#[test]
fn next_after_seek_matches_expansion_across_dst_and_count_limits() {
    const DST_CANDIDATE_BUDGET: usize = 2_000;
    const DST_RESULT_BUDGET: usize = 100;
    const FINITE_OCCURRENCE_COUNT: u32 = 12;
    let zone = chrono_tz::Europe::London;
    for (start, end) in [
        ("2026-03-27 01:15:00", "2026-04-02T00:00:00Z"),
        ("2026-10-23 01:15:00", "2026-10-29T00:00:00Z"),
    ] {
        for termination in [
            Termination::Forever,
            Termination::Count(NonZeroU32::new(FINITE_OCCURRENCE_COUNT).unwrap()),
        ] {
            for policy in [
                schedules_core::AmbiguityPolicy::Earlier,
                schedules_core::AmbiguityPolicy::Later,
            ] {
                let local = Fixture::local(start);
                let second_time = Fixture::local("2026-01-01 01:45:00").time();
                let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, termination)
                    .unwrap()
                    .with_times([local.time(), second_time]);
                let mut scheduler = Scheduler::new();
                scheduler.insert(
                    ItemId::Meeting,
                    Schedule::repeating(
                        Placement::point(local, zone, ResolutionPolicy::new(policy)).unwrap(),
                        Recurrence::calendar(rule),
                    )
                    .unwrap(),
                );
                let window = QueryWindow {
                    range: DateRange::new(local.and_utc(), Fixture::instant(end)).unwrap(),
                    all_day_zone: zone,
                    candidate_limit: NonZeroUsize::new(DST_CANDIDATE_BUDGET).unwrap(),
                    occurrence_limit: NonZeroUsize::new(DST_RESULT_BUDGET).unwrap(),
                };
                let reference = scheduler.occurrences(window).unwrap();
                for (anchor, expected) in reference.iter().zip(reference.iter().skip(1)) {
                    let actual = scheduler
                        .next_after(
                            anchor.item_id,
                            anchor.occurrence.key(),
                            zone,
                            window.candidate_limit,
                        )
                        .unwrap();
                    assert_eq!(
                        actual,
                        Some(*expected),
                        "{start} {termination:?} {policy:?}"
                    );
                }
            }
        }
    }
}
