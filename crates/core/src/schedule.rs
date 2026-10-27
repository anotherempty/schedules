use std::{cmp::Ordering, collections::BTreeMap, num::NonZeroUsize};

use chrono::{DateTime, Duration, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::query::{OccurrencePosition, SearchBudget};
use crate::recurrence::RecurrenceKind;
use crate::{
    Error, Occurrence, OccurrenceKey, OccurrenceOverride, Placement, QueryWindow, Recurrence,
    ResolutionPolicy, Termination,
};

const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;

/// Schedule definition. Caller owns identity and application payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    placement: Placement,
    recurrence: Option<Recurrence>,
    overrides: BTreeMap<OccurrenceKey, OccurrenceOverride>,
}

impl Schedule {
    pub fn once(placement: Placement) -> Self {
        Self {
            placement,
            recurrence: None,
            overrides: BTreeMap::new(),
        }
    }

    pub fn repeating(placement: Placement, recurrence: Recurrence) -> Result<Self, Error> {
        if matches!(recurrence.kind, RecurrenceKind::Elapsed { .. })
            && placement.timezone().is_none()
        {
            return Err(Error::ElapsedAllDay);
        }
        if let Some(rule) = recurrence.calendar_rule() {
            if !rule.times().is_empty() {
                if placement.dates().is_some() {
                    return Err(Error::AllDayTimes);
                }
                if !rule.times().contains(placement.local_start().time()) {
                    return Err(Error::InvalidAnchor);
                }
            }
            // DTSTART must belong to the rule; otherwise recurrence semantics are undefined.
            if !rule.matches(
                placement.local_start().date(),
                placement.local_start().date(),
            ) {
                return Err(Error::InvalidAnchor);
            }
        }
        match recurrence.termination() {
            Termination::Until(until) if until < placement.local_start() => {
                return Err(Error::InvalidTermination);
            }
            Termination::UntilInstant(until) => {
                if until < placement.bounds(chrono_tz::UTC)?.0 {
                    return Err(Error::InvalidTermination);
                }
            }
            _ => {}
        }
        Ok(Self {
            placement,
            recurrence: Some(recurrence),
            overrides: BTreeMap::new(),
        })
    }

    pub const fn placement(&self) -> Placement {
        self.placement
    }
    pub const fn recurrence(&self) -> Option<&Recurrence> {
        self.recurrence.as_ref()
    }
    pub fn overrides(&self) -> &BTreeMap<OccurrenceKey, OccurrenceOverride> {
        &self.overrides
    }

    /// Validate original membership before changing an occurrence. Budget includes skipped days.
    pub fn set_override(
        &mut self,
        key: OccurrenceKey,
        change: OccurrenceOverride,
        candidate_limit: NonZeroUsize,
    ) -> Result<(), Error> {
        let expected = if self.placement.dates().is_some() {
            OccurrenceKey::Date(self.placement.local_start().date())
        } else if self
            .recurrence
            .as_ref()
            .and_then(Recurrence::elapsed_interval)
            .is_some()
        {
            OccurrenceKey::Elapsed(self.placement.bounds(chrono_tz::UTC)?.0)
        } else {
            OccurrenceKey::Calendar(self.placement.local_start())
        };
        if std::mem::discriminant(&key) != std::mem::discriminant(&expected) || key < expected {
            return Err(Error::InvalidOccurrenceKey);
        }
        let mut cursor = Expansion::new(self)?;
        for _ in 0..candidate_limit.get() {
            if !cursor.advance(None)? {
                return Err(Error::InvalidOccurrenceKey);
            }
            match cursor.candidate()? {
                Candidate::Occurrence(occurrence) if occurrence.key() == key => {
                    self.overrides.insert(key, change);
                    return Ok(());
                }
                Candidate::Occurrence(occurrence) if occurrence.key() > key => {
                    return Err(Error::InvalidOccurrenceKey);
                }
                _ => {}
            }
        }
        Err(Error::CandidateLimit)
    }

    pub fn remove_override(&mut self, key: OccurrenceKey) -> Option<OccurrenceOverride> {
        self.overrides.remove(&key)
    }

    pub(crate) fn occurrence_at(
        &self,
        key: OccurrenceKey,
        budget: &mut SearchBudget,
    ) -> Result<Occurrence, Error> {
        budget.charge()?;
        // Override membership was validated when inserted.
        if let Some(change) = self.overrides.get(&key) {
            return match change {
                OccurrenceOverride::Cancel => Err(Error::InvalidOccurrenceKey),
                OccurrenceOverride::Replace(placement) => Ok(Occurrence::new(key, *placement)),
            };
        }
        let mut cursor = Expansion::at_key(self, key)?;
        while cursor.advance(None)? {
            budget.charge()?;
            if let Candidate::Occurrence(occurrence) = cursor.candidate()? {
                if std::mem::discriminant(&occurrence.key()) != std::mem::discriminant(&key) {
                    return Err(Error::InvalidOccurrenceKey);
                }
                match occurrence.key().cmp(&key) {
                    Ordering::Equal => return Ok(occurrence),
                    Ordering::Greater => return Err(Error::InvalidOccurrenceKey),
                    Ordering::Less => {}
                }
            }
        }
        Err(Error::InvalidOccurrenceKey)
    }

    pub(crate) fn successor(
        &self,
        anchor: OccurrencePosition,
        item_order: Ordering,
        zone: Tz,
        budget: &mut SearchBudget,
    ) -> Result<Option<Occurrence>, Error> {
        budget.charge()?;
        let mut best: Option<(OccurrencePosition, Occurrence)> = None;
        // Replacements can move arbitrarily far in either direction.
        for (&key, &change) in &self.overrides {
            budget.charge()?;
            if let OccurrenceOverride::Replace(placement) = change {
                let occurrence = Occurrence::new(key, placement);
                let position = OccurrencePosition::new(occurrence, zone)?;
                if position.follows(anchor, item_order)
                    && best.is_none_or(|(current, _)| position < current)
                {
                    best = Some((position, occurrence));
                }
            }
        }
        let mut cursor = Expansion::near(self, anchor.start(), zone)?;
        while cursor.advance(None)? {
            budget.charge()?;
            let Candidate::Occurrence(occurrence) = cursor.candidate()? else {
                continue;
            };
            let position = OccurrencePosition::new(occurrence, zone)?;
            // Regular occurrences are chronological; later ones cannot beat best.
            if best.is_some_and(|(current, _)| position >= current) {
                break;
            }
            if !self.overrides.contains_key(&occurrence.key())
                && position.follows(anchor, item_order)
            {
                best = Some((position, occurrence));
                break;
            }
        }
        Ok(best.map(|(_, occurrence)| occurrence))
    }

    /// Return overlapping occurrences ordered by resolved start, then original key.
    /// Expansion starts at anchor to preserve count semantics. Budget exhaustion is an error,
    /// never a successful truncated result. Moved occurrences can enter from outside the window.
    pub fn occurrences(&self, window: QueryWindow) -> Result<Vec<Occurrence>, Error> {
        let mut ordered = self
            .iter(window)?
            .map(|occurrence| {
                let occurrence = occurrence?;
                occurrence
                    .placement()
                    .bounds(window.all_day_zone)
                    .map(|(start, _)| (start, occurrence))
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Each original key appears once, including replaced occurrences.
        ordered.sort_unstable_by_key(|(start, occurrence)| (*start, occurrence.key()));
        let mut results: Vec<_> = ordered
            .into_iter()
            .map(|(_, occurrence)| occurrence)
            .collect();
        results.shrink_to_fit();
        Ok(results)
    }

    /// Lazy, bounded expansion. Replacements come first in original-key order, then regular
    /// occurrences in recurrence order. Use `occurrences` for chronological sorting.
    pub fn iter(&self, window: QueryWindow) -> Result<Occurrences<'_>, Error> {
        self.iter_with_budget(window, window.candidate_limit.get())
    }

    pub(crate) fn iter_with_budget(
        &self,
        window: QueryWindow,
        candidate_limit: usize,
    ) -> Result<Occurrences<'_>, Error> {
        Ok(Occurrences {
            cursor: Expansion::new(self)?,
            overrides: self.overrides.iter(),
            window,
            candidate_limit,
            work: 0,
            returned: 0,
            finished: false,
        })
    }
}

/// Fallible expansion; first error terminates the iterator. No occurrences are precomputed.
pub struct Occurrences<'a> {
    cursor: Expansion<'a>,
    overrides: std::collections::btree_map::Iter<'a, OccurrenceKey, OccurrenceOverride>,
    window: QueryWindow,
    candidate_limit: usize,
    work: usize,
    returned: usize,
    finished: bool,
}

impl Iterator for Occurrences<'_> {
    type Item = Result<Occurrence, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.next_occurrence() {
            Ok(Some(occurrence)) => Some(Ok(occurrence)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}

impl std::iter::FusedIterator for Occurrences<'_> {}

impl Occurrences<'_> {
    pub(crate) const fn work_used(&self) -> usize {
        self.work
    }

    fn charge(&mut self) -> Result<(), Error> {
        if self.work >= self.candidate_limit {
            return Err(Error::CandidateLimit);
        }
        self.work += 1;
        Ok(())
    }

    fn next_occurrence(&mut self) -> Result<Option<Occurrence>, Error> {
        loop {
            let candidate = if let Some((&key, &change)) = self.overrides.next() {
                self.charge()?;
                match change {
                    OccurrenceOverride::Cancel => continue,
                    OccurrenceOverride::Replace(placement) => Occurrence::new(key, placement),
                }
            } else {
                if !self.cursor.advance(Some(self.window))? {
                    return Ok(None);
                }
                self.charge()?;
                match self.cursor.candidate()? {
                    Candidate::Skip => continue,
                    Candidate::Occurrence(occurrence) => {
                        if self
                            .cursor
                            .schedule
                            .overrides
                            .contains_key(&occurrence.key())
                        {
                            continue;
                        }
                        occurrence
                    }
                }
            };
            if self.window.overlaps(candidate.placement())? {
                if self.returned >= self.window.occurrence_limit.get() {
                    return Err(Error::OccurrenceLimit);
                }
                self.returned += 1;
                return Ok(Some(candidate));
            }
        }
    }
}

struct Expansion<'a> {
    schedule: &'a Schedule,
    // Calendar cursor; elapsed recurrence advances `instant` instead.
    local: NaiveDateTime,
    instant: Option<DateTime<Utc>>,
    emitted: u32,
    started: bool,
    finished: bool,
}

impl<'a> Expansion<'a> {
    fn at_key(schedule: &'a Schedule, key: OccurrenceKey) -> Result<Self, Error> {
        let mut cursor = Self::new(schedule)?;
        match (key, cursor.instant) {
            (OccurrenceKey::Elapsed(instant), Some(_)) => cursor.seek_elapsed(instant)?,
            (OccurrenceKey::Calendar(local), None) if schedule.placement.dates().is_none() => {
                cursor.seek_local_key(local)?;
            }
            (OccurrenceKey::Date(date), None) if schedule.placement.dates().is_some() => {
                cursor.seek_local_key(date.and_time(NaiveTime::MIN))?;
            }
            _ => return Err(Error::InvalidOccurrenceKey),
        }
        Ok(cursor)
    }

    fn seek_local_key(&mut self, local: NaiveDateTime) -> Result<(), Error> {
        if local < self.local {
            return Err(Error::InvalidOccurrenceKey);
        }
        let Some(rule) = self
            .schedule
            .recurrence
            .as_ref()
            .and_then(Recurrence::calendar_rule)
        else {
            return if local == self.local {
                Ok(())
            } else {
                Err(Error::InvalidOccurrenceKey)
            };
        };
        let valid_time = if rule.times().is_empty() {
            local.time() == self.local.time()
        } else {
            rule.times().contains(local.time())
        };
        if !rule.matches(self.local.date(), local.date()) || !valid_time {
            return Err(Error::InvalidOccurrenceKey);
        }
        // Calendar COUNT counts resolved occurrences, excluding timezone gaps.
        // Its rank still requires bounded expansion; other rules seek directly.
        if !matches!(rule.termination(), Termination::Count(_)) {
            self.local = local;
        }
        Ok(())
    }

    fn near(schedule: &'a Schedule, start: DateTime<Utc>, zone: Tz) -> Result<Self, Error> {
        let mut cursor = Self::new(schedule)?;
        if cursor.instant.is_some() {
            cursor.seek_elapsed(start)?;
        } else if let Some(rule) = schedule
            .recurrence
            .as_ref()
            .and_then(Recurrence::calendar_rule)
            && !matches!(rule.termination(), Termination::Count(_))
        {
            let local = Placement::local_at(start, schedule.placement.timezone().unwrap_or(zone))?;
            // Start at the day's first selected time, preserving both sides of
            // folds when the anchor belongs to another schedule or was moved.
            let time = rule.times().first().unwrap_or(cursor.local.time());
            cursor.local = cursor.local.max(local.date().and_time(time));
        }
        Ok(cursor)
    }

    fn seek_elapsed(&mut self, target: DateTime<Utc>) -> Result<(), Error> {
        let Some(origin) = self.instant else {
            return Ok(());
        };
        if target <= origin {
            return Ok(());
        }
        let Some(recurrence) = self.schedule.recurrence.as_ref() else {
            return Ok(());
        };
        let Some(interval) = recurrence.elapsed_interval() else {
            return Ok(());
        };
        // i128 preserves nanosecond precision across Chrono's full date range.
        let delta = target.signed_duration_since(origin);
        let delta_nanos = i128::from(delta.num_seconds()) * NANOSECONDS_PER_SECOND
            + i128::from(delta.subsec_nanos());
        let interval_nanos = i128::from(interval.num_seconds()) * NANOSECONDS_PER_SECOND
            + i128::from(interval.subsec_nanos());
        let skipped = delta_nanos / interval_nanos;
        if let Termination::Count(count) = recurrence.termination() {
            if skipped >= i128::from(count.get()) {
                self.finished = true;
                return Ok(());
            }
            self.emitted = u32::try_from(skipped).map_err(|_| Error::OutOfRange)?;
        }
        let remainder = delta_nanos % interval_nanos;
        let seconds =
            i64::try_from(remainder / NANOSECONDS_PER_SECOND).map_err(|_| Error::OutOfRange)?;
        let nanos =
            i64::try_from(remainder % NANOSECONDS_PER_SECOND).map_err(|_| Error::OutOfRange)?;
        let remainder = Duration::try_seconds(seconds)
            .and_then(|value| value.checked_add(&Duration::nanoseconds(nanos)))
            .ok_or(Error::OutOfRange)?;
        self.instant = Some(
            target
                .checked_sub_signed(remainder)
                .ok_or(Error::OutOfRange)?,
        );
        Ok(())
    }

    fn new(schedule: &'a Schedule) -> Result<Self, Error> {
        let instant = if schedule
            .recurrence
            .as_ref()
            .and_then(Recurrence::elapsed_interval)
            .is_some()
        {
            Some(schedule.placement.bounds(chrono_tz::UTC)?.0)
        } else {
            None
        };
        Ok(Self {
            schedule,
            local: schedule.placement.local_start(),
            instant,
            emitted: 0,
            started: false,
            finished: false,
        })
    }

    fn past_window(&self, window: QueryWindow) -> Result<bool, Error> {
        if let Some(instant) = self.instant {
            return Ok(instant >= window.range.end());
        }
        let zone = self
            .schedule
            .placement
            .timezone()
            .unwrap_or(window.all_day_zone);
        // Use the earliest side of a fold: Reject must still report ambiguity
        // whenever either possible start precedes the exclusive window end.
        match ResolutionPolicy::default().resolve(self.local, zone) {
            Ok(start) => Ok(start >= window.range.end()),
            Err(Error::NonexistentTime { .. }) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn advance(&mut self, window: Option<QueryWindow>) -> Result<bool, Error> {
        if self.complete() {
            return Ok(false);
        }
        if let Some(window) = window
            && self.past_window(window)?
        {
            return Ok(false);
        }
        let recurrence = self.schedule.recurrence.as_ref();
        if self.started {
            match recurrence.map(|value| &value.kind) {
                None => {
                    self.finished = true;
                    return Ok(false);
                }
                Some(RecurrenceKind::Calendar(rule)) => {
                    if rule.matches(
                        self.schedule.placement.local_start().date(),
                        self.local.date(),
                    ) && let Some(time) = rule.times().after(self.local.time())
                    {
                        self.local = self.local.date().and_time(time);
                    } else {
                        let date = self.local.date().succ_opt().ok_or(Error::OutOfRange)?;
                        let time = rule
                            .times()
                            .first()
                            .unwrap_or(self.schedule.placement.local_start().time());
                        self.local = date.and_time(time);
                    }
                }
                Some(RecurrenceKind::Elapsed {
                    interval,
                    termination,
                }) => {
                    let Some(instant) = self.instant else {
                        return Err(Error::ElapsedAllDay);
                    };
                    if let Termination::UntilInstant(until) = termination
                        && *interval > until.signed_duration_since(instant)
                    {
                        self.finished = true;
                        return Ok(false);
                    }
                    if let Some(window) = window
                        && *interval >= window.range.end().signed_duration_since(instant)
                    {
                        return Ok(false);
                    }
                    let instant = instant
                        .checked_add_signed(*interval)
                        .ok_or(Error::OutOfRange)?;
                    self.instant = Some(instant);
                }
            }
        }
        self.started = true;
        if let Some(recurrence) = recurrence {
            match recurrence.termination() {
                Termination::Count(count) if self.emitted >= count.get() => {
                    self.finished = true;
                    return Ok(false);
                }
                Termination::Until(until) if self.local > until => {
                    self.finished = true;
                    return Ok(false);
                }
                Termination::UntilInstant(until)
                    if self.instant.is_some_and(|instant| instant > until) =>
                {
                    self.finished = true;
                    return Ok(false);
                }
                _ => {}
            }
        }
        if let Some(window) = window
            && self.past_window(window)?
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn candidate(&mut self) -> Result<Candidate, Error> {
        let recurrence = self.schedule.recurrence.as_ref();
        if let Some(recurrence) = recurrence
            && let RecurrenceKind::Calendar(rule) = &recurrence.kind
            && !rule.matches(
                self.schedule.placement.local_start().date(),
                self.local.date(),
            )
        {
            return Ok(Candidate::Skip);
        }
        let placement = match self.instant {
            Some(instant) => self.schedule.placement.at_instant(instant)?,
            None => match self.schedule.placement.at_local(self.local) {
                Err(Error::NonexistentTime { .. }) if recurrence.is_some() => {
                    return Ok(Candidate::Skip);
                }
                result => result?,
            },
        };
        self.emitted = self.emitted.checked_add(1).ok_or(Error::OutOfRange)?;
        let key = if let Some(instant) = self.instant {
            OccurrenceKey::Elapsed(instant)
        } else if placement.dates().is_some() {
            OccurrenceKey::Date(self.local.date())
        } else {
            OccurrenceKey::Calendar(self.local)
        };
        Ok(Candidate::Occurrence(Occurrence::new(key, placement)))
    }

    fn complete(&self) -> bool {
        self.finished
            || match self
                .schedule
                .recurrence
                .as_ref()
                .map(Recurrence::termination)
            {
                None => self.started,
                Some(Termination::Count(count)) => self.emitted >= count.get(),
                Some(Termination::Until(until)) => self.started && self.local >= until,
                Some(Termination::UntilInstant(until)) => {
                    self.started && self.instant.is_some_and(|instant| instant >= until)
                }
                _ => false,
            }
    }
}

enum Candidate {
    Occurrence(Occurrence),
    Skip,
}
