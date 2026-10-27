//! Calendar and elapsed recurrence rules, selections, and termination conditions.

use std::num::NonZeroU32;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};

use crate::Error;

/// Gregorian week length used to align recurrence intervals to Monday.
const DAYS_PER_WEEK: i64 = 7;
/// Gregorian year length in months for comparing calendar month positions.
const MONTHS_PER_YEAR: i64 = 12;
/// Membership bit shifted by a weekday's Monday-based index.
const WEEKDAY_BIT: u8 = 1;

/// Sorted, unique local times selected by a calendar rule.
///
/// The default set is empty and inherits the initial placement's time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeSet {
    times: Vec<NaiveTime>,
}

impl TimeSet {
    /// Inserts a local time, returning `true` only when it was absent.
    pub fn insert(&mut self, time: NaiveTime) -> bool {
        match self.times.binary_search(&time) {
            Ok(_) => false,
            Err(position) => {
                // Binary search returns an insertion position within 0..=len.
                self.times.insert(position, time);
                true
            }
        }
    }

    /// Tests explicit membership; an empty set contains no times.
    pub fn contains(&self, time: NaiveTime) -> bool {
        self.times.binary_search(&time).is_ok()
    }

    /// Returns whether no local times were explicitly selected.
    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    /// Yields selected local times in ascending order, without duplicates.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = NaiveTime> + ExactSizeIterator + '_ {
        self.times.iter().copied()
    }

    pub(crate) fn first(&self) -> Option<NaiveTime> {
        self.times.first().copied()
    }

    pub(crate) fn after(&self, time: NaiveTime) -> Option<NaiveTime> {
        let position = self.times.partition_point(|candidate| *candidate <= time);
        self.times.get(position).copied()
    }
}

/// A set of weekdays, empty by default.
///
/// An empty selection in a weekly [`CalendarRule`] inherits the anchor weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeekdaySet {
    bits: u8,
}

impl WeekdaySet {
    /// Adds a weekday; inserting it again leaves the set unchanged.
    pub fn insert(&mut self, day: Weekday) {
        self.bits |= WEEKDAY_BIT << day.num_days_from_monday();
    }

    /// Tests explicit weekday membership.
    pub fn contains(self, day: Weekday) -> bool {
        self.bits & (WEEKDAY_BIT << day.num_days_from_monday()) != 0
    }

    /// Returns whether no weekdays were explicitly selected.
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
}

/// Calendar unit used by a [`CalendarRule`]'s interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    /// Repeats every selected number of calendar days.
    Daily,
    /// Repeats in Monday-based weeks, using selected weekdays or the anchor weekday.
    Weekly,
    /// Repeats on the anchor day of the month, skipping months without that day.
    Monthly,
    /// Repeats on the anchor month and day, skipping years without that date.
    Yearly,
}

/// A recurrence count limit or inclusive cutoff, defaulting to no cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Termination {
    /// Repeats without a rule cutoff; queries still require finite budgets.
    #[default]
    Forever,
    /// Limits valid original occurrences, including the initial placement.
    ///
    /// Skipped timezone gaps and invalid dates do not count. Cancelled or replaced
    /// occurrences still count; overrides do not extend the series.
    Count(NonZeroU32),
    /// Inclusive local cutoff for calendar recurrence only.
    Until(NaiveDateTime),
    /// Inclusive UTC cutoff for elapsed recurrence only.
    UntilInstant(DateTime<Utc>),
}

/// Gregorian recurrence anchored to the initial placement.
///
/// Weeks start Monday. Monthly and yearly rules skip invalid dates rather than
/// clamping to month ends. Calendar times in timezone gaps are skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRule {
    frequency: Frequency,
    interval: NonZeroU32,
    weekdays: WeekdaySet,
    times: TimeSet,
    termination: Termination,
}

impl CalendarRule {
    /// Creates a calendar rule with an interval measured in units of `frequency`.
    ///
    /// Empty weekday and time selections inherit the initial placement values.
    /// [`crate::Schedule::repeating`] validates the rule against that placement.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroU32;
    /// use schedules_core::{CalendarRule, Error, Frequency, Termination};
    /// let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Termination::Forever)?;
    /// assert!(rule.times().is_empty());
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidTermination`] for [`Termination::UntilInstant`],
    /// which is reserved for elapsed recurrence.
    pub fn new(
        frequency: Frequency,
        interval: NonZeroU32,
        termination: Termination,
    ) -> Result<Self, Error> {
        if matches!(termination, Termination::UntilInstant(_)) {
            return Err(Error::InvalidTermination);
        }
        Ok(Self {
            frequency,
            interval,
            weekdays: WeekdaySet::default(),
            times: TimeSet::default(),
            termination,
        })
    }

    /// Replaces the weekday selection for a weekly rule.
    ///
    /// Empty input uses the anchor weekday; duplicates are removed. The anchor must
    /// belong to the resulting selection when constructing a repeating schedule.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroU32;
    /// use schedules_core::{CalendarRule, Error, Frequency, Termination};
    /// use schedules_core::Weekday;
    /// let rule = CalendarRule::new(Frequency::Weekly, NonZeroU32::MIN, Termination::Forever)?
    ///     .with_weekdays([Weekday::Mon, Weekday::Fri])?;
    /// assert!(rule.weekdays().contains(Weekday::Fri));
    /// assert!(!rule.weekdays().contains(Weekday::Tue));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidWeekdayFilter`] unless frequency is [`Frequency::Weekly`],
    /// even when the input is empty.
    pub fn with_weekdays(
        mut self,
        weekdays: impl IntoIterator<Item = Weekday>,
    ) -> Result<Self, Error> {
        if self.frequency != Frequency::Weekly {
            return Err(Error::InvalidWeekdayFilter);
        }
        self.weekdays = WeekdaySet::default();
        for day in weekdays {
            self.weekdays.insert(day);
        }
        Ok(self)
    }

    /// Returns the calendar unit in which the interval is measured.
    pub const fn frequency(&self) -> Frequency {
        self.frequency
    }
    /// Returns the positive number of frequency units between matching periods.
    pub const fn interval(&self) -> NonZeroU32 {
        self.interval
    }
    /// Returns explicit weekday selections; empty means the anchor weekday.
    pub const fn weekdays(&self) -> WeekdaySet {
        self.weekdays
    }
    /// Returns the recurrence count limit or inclusive cutoff.
    pub const fn termination(&self) -> Termination {
        self.termination
    }

    /// Replaces the selected local times for each matching date.
    ///
    /// Times are sorted and deduplicated. Empty input inherits the anchor time.
    /// A nonempty selection must include the timed anchor time and cannot be used
    /// with an all-day placement; [`crate::Schedule::repeating`] enforces these rules.
    /// Times before the initial placement on its date are not emitted.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroU32;
    /// use schedules_core::{CalendarRule, Error, Frequency, Termination};
    /// use schedules_core::NaiveTime;
    /// const NOON_HOUR: u32 = 12;
    /// let noon = NaiveTime::from_hms_opt(NOON_HOUR, 0, 0).ok_or(Error::OutOfRange)?;
    /// let rule = CalendarRule::new(Frequency::Daily, NonZeroU32::MIN, Termination::Forever)?
    ///     .with_times([noon, NaiveTime::MIN, noon]);
    /// assert_eq!(rule.times().iter().collect::<Vec<_>>(), vec![NaiveTime::MIN, noon]);
    /// # Ok::<(), Error>(())
    /// ```
    ///
    pub fn with_times(mut self, times: impl IntoIterator<Item = NaiveTime>) -> Self {
        self.times = TimeSet::default();
        for time in times {
            self.times.insert(time);
        }
        self
    }

    /// Returns explicit time selections; empty means the anchor time.
    pub const fn times(&self) -> &TimeSet {
        &self.times
    }

    pub(crate) fn matches(&self, anchor: NaiveDate, date: NaiveDate) -> bool {
        let days = date.signed_duration_since(anchor).num_days();
        if days < 0 {
            return false;
        }
        let interval = i64::from(self.interval.get());
        match self.frequency {
            Frequency::Daily => days % interval == 0,
            Frequency::Weekly => {
                let weeks =
                    (days + i64::from(anchor.weekday().num_days_from_monday())) / DAYS_PER_WEEK;
                weeks % interval == 0
                    && if self.weekdays.is_empty() {
                        date.weekday() == anchor.weekday()
                    } else {
                        self.weekdays.contains(date.weekday())
                    }
            }
            Frequency::Monthly => {
                let months = i64::from(date.year() - anchor.year()) * MONTHS_PER_YEAR
                    + i64::from(date.month())
                    - i64::from(anchor.month());
                months % interval == 0 && date.day() == anchor.day()
            }
            Frequency::Yearly => {
                i64::from(date.year() - anchor.year()) % interval == 0
                    && date.month() == anchor.month()
                    && date.day() == anchor.day()
            }
        }
    }
}

/// A calendar rule preserving wall time or an elapsed rule preserving UTC spacing.
///
/// Attach it to a placement with [`crate::Schedule::repeating`]. Calendar rules
/// skip invalid dates and nonexistent local starts; elapsed rules require a
/// timed placement and may shift local wall time across timezone transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    pub(crate) kind: RecurrenceKind,
}

impl Recurrence {
    /// Creates a recurrence that follows local calendar dates and times.
    pub fn calendar(rule: CalendarRule) -> Self {
        Self {
            kind: RecurrenceKind::Calendar(rule),
        }
    }

    /// Creates a recurrence with fixed positive spacing between UTC starts.
    ///
    /// # Examples
    ///
    /// ```
    /// use schedules_core::{Duration, Error, Recurrence, Termination};
    /// const POLL_INTERVAL: Duration = Duration::minutes(15);
    /// let recurrence = Recurrence::elapsed(POLL_INTERVAL, Termination::Forever)?;
    /// assert_eq!(recurrence.elapsed_interval(), Some(POLL_INTERVAL));
    /// # Ok::<(), Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDuration`] when `interval` is nonpositive or
    /// [`Error::InvalidTermination`] for [`Termination::Until`], which uses local time.
    pub fn elapsed(interval: Duration, termination: Termination) -> Result<Self, Error> {
        if interval <= Duration::zero() {
            return Err(Error::InvalidDuration);
        }
        if matches!(termination, Termination::Until(_)) {
            return Err(Error::InvalidTermination);
        }
        Ok(Self {
            kind: RecurrenceKind::Elapsed {
                interval,
                termination,
            },
        })
    }

    /// Returns the calendar rule, or `None` for elapsed recurrence.
    pub fn calendar_rule(&self) -> Option<&CalendarRule> {
        match &self.kind {
            RecurrenceKind::Calendar(rule) => Some(rule),
            RecurrenceKind::Elapsed { .. } => None,
        }
    }

    /// Returns UTC spacing between starts, or `None` for calendar recurrence.
    pub fn elapsed_interval(&self) -> Option<Duration> {
        match self.kind {
            RecurrenceKind::Elapsed { interval, .. } => Some(interval),
            RecurrenceKind::Calendar(_) => None,
        }
    }

    /// Returns the rule's count limit or inclusive cutoff.
    pub fn termination(&self) -> Termination {
        match &self.kind {
            RecurrenceKind::Calendar(rule) => rule.termination(),
            RecurrenceKind::Elapsed { termination, .. } => *termination,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecurrenceKind {
    Calendar(CalendarRule),
    Elapsed {
        interval: Duration,
        termination: Termination,
    },
}
