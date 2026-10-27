use ics_document::{Error, IcsDocument, Invalid};

struct Fixture;
impl Fixture {
    fn document(events: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\n{events}END:VCALENDAR\r\n")
    }
    fn event(properties: &str) -> String {
        format!(
            "BEGIN:VEVENT\r\nUID:sample\r\nDTSTAMP:20260101T000000Z\r\n{properties}END:VEVENT\r\n"
        )
    }
}

#[test]
fn wire_data_is_readable_without_a_scheduling_model() {
    const START_LINE: usize = 7;
    const DESCRIPTION_START_LINE: usize = 9;
    const DESCRIPTION_END_LINE: usize = 10;
    let input = Fixture::document(&Fixture::event(concat!(
        "DTSTART;TZID=Europe/Paris:20260101T090000\r\n",
        "RRULE:FREQ=HOURLY;BYMINUTE=15\r\n",
        "DESCRIPTION:folded\r\n continuation\r\n",
    )));
    let document = IcsDocument::deserialize(&input).unwrap();
    let event = &document.events()[0];
    let start = event
        .properties()
        .iter()
        .find(|p| p.name() == "DTSTART")
        .unwrap();
    assert_eq!(start.value(), Some("20260101T090000"));
    assert_eq!(start.location().start_line().get(), START_LINE);
    let (name, values) = start.parameters().next().unwrap();
    assert_eq!(name, "TZID");
    assert_eq!(values, ["Europe/Paris"]);
    let description = event
        .properties()
        .iter()
        .find(|p| p.name() == "DESCRIPTION")
        .unwrap();
    assert_eq!(description.value(), Some("foldedcontinuation"));
    assert_eq!(
        description.location().start_line().get(),
        DESCRIPTION_START_LINE
    );
    assert_eq!(
        description.location().end_line().get(),
        DESCRIPTION_END_LINE
    );
    assert!(
        document
            .serialize()
            .contains("RRULE:FREQ=HOURLY;BYMINUTE=15\r\n")
    );
}

#[test]
fn syntax_and_structure_errors_retain_original_locations_and_causes() {
    use std::error::Error as _;
    const MALFORMED_LINE: usize = 7;
    let input = Fixture::document(&Fixture::event("BROKEN\r\n"));
    let error = IcsDocument::deserialize(&input).unwrap_err();
    assert_eq!(error.location().unwrap().start_line().get(), MALFORMED_LINE);
    assert!(matches!(error.kind(), Error::Syntax(_)));
    assert!(error.source().unwrap().source().is_some());

    let mismatched = Fixture::document(&Fixture::event("")).replace("END:VEVENT", "END:VCALENDAR");
    assert!(matches!(
        IcsDocument::deserialize(&mismatched).unwrap_err().kind(),
        Error::Invalid(Invalid::ComponentEnd)
    ));
    let outside = Fixture::document("") + "UID:outside\r\n";
    assert!(matches!(
        IcsDocument::deserialize(&outside).unwrap_err().kind(),
        Error::Invalid(Invalid::OutsideCalendar)
    ));
}

#[test]
fn serialization_preserves_utf8_and_whitespace_across_folds() {
    const REPETITIONS: usize = 80;
    const MAX_LINE_OCTETS: usize = 75;
    let value = format!(" {} ", "日 ".repeat(REPETITIONS));
    let document = IcsDocument::deserialize(&Fixture::document(&Fixture::event(&format!(
        "SUMMARY:{value}\r\n"
    ))))
    .unwrap();
    let text = document.serialize();
    assert!(text.split("\r\n").all(|line| line.len() <= MAX_LINE_OCTETS));
    let decoded = IcsDocument::deserialize(&text).unwrap();
    let summary = decoded.events()[0]
        .properties()
        .iter()
        .find(|p| p.name() == "SUMMARY")
        .unwrap();
    assert_eq!(summary.value(), Some(value.as_str()));
}

#[test]
fn document_round_trip_preserves_metadata_parameters_and_alarms() {
    let input = Fixture::document(&Fixture::event(concat!(
        "DTSTART:20260101T090000Z\r\n",
        "SUMMARY:Caller metadata\r\n",
        "ATTENDEE;CN=\"Person, Name\":mailto:person@example.com\r\n",
        "X-APP-VALUE:opaque\\,text\r\n",
        "BEGIN:VALARM\r\n",
        "ACTION:DISPLAY\r\n",
        "TRIGGER:-PT15M\r\n",
        "DESCRIPTION:Reminder\r\n",
        "END:VALARM\r\n",
    )));
    let document = IcsDocument::deserialize(&input).unwrap();
    let encoded = document.serialize();
    assert!(encoded.contains("SUMMARY:Caller metadata\r\n"));
    assert!(encoded.contains("ATTENDEE;CN=\"Person, Name\":mailto:person@example.com\r\n"));
    assert!(encoded.contains("X-APP-VALUE:opaque\\,text\r\n"));
    assert!(encoded.contains(
        "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\n"
    ));
    assert_eq!(
        IcsDocument::deserialize(&encoded).unwrap().serialize(),
        encoded
    );
    assert_eq!(document.serialize(), encoded);
    const EXPECTED_EVENT_COUNT: usize = 1;
    assert_eq!(document.events().len(), EXPECTED_EVENT_COUNT);
}
