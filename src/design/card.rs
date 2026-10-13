//! The card components — the single most important primitive to get right.
//!
//! The Windows reference builds every control in the application from one
//! content model. Its WPF-UI `CardControl` carries an **icon**, and its
//! `CardHeaderControl` carries a title, a subtitle, **four status rows**
//! (`Info`, `Warning`, `Error`, `Success`) and an **accessory slot** for the
//! control itself.
//!
//! That anatomy is what makes the reference read as an application rather than
//! a form: a card states its name, explains itself, warns about consequences,
//! and only then offers a control. Earlier versions of this file had a title
//! and a widget in a box, which is why the two looked so different.
//!
//! Layout, matching the reference:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │ [icon]  Title                        [ accessory ] │
//! │         Subtitle                                    │
//! │         ⓘ Info row                                  │
//! │         ⚠ Warning row                               │
//! │         (body, for content that is not a control)   │
//! └─────────────────────────────────────────────────────┘
//! ```

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Image, Label, Orientation};

/// A severity of card-level status, matching the reference's four status rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Success,
}

impl Severity {
    /// The icon the reference pairs with each severity: `Info24`,
    /// `ErrorCircle24`, `DismissCircle24`, `CheckmarkCircle24`.
    fn icon(self) -> &'static str {
        match self {
            Self::Info => "dialog-information-symbolic",
            Self::Warning => "dialog-warning-symbolic",
            Self::Error => "dialog-error-symbolic",
            Self::Success => "emblem-ok-symbolic",
        }
    }

    fn css_class(self) -> &'static str {
        match self {
            Self::Info => "llt-status-info",
            Self::Warning => "llt-status-warning",
            Self::Error => "llt-status-error",
            Self::Success => "llt-status-success",
        }
    }
}

/// A card-level status message.
pub struct Status {
    pub severity: Severity,
    pub text: &'static str,
}

/// A rounded, bordered container with the reference's card anatomy.
#[derive(Clone)]
pub struct LltCard {
    root: GtkBox,
    /// Title, subtitle and status rows.
    text: GtkBox,
    /// Where the control goes — in the header, to the right of the text.
    accessory: GtkBox,
    /// Content below the header.
    body: GtkBox,
    /// The outcome of the last write, shown as a status row. Rebuilt on each
    /// change rather than mutated, because a status row's icon and colour come
    /// from its severity.
    outcome: gtk4::Box,
}

impl LltCard {
    pub fn new() -> Self {
        let root = GtkBox::new(Orientation::Vertical, 6);
        root.add_css_class("llt-card");

        let header = GtkBox::new(Orientation::Horizontal, 12);
        let text = GtkBox::new(Orientation::Vertical, 2);
        text.set_hexpand(true);
        header.append(&text);

        let accessory = GtkBox::new(Orientation::Horizontal, 8);
        accessory.set_valign(Align::Center);
        header.append(&accessory);

        let body = GtkBox::new(Orientation::Vertical, 6);
        body.set_valign(Align::Center);

        let outcome = GtkBox::new(Orientation::Vertical, 0);

        root.append(&header);
        root.append(&body);
        root.append(&outcome);

        Self {
            root,
            text,
            accessory,
            body,
            outcome,
        }
    }

    /// Show, replace, or clear the write-outcome row.
    pub fn set_outcome(&self, severity: Severity, text: &str) {
        while let Some(child) = self.outcome.first_child() {
            self.outcome.remove(&child);
        }
        if text.is_empty() {
            return;
        }
        // Leaked deliberately: a status row outlives no more than the card, and
        // the alternative is threading a lifetime through the whole widget tree
        // for a string that is displayed once.
        let text: &'static str = Box::leak(text.to_string().into_boxed_str());
        self.outcome.append(&status_row(&Status { severity, text }));
    }

    pub fn widget(&self) -> &GtkBox {
        &self.root
    }

    /// Fill the header. `status` may be empty; where it is not, each entry
    /// renders as an icon and a coloured line, exactly as the reference does.
    pub fn set_header(&self, icon: &str, title: &str, subtitle: Option<&str>, status: &[Status]) {
        // Icon first, vertically centred against the title, as in the reference.
        if !icon.is_empty() {
            let image = Image::from_icon_name(icon);
            image.set_pixel_size(20);
            image.set_valign(Align::Start);
            image.set_margin_top(2);
            image.add_css_class("llt-card-icon");
            if let Some(parent) = self.text.parent().and_downcast::<GtkBox>() {
                parent.prepend(&image);
            }
        }

        let title_label = Label::new(Some(title));
        title_label.add_css_class("llt-card-title");
        title_label.set_halign(Align::Start);
        self.text.append(&title_label);

        if let Some(subtitle) = subtitle {
            let label = Label::new(Some(subtitle));
            label.add_css_class("llt-card-subtitle");
            label.set_halign(Align::Start);
            label.set_wrap(true);
            label.set_xalign(0.0);
            self.text.append(&label);
        }

        for entry in status {
            self.text.append(&status_row(entry));
        }
    }

    /// Put the card's control in the header's accessory slot.
    pub fn set_accessory(&self, widget: &impl gtk4::prelude::IsA<gtk4::Widget>) {
        self.accessory.append(widget);
    }

    /// Append content below the header.
    pub fn append(&self, widget: &impl gtk4::prelude::IsA<gtk4::Widget>) {
        self.body.append(widget);
    }

    /// Disable every control in the card. Used while a write is in flight, so
    /// the user cannot queue a second one on top of an unfinished first.
    pub fn set_sensitive(&self, sensitive: bool) {
        self.root.set_sensitive(sensitive);
    }
}

impl Default for LltCard {
    fn default() -> Self {
        Self::new()
    }
}

/// One status line: a small icon and a coloured message.
fn status_row(entry: &Status) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 6);
    row.add_css_class("llt-card-status");
    row.add_css_class(entry.severity.css_class());

    let icon = Image::from_icon_name(entry.severity.icon());
    icon.set_pixel_size(12);
    icon.set_valign(Align::Start);
    icon.set_margin_top(2);
    row.append(&icon);

    let label = Label::new(Some(entry.text));
    label.add_css_class("llt-card-status-text");
    label.set_halign(Align::Start);
    label.set_wrap(true);
    label.set_xalign(0.0);
    row.append(&label);

    row
}
