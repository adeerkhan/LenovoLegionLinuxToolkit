//! The auto-hiding capability card.
//!
//! This is the behaviour the whole design depends on. The Windows reference's
//! `AbstractRefreshingControl` hides any card that throws
//! `NotSupportedException` while refreshing, which is how one dashboard stays
//! correct across machines with wildly different feature sets.
//!
//! We go one step further and explain *why* a control is missing, because on
//! Linux the answer is usually one of two different things — "the driver is not
//! loaded" versus "your hardware does not have this" — and a greyed-out widget
//! with no reason is the single most common complaint about control panels.

use gtk4::prelude::*;
use gtk4::{Align, DropDown, Label, Orientation, Scale, SpinButton, Switch};

use crate::design::card::LltCard;
use legiontoolkit::capability::{Capability, Kind, Value};

/// A card rendered from one capability. Owns the capability so it can refresh
/// itself from the backend on demand.
pub struct FeatureCard {
    card: LltCard,
    capability: Option<Capability>,
    value_label: Option<Label>,
    meter: Option<Scale>,
    toggle: Option<Switch>,
    dropdown: Option<(DropDown, Vec<String>)>,
    spin: Option<SpinButton>,
}

impl FeatureCard {
    pub fn new(cap: Capability) -> Self {
        let card = LltCard::new();
        card.set_header(&cap.label, cap.note);

        let mut this = Self {
            card,
            capability: None,
            value_label: None,
            meter: None,
            toggle: None,
            dropdown: None,
            spin: None,
        };

        // Unavailable: say why, and stop. We do NOT render a disabled control —
        // an explanation is actionable, a greyed-out widget is not.
        if let Some(reason) = cap.unavailable_reason {
            this.show_unavailable(reason);
            return this;
        }

        match cap.kind {
            Kind::ReadOnly => this.build_readonly(&cap),
            Kind::Toggle => this.build_toggle(),
            Kind::Enum => this.build_enum(&cap),
            Kind::Slider => this.build_slider(&cap),
            Kind::Custom => this.show_unavailable("Not yet implemented"),
        }

        this.capability = Some(cap);
        this
    }

    fn show_unavailable(&self, reason: &str) {
        let note = Label::new(Some(reason));
        note.add_css_class("llt-unavailable");
        note.set_halign(Align::Start);
        note.set_wrap(true);
        self.card.append(&note);
    }

    fn build_readonly(&mut self, cap: &Capability) {
        let row = gtk4::Box::new(Orientation::Horizontal, 8);
        row.set_hexpand(true);

        let value = Label::new(None);
        value.add_css_class("llt-value");
        value.set_halign(Align::End);
        row.append(&value);

        // A compact meter makes trends readable at a glance, which is the whole
        // point of a dashboard row. Scale by unit: temperature tops out near
        // 110 C, fans near 6000 rpm, percentages at 100.
        let (min, max) = match cap.unit {
            Some("°C") => (0.0, 110.0),
            Some("rpm") => (0.0, 6000.0),
            _ => (0.0, 100.0),
        };
        let meter = Scale::with_range(Orientation::Horizontal, min, max, 1.0);
        meter.add_css_class("llt-meter");
        meter.set_draw_value(false);
        meter.set_hexpand(true);
        meter.set_sensitive(false);
        row.append(&meter);

        self.value_label = Some(value);
        self.meter = Some(meter);
        self.card.append(&row);
    }

    fn build_toggle(&mut self) {
        let sw = Switch::new();
        sw.set_halign(Align::End);
        sw.set_valign(Align::Center);
        self.toggle = Some(sw);
    }

    fn build_enum(&mut self, cap: &Capability) {
        let labels: Vec<&str> = cap.values.iter().map(|v| v.label.as_str()).collect();
        let values: Vec<String> = cap.values.iter().map(|v| v.value.clone()).collect();

        let dd = DropDown::from_strings(&labels);
        dd.set_halign(Align::End);
        dd.set_valign(Align::Center);
        self.dropdown = Some((dd, values));
    }

    fn build_slider(&mut self, cap: &Capability) {
        let limits = cap.limits.unwrap_or(legiontoolkit::capability::Limits {
            min: 0.0,
            max: 100.0,
            step: 1.0,
        });
        let spin = SpinButton::with_range(limits.min, limits.max, limits.step);
        spin.set_halign(Align::End);
        spin.set_valign(Align::Center);
        self.spin = Some(spin);
    }

    /// Re-read the capability and update every widget.
    ///
    /// A read failure blanks the row rather than removing the card: a file that
    /// vanished this instant is far more likely to come back than to mean the
    /// hardware lost the feature.
    pub fn refresh(&self) {
        let cap = match &self.capability {
            Some(c) => c,
            None => return,
        };

        let Ok(value) = cap.read() else {
            if let Some(label) = &self.value_label {
                label.set_label("—");
            }
            return;
        };

        let unit = cap.unit.unwrap_or("");

        if let Some(label) = &self.value_label {
            label.set_label(&match &value {
                Value::Float(f) => {
                    if unit.is_empty() {
                        format!("{f:.1}")
                    } else {
                        format!("{f:.1} {unit}")
                    }
                }
                other => {
                    if unit.is_empty() {
                        other.to_string()
                    } else {
                        format!("{other} {unit}")
                    }
                }
            });
        }

        if let Some(meter) = &self.meter {
            let n = match &value {
                Value::Float(f) => *f,
                Value::Int(i) => *i as f64,
                _ => 0.0,
            };
            meter.set_value(n);
        }

        if let Some(sw) = &self.toggle {
            sw.set_active(matches!(value, Value::Bool(true)));
        }

        if let Some((dd, values)) = &self.dropdown {
            if let Value::Text(s) = &value {
                if let Some(idx) = values.iter().position(|v| v == s) {
                    dd.set_selected(idx as u32);
                }
            }
        }

        if let Some(spin) = &self.spin {
            match &value {
                Value::Int(i) => spin.set_value(*i as f64),
                Value::Float(f) => spin.set_value(*f),
                _ => {}
            }
        }
    }

    /// The GTK widget to add to a container.
    pub fn gtk_widget(&self) -> &gtk4::Box {
        self.card.widget()
    }
}
