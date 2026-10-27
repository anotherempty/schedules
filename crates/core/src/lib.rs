//! Calendar-aware schedule definitions and bounded occurrence expansion.
//!
//! Calendar rules preserve local wall time. Elapsed rules preserve exact duration.
//! All intervals and query windows use inclusive starts and exclusive ends.
//!
//! ```
//! use std::num::NonZeroU32;
//! use scheduler_core::{CalendarRule, Error, Frequency, NaiveDateTime, Placement,
//!     Recurrence, ResolutionPolicy, Schedule, Termination, Tz};
//!
//! fn daily_at(start: NaiveDateTime, zone: Tz) -> Result<Schedule, Error> {
//!     let placement = Placement::point(start, zone, ResolutionPolicy::default())?;
//!     let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN,
//!         Termination::Forever)?;
//!     Schedule::repeating(placement, Recurrence::calendar(rule))
//! }
//! ```
//!
//! Use [`Schedule::iter`] for lazy expansion or [`Schedule::occurrences`] for sorted results.
//! Both require a [`QueryWindow`] with explicit candidate and occurrence budgets.

mod date_range;
mod error;
mod occurrence;
mod query;
mod recurrence;
mod schedule;
mod scheduler;
mod temporal;

pub use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};
pub use chrono_tz::Tz;
pub use date_range::DateRange;
pub use error::Error;
pub use occurrence::{Occurrence, OccurrenceKey, OccurrenceOverride};
pub use query::QueryWindow;
pub use recurrence::{CalendarRule, Frequency, Recurrence, Termination, TimeSet, WeekdaySet};
pub use schedule::{Occurrences, Schedule};
pub use scheduler::{ScheduledOccurrence, Scheduler};
pub use temporal::{AmbiguityPolicy, Placement, ResolutionPolicy};
