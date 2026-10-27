use chrono::NaiveDateTime;
use chrono_tz::Tz;

/// Failures in construction, local-time resolution, or bounded expansion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("end must be later than start")]
    InvalidRange,
    #[error("duration must be positive")]
    InvalidDuration,
    #[error("local time {local} is ambiguous in {zone}")]
    AmbiguousTime { local: NaiveDateTime, zone: Tz },
    #[error("local time {local} does not exist in {zone}")]
    NonexistentTime { local: NaiveDateTime, zone: Tz },
    #[error("date or time arithmetic exceeded supported range")]
    OutOfRange,
    #[error("elapsed recurrence requires a timed placement")]
    ElapsedAllDay,
    #[error("all-day recurrence cannot select times of day")]
    AllDayTimes,
    #[error("weekday filters require weekly frequency")]
    InvalidWeekdayFilter,
    #[error("initial placement must match recurrence rule")]
    InvalidAnchor,
    #[error("recurrence cutoff has incompatible time basis or precedes initial placement")]
    InvalidTermination,
    #[error("expansion exceeded candidate budget")]
    CandidateLimit,
    #[error("expansion exceeded occurrence limit")]
    OccurrenceLimit,
    #[error("override key does not identify an occurrence in this schedule")]
    InvalidOccurrenceKey,
}
