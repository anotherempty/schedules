//! Typed ICS failures and their protocol context.

use ics_document::IcsLocation;

/// An iCalendar property, recurrence part, or value parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IcsField {
    /// The `UID` field.
    Uid,
    /// The `VERSION` field.
    Version,
    /// The `METHOD` field.
    Method,
    /// The `CALSCALE` field.
    CalendarScale,
    /// The `RDATE` field.
    RecurrenceDate,
    /// The `EXRULE` field.
    ExceptionRule,
    /// The `DURATION` field.
    Duration,
    /// The `RECURRENCE-ID` field.
    RecurrenceId,
    /// The `STATUS` field.
    Status,
    /// The `RRULE` field.
    RecurrenceRule,
    /// The `EXDATE` field.
    ExceptionDate,
    /// The `DTSTART` field.
    Start,
    /// The `DTEND` field.
    End,
    /// The `FREQ` field.
    Frequency,
    /// The `INTERVAL` field.
    Interval,
    /// The `COUNT` field.
    Count,
    /// The `VALUE` field.
    Value,
}

impl std::fmt::Display for IcsField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl IcsField {
    /// Returns the protocol spelling of this field.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uid => "UID",
            Self::Version => "VERSION",
            Self::Method => "METHOD",
            Self::CalendarScale => "CALSCALE",
            Self::RecurrenceDate => "RDATE",
            Self::ExceptionRule => "EXRULE",
            Self::Duration => "DURATION",
            Self::RecurrenceId => "RECURRENCE-ID",
            Self::Status => "STATUS",
            Self::RecurrenceRule => "RRULE",
            Self::ExceptionDate => "EXDATE",
            Self::Start => "DTSTART",
            Self::End => "DTEND",
            Self::Frequency => "FREQ",
            Self::Interval => "INTERVAL",
            Self::Count => "COUNT",
            Self::Value => "VALUE",
        }
    }
}

/// The precise reason an ICS document or field is malformed.
#[derive(Debug, thiserror::Error)]
pub enum IcsInvalid {
    /// A missing field.
    #[error("missing field: {0}")]
    Missing(IcsField),
    /// A duplicate singleton field.
    #[error("duplicate singleton field: {0}")]
    Duplicate(IcsField),
    /// An invalid field value.
    #[error("invalid field value: {0}")]
    Field(IcsField),
    /// Invalid DTEND value type.
    #[error("invalid DTEND value type")]
    EndTypeMismatch,
    /// Invalid exception value type.
    #[error("invalid exception value type")]
    ExceptionTypeMismatch,
    /// Invalid COUNT with UNTIL.
    #[error("invalid COUNT with UNTIL")]
    ConflictingTermination,
    /// Invalid TEXT escape.
    #[error("invalid TEXT escape")]
    TextEscape,
    /// Invalid DATE.
    #[error("invalid DATE")]
    Date,
    /// Invalid DATE-TIME.
    #[error("invalid DATE-TIME")]
    DateTime,
    /// A numeric field failed parsing or its nonzero constraint.
    #[error("invalid integer in {field}: {source}")]
    Integer {
        /// The field containing the integer.
        field: IcsField,
        /// Original integer parsing failure.
        #[source]
        source: std::num::ParseIntError,
    },
    /// A date failed calendar parsing.
    #[error("invalid date: {source}")]
    DateParse {
        /// Original date parsing failure.
        #[source]
        source: chrono::ParseError,
    },
    /// A timestamp failed calendar parsing.
    #[error("invalid date-time: {source}")]
    DateTimeParse {
        /// Original timestamp parsing failure.
        #[source]
        source: chrono::ParseError,
    },
}

/// A timing construct the adapter cannot represent without changing semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum IcsUnsupported {
    /// Unsupported ordinal or invalid BYDAY.
    #[error("unsupported ordinal or invalid BYDAY")]
    Weekday,
    /// Unsupported METHOD.
    #[error("unsupported METHOD")]
    SchedulingMethod,
    /// Unsupported CALSCALE.
    #[error("unsupported CALSCALE")]
    CalendarScale,
    /// Unsupported cancelled master.
    #[error("unsupported cancelled master")]
    CancelledMaster,
    /// Unsupported recurrence on exception.
    #[error("unsupported recurrence on exception")]
    RecurrenceOnException,
    /// Unsupported exception value type change.
    #[error("unsupported exception value type change")]
    ExceptionTypeChange,
    /// Unsupported date exclusions.
    #[error("unsupported date exclusions")]
    DateExclusions,
    /// Unsupported zoned recurrence.
    #[error("unsupported zoned recurrence")]
    ZonedRecurrence,
    /// Unsupported property parameters.
    #[error("unsupported property parameters")]
    PropertyParameters,
    /// Unsupported WKST other than MO.
    #[error("unsupported WKST other than MO")]
    WeekStart,
    /// Unsupported RRULE part.
    #[error("unsupported RRULE part")]
    RecurrencePart,
    /// Unsupported elapsed recurrence.
    #[error("unsupported elapsed recurrence")]
    ElapsedRecurrence,
    /// Unsupported explicit recurrence times.
    #[error("unsupported explicit recurrence times")]
    ExplicitTimes,
    /// Unsupported instant cutoff on calendar rule.
    #[error("unsupported instant cutoff on calendar rule")]
    CalendarInstantCutoff,
    /// Unsupported zoned exception key.
    #[error("unsupported zoned exception key")]
    ZonedExceptionKey,
    /// Unsupported elapsed exception.
    #[error("unsupported elapsed exception")]
    ElapsedException,
    /// Unsupported TZID or timing parameter.
    #[error("unsupported TZID or timing parameter")]
    TimingParameter,
    /// Unsupported floating or zoned DATE-TIME.
    #[error("unsupported floating or zoned DATE-TIME")]
    NonUtcDateTime,
    /// Unsupported year outside four digits.
    #[error("unsupported year outside four digits")]
    YearRange,
    /// Unsupported subsecond or leap-second precision.
    #[error("unsupported subsecond or leap-second precision")]
    TimePrecision,
    /// Unsupported FREQ.
    #[error("unsupported FREQ")]
    Frequency,
    /// Unsupported RDATE.
    #[error("unsupported RDATE")]
    RecurrenceDates,
    /// Unsupported EXRULE.
    #[error("unsupported EXRULE")]
    ExceptionRule,
    /// Unsupported DURATION.
    #[error("unsupported DURATION")]
    Duration,
}

/// An invalid document or a timing construct that cannot be represented exactly.
#[derive(Debug, thiserror::Error)]
pub enum IcsError {
    /// A parsing failure with its original document location.
    #[error("{location}: {source}")]
    Located {
        /// Physical content-line range, or enclosing component when a field is absent.
        location: IcsLocation,
        /// Concrete failure with its original source chain.
        #[source]
        source: Box<IcsError>,
    },
    /// The independent document parser rejected the input.
    #[error(transparent)]
    Wire(#[from] ics_document::Error),
    /// Typed field or document validation failure.
    #[error(transparent)]
    Invalid(#[from] IcsInvalid),
    /// Conversion would lose scheduling semantics.
    #[error(transparent)]
    Unsupported(#[from] IcsUnsupported),
    /// More than one master or exported event uses the same UID.
    #[error("duplicate event UID")]
    DuplicateUid,
    /// A detached exception has no matching master.
    #[error("exception has no master")]
    OrphanException,
    /// Multiple detached exceptions target the same original occurrence.
    #[error("duplicate exception for {key:?}")]
    DuplicateException {
        /// The original occurrence targeted more than once.
        key: crate::OccurrenceKey,
    },
    /// Schedule construction or exception validation failed.
    #[error(transparent)]
    Schedule(#[from] crate::Error),
}

impl IcsError {
    pub(super) fn at(self, location: IcsLocation) -> Self {
        if self.location().is_some() {
            self
        } else {
            Self::Located {
                location,
                source: Box::new(self),
            }
        }
    }

    /// Returns document location for parsing failures; standalone conversion and export errors have none.
    pub const fn location(&self) -> Option<IcsLocation> {
        match self {
            Self::Located { location, .. } => Some(*location),
            Self::Wire(error) => error.location(),
            _ => None,
        }
    }

    /// Borrows the typed failure beneath source-location context.
    pub fn kind(&self) -> &Self {
        match self {
            Self::Located { source, .. } => source.kind(),
            _ => self,
        }
    }

    /// Removes source-location context, retaining the concrete failure and its underlying causes.
    pub fn into_kind(self) -> Self {
        match self {
            Self::Located { source, .. } => source.into_kind(),
            Self::Wire(error) => Self::Wire(error.into_kind()),
            _ => self,
        }
    }
}
