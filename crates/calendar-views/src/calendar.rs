//! Generic calendar entries and lazy day/week grouping.

use chrono::{Datelike, NaiveDate, Weekday};

use crate::{CalendarWindow, DAYS_PER_WEEK, Error, TimeSpan};

/// Arbitrary owned or borrowed application data paired with its temporal span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarEntry<T> {
    item: T,
    span: TimeSpan,
}

impl<T> CalendarEntry<T> {
    /// Associates a payload with a validated UTC span.
    pub const fn new(item: T, span: TimeSpan) -> Self {
        Self { item, span }
    }

    /// Borrows the original application payload.
    pub const fn item(&self) -> &T {
        &self.item
    }

    /// Returns the original span, without clipping it to a day boundary.
    pub const fn span(&self) -> TimeSpan {
        self.span
    }

    /// Transfers the original payload to the caller.
    pub fn into_item(self) -> T {
        self.item
    }
}

/// A finite snapshot of arbitrary items for a local-date calendar window.
///
/// This type knows nothing about schedules, identifiers, recurrence, or persistence.
/// It owns entries; use reference payloads to borrow application data without cloning.
/// Day iteration costs O(days × entries), with only each yielded day's matches
/// allocated. No day grid or duplicate payloads are stored in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calendar<T> {
    window: CalendarWindow,
    entries: Vec<CalendarEntry<T>>,
}

impl<T> Calendar<T> {
    /// Collects overlapping entries and sorts by UTC start, retaining input order on ties.
    ///
    /// Consumes a finite input iterator. Entries outside the window are discarded.
    /// Payloads need not implement `Clone`, ordering, or any calendar-specific trait.
    pub fn new(
        window: CalendarWindow,
        entries: impl IntoIterator<Item = CalendarEntry<T>>,
    ) -> Self {
        let mut entries: Vec<_> = entries
            .into_iter()
            .filter(|entry| entry.span.overlaps(window))
            .collect();
        entries.sort_by_key(|entry| entry.span.start());
        Self { window, entries }
    }

    /// Returns the local-date range and viewing timezone.
    pub const fn window(&self) -> CalendarWindow {
        self.window
    }

    /// Returns snapshot entries in chronological order, including each payload once.
    pub fn entries(&self) -> &[CalendarEntry<T>] {
        &self.entries
    }

    /// Lazily yields every requested date, including days with no entries.
    ///
    /// Spanning intervals appear on every overlapping day; end-at-midnight intervals
    /// do not appear on the following day. Points appear on exactly one local date.
    /// Each item is a `Result`; missing midnight or invalid day bounds terminate
    /// iteration with a typed error. Previously yielded days remain valid.
    pub fn days(&self) -> DayViews<'_, T> {
        DayViews {
            calendar: self,
            next_date: Some(self.window.start()),
        }
    }

    /// Lazily groups requested days into weeks beginning on the selected weekday.
    ///
    /// Each week has seven positions. Dates outside the requested range are `None`;
    /// requested empty dates are `Some(DayView)` with no entries. To request complete
    /// weeks, expand the window with [`CalendarWindow::covering_weeks`] before
    /// loading entries and constructing this snapshot. Iteration errors match days.
    ///
    /// # Errors
    ///
    /// Returns [`Error::OutOfRange`] if the first week's start precedes supported dates.
    pub fn weeks(&self, week_start: Weekday) -> Result<WeekViews<'_, T>, Error> {
        Ok(WeekViews {
            days: self.days(),
            week_start,
            next_week: Some(self.window.week_start(week_start)?),
        })
    }
}

/// One local date and references to entries overlapping its UTC boundaries.
#[derive(Debug)]
pub struct DayView<'a, T> {
    window: CalendarWindow,
    entries: Vec<&'a CalendarEntry<T>>,
}

impl<'a, T> DayView<'a, T> {
    /// Returns the represented local date.
    pub const fn date(&self) -> NaiveDate {
        self.window.start()
    }

    /// Returns this day's actual midnight-to-midnight UTC boundaries and timezone.
    pub const fn window(&self) -> CalendarWindow {
        self.window
    }

    /// Returns borrowed entries in original UTC-start order, without cloning payloads.
    pub fn entries(&self) -> &[&'a CalendarEntry<T>] {
        &self.entries
    }
}

/// A lazy day iterator that stops permanently on its first error or exhaustion.
pub struct DayViews<'a, T> {
    calendar: &'a Calendar<T>,
    next_date: Option<NaiveDate>,
}

impl<'a, T> Iterator for DayViews<'a, T> {
    type Item = Result<DayView<'a, T>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let date = self.next_date.take()?;
        let Some(end) = date.succ_opt() else {
            return Some(Err(Error::OutOfRange));
        };
        let window = match CalendarWindow::new(date, end, self.calendar.window.timezone()) {
            Ok(window) => window,
            Err(error) => return Some(Err(error)),
        };
        if end < self.calendar.window.end() {
            self.next_date = Some(end);
        }
        let entries = self
            .calendar
            .entries
            .iter()
            .filter(|entry| entry.span.overlaps(window))
            .collect();
        Some(Ok(DayView { window, entries }))
    }
}

impl<T> std::iter::FusedIterator for DayViews<'_, T> {}

/// A week with seven positions, distinguishing outside-window dates from empty days.
#[derive(Debug)]
pub struct WeekView<'a, T> {
    start: NaiveDate,
    days: [Option<DayView<'a, T>>; DAYS_PER_WEEK],
}

impl<'a, T> WeekView<'a, T> {
    /// Returns the first local date of this week, even when outside the window.
    pub const fn start(&self) -> NaiveDate {
        self.start
    }

    /// Returns seven consecutive positions ordered from the configured week start.
    pub const fn days(&self) -> &[Option<DayView<'a, T>>; DAYS_PER_WEEK] {
        &self.days
    }
}

/// A lazy week iterator that stops permanently on its first error or exhaustion.
pub struct WeekViews<'a, T> {
    days: DayViews<'a, T>,
    week_start: Weekday,
    next_week: Option<NaiveDate>,
}

impl<'a, T> Iterator for WeekViews<'a, T> {
    type Item = Result<WeekView<'a, T>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.next_week.take()?;
        let next_date = self.days.next_date?;
        let offset = CalendarWindow::weekday_offset(next_date.weekday(), self.week_start);
        let offset = match usize::try_from(offset) {
            Ok(offset) => offset,
            Err(_) => return Some(Err(Error::OutOfRange)),
        };
        let mut days = std::array::from_fn(|_| None);
        for slot in days.iter_mut().skip(offset) {
            match self.days.next() {
                Some(Ok(day)) => *slot = Some(day),
                Some(Err(error)) => return Some(Err(error)),
                None => break,
            }
        }
        self.next_week = self.days.next_date;
        Some(Ok(WeekView { start, days }))
    }
}

impl<T> std::iter::FusedIterator for WeekViews<'_, T> {}
