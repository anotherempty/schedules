use std::{collections::BTreeMap, num::NonZeroUsize};

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::recurrence::RecurrenceKind;
use crate::{
    Error, Occurrence, OccurrenceKey, OccurrenceOverride, Placement, QueryWindow, Recurrence,
    ResolutionPolicy, Termination,
};

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
