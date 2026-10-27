//! Calendar-aware schedule definitions and bounded occurrence expansion.
//!
//! Build a [`Placement`], attach an optional [`Recurrence`] with [`Schedule`],
//! then query a [`QueryWindow`]. [`ScheduleStore`] associates schedules with
//! application identifiers and queries across them.
//!
//! Calendar rules preserve local wall time; elapsed rules preserve exact UTC
//! spacing. All intervals and query windows have inclusive starts and exclusive
//! ends. All-day dates resolve in the query's viewing timezone.
//!
//! Use [`Schedule::iter`] for lazy, fallible expansion or
//! [`Schedule::occurrences`] for sorted results. Both require explicit candidate
//! and occurrence budgets. [`OccurrenceKey`] retains original identity when an
//! [`OccurrenceOverride`] moves or cancels an occurrence.
//!
//! # Examples
//!
//! ```
//! use std::num::NonZeroUsize;
//! use schedules_core::{DateRange, DateTime, Duration, Error, Placement, QueryWindow, Schedule, Tz};
//! const WINDOW_LENGTH: Duration = Duration::days(1);
//! let start = DateTime::UNIX_EPOCH;
//! let end = start.checked_add_signed(WINDOW_LENGTH).ok_or(Error::OutOfRange)?;
//! let schedule = Schedule::once(Placement::instant(start, Tz::UTC)?);
//! let window = QueryWindow {
//!     range: DateRange::new(start, end)?,
//!     all_day_zone: Tz::UTC,
//!     candidate_limit: NonZeroUsize::MIN,
//!     occurrence_limit: NonZeroUsize::MIN,
//! };
//! let occurrences = schedule.occurrences(window)?;
//! assert_eq!(occurrences.first().map(|entry| entry.placement()), Some(schedule.placement()));
//! # Ok::<(), Error>(())
//! ```

mod analysis;
mod date_range;
mod error;
mod occurrence;
mod query;
mod recurrence;
mod schedule;
mod schedule_store;
mod temporal;

#[doc(inline)]
pub use analysis::{Overlap, ScheduleAnalysis, ScheduleDensity};
/// Generic calendar views, available with the `calendar-views` feature.
#[cfg(feature = "calendar-views")]
#[doc(inline)]
pub use calendar_views as calendar;
#[doc(inline)]
pub use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};
#[doc(inline)]
pub use chrono_tz::Tz;
#[doc(inline)]
pub use date_range::DateRange;
#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use occurrence::{Occurrence, OccurrenceKey, OccurrenceOverride};
#[doc(inline)]
pub use query::QueryWindow;
#[doc(inline)]
pub use recurrence::{CalendarRule, Frequency, Recurrence, Termination, TimeSet, WeekdaySet};
#[doc(inline)]
pub use schedule::{Occurrences, Schedule};
#[doc(inline)]
pub use schedule_store::{ScheduleStore, ScheduledOccurrence};
#[doc(inline)]
pub use temporal::{AmbiguityPolicy, Placement, ResolutionPolicy};
