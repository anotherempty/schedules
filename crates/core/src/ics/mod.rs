//! Optional, strict RFC 5545 timing adapter for UTC and date-only events.

pub(crate) mod error;

use ics_document::IcsLocation;

use error::{IcsError, IcsField, IcsInvalid, IcsUnsupported};

use std::{
    collections::BTreeMap,
    num::{NonZeroU32, NonZeroUsize},
    str::FromStr,
};

use chrono::{Datelike, Timelike};
use ics_document::{IcsDocument, IcsProperty};

use crate::{
    CalendarRule, DateTime, Frequency, NaiveDate, NaiveDateTime, OccurrenceKey, OccurrenceOverride,
    Placement, Recurrence, ResolutionPolicy, Schedule, Termination, Tz, Utc, Weekday,
};

const DATE_FORMAT: &str = "%Y%m%d";
const UTC_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// A Chrono weekday encoded using RFC 5545's two-letter names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IcsWeekday(Weekday);

impl FromStr for IcsWeekday {
    type Err = IcsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let day = match value {
            "MO" => Weekday::Mon,
            "TU" => Weekday::Tue,
            "WE" => Weekday::Wed,
            "TH" => Weekday::Thu,
            "FR" => Weekday::Fri,
            "SA" => Weekday::Sat,
            "SU" => Weekday::Sun,
            _ => return Err(IcsError::Unsupported(IcsUnsupported::Weekday)),
        };
        Ok(Self(day))
    }
}

impl IcsWeekday {
    fn as_str(self) -> &'static str {
        match self.0 {
            Weekday::Mon => "MO",
            Weekday::Tue => "TU",
            Weekday::Wed => "WE",
            Weekday::Thu => "TH",
            Weekday::Fri => "FR",
            Weekday::Sat => "SA",
            Weekday::Sun => "SU",
        }
    }

    fn all() -> impl Iterator<Item = Self> {
        std::iter::successors(Some(Self(Weekday::Mon)), |day| {
            (day.0 != Weekday::Sun).then(|| Self(day.0.succ()))
        })
    }
}

/// A schedule paired with an application-supplied iCalendar textual UID.
///
/// Descriptions, attendees, alarms, and other metadata are intentionally not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcsEvent {
    uid: String,
    schedule: Schedule,
}

impl IcsEvent {
    fn location_in(props: &[IcsProperty], field: IcsField, fallback: IcsLocation) -> IcsLocation {
        props
            .iter()
            .find(|prop| prop.name() == field.as_str())
            .map(|prop| prop.location())
            .unwrap_or(fallback)
    }

    /// Associates a schedule with a nonempty textual UID.
    ///
    /// # Errors
    /// Returns [`IcsError::Invalid`] for an empty UID or control characters.
    pub fn new(uid: String, schedule: Schedule) -> Result<Self, IcsError> {
        if uid.is_empty() || uid.chars().any(char::is_control) {
            return Err(IcsError::Invalid(IcsInvalid::Field(IcsField::Uid)));
        }
        Ok(Self { uid, schedule })
    }

    /// Returns the decoded textual UID.
    pub fn uid(&self) -> &str {
        &self.uid
    }

    /// Borrows the scheduling definition.
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// Transfers ownership of the UID and schedule to the caller.
    pub fn into_parts(self) -> (String, Schedule) {
        (self.uid, self.schedule)
    }
    fn property(props: &[IcsProperty], name: IcsField) -> Result<Option<&IcsProperty>, IcsError> {
        let mut matching = props.iter().filter(|p| p.name() == name.as_str());
        let found = matching.next();
        if let Some(duplicate) = matching.next() {
            return Err(IcsError::Invalid(IcsInvalid::Duplicate(name)).at(duplicate.location()));
        }
        Ok(found)
    }

    fn value(props: &[IcsProperty], name: IcsField) -> Result<Option<&str>, IcsError> {
        Self::property(props, name)?
            .map(|p| {
                if p.parameters().next().is_some() {
                    return Err(
                        IcsError::Unsupported(IcsUnsupported::PropertyParameters).at(p.location())
                    );
                }
                p.value()
                    .ok_or(IcsError::Invalid(IcsInvalid::Missing(name)))
            })
            .transpose()
    }

    fn placement(props: &[IcsProperty]) -> Result<Placement, IcsError> {
        let start = Self::property(props, IcsField::Start)?
            .ok_or(IcsError::Invalid(IcsInvalid::Missing(IcsField::Start)))?;
        let start = DateValue::parse(
            start,
            start
                .value()
                .ok_or(IcsError::Invalid(IcsInvalid::Missing(IcsField::Start)))?,
        )?;
        let end = Self::property(props, IcsField::End)?
            .map(|p| {
                DateValue::parse(
                    p,
                    p.value()
                        .ok_or(IcsError::Invalid(IcsInvalid::Missing(IcsField::End)))?,
                )
            })
            .transpose()?;
        let location = Self::property(props, IcsField::End)?
            .or(Self::property(props, IcsField::Start)?)
            .map(|p| p.location())
            .ok_or(IcsInvalid::Missing(IcsField::Start))?;
        (|| {
            Ok(match (start, end) {
                (DateValue::Date(start), None) => {
                    Placement::all_day(start, start.succ_opt().ok_or(crate::Error::OutOfRange)?)?
                }
                (DateValue::Date(start), Some(DateValue::Date(end))) => {
                    Placement::all_day(start, end)?
                }
                (DateValue::Utc(start), None) => {
                    Placement::point(start, Tz::UTC, ResolutionPolicy::default())?
                }
                (DateValue::Utc(start), Some(DateValue::Utc(end))) => {
                    Placement::timed(start, Tz::UTC, end - start, ResolutionPolicy::default())?
                }
                _ => return Err(IcsError::Invalid(IcsInvalid::EndTypeMismatch)),
            })
        })()
        .map_err(|error: IcsError| error.at(location))
    }

    fn key(
        prop: &IcsProperty,
        value: &str,
        placement: Placement,
    ) -> Result<OccurrenceKey, IcsError> {
        match (DateValue::parse(prop, value)?, placement.dates().is_some()) {
            (DateValue::Date(date), true) => Ok(OccurrenceKey::Date(date)),
            (DateValue::Utc(time), false) => Ok(OccurrenceKey::Calendar(time)),
            _ => Err(IcsError::Invalid(IcsInvalid::ExceptionTypeMismatch).at(prop.location())),
        }
    }

    fn rule(value: &str, placement: Placement) -> Result<Recurrence, IcsError> {
        let upper = value.to_ascii_uppercase();
        let mut parts = BTreeMap::new();
        for part in upper.split(';') {
            let (key, value) = part
                .split_once('=')
                .ok_or(IcsError::Invalid(IcsInvalid::Field(
                    IcsField::RecurrenceRule,
                )))?;
            if parts.insert(key, value).is_some() {
                return Err(IcsError::Invalid(IcsInvalid::Duplicate(
                    IcsField::RecurrenceRule,
                )));
            }
        }
        let frequency = Frequency::from_ics(
            parts
                .remove("FREQ")
                .ok_or(IcsError::Invalid(IcsInvalid::Missing(IcsField::Frequency)))?,
        )?;
        let interval = parts
            .remove("INTERVAL")
            .map(|v| {
                v.parse::<NonZeroU32>().map_err(|source| {
                    IcsError::Invalid(IcsInvalid::Integer {
                        field: IcsField::Interval,
                        source,
                    })
                })
            })
            .transpose()?
            .unwrap_or(NonZeroU32::MIN);
        let count = parts.remove("COUNT");
        let until = parts.remove("UNTIL");
        let termination = match (count, until) {
            (Some(_), Some(_)) => {
                return Err(IcsError::Invalid(IcsInvalid::ConflictingTermination));
            }
            (Some(v), None) => Termination::Count(v.parse().map_err(|source| {
                IcsError::Invalid(IcsInvalid::Integer {
                    field: IcsField::Count,
                    source,
                })
            })?),
            (None, Some(v)) => {
                let end = match DateValue::parse_value(v, placement.dates().is_some())? {
                    DateValue::Date(date) => date.and_time(chrono::NaiveTime::MIN),
                    DateValue::Utc(time) => time,
                };
                Termination::Until(end)
            }
            (None, None) => Termination::Forever,
        };
        if parts.remove("WKST").is_some_and(|v| v != "MO") {
            return Err(IcsError::Unsupported(IcsUnsupported::WeekStart));
        }
        let mut weekdays = Vec::new();
        if let Some(days) = parts.remove("BYDAY") {
            for day in days.split(',') {
                weekdays.push(day.parse::<IcsWeekday>()?.0);
            }
        }
        if !parts.is_empty() {
            return Err(IcsError::Unsupported(IcsUnsupported::RecurrencePart));
        }
        Ok(Recurrence::calendar(
            CalendarRule::new(frequency, interval, termination)?.with_weekdays(weekdays),
        ))
    }

    fn write_rule(recurrence: &Recurrence, placement: Placement) -> Result<String, IcsError> {
        let rule = recurrence
            .calendar_rule()
            .ok_or(IcsError::Unsupported(IcsUnsupported::ElapsedRecurrence))?;
        if !rule.times().is_empty() {
            return Err(IcsError::Unsupported(IcsUnsupported::ExplicitTimes));
        }
        let frequency = rule.frequency().as_ics();
        let mut text = format!("RRULE:FREQ={frequency};INTERVAL={}", rule.interval());
        let days: Vec<_> = IcsWeekday::all()
            .filter(|day| rule.weekdays().contains(day.0))
            .map(IcsWeekday::as_str)
            .collect();
        if !days.is_empty() {
            text.push_str(&format!(";BYDAY={}", days.join(",")));
        }
        match rule.termination() {
            Termination::Forever => {}
            Termination::Count(count) => text.push_str(&format!(";COUNT={count}")),
            Termination::Until(until) => {
                DateValue::check_datetime(until)?;
                let format = if placement.dates().is_some() {
                    DATE_FORMAT
                } else {
                    UTC_FORMAT
                };
                text.push_str(&format!(";UNTIL={}", until.format(format)));
            }
            Termination::UntilInstant(_) => {
                return Err(IcsError::Unsupported(IcsUnsupported::CalendarInstantCutoff));
            }
        }
        Ok(text)
    }

    fn header(lines: &mut Vec<String>, event: &IcsEvent, stamp: DateTime<Utc>) {
        lines.push("BEGIN:VEVENT".into());
        let uid = event
            .uid
            .replace('\\', "\\\\")
            .replace(';', "\\;")
            .replace(',', "\\,");
        lines.push(format!("UID:{uid}"));
        lines.push(format!("DTSTAMP:{}", stamp.format(UTC_FORMAT)));
    }

    fn write_placement(lines: &mut Vec<String>, placement: Placement) -> Result<(), IcsError> {
        if let Some((start, end)) = placement.dates() {
            DateValue::check_date(start)?;
            DateValue::check_date(end)?;
            lines.push(format!("DTSTART;VALUE=DATE:{}", start.format(DATE_FORMAT)));
            lines.push(format!("DTEND;VALUE=DATE:{}", end.format(DATE_FORMAT)));
        } else {
            let (start, end) = placement.bounds(Tz::UTC)?;
            DateValue::check_datetime(start.naive_utc())?;
            lines.push(format!("DTSTART:{}", start.format(UTC_FORMAT)));
            if let Some(end) = end {
                DateValue::check_datetime(end.naive_utc())?;
                lines.push(format!("DTEND:{}", end.format(UTC_FORMAT)));
            }
        }
        Ok(())
    }

    fn write_key(name: &str, key: OccurrenceKey, placement: Placement) -> Result<String, IcsError> {
        match key {
            OccurrenceKey::Date(date) => {
                DateValue::check_date(date)?;
                Ok(format!("{name};VALUE=DATE:{}", date.format(DATE_FORMAT)))
            }
            OccurrenceKey::Calendar(local) => {
                // One-off keys can carry the original local time in a non-UTC zone.
                let instant = if placement.timezone() == Some(Tz::UTC) {
                    local.and_utc()
                } else if local == placement.local_start() {
                    placement.bounds(Tz::UTC)?.0
                } else {
                    return Err(IcsError::Unsupported(IcsUnsupported::ZonedExceptionKey));
                };
                DateValue::check_datetime(instant.naive_utc())?;
                Ok(format!("{name}:{}", instant.format(UTC_FORMAT)))
            }
            OccurrenceKey::Elapsed(_) => {
                Err(IcsError::Unsupported(IcsUnsupported::ElapsedException))
            }
        }
    }

    fn decode_text(value: &str) -> Result<String, IcsError> {
        let mut result = String::new();
        let mut chars = value.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some(escaped @ ('\\' | ',' | ';')) => result.push(escaped),
                    Some('n' | 'N') => result.push('\n'),
                    _ => return Err(IcsError::Invalid(IcsInvalid::TextEscape)),
                }
            } else {
                result.push(ch);
            }
        }
        Ok(result)
    }
}

/// Schedule conversions for the independent [`IcsDocument`] type.
///
/// Import this trait to call `to_schedules` and `from_schedules`.
///
/// ```
/// use std::num::NonZeroUsize;
/// use schedules_core::{DateTime, IcsDocument, IcsError, IcsEvent, IcsScheduleExt, Placement, Schedule, Tz};
/// let event = IcsEvent::new("id@example.com".into(), Schedule::once(Placement::instant(DateTime::UNIX_EPOCH, Tz::UTC)?))?;
/// let document = IcsDocument::from_schedules(std::slice::from_ref(&event), DateTime::UNIX_EPOCH)?;
/// assert_eq!(document.to_schedules(NonZeroUsize::MIN)?, [event]);
/// # Ok::<(), IcsError>(())
/// ```
pub trait IcsScheduleExt: Sized {
    /// Converts timing to UID-associated schedules, preserving original error locations.
    ///
    /// Metadata stays in the document. The budget applies per exception.
    /// # Errors
    /// Returns typed conversion errors for incompatible timing, identity conflicts,
    /// or exhausted exception-validation budgets.
    fn to_schedules(&self, candidate_limit: NonZeroUsize) -> Result<Vec<IcsEvent>, IcsError>;

    /// Builds a document from schedules and a caller-supplied timestamp.
    /// # Errors
    /// Returns typed conversion errors if timing cannot be represented exactly.
    fn from_schedules(events: &[IcsEvent], stamp: DateTime<Utc>) -> Result<Self, IcsError>;
}

impl IcsScheduleExt for IcsDocument {
    /// Converts document timing into UID-associated schedules.
    ///
    /// Metadata remains in this document. The budget limits validation work per
    /// exception; callers must bound document size and event count separately.
    ///
    /// # Errors
    /// Returns [`IcsError`] for malformed or unsupported timing, duplicate UIDs,
    /// or exceptions that fail bounded occurrence-key validation.
    fn to_schedules(&self, candidate_limit: NonZeroUsize) -> Result<Vec<IcsEvent>, IcsError> {
        let mut context = IcsLocation::new(NonZeroUsize::MIN);
        (|| {
            let calendar = self;
            context = calendar.location();
            let version = IcsEvent::value(calendar.properties(), IcsField::Version)?
                .ok_or(IcsInvalid::Missing(IcsField::Version))?;
            if version != "2.0" {
                return Err(
                    (IcsError::Invalid(IcsInvalid::Field(IcsField::Version))).at(
                        IcsEvent::location_in(calendar.properties(), IcsField::Version, context),
                    ),
                );
            }
            if IcsEvent::value(calendar.properties(), IcsField::Method)?.is_some() {
                return Err(
                    (IcsError::Unsupported(IcsUnsupported::SchedulingMethod)).at(
                        IcsEvent::location_in(calendar.properties(), IcsField::Method, context),
                    ),
                );
            }
            if let Some(scale) = IcsEvent::value(calendar.properties(), IcsField::CalendarScale)?
                && !scale.eq_ignore_ascii_case("GREGORIAN")
            {
                return Err((IcsError::Unsupported(IcsUnsupported::CalendarScale)).at(
                    IcsEvent::location_in(calendar.properties(), IcsField::CalendarScale, context),
                ));
            }
            let mut events = BTreeMap::new();
            let mut exceptions = Vec::new();
            for props in calendar.events() {
                context = props.location();
                let uid = IcsEvent::decode_text(
                    IcsEvent::value(props, IcsField::Uid)?
                        .ok_or(IcsError::Invalid(IcsInvalid::Missing(IcsField::Uid)))?,
                )
                .map_err(|error| error.at(IcsEvent::location_in(props, IcsField::Uid, context)))?;
                for (name, unsupported) in [
                    (IcsField::RecurrenceDate, IcsUnsupported::RecurrenceDates),
                    (IcsField::ExceptionRule, IcsUnsupported::ExceptionRule),
                    (IcsField::Duration, IcsUnsupported::Duration),
                ] {
                    if IcsEvent::value(props, name)?.is_some() {
                        return Err(IcsError::Unsupported(unsupported)
                            .at(IcsEvent::location_in(props, name, context)));
                    }
                }
                if IcsEvent::property(props, IcsField::RecurrenceId)?.is_some() {
                    exceptions.push((uid, props));
                    continue;
                }
                if IcsEvent::value(props, IcsField::Status)?
                    .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
                {
                    return Err((IcsError::Unsupported(IcsUnsupported::CancelledMaster))
                        .at(IcsEvent::location_in(props, IcsField::Status, context)));
                }
                let placement = IcsEvent::placement(props)?;
                let mut schedule = match IcsEvent::value(props, IcsField::RecurrenceRule)? {
                    Some(value) => {
                        let location = IcsEvent::property(props, IcsField::RecurrenceRule)?
                            .map(|p| p.location())
                            .unwrap_or(context);
                        let rule =
                            IcsEvent::rule(value, placement).map_err(|error| error.at(location))?;
                        Schedule::repeating(placement, rule)
                            .map_err(|error| IcsError::from(error).at(location))?
                    }
                    None => Schedule::once(placement),
                };
                for prop in props.iter().filter(|p| p.name() == "EXDATE") {
                    for value in prop
                        .value()
                        .ok_or(IcsError::Invalid(IcsInvalid::Missing(
                            IcsField::ExceptionDate,
                        )))?
                        .split(',')
                    {
                        let key = IcsEvent::key(prop, value, placement)?;
                        schedule
                            .set_override(key, OccurrenceOverride::Cancel, candidate_limit)
                            .map_err(|error| IcsError::from(error).at(prop.location()))?;
                    }
                }
                let entry = IcsEvent::new(uid.clone(), schedule).map_err(|error| {
                    error.at(props
                        .iter()
                        .find(|p| p.name() == IcsField::Uid.as_str())
                        .map(|p| p.location())
                        .unwrap_or(context))
                })?;
                if events.insert(uid, entry).is_some() {
                    return Err((IcsError::DuplicateUid).at(IcsEvent::location_in(
                        props,
                        IcsField::Uid,
                        context,
                    )));
                }
            }
            let mut seen = std::collections::BTreeSet::new();
            for (uid, props) in exceptions {
                context = props.location();
                let entry = events.get_mut(&uid).ok_or_else(|| {
                    IcsError::OrphanException.at(IcsEvent::location_in(
                        props,
                        IcsField::RecurrenceId,
                        context,
                    ))
                })?;
                let prop = IcsEvent::property(props, IcsField::RecurrenceId)?.ok_or(
                    IcsError::Invalid(IcsInvalid::Missing(IcsField::RecurrenceId)),
                )?;
                let key = IcsEvent::key(
                    prop,
                    prop.value().ok_or(IcsError::Invalid(IcsInvalid::Missing(
                        IcsField::RecurrenceId,
                    )))?,
                    entry.schedule.placement(),
                )?;
                if !seen.insert((uid, key)) {
                    return Err(
                        (IcsError::DuplicateException { key }).at(IcsEvent::location_in(
                            props,
                            IcsField::RecurrenceId,
                            context,
                        )),
                    );
                }
                if IcsEvent::value(props, IcsField::RecurrenceRule)?.is_some()
                    || IcsEvent::value(props, IcsField::ExceptionDate)?.is_some()
                {
                    return Err(IcsError::Unsupported(IcsUnsupported::RecurrenceOnException));
                }
                let change = if IcsEvent::value(props, IcsField::Status)?
                    .is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
                {
                    OccurrenceOverride::Cancel
                } else {
                    let replacement = IcsEvent::placement(props)?;
                    if replacement.dates().is_some() != entry.schedule.placement().dates().is_some()
                    {
                        return Err((IcsError::Unsupported(IcsUnsupported::ExceptionTypeChange))
                            .at(IcsEvent::location_in(props, IcsField::Start, context)));
                    }
                    OccurrenceOverride::Replace(replacement)
                };
                // RFC EXDATE takes precedence over a detached replacement.
                if !entry.schedule.overrides().contains_key(&key) {
                    entry
                        .schedule
                        .set_override(key, change, candidate_limit)
                        .map_err(|error| IcsError::from(error).at(prop.location()))?;
                }
            }
            Ok(events.into_values().collect())
        })()
        .map_err(|error: IcsError| error.at(context))
    }

    /// Builds a document from schedules and an explicit DTSTAMP.
    ///
    /// Caller supplies DTSTAMP; no system clock or application metadata is used.
    /// One-off zoned placements are converted to UTC. Recurrence must use UTC or
    /// date-only placements. Stored date exclusions and elapsed recurrence are
    /// rejected; explicit occurrence cancellations are supported.
    ///
    /// # Errors
    /// Returns [`IcsError`] if conversion would lose timing precision/semantics,
    /// or UIDs are duplicated. No partial document is returned.
    fn from_schedules(events: &[IcsEvent], stamp: DateTime<Utc>) -> Result<Self, IcsError> {
        DateValue::check_datetime(stamp.naive_utc())?;
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//schedules-core//ICS//EN".into(),
        ];
        let mut ids = std::collections::BTreeSet::new();
        for event in events {
            if !ids.insert(event.uid()) {
                return Err(IcsError::DuplicateUid);
            }
            let schedule = &event.schedule;
            if !schedule.excluded_dates().is_empty() {
                return Err(IcsError::Unsupported(IcsUnsupported::DateExclusions));
            }
            if schedule.recurrence().is_some()
                && schedule
                    .placement()
                    .timezone()
                    .is_some_and(|tz| tz != Tz::UTC)
            {
                return Err(IcsError::Unsupported(IcsUnsupported::ZonedRecurrence));
            }
            IcsEvent::header(&mut lines, event, stamp);
            IcsEvent::write_placement(&mut lines, schedule.placement())?;
            if let Some(recurrence) = schedule.recurrence() {
                lines.push(IcsEvent::write_rule(recurrence, schedule.placement())?);
            }
            for (key, change) in schedule.overrides() {
                if matches!(change, OccurrenceOverride::Cancel) {
                    lines.push(IcsEvent::write_key("EXDATE", *key, schedule.placement())?);
                }
            }
            lines.push("END:VEVENT".into());
            for (key, change) in schedule.overrides() {
                if let OccurrenceOverride::Replace(placement) = change {
                    if placement.dates().is_some() != schedule.placement().dates().is_some() {
                        return Err(IcsError::Unsupported(IcsUnsupported::ExceptionTypeChange));
                    }
                    IcsEvent::header(&mut lines, event, stamp);
                    lines.push(IcsEvent::write_key(
                        "RECURRENCE-ID",
                        *key,
                        schedule.placement(),
                    )?);
                    IcsEvent::write_placement(&mut lines, *placement)?;
                    lines.push("END:VEVENT".into());
                }
            }
        }
        lines.push("END:VCALENDAR".into());
        Ok(IcsDocument::deserialize(&lines.join("\r\n"))?)
    }
}

#[derive(Debug, Clone, Copy)]
enum DateValue {
    Date(NaiveDate),
    Utc(NaiveDateTime),
}

impl DateValue {
    fn parse(prop: &IcsProperty, value: &str) -> Result<Self, IcsError> {
        Self::parse_property(prop, value).map_err(|error| error.at(prop.location()))
    }

    fn parse_property(prop: &IcsProperty, value: &str) -> Result<Self, IcsError> {
        let mut date = false;
        let mut seen = false;
        for (name, values) in prop.parameters() {
            if name != "VALUE" {
                return Err(IcsError::Unsupported(IcsUnsupported::TimingParameter));
            }
            if seen || values.len() != 1 {
                return Err(IcsError::Invalid(IcsInvalid::Field(IcsField::Value)));
            }
            seen = true;
            match values.first().map(String::as_str) {
                Some(v) if v.eq_ignore_ascii_case("DATE") => date = true,
                Some(v) if v.eq_ignore_ascii_case("DATE-TIME") => {}
                _ => return Err(IcsError::Invalid(IcsInvalid::Field(IcsField::Value))),
            }
        }
        Self::parse_value(value, date)
    }

    fn parse_value(value: &str, date: bool) -> Result<Self, IcsError> {
        if date {
            let parsed = NaiveDate::parse_from_str(value, DATE_FORMAT)
                .map_err(|source| IcsError::Invalid(IcsInvalid::DateParse { source }))?;
            Self::check_date(parsed)?;
            if parsed.format(DATE_FORMAT).to_string() != value {
                return Err(IcsError::Invalid(IcsInvalid::Date));
            }
            Ok(Self::Date(parsed))
        } else {
            if !value.ends_with('Z') {
                return Err(IcsError::Unsupported(IcsUnsupported::NonUtcDateTime));
            }
            let parsed = NaiveDateTime::parse_from_str(value, UTC_FORMAT)
                .map_err(|source| IcsError::Invalid(IcsInvalid::DateTimeParse { source }))?;
            Self::check_datetime(parsed)?;
            if parsed.format(UTC_FORMAT).to_string() != value {
                return Err(IcsError::Invalid(IcsInvalid::DateTime));
            }
            Ok(Self::Utc(parsed))
        }
    }

    fn check_date(date: NaiveDate) -> Result<(), IcsError> {
        const LAST_FOUR_DIGIT_YEAR: i32 = 9999;
        if !(0..=LAST_FOUR_DIGIT_YEAR).contains(&date.year()) {
            return Err(IcsError::Unsupported(IcsUnsupported::YearRange));
        }
        Ok(())
    }

    fn check_datetime(time: NaiveDateTime) -> Result<(), IcsError> {
        Self::check_date(time.date())?;
        if time.nanosecond() != 0 {
            return Err(IcsError::Unsupported(IcsUnsupported::TimePrecision));
        }
        Ok(())
    }
}
