//! The capability card: renders one capability and, when it is writable,
//! performs the write.
//!
//! Two ideas carry most of the weight here.
//!
//! The first is the Windows reference's `AbstractRefreshingControl`: a card
//! whose capability is unavailable explains itself and stops, rather than
//! rendering a disabled control. On Linux the difference matters more than it
//! does on Windows — "the driver is not loaded" and "your hardware does not
//! have this" need different actions from the user, and a greyed-out switch
//! cannot tell them apart.
//!
//! The second is that writes go through [`crate::services::privilege`], which
//! escalates to a helper binary under polkit. A polkit prompt is a blocking
//! user interaction, so the write runs off the GTK main thread and its result
//! comes back through a channel — otherwise the window freezes for the duration
//! of the prompt, which looks exactly like a crash.

use std::sync::Arc;

use adw::prelude::*;
use gtk4::{Align, DropDown, Label, Orientation, Scale, SpinButton, Switch};

use crate::design::card::LltCard;
use legiontoolkit::capability::{Capability, Kind, Limits, Value};

/// A card rendered from one capability. Owns the capability so it can refresh
/// itself from the backend on demand.
pub struct FeatureCard {
    card: LltCard,
    capability: Option<Arc<Capability>>,
    value_label: Option<Label>,
    meter: Option<Scale>,
    toggle: Option<Switch>,
    dropdown: Option<(DropDown, Vec<String>)>,
    spin: Option<SpinButton>,
    /// Shown under the control while a write is running, or when one fails.
    status: Label,
}

impl FeatureCard {
    pub fn new(cap: Capability) -> Self {
        let card = LltCard::new();
        card.set_header(&cap.label, cap.note);

        let status = Label::new(None);
        status.add_css_class("llt-status");
        status.set_halign(Align::Start);
        status.set_wrap(true);
        status.set_visible(false);

        let mut this = Self {
            card,
            value_label: None,
            meter: None,
            toggle: None,
            dropdown: None,
            spin: None,
            capability: None,
            status,
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

        this.capability = Some(Arc::new(cap));
        this.card.append(&this.status);
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
        let limits = cap.limits.unwrap_or(Limits {
            min: 0.0,
            max: 100.0,
            step: 1.0,
        });
        let spin = SpinButton::with_range(limits.min, limits.max, limits.step);
        spin.set_halign(Align::End);
        spin.set_valign(Align::Center);
        self.spin = Some(spin);
    }

    /// Connect the controls to their capability.
    ///
    /// Kept separate from `new` so the card can be built without a parent
    /// window, which is what the tests and the capability matrix exercise.
    /// Wire the controls to their capability.
    ///
    /// `parent` is taken by value so the confirmation dialog's closure can own
    /// a reference to the window without a lifetime.
    pub fn connect_controls(&self, parent: gtk4::Widget) {
        let cap = match &self.capability {
            Some(c) if c.can_write() => Arc::clone(c),
            _ => return,
        };

        if let Some(sw) = &self.toggle {
            let active_now = cap.read().map(|v| matches!(v, Value::Bool(true)));
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let status = self.status.clone();
            let card = self.card.clone();

            sw.connect_state_set(move |sw, _| {
                // `connect_state_set` fires for programmatic changes too, so
                // compare against what the hardware said before deciding this
                // was a user action.
                let current = active_now.as_ref().map(|b| *b).unwrap_or(false);
                let requested = sw.is_active();

                if requested == current {
                    return glib::Propagation::Proceed;
                }

                confirm_and_write(&parent, &card, &status, &cap, Value::Bool(requested));
                glib::Propagation::Proceed
            });
        }

        if let Some((dd, values)) = &self.dropdown {
            let cap = Arc::clone(&cap);
            let status = self.status.clone();
            let parent = parent.clone();
            let card = self.card.clone();
            let values = values.clone();
            let dd_weak = dd.clone();

            dd.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                let Some(value) = values.get(idx).cloned() else {
                    return;
                };
                // Ignore the programmatic selection that `refresh` performs.
                if let Ok(Value::Text(current)) = cap.read() {
                    if current == value {
                        return;
                    }
                }
                confirm_and_write(&parent, &card, &status, &cap, Value::Text(value));
                let _ = dd_weak.clone();
            });
        }

        if let Some(spin) = &self.spin {
            let cap = Arc::clone(&cap);
            let status = self.status.clone();
            let parent = parent.clone();
            let card = self.card.clone();

            spin.connect_value_changed(move |spin| {
                let requested = Value::Float(spin.value());
                if let Ok(current) = cap.read() {
                    if current == requested {
                        return;
                    }
                }
                confirm_and_write(&parent, &card, &status, &cap, requested);
            });
        }
    }

    /// Re-read the capability and update every widget.
    ///
    /// A read failure blanks the value rather than removing the card: a file
    /// that vanished this instant is far more likely to come back than to mean
    /// the hardware permanently lost the feature.
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

/// Ask for confirmation when the write warrants it, then run it.
fn confirm_and_write(
    parent: &impl gtk4::prelude::IsA<gtk4::Widget>,
    card: &LltCard,
    status: &Label,
    cap: &Arc<Capability>,
    value: Value,
) {
    if !cap.dangerous {
        submit_write(card, status, cap, value);
        return;
    }

    // `MessageDialog` is deprecated in favour of `AdwAlertDialog` from GTK 4.10,
    // which is also what gets the libadwaita button styling for free.
    let dialog = adw::AlertDialog::builder()
        .heading("Hold the fans at maximum?")
        .body(
            "A write on the wrong path can leave the fans running at maximum              speed until the machine is rebooted. Recovering without a reboot \
             means powering off manually.",
        )
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("continue", "Continue");
    dialog.set_response_appearance("continue", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    let cap = Arc::clone(cap);
    let card = card.clone();
    let status = status.clone();

    dialog.choose(
        Some(parent),
        None::<&gtk4::gio::Cancellable>,
        move |response: glib::GString| {
            if response == "continue" {
                submit_write(&card, &status, &cap, value.clone());
            }
        },
    );
}

/// Run a write off the main thread, then report the outcome on the main thread.
///
/// glib 0.22 dropped the channel API, and `MainContext::invoke` requires `Send`
/// — which GTK widgets are not in gtk4 0.11. So the worker hands its result over
/// a `std::sync::mpsc` channel and a short local timer drains it.
fn submit_write(card: &LltCard, status: &Label, cap: &Arc<Capability>, value: Value) {
    status.set_label("Applying…");
    status.add_css_class("applying");
    status.set_visible(true);
    card.set_sensitive(false);

    let cap = Arc::clone(cap);
    let (sender, receiver) = std::sync::mpsc::channel::<std::result::Result<(), String>>();

    // The write is the slow part — it may block on a polkit prompt — so it must
    // not run on the thread that owns the window.
    std::thread::spawn(move || {
        let outcome = cap.write(value).map_err(|e| e.to_string());
        let _ = sender.send(outcome);
    });

    let card = card.clone();
    let status = status.clone();
    let receiver = std::rc::Rc::new(std::cell::RefCell::new(receiver));

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        // `try_recv` only needs an immutable borrow, so a RefCell is enough.
        let outcome = receiver.borrow().try_recv().ok();
        match outcome {
            Some(outcome) => {
                card.set_sensitive(true);
                status.remove_css_class("applying");
                match outcome {
                    Ok(()) => {
                        status.set_label("");
                        status.set_visible(false);
                    }
                    Err(message) => {
                        status.set_label(&message);
                        status.set_visible(true);
                    }
                }
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        }
    });
}
