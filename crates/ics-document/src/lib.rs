//! Scheduling-agnostic iCalendar document parsing, serialization, and source locations.
//!
//! ```
//! use ics_document::IcsDocument;
//! let input = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";
//! let document = IcsDocument::deserialize(input)?;
//! assert_eq!(document.serialize(), input);
//! # Ok::<(), ics_document::Error>(())
//! ```

mod error;

pub use error::{Error, IcsLocation, Invalid};
use ical::property::Property as RawProperty;
use std::num::NonZeroUsize;

/// RFC 5545 recommends physical content lines no longer than 75 octets.
const LINE_OCTETS: usize = 75;

/// Structured iCalendar data independent of any scheduling or event model.
///
/// Parsing checks syntax and component structure, not timing semantics. Values,
/// parameters, metadata, and alarms are retained. Serialization normalizes
/// property order, quoting, line folding, and CRLF endings.
#[derive(Debug, Clone)]
pub struct IcsDocument {
    location: IcsLocation,
    properties: Vec<IcsProperty>,
    events: Vec<IcsComponent>,
}

impl IcsDocument {
    /// Borrows calendar properties in input order.
    pub fn properties(&self) -> &[IcsProperty] {
        &self.properties
    }
    /// Borrows event components in input order.
    pub fn events(&self) -> &[IcsComponent] {
        &self.events
    }
    /// Returns the opening calendar line in the original input.
    pub const fn location(&self) -> IcsLocation {
        self.location
    }
    /// Serializes structured data with CRLF endings and UTF-8-safe line folding.
    ///
    /// No timing interpretation is performed. Unrecognized metadata and timing
    /// properties remain in the output. Stored source locations still refer to
    /// the original input, not to normalized output.
    pub fn serialize(&self) -> String {
        let mut lines = vec!["BEGIN:VCALENDAR".into()];
        lines.extend(self.properties.iter().map(IcsProperty::serialize));
        for event in &self.events {
            event.serialize(Component::Event, &mut lines);
        }
        lines.push("END:VCALENDAR".into());
        let mut output = String::new();
        for line in lines {
            let mut used = 0;
            for ch in line.chars() {
                if used + ch.len_utf8() > LINE_OCTETS {
                    output.push_str("\r\n ");
                    used = 1;
                }
                output.push(ch);
                used += ch.len_utf8();
            }
            output.push_str("\r\n");
        }
        output
    }

    /// Deserializes one VCALENDAR while retaining property values and source locations.
    ///
    /// Only VCALENDAR, VEVENT, and nested VALARM components are supported.
    /// Timing semantics are left to consumers; no schedule conversion occurs.
    /// Callers must bound input size before parsing.
    ///
    /// # Errors
    /// Returns a located [`Error`] for malformed syntax, invalid nesting, or
    /// unsupported component kinds. No partial document is returned.
    pub fn deserialize(input: &str) -> Result<Self, Error> {
        let mut context = IcsLocation::new(NonZeroUsize::MIN);
        (|| {
            // The dependency trims physical-line whitespace. Unfold here and restore
            // exact values after parameter parsing so textual UIDs retain identity.
            let mut lines: Vec<(String, IcsLocation)> = Vec::new();
            for (index, line) in input.lines().enumerate() {
                let number = index
                    .checked_add(1)
                    .and_then(NonZeroUsize::new)
                    .ok_or(Error::OutOfRange)?;
                context = IcsLocation::new(number);
                if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
                    let (text, location) = lines
                        .last_mut()
                        .ok_or(Error::Invalid(Invalid::FoldedLine))?;
                    text.push_str(rest);
                    location.extend(number);
                } else if !line.is_empty() {
                    lines.push((line.into(), context));
                }
            }
            Self::from_lines(lines, context)
        })()
        .map_err(|error: Error| error.at(context))
    }

    fn from_lines(lines: Vec<(String, IcsLocation)>, eof: IcsLocation) -> Result<Self, Error> {
        let mut context = eof;
        (|| {
            let mut document = Self {
                location: lines
                    .first()
                    .map(|(_, location)| *location)
                    .unwrap_or(context),
                properties: Vec::new(),
                events: Vec::new(),
            };
            let mut stack = Vec::new();
            let mut opened = false;
            for (line, location) in lines {
                context = location;
                let mut prop = ical::property::PropertyParser::from_reader(line.as_bytes())
                    .next()
                    .ok_or(Error::Invalid(Invalid::ContentLine))??;
                let mut quoted = false;
                let delimiter = line
                    .char_indices()
                    .find_map(|(index, ch)| {
                        if ch == '"' {
                            quoted = !quoted;
                        }
                        (ch == ':' && !quoted).then_some(index)
                    })
                    .ok_or(Error::Invalid(Invalid::ContentLine))?;
                prop.value = Some(
                    line.get(delimiter + ':'.len_utf8()..)
                        .ok_or(Error::Invalid(Invalid::ContentLine))?
                        .into(),
                );
                let prop = IcsProperty::new(prop, location);
                match prop.name.as_str() {
                    "BEGIN" | "END" => {
                        if prop.params.is_some() {
                            return Err(Error::Invalid(Invalid::ComponentParameters));
                        }
                        let component = Component::parse(
                            prop.value
                                .as_deref()
                                .ok_or(Error::Invalid(Invalid::Component))?,
                        )?;
                        if prop.name == "END" {
                            if stack.pop() != Some(component) {
                                return Err(Error::Invalid(Invalid::ComponentEnd));
                            }
                            continue;
                        }
                        match (stack.last(), component) {
                            (None, Component::Calendar) if !opened => opened = true,
                            (Some(Component::Calendar), Component::Event) => {
                                document.events.push(IcsComponent {
                                    values: Vec::new(),
                                    alarms: Vec::new(),
                                    location,
                                })
                            }
                            (Some(Component::Event), Component::Alarm) => {
                                document
                                    .events
                                    .last_mut()
                                    .ok_or(Invalid::Event)?
                                    .alarms
                                    .push(IcsComponent {
                                        values: Vec::new(),
                                        alarms: Vec::new(),
                                        location,
                                    });
                            }
                            _ => return Err(Error::Invalid(Invalid::ComponentNesting)),
                        }
                        stack.push(component);
                    }
                    _ => match stack.last() {
                        Some(Component::Calendar) => document.properties.push(prop),
                        Some(Component::Event) => document
                            .events
                            .last_mut()
                            .ok_or(Error::Invalid(Invalid::Event))?
                            .values
                            .push(prop),
                        Some(Component::Alarm) => document
                            .events
                            .last_mut()
                            .and_then(|event| event.alarms.last_mut())
                            .ok_or(Invalid::ComponentNesting)?
                            .values
                            .push(prop),
                        None => return Err(Error::Invalid(Invalid::OutsideCalendar)),
                    },
                }
            }
            context = eof;
            if !opened || !stack.is_empty() {
                return Err(Error::Invalid(Invalid::Calendar));
            }
            Ok(document)
        })()
        .map_err(|error: Error| error.at(context))
    }
}

/// Only structural states understood by the timing adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Component {
    Calendar,
    Event,
    Alarm,
}

impl Component {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Calendar => "VCALENDAR",
            Self::Event => "VEVENT",
            Self::Alarm => "VALARM",
        }
    }

    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "VCALENDAR" => Ok(Self::Calendar),
            "VEVENT" => Ok(Self::Event),
            "VALARM" => Ok(Self::Alarm),
            _ => Err(Error::UnsupportedComponent),
        }
    }
}

/// A parsed wire property with read-only parameters, value, and source location.
#[derive(Debug, Clone)]
pub struct IcsProperty {
    name: String,
    params: Option<Vec<(String, Vec<String>)>>,
    value: Option<String>,
    location: IcsLocation,
}

impl IcsProperty {
    /// Returns the property name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the undecoded wire value, if present.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }
    /// Iterates parameter names and their parsed textual values.
    pub fn parameters(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.params
            .iter()
            .flatten()
            .map(|(name, values)| (name.as_str(), values.as_slice()))
    }
    /// Returns the original physical content-line range.
    pub const fn location(&self) -> IcsLocation {
        self.location
    }

    fn serialize(&self) -> String {
        let mut line = self.name.clone();
        for (name, values) in self.params.iter().flatten() {
            line.push(';');
            line.push_str(name);
            line.push('=');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    line.push(',');
                }
                let quoted = value.contains([':', ';', ','])
                    || value.starts_with(' ')
                    || value.ends_with(' ');
                if quoted {
                    line.push('"');
                }
                line.push_str(value);
                if quoted {
                    line.push('"');
                }
            }
        }
        line.push(':');
        if let Some(value) = &self.value {
            line.push_str(value);
        }
        line
    }

    fn new(inner: RawProperty, location: IcsLocation) -> Self {
        Self {
            name: inner.name,
            params: inner.params,
            value: inner.value,
            location,
        }
    }
}

/// A parsed VEVENT or VALARM component; enclosing collections identify its kind.
#[derive(Debug, Clone)]
pub struct IcsComponent {
    values: Vec<IcsProperty>,
    alarms: Vec<IcsComponent>,
    location: IcsLocation,
}

impl std::ops::Deref for IcsComponent {
    type Target = [IcsProperty];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl IcsComponent {
    /// Borrows properties in input order.
    pub fn properties(&self) -> &[IcsProperty] {
        &self.values
    }
    /// Borrows nested alarms in input order.
    pub fn alarms(&self) -> &[Self] {
        &self.alarms
    }
    /// Returns the opening component line.
    pub const fn location(&self) -> IcsLocation {
        self.location
    }

    fn serialize(&self, component: Component, lines: &mut Vec<String>) {
        lines.push(format!("BEGIN:{}", component.as_str()));
        lines.extend(self.values.iter().map(IcsProperty::serialize));
        for alarm in &self.alarms {
            alarm.serialize(Component::Alarm, lines);
        }
        lines.push(format!("END:{}", component.as_str()));
    }
}
