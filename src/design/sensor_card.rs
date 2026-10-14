//! The sensor card — the signature visual of the reference application.
//!
//! Ported from `SensorsControlV2.xaml` in the Lenovo Legion Toolkit reference:
//! a grid of cards, each with a semibold title and rows of *label, value, and a
//! small accent-coloured meter*. The 40×6 meter is the detail that makes a row
//! scannable at a glance rather than requiring you to read the number.
//!
//! The reference computes each meter's ceiling from the sensor's own maximum
//! (`UpdateValue` sets `bar.Maximum = max`), so a bar means the same thing on
//! every row — "how close to the top of this sensor's range", not "how big the
//! number is".

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Grid, Label, Orientation, ProgressBar};

/// One row inside a sensor card.
struct Row {
    unit: &'static str,
    /// How many decimal places to show. Reference uses 0 for temperature and
    /// RPM, 1 for clocks.
    precision: usize,
    /// The sensor's ceiling, so the bar reads as a fraction of range.
    max: f64,
    value: Label,
    meter: ProgressBar,
}

/// A card of related readings, matching the reference's `SensorCardStyle`.
pub struct SensorCard {
    root: GtkBox,
    /// The title row, so a right-aligned detail can be added beside the title.
    detail: GtkBox,
    rows: Vec<Row>,
}

impl SensorCard {
    pub fn new(title: &str) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 6);
        root.add_css_class("llt-card");
        root.add_css_class("llt-sensor-card");

        // The reference puts the device model at the right of the card title
        // rather than repeating it on every row.
        let title_row = GtkBox::new(Orientation::Horizontal, 8);
        let title_label = Label::new(Some(title));
        title_label.add_css_class("llt-sensor-title");
        title_label.set_halign(Align::Start);
        title_label.set_hexpand(true);
        title_row.append(&title_label);
        root.append(&title_row);

        Self {
            root,
            detail: title_row,
            rows: Vec::new(),
        }
    }

    /// Set the small right-aligned label beside the title — a device model.
    pub fn set_detail(&self, text: &str) {
        // Replace any previous detail so a re-render cannot stack them up.
        while let Some(child) = self.detail.last_child() {
            if child.downcast_ref::<Label>().is_some()
                && self.detail.observe_children().n_items() > 1
            {
                self.detail.remove(&child);
            } else {
                break;
            }
        }
        if text.is_empty() {
            return;
        }
        let label = Label::new(Some(text));
        label.add_css_class("llt-sensor-detail");
        label.set_halign(Align::End);
        label.set_valign(Align::Start);
        self.detail.append(&label);
    }

    /// Add a row of `label → value + meter`.
    ///
    /// `meter_max` is the sensor's ceiling, taken from the driver or a sensible
    /// default for the unit — never from the current reading, which would make
    /// the meter jitter as the value moves.
    pub fn add_row(
        &mut self,
        label: &str,
        unit: &'static str,
        precision: usize,
        meter_max: f64,
    ) -> usize {
        let row = Grid::new();
        row.set_column_spacing(8);

        let label_widget = Label::new(Some(label));
        label_widget.add_css_class("llt-sensor-label");
        label_widget.set_halign(Align::Start);
        label_widget.set_hexpand(true);
        label_widget.set_wrap(true);
        label_widget.set_max_width_chars(18);
        row.attach(&label_widget, 0, 0, 1, 1);

        let value = Label::new(None);
        value.add_css_class("llt-sensor-value");
        value.set_halign(Align::End);
        value.set_label("—");
        row.attach(&value, 1, 0, 1, 1);

        let meter = ProgressBar::new();
        meter.add_css_class("llt-meter");
        meter.set_show_text(false);
        meter.set_size_request(40, 6);
        meter.set_valign(Align::Center);
        row.attach(&meter, 2, 0, 1, 1);

        self.root.append(&row);
        self.rows.push(Row {
            unit,
            precision,
            max: meter_max,
            value,
            meter,
        });
        self.rows.len() - 1
    }

    /// Update one row. `None` blanks it rather than hiding the row, so a
    /// sensor that appears or disappears does not make the card jump.
    pub fn set_value(&self, row: usize, value: Option<f64>) {
        let Some(row_data) = self.rows.get(row) else {
            return;
        };
        match value {
            Some(value) => {
                // Temperatures are converted for display; the sensor always
                // reports the unit it measured in. The meter is unaffected —
                // a fraction of range is the same in either scale.
                let (display, unit) = legiontoolkit::settings::for_display(value, row_data.unit);
                row_data
                    .value
                    .set_label(&format!("{:.*} {}", row_data.precision, display, unit));
                row_data
                    .meter
                    .set_fraction((value / row_data.max).clamp(0.0, 1.0));
            }
            None => {
                row_data.value.set_label("—");
                row_data.meter.set_fraction(0.0);
            }
        }
    }

    /// Show a value that is not a bare number, e.g. the reference's
    /// `10.8/15.7 GB` on its RAM card.
    ///
    /// The meter still takes a fraction, so the row keeps its at-a-glance
    /// reading while the label carries the absolute.
    pub fn set_text(&self, row: usize, fraction: f64, text: &str) {
        let Some(row_data) = self.rows.get(row) else {
            return;
        };
        row_data.value.set_label(text);
        row_data.meter.set_fraction(fraction.clamp(0.0, 1.0));
    }

    pub fn widget(&self) -> &GtkBox {
        &self.root
    }
}
