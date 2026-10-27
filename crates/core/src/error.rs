//! Typed construction, local-time resolution, and bounded query failures.

use chrono::NaiveDateTime;
use chrono_tz::Tz;

/// Failures in construction, local-time resolution, or bounded expansion.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An interval end is not later than its start.
    #[error("end must be later than start")]
    InvalidRange,
    /// An elapsed duration or recurrence interval is not positive.
    #[error("duration must be positive")]
    InvalidDuration,
    /// A local time occurs twice and the resolution policy rejects ambiguity.
    #[error("local time {local} is ambiguous in {zone}")]
    AmbiguousTime {
        /// Local date and time that could not be resolved.
        local: NaiveDateTime,
        /// Timezone in which resolution failed.
        zone: Tz,
    },
    /// A local time has no corresponding instant in the selected timezone.
    #[error("local time {local} does not exist in {zone}")]
    NonexistentTime {
        /// Local date and time that could not be resolved.
        local: NaiveDateTime,
        /// Timezone in which resolution failed.
        zone: Tz,
    },
    /// Date, time, duration, or occurrence-count arithmetic exceeds supported bounds.
    #[error("date or time arithmetic exceeded supported range")]
    OutOfRange,
    /// Elapsed recurrence was paired with an all-day placement.
    #[error("elapsed recurrence requires a timed placement")]
    ElapsedAllDay,
    /// An all-day calendar recurrence explicitly selects times of day.
    #[error("all-day recurrence cannot select times of day")]
    AllDayTimes,
    /// The initial placement does not match the selected weekdays or times.
    #[error("initial placement must match recurrence rule")]
    InvalidAnchor,
    /// The cutoff has the wrong time basis or precedes the initial placement.
    #[error("recurrence cutoff has incompatible time basis or precedes initial placement")]
    InvalidTermination,
    /// Required search or expansion work exceeds the candidate budget.
    #[error("expansion exceeded candidate budget")]
    CandidateLimit,
    /// Matching results exceed the occurrence limit.
    #[error("expansion exceeded occurrence limit")]
    OccurrenceLimit,
    /// A key is invalid for the requested schedule/query, or its anchor is cancelled or excluded.
    #[error("key does not identify an occurrence in the schedule or query results")]
    InvalidOccurrenceKey,
}
