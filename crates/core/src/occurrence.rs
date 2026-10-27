//! Schedule-scoped occurrence identity and per-occurrence changes.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use crate::Placement;

/// An occurrence's original position, unchanged by replacement.
///
/// Keys are scoped to a [`crate::Schedule`]; they are not globally unique.
/// Derived ordering groups variants, then compares original positions.
/// Use query methods for ordering by actual placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OccurrenceKey {
    /// Original local start for a timed calendar recurrence or timed one-off.
    Calendar(NaiveDateTime),
    /// Original UTC start for an elapsed recurrence.
    Elapsed(DateTime<Utc>),
    /// Original start date for an all-day occurrence.
    Date(NaiveDate),
}

/// A cancellation or replacement applied to one original occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OccurrenceOverride {
    /// Suppresses the occurrence without extending recurrence termination.
    Cancel,
    /// Replaces the entire placement while retaining the original key.
    ///
    /// The replacement may change placement kind or move outside the original window.
    Replace(Placement),
}

/// An original occurrence key paired with its effective placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Occurrence {
    key: OccurrenceKey,
    placement: Placement,
}

impl Occurrence {
    pub(crate) const fn new(key: OccurrenceKey, placement: Placement) -> Self {
        Self { key, placement }
    }
    /// Returns the original position, even when a replacement moved the occurrence.
    pub const fn key(self) -> OccurrenceKey {
        self.key
    }
    /// Returns the effective placement after any replacement.
    pub const fn placement(self) -> Placement {
        self.placement
    }
}
