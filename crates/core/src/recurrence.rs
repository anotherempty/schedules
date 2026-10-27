use std::{collections::BTreeSet, num::NonZeroU32, ops::Bound};

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Utc, Weekday};

use crate::Error;

const DAYS_PER_WEEK: i64 = 7;
const MONTHS_PER_YEAR: i64 = 12;
const WEEKDAY_BIT: u8 = 1;

/// Sorted, unique local times. Empty set inherits the initial placement's time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeSet {
    times: BTreeSet<NaiveTime>,
}

impl TimeSet {
    pub fn insert(&mut self, time: NaiveTime) -> bool {
        self.times.insert(time)
    }

    pub fn contains(&self, time: NaiveTime) -> bool {
        self.times.contains(&time)
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = NaiveTime> + ExactSizeIterator + '_ {
        self.times.iter().copied()
    }

    pub(crate) fn first(&self) -> Option<NaiveTime> {
        self.times.first().copied()
    }

    pub(crate) fn after(&self, time: NaiveTime) -> Option<NaiveTime> {
        self.times
            .range((Bound::Excluded(time), Bound::Unbounded))
            .next()
            .copied()
    }
}

/// Set with exactly seven possible weekday members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeekdaySet {
    bits: u8,
}

impl WeekdaySet {
    pub fn insert(&mut self, day: Weekday) {
        self.bits |= WEEKDAY_BIT << day.num_days_from_monday();
    }

    pub fn contains(self, day: Weekday) -> bool {
        self.bits & (WEEKDAY_BIT << day.num_days_from_monday()) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// Count includes initial occurrence and excludes skipped gaps and invalid dates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Termination {
    #[default]
    Forever,
    Count(NonZeroU32),
    /// Inclusive local cutoff for calendar recurrence only.
    Until(NaiveDateTime),
    /// Inclusive UTC cutoff for elapsed recurrence only.
    UntilInstant(DateTime<Utc>),
}

/// Gregorian recurrence anchored to original placement. Weeks start Monday.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarRule {
    frequency: Frequency,
    interval: NonZeroU32,
    weekdays: WeekdaySet,
    times: TimeSet,
    termination: Termination,
}

impl CalendarRule {
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

    /// Empty filter uses anchor weekday. Duplicate weekdays are removed.
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

    pub const fn frequency(&self) -> Frequency {
        self.frequency
    }
    pub const fn interval(&self) -> NonZeroU32 {
        self.interval
    }
    pub const fn weekdays(&self) -> WeekdaySet {
        self.weekdays
    }
    pub const fn termination(&self) -> Termination {
        self.termination
    }

    /// Select arbitrary local times per matching date. Empty input inherits anchor time.
    pub fn with_times(mut self, times: impl IntoIterator<Item = NaiveTime>) -> Self {
        self.times = TimeSet::default();
        for time in times {
            self.times.insert(time);
        }
        self
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    pub(crate) kind: RecurrenceKind,
}

impl Recurrence {
    pub fn calendar(rule: CalendarRule) -> Self {
        Self {
            kind: RecurrenceKind::Calendar(rule),
        }
    }

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

    pub fn calendar_rule(&self) -> Option<&CalendarRule> {
        match &self.kind {
            RecurrenceKind::Calendar(rule) => Some(rule),
            RecurrenceKind::Elapsed { .. } => None,
        }
    }

    pub fn elapsed_interval(&self) -> Option<Duration> {
        match self.kind {
            RecurrenceKind::Elapsed { interval, .. } => Some(interval),
            RecurrenceKind::Calendar(_) => None,
        }
    }

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
