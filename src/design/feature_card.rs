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

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk4::{Align, DropDown, Label, Orientation, ProgressBar, SpinButton, Switch};

use crate::design::card::{LltCard, Severity, Status};
use legiontoolkit::capability::{Capability, Kind, Limits, Value};

/// A card rendered from one capability. Owns the capability so it can refresh
/// itself from the backend on demand.
pub struct FeatureCard {
    card: LltCard,
    capability: Option<Arc<Capability>>,
    /// True while `refresh` is writing values into the controls.
    ///
    /// Setting a `SpinButton` or `DropDown` fires the same `changed` signal a
    /// user interaction does, so without this every refresh looked like an edit.
    /// The earlier guard compared the widget's value against the capability's
    /// and skipped if equal — which is unreliable for floats: a spin button
    /// rounding 55.0 to 54.999 read as a change and issued a write. The
    /// reference guards the same way, with an `IsRefreshing` field.
    updating: Rc<Cell<bool>>,
    value_label: Option<Label>,
    meter: Option<(ProgressBar, f64, f64)>,
    toggle: Option<Switch>,
    dropdown: Option<(DropDown, Vec<String>)>,
    spin: Option<SpinButton>,
}

impl FeatureCard {
    pub fn new(cap: Arc<Capability>) -> Self {
        let card = LltCard::new();

        // The reference's card anatomy: icon, title, subtitle, then any status
        // rows. Status comes from the capability so a control can explain a
        // consequence before the user touches it.
        // Info rows are explanatory and render even when the capability is
        // unavailable: on an unsupported machine they are the only way to say
        // what the control would do. A *warning* is suppressed there, because
        // warning about a consequence the user cannot cause is noise.
        // `compact_mode` hides the subtitle and the status rows, matching the
        // reference's compact layout. Read at build time: pages are built once,
        // so the switch is labelled as taking effect on the next launch rather
        // than pretending to be live.
        let compact = legiontoolkit::settings::current().compact_mode;

        let mut status: Vec<Status> = if compact {
            Vec::new()
        } else {
            cap.infos
                .iter()
                .map(|text| Status {
                    severity: Severity::Info,
                    text,
                })
                .collect()
        };
        if !compact && cap.unavailable_reason.is_none() {
            status.extend(cap.warnings.iter().map(|text| Status {
                severity: Severity::Warning,
                text,
            }));
        }

        card.set_header(
            cap.icon,
            &cap.label,
            if compact { None } else { cap.note.as_deref() },
            &status,
        );

        let mut this = Self {
            card,
            updating: Rc::new(Cell::new(false)),
            value_label: None,
            meter: None,
            toggle: None,
            dropdown: None,
            spin: None,
            capability: None,
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
        // The reference's compact bar is a fixed 40x6 at the end of the row
        // (`CompactProgressBarStyle`), not a full-width track. A bar that spans
        // the card competes with the number instead of supporting it.
        let meter = ProgressBar::new();
        meter.add_css_class("llt-meter");
        meter.set_show_text(false);
        meter.set_size_request(40, 6);
        meter.set_hexpand(false);
        meter.set_valign(Align::Center);
        meter.set_margin_start(8);
        row.append(&meter);

        self.value_label = Some(value);
        // Store both bounds: a bar should read as position within the range,
        // which is only the raw value when the range starts at zero.
        self.meter = Some((meter, min, max));
        self.card.set_accessory(&row);
    }

    fn build_toggle(&mut self) {
        let sw = Switch::new();
        sw.set_halign(Align::End);
        sw.set_valign(Align::Center);
        // The control belongs in the header's accessory slot, as in the
        // reference. It was previously built and then never added at all.
        self.card.set_accessory(&sw);
        self.toggle = Some(sw);
    }

    fn build_enum(&mut self, cap: &Capability) {
        let labels: Vec<&str> = cap.values.iter().map(|v| v.label.as_str()).collect();
        let values: Vec<String> = cap.values.iter().map(|v| v.value.clone()).collect();

        let dd = DropDown::from_strings(&labels);
        dd.set_halign(Align::End);
        dd.set_valign(Align::Center);
        self.card.set_accessory(&dd);
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
        self.spin = Some(spin.clone());

        // A bare number is not a value. Show the unit beside the control so
        // "97" reads as "97 °C", as the reference does.
        match cap.unit {
            Some(unit) => {
                let group = gtk4::Box::new(Orientation::Horizontal, 6);
                group.set_valign(Align::Center);
                group.append(&spin);
                let unit_label = Label::new(Some(unit));
                unit_label.add_css_class("llt-card-subtitle");
                unit_label.set_valign(Align::Center);
                group.append(&unit_label);
                self.card.set_accessory(&group);
            }
            None => self.card.set_accessory(&spin),
        }
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
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();

            let updating = Rc::clone(&self.updating);
            sw.connect_state_set(move |sw, _| {
                // A programmatic set fires this too; only a user gesture should
                // reach the write path.
                if updating.get() {
                    return glib::Propagation::Proceed;
                }
                let requested = sw.is_active();

                confirm_and_write(&parent, &card, &cap, Value::Bool(requested));
                glib::Propagation::Proceed
            });
        }

        if let Some((dd, values)) = &self.dropdown {
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();
            let values = values.clone();
            let dd_weak = dd.clone();

            let updating = Rc::clone(&self.updating);
            dd.connect_selected_notify(move |dd| {
                if updating.get() {
                    return;
                }
                let idx = dd.selected() as usize;
                let Some(value) = values.get(idx).cloned() else {
                    return;
                };
                confirm_and_write(&parent, &card, &cap, Value::Text(value));
                let _ = dd_weak.clone();
            });
        }

        if let Some(spin) = &self.spin {
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();

            let updating = Rc::clone(&self.updating);
            spin.connect_value_changed(move |spin| {
                if updating.get() {
                    return;
                }
                confirm_and_write(&parent, &card, &cap, Value::Float(spin.value()));
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
        self.updating.set(true);
        // Cleared on every exit path, so a panic in a widget setter cannot leave
        // the card permanently deaf to the user.
        let _guard = UpdateGuard(&self.updating);

        let Ok(value) = cap.read() else {
            if let Some(label) = &self.value_label {
                label.set_label("—");
            }
            return;
        };

        let unit = cap.unit.unwrap_or("");

        // A temperature is converted for display only; the capability keeps
        // reporting Celsius, which is what the hardware gives.
        // One lookup: `for_display` returns both the converted value and the
        // unit it converted to.
        let (value, unit) = match &value {
            Value::Float(f) if !unit.is_empty() => {
                let (converted, unit) = legiontoolkit::settings::for_display(*f, unit);
                (Value::Float(converted), unit)
            }
            other => (other.clone(), unit),
        };

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

        if let Some((meter, min, max)) = &self.meter {
            let n = match &value {
                Value::Float(f) => *f,
                Value::Int(i) => *i as f64,
                _ => 0.0,
            };
            let span = (max - min).max(f64::EPSILON);
            meter.set_fraction(((n - min) / span).clamp(0.0, 1.0));
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
    cap: &Arc<Capability>,
    value: Value,
) {
    if !cap.dangerous {
        submit_write(card, cap, value);
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

    dialog.choose(
        Some(parent),
        None::<&gtk4::gio::Cancellable>,
        move |response: glib::GString| {
            if response == "continue" {
                submit_write(&card, &cap, value.clone());
            }
        },
    );
}

/// Run a write off the main thread, then report the outcome on the main thread.
///
/// glib 0.22 dropped the channel API, and `MainContext::invoke` requires `Send`
/// — which GTK widgets are not in gtk4 0.11. So the worker hands its result over
/// a `std::sync::mpsc` channel and a short local timer drains it.
fn submit_write(card: &LltCard, cap: &Arc<Capability>, value: Value) {
    card.set_outcome(Severity::Info, "Applying…");
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
    let receiver = std::rc::Rc::new(std::cell::RefCell::new(receiver));

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        // `try_recv` only needs an immutable borrow, so a RefCell is enough.
        let outcome = receiver.borrow().try_recv().ok();
        match outcome {
            Some(outcome) => {
                card.set_sensitive(true);
                match outcome {
                    // A successful write clears the row rather than announcing
                    // itself: the control's new value is the confirmation.
                    Ok(()) => card.set_outcome(Severity::Success, ""),
                    Err(message) => card.set_outcome(Severity::Error, &message),
                }
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        }
    });
}

/// Clears an `updating` flag when dropped.
struct UpdateGuard<'a>(&'a std::cell::Cell<bool>);

impl Drop for UpdateGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
