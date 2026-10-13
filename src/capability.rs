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
    Thermal,
    Lighting,
    Battery,
    System,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Power => "Power",
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
    pub key: &'static str,
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

    /// For read-only capabilities, an extra hint under the value.
    pub note: Option<&'static str>,

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
    key: &'static str,
    label: String,
    icon: &'static str,
    kind: Kind,
    group: Group,
    unavailable_reason: Option<&'static str>,
    values: Vec<NamedValue>,
    limits: Option<Limits>,
    unit: Option<&'static str>,
    note: Option<&'static str>,
    dangerous: bool,
    read: Option<Box<dyn Fn() -> anyhow::Result<Value> + Send + Sync>>,
    write: Option<Box<dyn Fn(Value) -> anyhow::Result<()> + Send + Sync>>,
}

impl Builder {
    pub fn new(key: &'static str, kind: Kind, group: Group) -> Self {
        Self {
            key,
            label: String::new(),
            icon: "",
            kind,
            group,
            unavailable_reason: None,
            values: Vec::new(),
            limits: None,
            unit: None,
            note: None,
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

    pub fn note(mut self, note: &'static str) -> Self {
        self.note = Some(note);
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
            dangerous: self.dangerous,
            read: self
                .read
                .unwrap_or_else(|| Box::new(|| Ok(Value::Text("—".into())))),
            write: self.write,
        }
    }
}
