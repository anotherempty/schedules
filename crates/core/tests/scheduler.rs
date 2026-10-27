use std::num::{NonZeroU32, NonZeroUsize};

use scheduler_core::{
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
