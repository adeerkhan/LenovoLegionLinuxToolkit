//! The capability model — the stable API the UI is written against.
//!
//! The UI never touches a sysfs path. It holds [`Capability`] descriptors,
//! which carry availability, the enumerated values, units and limits needed to
//! render the right control. This is what lets one dashboard work across a
//! Legion 5 15IRX10 (no fan curve, no power limits) and a Legion Pro 7 (both),
//! without a single `if model == ...` in the presentation layer.

use std::fmt;

/// What kind of widget a capability should render as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A switch.
    Toggle,
    /// A row of choices, rendered as a segmented control.
    Enum,
    /// A numeric control with a range.
    Slider,
    /// Display only.
    ReadOnly,
    /// The capability renders something bespoke (a fan-curve canvas, etc).
    Custom,
}

/// Which dashboard section a capability belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Power,
    /// CPU and GPU controls and readouts, kept apart from Power.
    ///
    /// The power limits and the GPU telemetry are the bulk of what the
    /// firmware exposes; leaving them under "Power" buried them behind the mode
    /// selector.
    Processors,
    Thermal,
    Lighting,
    Battery,
    System,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Power => "Power",
            Self::Processors => "Processors & GPU",
            Self::Thermal => "Thermal",
            Self::Lighting => "Lighting",
            Self::Battery => "Battery",
            Self::System => "System",
        }
    }
}

/// One selectable value of an [`Kind::Enum`] capability.
#[derive(Debug, Clone)]
pub struct NamedValue {
    /// The wire value written back to the backend.
    pub value: String,
    /// What the user sees.
    pub label: String,
}

/// Range and granularity for a [`Kind::Slider`].
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// Read a whole file, trimmed. sysfs files are newline-terminated and carry no
/// meaningful whitespace, so one helper covers both string and numeric reads.
pub fn read_trim(path: &std::path::PathBuf) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?.trim().to_string())
}

/// Rebuild a [`Value`] of the right shape from a stored string.
///
/// A preset or an automation step stores every value as text. Writing a
/// toggle's `"1"` back as `Value::Text("1")` does not match the
/// `Value::Bool(true)` its write closure tests for, so the string is
/// interpreted by the capability's own [`Kind`] — the only place that knows
/// what shape the value takes.
pub fn parse_stored(kind: Kind, stored: &str) -> Value {
    let trimmed = stored.trim();
    match kind {
        Kind::Toggle => Value::Bool(matches!(trimmed, "1" | "true" | "on" | "yes")),
        Kind::Slider => trimmed
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or_else(|_| Value::Text(stored.to_string())),
        _ => Value::Text(stored.to_string()),
    }
}

/// The value read from or written to a capability.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Text(String),
    Int(i64),
    Float(f64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(b) => write!(f, "{}", if *b { "on" } else { "off" }),
            Self::Text(s) => write!(f, "{s}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Float(v) => write!(f, "{v}"),
        }
    }
}

/// A single controllable or observable thing.
///
/// `read` is always present; `write` is `None` for read-only capabilities. Both
/// capture their own backing path so a capability is self-contained and can be
/// handed to the UI without it knowing anything about sysfs.
pub struct Capability {
    /// Stable identifier, e.g. `"powermode"`. Persisted in profiles.
    ///
    /// Owned rather than `&'static str` because some capabilities are
    /// *discovered* rather than declared — the firmware-attributes class
    /// exposes a variable set of power limits whose names differ per model, and
    /// they cannot be written as literals.
    pub key: String,
    pub label: String,
    pub icon: &'static str,
    pub kind: Kind,
    pub group: Group,

    /// Why the capability is unavailable, when it is. Shown to the user rather
    /// than leaving a disabled mystery.
    pub unavailable_reason: Option<&'static str>,

    /// Values for an enum, taken from sysfs where the driver offers a
    /// `choices` file.
    pub values: Vec<NamedValue>,

    /// Range for a slider.
    pub limits: Option<Limits>,

    /// `Some("°C")` — appended by the UI for display only.
    pub unit: Option<&'static str>,

    /// A description under the title, matching the reference's subtitle.
    ///
    /// Owned rather than `&'static str`: a note composed at build time (the
    /// disk-usage card's "150 GiB of 230 GiB") would otherwise have to be
    /// leaked, once per `capabilities()` call.
    pub note: Option<String>,

    /// Card status rows, matching the reference's `Warning` / `Info` / `Error` /
    /// `Success`. These are how a control explains a consequence before the user
    /// touches it, rather than after.
    pub warnings: Vec<&'static str>,
    pub infos: Vec<&'static str>,

    /// Marks a write that can leave hardware in a state the user cannot undo
    /// from the UI. Not a permission gate — a prompt, and a record that the
    /// consequence is understood.
    pub dangerous: bool,

    read: Box<dyn Fn() -> anyhow::Result<Value> + Send + Sync>,
    write: Option<Box<dyn Fn(Value) -> anyhow::Result<()> + Send + Sync>>,
}

impl Capability {
    pub fn read(&self) -> anyhow::Result<Value> {
        (self.read)()
    }

    pub fn write(&self, value: Value) -> anyhow::Result<()> {
        match &self.write {
            Some(f) => f(value),
            None => Err(anyhow::anyhow!("{} is read-only", self.key)),
        }
    }

    /// True when the backing file exists and is readable right now.
    pub fn is_available(&self) -> bool {
        self.unavailable_reason.is_none()
    }

    /// True when this capability can be written.
    ///
    /// Used by [`crate::policy`] to prove the GUI's write surface and the
    /// privileged helper's allowlist agree.
    pub fn can_write(&self) -> bool {
        self.write.is_some()
    }
}

/// A builder, because `Capability` has more fields than any call site cares
/// about and most of them are `None`.
pub struct Builder {
    key: String,
    label: String,
    icon: &'static str,
    kind: Kind,
    group: Group,
    unavailable_reason: Option<&'static str>,
    values: Vec<NamedValue>,
    limits: Option<Limits>,
    unit: Option<&'static str>,
    note: Option<String>,
    warnings: Vec<&'static str>,
    infos: Vec<&'static str>,
    dangerous: bool,
    read: Option<Box<dyn Fn() -> anyhow::Result<Value> + Send + Sync>>,
    write: Option<Box<dyn Fn(Value) -> anyhow::Result<()> + Send + Sync>>,
}

impl Builder {
    pub fn new(key: impl Into<String>, kind: Kind, group: Group) -> Self {
        Self {
            key: key.into(),
            label: String::new(),
            icon: "",
            kind,
            group,
            unavailable_reason: None,
            values: Vec::new(),
            limits: None,
            unit: None,
            note: None,
            warnings: Vec::new(),
            infos: Vec::new(),
            dangerous: false,
            read: None,
            write: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }

    /// Mark the capability unavailable with a reason shown to the user.
    pub fn unavailable(mut self, reason: &'static str) -> Self {
        self.unavailable_reason = Some(reason);
        self
    }

    pub fn values(mut self, values: Vec<NamedValue>) -> Self {
        self.values = values;
        self
    }

    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }

    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// A caution shown on the card itself.
    pub fn warning(mut self, text: &'static str) -> Self {
        self.warnings.push(text);
        self
    }

    /// A neutral explanation shown on the card itself.
    pub fn info(mut self, text: &'static str) -> Self {
        self.infos.push(text);
        self
    }

    /// Require confirmation before this write is allowed through.
    pub fn dangerous(mut self) -> Self {
        self.dangerous = true;
        self
    }

    pub fn read(mut self, f: impl Fn() -> anyhow::Result<Value> + Send + Sync + 'static) -> Self {
        self.read = Some(Box::new(f));
        self
    }

    pub fn write(
        mut self,
        f: impl Fn(Value) -> anyhow::Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.write = Some(Box::new(f));
        self
    }

    pub fn build(self) -> Capability {
        Capability {
            key: self.key,
            label: self.label,
            icon: self.icon,
            kind: self.kind,
            group: self.group,
            unavailable_reason: self.unavailable_reason,
            values: self.values,
            limits: self.limits,
            unit: self.unit,
            note: self.note,
            warnings: self.warnings,
            infos: self.infos,
            dangerous: self.dangerous,
            read: self
                .read
                .unwrap_or_else(|| Box::new(|| Ok(Value::Text("—".into())))),
            write: self.write,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toggle stored as `"1"` must come back as `Bool(true)`: its write
    /// closure treats only `Bool(true)` as on, so a plain `Text("1")` would
    /// write `0` and silently turn the setting off.
    #[test]
    fn stored_values_recover_their_kind() {
        assert_eq!(parse_stored(Kind::Toggle, "1"), Value::Bool(true));
        assert_eq!(parse_stored(Kind::Toggle, "true"), Value::Bool(true));
        assert_eq!(parse_stored(Kind::Toggle, "0"), Value::Bool(false));
        assert_eq!(parse_stored(Kind::Slider, "70"), Value::Float(70.0));
        assert_eq!(
            parse_stored(Kind::Enum, "performance"),
            Value::Text("performance".into())
        );
    }

    /// A slider value that is not a number must not silently become zero.
    #[test]
    fn a_nonsense_slider_value_is_kept_as_text() {
        assert_eq!(
            parse_stored(Kind::Slider, "high"),
            Value::Text("high".into())
        );
    }
}
