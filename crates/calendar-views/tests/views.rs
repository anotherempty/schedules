use calendar_views::{
    Calendar, CalendarEntry, CalendarWindow, DAYS_PER_WEEK, DateTime, Duration, Error, NaiveDate,
    TimeSpan, Tz, Utc, Weekday,
};

const SPRING_DAY: Duration = Duration::hours(23);
const FALL_DAY: Duration = Duration::hours(25);
const TWO_WEEKS: usize = 2;

// No Clone, Copy, or scheduling traits: arbitrary data stays borrowed by views.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    Spanning,
    Point,
    FirstTie,
    SecondTie,
    Outside,
}

struct Fixture;

impl Fixture {
    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }
    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn window(start: &str, end: &str, zone: Tz) -> CalendarWindow {
        CalendarWindow::new(Self::date(start), Self::date(end), zone).unwrap()
    }
    fn interval(start: &str, end: &str) -> TimeSpan {
        TimeSpan::interval(Self::utc(start), Self::utc(end)).unwrap()
    }
}

#[test]
fn arbitrary_payloads_group_by_overlap_with_exclusive_ends_and_stable_ties() {
    let window = Fixture::window("2026-01-01", "2026-01-04", Tz::UTC);
    let calendar = Calendar::new(
        window,
        [
            CalendarEntry::new(
                Item::Point,
                TimeSpan::point(Fixture::utc("2026-01-02T00:00:00Z")),
            ),
            CalendarEntry::new(
                Item::FirstTie,
                Fixture::interval("2026-01-01T09:00:00Z", "2026-01-02T00:00:00Z"),
            ),
            CalendarEntry::new(
                Item::SecondTie,
                Fixture::interval("2026-01-01T09:00:00Z", "2026-01-01T10:00:00Z"),
            ),
            CalendarEntry::new(
                Item::Spanning,
                Fixture::interval("2025-12-31T23:00:00Z", "2026-01-03T00:00:00Z"),
            ),
            CalendarEntry::new(Item::Outside, TimeSpan::point(window.utc_end())),
        ],
    );
    let days = calendar.days().collect::<Result<Vec<_>, _>>().unwrap();
    let items: Vec<Vec<_>> = days
        .iter()
        .map(|day| day.entries().iter().map(|entry| entry.item()).collect())
        .collect();
    assert_eq!(
        items,
        vec![
            vec![&Item::Spanning, &Item::FirstTie, &Item::SecondTie],
            vec![&Item::Spanning, &Item::Point],
            vec![]
        ]
    );
    assert!(std::ptr::eq(
        days.first().unwrap().entries().first().unwrap().item(),
        days.get(1).unwrap().entries().first().unwrap().item()
    ));
    assert_eq!(
        calendar.entries().first().unwrap().span().start(),
        Fixture::utc("2025-12-31T23:00:00Z")
    );
}

#[test]
fn borrowed_payloads_need_no_clone_and_can_be_recovered_from_entries() {
    let item = Item::Point;
    let window = Fixture::window("2026-01-01", "2026-01-02", Tz::UTC);
    let entry = CalendarEntry::new(&item, TimeSpan::point(window.utc_start()));
    assert!(std::ptr::eq(entry.into_item(), &item));
    let calendar = Calendar::new(window, [entry]);
    let day = calendar.days().next().unwrap().unwrap();
    assert!(std::ptr::eq(*day.entries().first().unwrap().item(), &item));
}

#[test]
fn local_days_follow_dst_and_points_follow_viewing_timezone() {
    for (start, end, expected) in [
        ("2026-03-29", "2026-03-30", SPRING_DAY),
        ("2026-10-25", "2026-10-26", FALL_DAY),
    ] {
        let window = Fixture::window(start, end, Tz::Europe__London);
        let calendar = Calendar::new(window, [CalendarEntry::new(Item::Spanning, window.span())]);
        let day = calendar.days().next().unwrap().unwrap();
        assert_eq!(day.window().utc_end() - day.window().utc_start(), expected);
        assert_eq!(day.entries().first().unwrap().item(), &Item::Spanning);
    }
    let window = Fixture::window("2026-01-01", "2026-01-03", Tz::Africa__Nairobi);
    let calendar = Calendar::new(
        window,
        [CalendarEntry::new(
            Item::Point,
            TimeSpan::point(Fixture::utc("2026-01-01T22:00:00Z")),
        )],
    );
    let days = calendar.days().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(days.first().unwrap().entries().is_empty());
    assert_eq!(days.last().unwrap().date(), Fixture::date("2026-01-02"));
    assert_eq!(
        days.last().unwrap().entries().first().unwrap().item(),
        &Item::Point
    );
}

#[test]
fn partial_weeks_distinguish_padding_from_empty_requested_days() {
    let window = Fixture::window("2026-01-01", "2026-01-10", Tz::UTC);
    let calendar = Calendar::<Item>::new(window, []);
    let weeks = calendar
        .weeks(Weekday::Mon)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        weeks.iter().map(|week| week.start()).collect::<Vec<_>>(),
        ["2025-12-29", "2026-01-05"].map(Fixture::date)
    );
    let dates: Vec<_> = weeks
        .iter()
        .map(|week| {
            week.days()
                .each_ref()
                .map(|day| day.as_ref().map(|day| day.date()))
        })
        .collect();
    assert_eq!(
        dates,
        vec![
            [
                None,
                None,
                None,
                Some(Fixture::date("2026-01-01")),
                Some(Fixture::date("2026-01-02")),
                Some(Fixture::date("2026-01-03")),
                Some(Fixture::date("2026-01-04"))
            ],
            [
                Some(Fixture::date("2026-01-05")),
                Some(Fixture::date("2026-01-06")),
                Some(Fixture::date("2026-01-07")),
                Some(Fixture::date("2026-01-08")),
                Some(Fixture::date("2026-01-09")),
                None,
                None
            ],
        ]
    );
    assert!(
        weeks
            .iter()
            .flat_map(|week| week.days().iter().flatten())
            .all(|day| day.entries().is_empty())
    );
    assert_eq!(
        calendar
            .weeks(Weekday::Sun)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .start(),
        Fixture::date("2025-12-28")
    );
}

#[test]
fn complete_week_padding_is_explicit_and_idempotent() {
    let window = Fixture::window("2026-01-01", "2026-01-10", Tz::UTC)
        .covering_weeks(Weekday::Mon)
        .unwrap();
    assert_eq!(window.start(), Fixture::date("2025-12-29"));
    assert_eq!(window.end(), Fixture::date("2026-01-12"));
    assert_eq!(window.covering_weeks(Weekday::Mon).unwrap(), window);
    let calendar = Calendar::<Item>::new(window, []);
    let weeks = calendar
        .weeks(Weekday::Mon)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(weeks.len(), TWO_WEEKS);
    assert!(
        weeks
            .iter()
            .all(|week| week.days().iter().flatten().count() == DAYS_PER_WEEK)
    );
}

#[test]
fn missing_midnight_is_typed_and_day_and_week_iterators_fuse_on_error() {
    let zone = Tz::America__Sao_Paulo;
    let missing = Fixture::date("2018-11-04");
    let expected = Error::NonexistentMidnight {
        date: missing,
        zone,
    };
    assert_eq!(
        CalendarWindow::new(missing, Fixture::date("2018-11-05"), zone),
        Err(expected.clone())
    );
    let calendar = Calendar::<Item>::new(Fixture::window("2018-11-02", "2018-11-06", zone), []);
    let mut days = calendar.days();
    assert_eq!(
        days.next().unwrap().unwrap().date(),
        Fixture::date("2018-11-02")
    );
    assert_eq!(days.next().unwrap().unwrap_err(), expected);
    assert!(days.next().is_none());
    assert!(days.next().is_none());
    let mut weeks = calendar.weeks(Weekday::Mon).unwrap();
    assert_eq!(weeks.next().unwrap().unwrap_err(), expected);
    assert!(weeks.next().is_none());
    assert!(weeks.next().is_none());
}

#[test]
fn ambiguous_midnight_selects_earlier_instant() {
    let window = Fixture::window("2020-11-01", "2020-11-02", Tz::America__Havana);
    assert_eq!(window.utc_start(), Fixture::utc("2020-11-01T04:00:00Z"));
    assert_eq!(window.utc_end() - window.utc_start(), FALL_DAY);
}

#[test]
fn invalid_ranges_and_unrepresentable_week_padding_return_errors() {
    let date = Fixture::date("2026-01-01");
    assert_eq!(
        CalendarWindow::new(date, date, Tz::UTC),
        Err(Error::InvalidRange)
    );
    let instant = Fixture::utc("2026-01-01T00:00:00Z");
    assert_eq!(
        TimeSpan::interval(instant, instant),
        Err(Error::InvalidRange)
    );
    assert_eq!(
        TimeSpan::interval(DateTime::<Utc>::MAX_UTC, DateTime::<Utc>::MIN_UTC),
        Err(Error::InvalidRange)
    );
    let window =
        CalendarWindow::new(NaiveDate::MIN, NaiveDate::MIN.succ_opt().unwrap(), Tz::UTC).unwrap();
    use chrono::Datelike;
    let previous_weekday = NaiveDate::MIN.weekday().pred();
    assert_eq!(
        window.covering_weeks(previous_weekday),
        Err(Error::OutOfRange)
    );
    let calendar = Calendar::<Item>::new(window, []);
    assert!(matches!(
        calendar.weeks(previous_weekday),
        Err(Error::OutOfRange)
    ));
}
