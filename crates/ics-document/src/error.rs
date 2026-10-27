//! Document syntax errors and original physical source locations.

/// Inclusive, one-based physical line range of an ICS content line or component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IcsLocation {
    start_line: std::num::NonZeroUsize,
    end_line: std::num::NonZeroUsize,
}

impl std::fmt::Display for IcsLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start_line == self.end_line {
            write!(f, "line {}", self.start_line)
        } else {
            write!(f, "lines {}-{}", self.start_line, self.end_line)
        }
    }
}

impl IcsLocation {
    /// Creates a range containing one physical line.
    pub const fn new(line: std::num::NonZeroUsize) -> Self {
        Self {
            start_line: line,
            end_line: line,
        }
    }

    pub(super) fn extend(&mut self, line: std::num::NonZeroUsize) {
        self.end_line = self.end_line.max(line);
    }

    /// Returns the first physical line, numbered from one.
    pub const fn start_line(self) -> std::num::NonZeroUsize {
        self.start_line
    }

    /// Returns the last physical line, including folded continuations.
    pub const fn end_line(self) -> std::num::NonZeroUsize {
        self.end_line
    }
}

/// A malformed document structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Invalid {
    /// Continuation without a preceding content line.
    #[error("continuation without a preceding content line")]
    FoldedLine,
    /// Malformed content line.
    #[error("malformed content line")]
    ContentLine,
    /// Parameters on a component boundary.
    #[error("parameters on a component boundary")]
    ComponentParameters,
    /// Missing component name.
    #[error("missing component name")]
    Component,
    /// Mismatched component end.
    #[error("mismatched component end")]
    ComponentEnd,
    /// Invalid component nesting.
    #[error("invalid component nesting")]
    ComponentNesting,
    /// Property outside an event.
    #[error("property outside an event")]
    Event,
    /// Content outside the calendar.
    #[error("content outside the calendar")]
    OutsideCalendar,
    /// Missing or incomplete calendar.
    #[error("missing or incomplete calendar")]
    Calendar,
}

/// A syntax or structural failure independent of any schedule model.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A failure with its original input location.
    #[error("{location}: {source}")]
    Located {
        /// Original physical line range.
        location: IcsLocation,
        /// Concrete underlying error.
        #[source]
        source: Box<Error>,
    },
    /// Invalid content-line syntax.
    #[error("invalid iCalendar syntax: {0}")]
    Syntax(#[from] ical::property::PropertyError),
    /// Invalid component structure.
    #[error(transparent)]
    Invalid(#[from] Invalid),
    /// A component kind is outside the supported document grammar.
    #[error("unsupported iCalendar component")]
    UnsupportedComponent,
    /// Input position exceeds representable bounds.
    #[error("document position exceeds supported range")]
    OutOfRange,
}
impl Error {
    pub(super) fn at(self, location: IcsLocation) -> Self {
        if matches!(self, Self::Located { .. }) {
            self
        } else {
            Self::Located {
                location,
                source: Box::new(self),
            }
        }
    }

    /// Returns the original document location when attached to a parsing failure.
    pub const fn location(&self) -> Option<IcsLocation> {
        match self {
            Self::Located { location, .. } => Some(*location),
            _ => None,
        }
    }

    /// Borrows the typed failure beneath source-location context.
    pub fn kind(&self) -> &Self {
        match self {
            Self::Located { source, .. } => source.kind(),
            _ => self,
        }
    }

    /// Removes source-location context, retaining the concrete failure and its underlying causes.
    pub fn into_kind(self) -> Self {
        match self {
            Self::Located { source, .. } => source.into_kind(),
            _ => self,
        }
    }
}
