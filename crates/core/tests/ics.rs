#![cfg(feature = "ics")]

use schedules_core::{
    DateRange, DateTime, Duration, Frequency, IcsDocument, IcsError, IcsEvent, IcsField,
    IcsInvalid, IcsScheduleExt, IcsUnsupported, OccurrenceKey, OccurrenceOverride, Placement,
    QueryWindow, Schedule, Tz,
};
use std::num::NonZeroUsize;

const BUDGET: NonZeroUsize = NonZeroUsize::new(100).unwrap();
const DAY: Duration = Duration::days(1);
const MULTIDAY_DURATION: Duration = Duration::days(2);

#[test]
fn deserialization_is_separate_from_schedule_compatibility() {
    const RULE_LINE: usize = 8;
    let input = Fixture::document(&Fixture::event(
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=HOURLY\r\n",
    ));
    let document = IcsDocument::deserialize(&input).unwrap();
    assert!(document.serialize().contains("RRULE:FREQ=HOURLY\r\n"));
    let error = document.to_schedules(BUDGET).unwrap_err();
    assert!(matches!(
        error.kind(),
        IcsError::Unsupported(IcsUnsupported::Frequency)
    ));
    assert_eq!(error.location().unwrap().start_line().get(), RULE_LINE);

    let malformed_time = Fixture::document(&Fixture::event("DTSTART:20260230T090000Z\r\n"));
    let document = IcsDocument::deserialize(&malformed_time).unwrap();
    assert!(matches!(
        document.to_schedules(BUDGET).unwrap_err().kind(),
        IcsError::Invalid(IcsInvalid::DateTimeParse { .. })
    ));

    let malformed_structure = input.replace("END:VEVENT", "END:VCALENDAR");
    assert!(IcsDocument::deserialize(&malformed_structure).is_err());
}

#[test]
fn parse_locations_preserve_physical_lines_and_folded_ranges() {
    use std::error::Error;
    const SECOND_EVENT_START_LINE: usize = 12;
    const RULE_START_LINE: usize = 8;
    const RULE_END_LINE: usize = 9;
    const SYNTAX_LINE: usize = 8;
    const EVENT_BEGIN_LINE: usize = 4;
    const END_VALUE_LINE: usize = 8;

    let first = Fixture::event("DTSTART:20260101T090000Z\r\n");
    let second = Fixture::event("DTSTART:20260230T090000Z\r\n").replace("UID:sample", "UID:second");
    let error = Fixture::import(&Fixture::document(&(first + &second)), BUDGET).unwrap_err();
    assert_eq!(
        error.location().unwrap().start_line().get(),
        SECOND_EVENT_START_LINE
    );
    assert!(matches!(
        error.kind(),
        IcsError::Invalid(IcsInvalid::DateTimeParse { .. })
    ));
    let cause = error.source().unwrap();
    assert!(cause.is::<Box<IcsError>>());
    assert!(cause.source().unwrap().is::<chrono::ParseError>());

    let folded = Fixture::document(&Fixture::event(
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;\r\n COUNT=0\r\n",
    ));
    let error = Fixture::import(&folded, BUDGET).unwrap_err();
    let location = error.location().unwrap();
    assert_eq!(location.start_line().get(), RULE_START_LINE);
    assert_eq!(location.end_line().get(), RULE_END_LINE);
    assert!(matches!(
        error.kind(),
        IcsError::Invalid(IcsInvalid::Integer {
            field: IcsField::Count,
            ..
        })
    ));

    let syntax = Fixture::document(&Fixture::event("DTSTART:20260101T090000Z\r\nBROKEN\r\n"));
    let error = Fixture::import(&syntax, BUDGET).unwrap_err();
    assert_eq!(error.location().unwrap().start_line().get(), SYNTAX_LINE);
    assert!(matches!(error.kind(), IcsError::Wire(_)));

    let missing = Fixture::document(&Fixture::event(""));
    let error = Fixture::import(&missing, BUDGET).unwrap_err();
    assert_eq!(
        error.location().unwrap().start_line().get(),
        EVENT_BEGIN_LINE
    );
    assert!(matches!(
        error.kind(),
        IcsError::Invalid(IcsInvalid::Missing(IcsField::Start))
    ));

    let invalid_end = Fixture::document(&Fixture::event(
        "DTSTART:20260101T090000Z\r\nDTEND:20260101T080000Z\r\n",
    ));
    let error = Fixture::import(&invalid_end, BUDGET).unwrap_err();
    assert_eq!(error.location().unwrap().start_line().get(), END_VALUE_LINE);
    assert!(matches!(
        error.kind(),
        IcsError::Schedule(schedules_core::Error::InvalidDuration)
    ));
    assert!(
        Frequency::from_ics("HOURLY")
            .unwrap_err()
            .location()
            .is_none()
    );
}

#[test]
fn frequency_owns_ics_conversion() {
    for (frequency, text) in [
        (Frequency::Daily, "DAILY"),
        (Frequency::Weekly, "WEEKLY"),
        (Frequency::Monthly, "MONTHLY"),
        (Frequency::Yearly, "YEARLY"),
    ] {
        assert_eq!(frequency.as_ics(), text);
        assert_eq!(Frequency::from_ics(text).unwrap(), frequency);
        assert_eq!(
            Frequency::from_ics(&text.to_ascii_lowercase()).unwrap(),
            frequency
        );
    }
    for text in ["", "HOURLY", " DAILY", "DAILY ", "DAILYY"] {
        assert!(matches!(
            Frequency::from_ics(text),
            Err(IcsError::Unsupported(IcsUnsupported::Frequency))
        ));
    }
}

struct Fixture;

impl Fixture {
    fn import(input: &str, budget: NonZeroUsize) -> Result<Vec<IcsEvent>, IcsError> {
        IcsDocument::deserialize(input)?.to_schedules(budget)
    }

    fn export(
        events: &[IcsEvent],
        stamp: DateTime<schedules_core::Utc>,
    ) -> Result<String, IcsError> {
        Ok(IcsDocument::from_schedules(events, stamp)?.serialize())
    }

    // Existing behavior assertions intentionally inspect the underlying typed cause.
    fn parse_document(input: &str, budget: NonZeroUsize) -> Result<Vec<IcsEvent>, IcsError> {
        Fixture::import(input, budget).map_err(IcsError::into_kind)
    }

    fn document(events: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\n{events}END:VCALENDAR\r\n")
    }

    fn event(properties: &str) -> String {
        format!(
            "BEGIN:VEVENT\r\nUID:sample\r\nDTSTAMP:20260101T000000Z\r\n{properties}END:VEVENT\r\n"
        )
    }

    fn parse(properties: &str) -> Result<Vec<IcsEvent>, IcsError> {
        Fixture::parse_document(&Self::document(&Self::event(properties)), BUDGET)
    }

    fn starts(schedule: &Schedule) -> Vec<String> {
        let start = "2026-01-01T00:00:00Z".parse().unwrap();
        let end = "2026-02-01T00:00:00Z".parse().unwrap();
        schedule
            .occurrences(QueryWindow {
                range: DateRange::new(start, end).unwrap(),
                all_day_zone: Tz::UTC,
                candidate_limit: BUDGET,
                occurrence_limit: BUDGET,
            })
            .unwrap()
            .into_iter()
            .map(|o| o.placement().local_start().to_string())
            .collect()
    }
}

#[test]
fn validation_failures_have_typed_context_and_preserve_sources() {
    use std::error::Error;

    assert!(matches!(
        Fixture::parse(""),
        Err(IcsError::Invalid(IcsInvalid::Missing(IcsField::Start)))
    ));
    assert!(matches!(
        Fixture::parse("DTSTART:20260101T090000Z\r\nDTSTART:20260102T090000Z\r\n"),
        Err(IcsError::Invalid(IcsInvalid::Duplicate(IcsField::Start)))
    ));
    assert!(matches!(
        Fixture::parse("DTSTART:20260101T090000Z\r\nDTEND;VALUE=DATE:20260102\r\n"),
        Err(IcsError::Invalid(IcsInvalid::EndTypeMismatch))
    ));
    assert!(matches!(
        Fixture::parse(
            "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=2;UNTIL=20260103T090000Z\r\n"
        ),
        Err(IcsError::Invalid(IcsInvalid::ConflictingTermination))
    ));

    let error =
        Fixture::parse("DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=0\r\n").unwrap_err();
    assert!(matches!(
        &error,
        IcsError::Invalid(IcsInvalid::Integer {
            field: IcsField::Count,
            ..
        })
    ));
    assert!(error.source().unwrap().is::<std::num::ParseIntError>());

    let error = Fixture::parse("DTSTART:20260230T090000Z\r\n").unwrap_err();
    assert!(matches!(
        &error,
        IcsError::Invalid(IcsInvalid::DateTimeParse { .. })
    ));
    assert!(error.source().unwrap().is::<chrono::ParseError>());

    let error = Fixture::parse("MALFORMED\r\n").unwrap_err();
    assert!(matches!(&error, IcsError::Wire(_)));
    assert!(error.source().is_some());

    let error =
        Fixture::parse("DTSTART:20260101T090000Z\r\nDTEND:20260101T080000Z\r\n").unwrap_err();
    assert!(matches!(
        &error,
        IcsError::Schedule(schedules_core::Error::InvalidDuration)
    ));
}

#[test]
fn duplicate_exception_reports_original_occurrence_key() {
    let master = Fixture::event("DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=2\r\n");
    let exception = Fixture::event("RECURRENCE-ID:20260102T090000Z\r\nSTATUS:CANCELLED\r\n");
    let expected = OccurrenceKey::Calendar("2026-01-02T09:00:00".parse().unwrap());
    let result = Fixture::parse_document(
        &Fixture::document(&(master + &exception + &exception)),
        BUDGET,
    );
    assert!(matches!(result, Err(IcsError::DuplicateException { key }) if key == expected));
}

#[test]
fn imports_count_cancellations_and_moved_occurrences() {
    let master = Fixture::event(
        "DTSTART:20260101T090000Z\r\nDTEND:20260101T100000Z\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE:20260102T090000Z\r\n",
    );
    let replacement = Fixture::event(
        "RECURRENCE-ID:20260103T090000Z\r\nDTSTART:20260103T140000Z\r\nDTEND:20260103T160000Z\r\n",
    );
    // Detached exceptions may precede the master.
    let events =
        Fixture::parse_document(&Fixture::document(&(replacement + &master)), BUDGET).unwrap();
    assert_eq!(
        Fixture::starts(events[0].schedule()),
        [
            "2026-01-01 09:00:00",
            "2026-01-03 14:00:00",
            "2026-01-04 09:00:00"
        ]
    );
    let encoded = Fixture::export(&events, DateTime::UNIX_EPOCH).unwrap();
    assert_eq!(Fixture::parse_document(&encoded, BUDGET).unwrap(), events);
}

#[test]
fn defaults_all_day_to_one_day_and_keeps_exclusive_end() {
    for (properties, duration) in [
        ("DTSTART;VALUE=DATE:20260101\r\n", DAY),
        (
            "DTSTART;VALUE=DATE:20260101\r\nDTEND;VALUE=DATE:20260103\r\n",
            MULTIDAY_DURATION,
        ),
    ] {
        let events = Fixture::parse(properties).unwrap();
        let (start, end) = events[0].schedule().placement().bounds(Tz::UTC).unwrap();
        assert_eq!(end.unwrap() - start, duration);
        let encoded = Fixture::export(&events, DateTime::UNIX_EPOCH).unwrap();
        assert_eq!(Fixture::parse_document(&encoded, BUDGET).unwrap(), events);
    }
}

#[test]
fn weekday_filter_until_and_default_week_start_are_preserved() {
    let events = Fixture::parse("DTSTART:20260101T090000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=TH,FR;UNTIL=20260116T090000Z\r\n").unwrap();
    assert_eq!(
        Fixture::starts(events[0].schedule()),
        [
            "2026-01-01 09:00:00",
            "2026-01-02 09:00:00",
            "2026-01-15 09:00:00",
            "2026-01-16 09:00:00"
        ]
    );
    let encoded = Fixture::export(&events, DateTime::UNIX_EPOCH).unwrap();
    assert_eq!(Fixture::parse_document(&encoded, BUDGET).unwrap(), events);
}

#[test]
fn all_day_until_is_inclusive_and_exdate_retains_count_semantics() {
    let events = Fixture::parse("DTSTART;VALUE=DATE:20260101\r\nRRULE:FREQ=DAILY;UNTIL=20260103\r\nEXDATE;VALUE=DATE:20260102\r\n").unwrap();
    assert_eq!(
        Fixture::starts(events[0].schedule()),
        ["2026-01-01 00:00:00", "2026-01-03 00:00:00"]
    );
    assert_eq!(
        Fixture::parse_document(
            &Fixture::export(&events, DateTime::UNIX_EPOCH).unwrap(),
            BUDGET
        )
        .unwrap(),
        events
    );
}

#[test]
fn folds_utf8_lines_and_escapes_uid_without_metadata_model() {
    const UID_REPETITIONS: usize = 40;
    const MAX_LINE_OCTETS: usize = 75;
    let uid = format!(" {},;\\ ", "日 ".repeat(UID_REPETITIONS));
    let event = IcsEvent::new(
        uid,
        Schedule::once(Placement::instant(DateTime::UNIX_EPOCH, Tz::UTC).unwrap()),
    )
    .unwrap();
    let encoded = Fixture::export(std::slice::from_ref(&event), DateTime::UNIX_EPOCH).unwrap();
    assert!(
        encoded
            .split("\r\n")
            .all(|line| line.len() <= MAX_LINE_OCTETS)
    );
    assert!(encoded.contains("\r\n "));
    assert_eq!(Fixture::parse_document(&encoded, BUDGET).unwrap(), [event]);
    assert!(Fixture::parse("DTSTART:20260101T090000Z\r\nSUMMARY:Application owned\r\nATTENDEE:mailto:person@example.com\r\n").is_ok());
}

#[test]
fn rejects_unsupported_timing_instead_of_changing_it() {
    for props in [
        "DTSTART:20260101T090000\r\n",
        "DTSTART;TZID=Europe/Paris:20260101T090000\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=1TH\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=1\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=WEEKLY;WKST=SU\r\n",
        "DTSTART:20260101T090000Z\r\nRDATE:20260102T090000Z\r\n",
        "DTSTART:20260101T090000Z\r\nDURATION:PT1H\r\n",
    ] {
        assert!(
            matches!(Fixture::parse(props), Err(IcsError::Unsupported(_))),
            "{props}"
        );
    }
}

#[test]
fn rejects_malformed_duplicates_and_type_mismatches() {
    for props in [
        "DTSTART:20260101T090000Z\r\nDTSTART:20260102T090000Z\r\n",
        "DTSTART:20260101T090000Z\r\nDTEND;VALUE=DATE:20260102\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=2;UNTIL=20260103T090000Z\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;INTERVAL=0\r\n",
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=0\r\n",
        "DTSTART:20260230T090000Z\r\n",
    ] {
        assert!(
            matches!(Fixture::parse(props), Err(IcsError::Invalid(_))),
            "{props}"
        );
    }
    let event = Fixture::event("DTSTART:20260101T090000Z\r\n");
    assert!(matches!(
        Fixture::parse_document(&Fixture::document(&event.repeat(2)), BUDGET),
        Err(IcsError::DuplicateUid)
    ));
    assert!(
        Fixture::parse_document(
            &Fixture::document(&event).replace("END:VEVENT", "END:VTODO"),
            BUDGET
        )
        .is_err()
    );
    assert!(
        Fixture::parse_document(&(Fixture::document(&event) + "UID:trailing\r\n"), BUDGET).is_err()
    );
}

#[test]
fn rejects_orphans_range_exceptions_and_excessive_validation_work() {
    assert!(matches!(
        Fixture::parse("RECURRENCE-ID:20260101T090000Z\r\nDTSTART:20260102T090000Z\r\n"),
        Err(IcsError::OrphanException)
    ));
    let master = Fixture::event("DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=20\r\n");
    let exception = Fixture::event(
        "RECURRENCE-ID;RANGE=THISANDFUTURE:20260101T090000Z\r\nDTSTART:20260102T090000Z\r\n",
    );
    assert!(matches!(
        Fixture::parse_document(&Fixture::document(&(master + &exception)), BUDGET),
        Err(IcsError::Unsupported(_))
    ));
    let document = Fixture::document(&Fixture::event(
        "DTSTART:20260101T090000Z\r\nRRULE:FREQ=DAILY;COUNT=20\r\nEXDATE:20260120T090000Z\r\n",
    ));
    assert!(matches!(
        Fixture::parse_document(&document, NonZeroUsize::MIN),
        Err(IcsError::Schedule(schedules_core::Error::CandidateLimit))
    ));
}

#[test]
fn normalizes_zoned_one_off_without_moving_instant() {
    let placement = Placement::instant(DateTime::UNIX_EPOCH, Tz::America__New_York).unwrap();
    let mut schedule = Schedule::once(placement);
    schedule
        .set_override(
            OccurrenceKey::Calendar(placement.local_start()),
            OccurrenceOverride::Cancel,
            BUDGET,
        )
        .unwrap();
    let event = IcsEvent::new("zoned".into(), schedule).unwrap();
    let encoded = Fixture::export(&[event], DateTime::UNIX_EPOCH).unwrap();
    assert!(encoded.contains("DTSTART:19700101T000000Z\r\n"));
    assert!(encoded.contains("EXDATE:19700101T000000Z\r\n"));
    assert!(Fixture::parse_document(&encoded, BUDGET).is_ok());
}

#[test]
fn rejects_export_precision_loss_and_date_exclusions() {
    const SUBSECOND_PRECISION: Duration = Duration::nanoseconds(1);
    let subsecond = DateTime::UNIX_EPOCH + SUBSECOND_PRECISION;
    let event = IcsEvent::new(
        "precise".into(),
        Schedule::once(Placement::instant(subsecond, Tz::UTC).unwrap()),
    )
    .unwrap();
    assert!(matches!(
        Fixture::export(&[event], DateTime::UNIX_EPOCH),
        Err(IcsError::Unsupported(_))
    ));
    let mut schedule = Schedule::once(Placement::instant(DateTime::UNIX_EPOCH, Tz::UTC).unwrap());
    schedule.exclude_date(DateTime::UNIX_EPOCH.date_naive());
    assert!(matches!(
        Fixture::export(
            &[IcsEvent::new("excluded".into(), schedule).unwrap()],
            DateTime::UNIX_EPOCH
        ),
        Err(IcsError::Unsupported(_))
    ));
    assert!(
        IcsEvent::new(
            "injection\r\nBEGIN:VEVENT".into(),
            Schedule::once(Placement::instant(DateTime::UNIX_EPOCH, Tz::UTC).unwrap())
        )
        .is_err()
    );
}
