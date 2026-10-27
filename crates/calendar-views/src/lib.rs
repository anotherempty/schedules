//! Calendar views for arbitrary payloads, independent of scheduling or recurrence.
//!
//! Pair owned or borrowed data with a [`TimeSpan`] using [`CalendarEntry`], then
//! create a [`Calendar`]. Day and week iteration includes empty dates, uses local
//! midnight boundaries, and repeats spanning items on every overlapping day.
//! Times use the proleptic Gregorian calendar and named IANA timezones.
//!
//! # Examples
//!
//! ```
//! # fn main() -> Result<(), ExampleError> {
//! use calendar_views::{Calendar, CalendarEntry, CalendarWindow, NaiveDate, TimeSpan, Tz};
//! let start: NaiveDate = "2026-03-29".parse()?;
//! let end: NaiveDate = "2026-03-30".parse()?;
//! let window = CalendarWindow::new(start, end, Tz::Europe__London)?;
//! let payload = "Appointment";
//! let calendar = Calendar::new(window, [CalendarEntry::new(&payload,
//!     TimeSpan::point(window.utc_start()))]);
//! let days = calendar.days().collect::<Result<Vec<_>, _>>()?;
//! assert_eq!(days.first().map(|day| day.entries().len()), Some(1));
//! # Ok::<(), ExampleError>(())
//! # }
//! # #[derive(Debug, thiserror::Error)]
//! # enum ExampleError {
//! #     #[error(transparent)] Parse(#[from] chrono::ParseError),
//! #     #[error(transparent)] Calendar(#[from] calendar_views::Error),
//! # }
//! ```

mod calendar;
mod error;
mod temporal;

#[doc(inline)]
pub use calendar::{Calendar, CalendarEntry, DayView, DayViews, WeekView, WeekViews};
#[doc(inline)]
pub use chrono::{DateTime, Duration, NaiveDate, Utc, Weekday};
#[doc(inline)]
pub use chrono_tz::Tz;
#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use temporal::{CalendarWindow, TimeSpan};

/// Number of day positions in a Gregorian week.
pub const DAYS_PER_WEEK: usize = 7;
