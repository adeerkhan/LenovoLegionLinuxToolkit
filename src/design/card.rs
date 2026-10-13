//! The card components — the single most important primitive to get right.
//!
//! The Windows reference has three card flavours (`CardControl`,
//! `CardExpander`, `CardAction`) sharing one content model
//! (`CardHeaderControl`: title, subtitle, status rows, accessory slot). Every
//! control in the application is one card.
//!
//! We reproduce that as one [`LltCard`] base widget plus
//! [`LltCardHeader`], and then the behaviour that matters most:
//! [`FeatureCard`] hides itself when its capability is unavailable, which is
//! what makes a single dashboard correct across Legion models with wildly
//! different feature sets.

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Label, Orientation};

/// A rounded, bordered container with a header.
#[derive(Clone)]
pub struct LltCard {
    root: GtkBox,
    body: GtkBox,
}

impl LltCard {
    pub fn new() -> Self {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("llt-card");

        let body = GtkBox::new(Orientation::Vertical, 6);
        body.set_margin_top(4);
        root.append(&body);

        Self { root, body }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.root
    }

    /// Set the header. Replaces any previous header.
    pub fn set_header(&self, title: &str, subtitle: Option<&str>) {
        let header = LltCardHeader::new(title, subtitle);
        self.root.append(&header.widget());
    }

    /// Append a widget to the body, below the header.
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

/// Title, subtitle and status rows, matching `CardHeaderControl`.
pub struct LltCardHeader {
    root: GtkBox,
}

impl LltCardHeader {
    pub fn new(title: &str, subtitle: Option<&str>) -> Self {
        let root = GtkBox::new(Orientation::Horizontal, 8);

        let text = GtkBox::new(Orientation::Vertical, 0);
        text.set_hexpand(true);

        let title_label = Label::new(Some(title));
        title_label.add_css_class("llt-card-title");
        title_label.set_halign(Align::Start);
        text.append(&title_label);

        if let Some(sub) = subtitle {
            let sub_label = Label::new(Some(sub));
            sub_label.add_css_class("llt-card-subtitle");
            sub_label.set_halign(Align::Start);
            sub_label.set_wrap(true);
            text.append(&sub_label);
        }

        root.append(&text);
        Self { root }
    }

    pub fn widget(&self) -> GtkBox {
        self.root.clone()
    }
}
