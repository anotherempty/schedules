//! Typed range, timezone-boundary, and date arithmetic failures.

use chrono::NaiveDate;
use chrono_tz::Tz;

/// Failures constructing ranges or resolving local day boundaries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// An exclusive end is not later than its inclusive start.
    #[error("end must be later than start")]
    InvalidRange,
    /// Local midnight does not exist in the chosen timezone.
    #[error("midnight on {date} does not exist in {zone}")]
    NonexistentMidnight {
        /// Date whose midnight cannot be resolved.
        date: NaiveDate,
        /// Viewing timezone.
        zone: Tz,
    },
    /// Computing an adjacent date would exceed the supported calendar range.
    #[error("calendar date arithmetic exceeded supported range")]
    OutOfRange,
}
