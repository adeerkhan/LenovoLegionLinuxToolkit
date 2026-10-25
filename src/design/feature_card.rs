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

use std::cell::RefCell;
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
    /// The last value `refresh` wrote into the control, so the control's own
    /// `changed` signal can be told apart from a user gesture.
    ///
    /// Setting a `Switch`/`DropDown`/`SpinButton` fires the same signal a user
    /// interaction does. Two earlier designs got this wrong in opposite
    /// directions: comparing the widget against the capability misfired for
    /// floats (a spin button rounding 55.0 to 54.999 read as an edit and
    /// issued a write), and a single "updating" flag held for the whole
    /// refresh swallowed any click that landed inside the refresh window —
    /// dropped with no write, no feedback, reported as the G-Sync toggle
    /// "snapping back". Comparing against the value the last refresh actually
    /// wrote suppresses the echo and leaves real gestures intact; the spin
    /// comparison tolerates half a step for the rounding.
    programmatic: Rc<RefCell<Option<Value>>>,
    value_label: Option<Label>,
    meter: Option<(ProgressBar, f64, f64)>,
    toggle: Option<Switch>,
    dropdown: Option<(DropDown, Vec<String>)>,
    spin: Option<SpinButton>,
    /// The spin button's step, for the echo tolerance. Unused by the other
    /// kinds, which compare exactly.
    step: Rc<std::cell::Cell<f64>>,
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
            programmatic: Rc::new(RefCell::new(None)),
            value_label: None,
            meter: None,
            toggle: None,
            dropdown: None,
            spin: None,
            step: Rc::new(std::cell::Cell::new(1.0)),
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
        // Fixed width: the selected label's length must not resize the card.
        dd.set_size_request(200, -1);
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
        // Remembered for the echo tolerance in `connect_controls`.
        self.step.set(limits.step.max(f64::EPSILON));
        spin.set_halign(Align::End);
        spin.set_valign(Align::Center);
        // Fixed width: a value gaining a digit must not shift the card's layout.
        spin.set_width_chars(4);
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

        // Every control kind gets the same two pieces: an echo test against
        // [`Self::programmatic`], and a `revert` that puts the widget back to
        // the last-read hardware value when a write fails — without it the
        // control lies about the state until the next refresh tick.
        if let Some(sw) = &self.toggle {
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();

            let programmatic = Rc::clone(&self.programmatic);
            sw.connect_state_set(move |sw, _| {
                // A programmatic set fires this too; only a user gesture should
                // reach the write path. The echo is the value the last refresh
                // wrote into this switch.
                let requested = sw.is_active();
                if matches!(&*programmatic.borrow(), Some(Value::Bool(b)) if *b == requested) {
                    return glib::Propagation::Proceed;
                }

                confirm_and_write(
                    &parent,
                    &card,
                    &cap,
                    Value::Bool(requested),
                    toggle_revert(sw, &programmatic),
                );
                glib::Propagation::Proceed
            });
        }

        if let Some((dd, values)) = &self.dropdown {
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();
            let values = values.clone();
            let dd_weak = dd.clone();
            let revert_values = values.clone();

            let programmatic = Rc::clone(&self.programmatic);
            let revert_programmatic = Rc::clone(&self.programmatic);
            let revert_dd = dd.clone();
            let revert: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(Value::Text(s)) = revert_programmatic.borrow().as_ref() {
                    if let Some(idx) = revert_values.iter().position(|v| v == s) {
                        revert_dd.set_selected(idx as u32);
                    }
                }
            });

            dd.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                let Some(value) = values.get(idx).cloned() else {
                    return;
                };
                if matches!(&*programmatic.borrow(), Some(Value::Text(last)) if *last == value) {
                    return;
                }
                confirm_and_write(&parent, &card, &cap, Value::Text(value), Rc::clone(&revert));
                let _ = dd_weak.clone();
            });
        }

        if let Some(spin) = &self.spin {
            let cap = Arc::clone(&cap);
            let parent = parent.clone();
            let card = self.card.clone();

            let programmatic = Rc::clone(&self.programmatic);
            let step = Rc::clone(&self.step);
            let revert_programmatic = Rc::clone(&self.programmatic);
            let revert_spin = spin.clone();
            let revert: Rc<dyn Fn()> = Rc::new(move || {
                let last = revert_programmatic.borrow().clone();
                match last {
                    Some(Value::Int(i)) => revert_spin.set_value(i as f64),
                    Some(Value::Float(f)) => revert_spin.set_value(f),
                    _ => {}
                }
            });

            spin.connect_value_changed(move |spin| {
                let new = spin.value();
                // Half a step of tolerance: a spin button may report 54.999
                // for the 55.0 that was set into it, and that must not read
                // as an edit.
                let echo = match &*programmatic.borrow() {
                    Some(Value::Float(f)) => (new - f).abs() < step.get() / 2.0,
                    Some(Value::Int(i)) => (new - *i as f64).abs() < step.get() / 2.0,
                    _ => false,
                };
                if echo {
                    return;
                }
                confirm_and_write(&parent, &card, &cap, Value::Float(new), Rc::clone(&revert));
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

        // Store what the controls are about to show *before* touching them.
        // The setters fire their `changed` signals synchronously, and an echo
        // is recognised by equality with the stored value — so the store must
        // happen first, or an echo for a value that changed since the last
        // cycle (Fn+Q flipped the mode, say) would compare against the stale
        // one and read as a user gesture. `None` — value not representable in
        // this control — disables echo suppression for the cycle, so a real
        // gesture is never mistaken for an echo.
        let shown = if self.toggle.is_some() {
            Some(value.clone())
        } else if let Some((_, values)) = &self.dropdown {
            match &value {
                Value::Text(s) if values.contains(s) => Some(value.clone()),
                _ => None,
            }
        } else if self.spin.is_some() {
            matches!(&value, Value::Int(_) | Value::Float(_)).then(|| value.clone())
        } else {
            None
        };
        *self.programmatic.borrow_mut() = shown;

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

/// A toggle's revert: put the switch back to the last value the hardware
/// reported.
fn toggle_revert(sw: &Switch, programmatic: &Rc<RefCell<Option<Value>>>) -> Rc<dyn Fn()> {
    let sw = sw.clone();
    let programmatic = Rc::clone(programmatic);
    Rc::new(move || {
        if let Some(Value::Bool(b)) = programmatic.borrow().as_ref() {
            sw.set_active(*b);
        }
    })
}

/// Ask for confirmation when the write warrants it, then run it.
fn confirm_and_write(
    parent: &impl gtk4::prelude::IsA<gtk4::Widget>,
    card: &LltCard,
    cap: &Arc<Capability>,
    value: Value,
    revert: Rc<dyn Fn()>,
) {
    if !cap.dangerous {
        submit_write(card, cap, value, revert);
        return;
    }

    // `MessageDialog` is deprecated in favour of `AdwAlertDialog` from GTK 4.10,
    // which is also what gets the libadwaita button styling for free.
    let dialog = adw::AlertDialog::builder()
        .heading("Hold the fans at maximum?")
        .body(
            "A write on the wrong path can leave the fans running at maximum \
             speed until the machine is rebooted. Recovering without a reboot \
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
                submit_write(&card, &cap, value.clone(), Rc::clone(&revert));
            }
        },
    );
}

/// Run a write off the main thread, then report the outcome on the main thread.
///
/// glib 0.22 dropped the channel API, and `MainContext::invoke` requires `Send`
/// — which GTK widgets are not in gtk4 0.11. So the worker hands its result over
/// a `std::sync::mpsc` channel and a short local timer drains it.
///
/// On failure `revert` puts the control back to the last-read hardware value
/// immediately; waiting for the next refresh tick to do it reads as the toggle
/// "snapping back" with no explanation.
fn submit_write(card: &LltCard, cap: &Arc<Capability>, value: Value, revert: Rc<dyn Fn()>) {
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
                    Err(message) => {
                        card.set_outcome(Severity::Error, &message);
                        revert();
                    }
                }
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        }
    });
}
