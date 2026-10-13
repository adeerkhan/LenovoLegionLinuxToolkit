//! The main window: sidebar navigation, one dashboard, and one page per
//! capability group.
//!
//! The reference application has a landing dashboard built from capability
//! cards, plus dedicated pages. This reproduces both halves — earlier the
//! dashboard existed and four of the five sidebar entries were a placeholder.
//!
//! The dashboard itself is still generated from the capability report rather
//! than hand-written per model, which is why the same binary works on a Legion
//! 5 15IRX10 (no fan curve, no power limits) and on a model that has both.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::ApplicationWindow;
use gtk4::{Box as GtkBox, Label, ListBox, Orientation, ScrolledWindow, Stack};

use crate::design::FeatureCard;
use crate::pages::Pages;
use legiontoolkit::backends::{Backend, MockBackend, SysfsBackend};
use legiontoolkit::capability::Group;

/// Every dashboard group, in display order.
const GROUPS: [Group; 5] = [
    Group::Power,
    Group::Thermal,
    Group::Lighting,
    Group::Battery,
    Group::System,
];

/// Sidebar entries, in the order the reference uses. The dashboard is always
/// first; the rest are one per capability group, plus About.
const NAV: [(&str, &str, &str); 7] = [
    ("go-home-symbolic", "Dashboard", "dashboard"),
    ("power-profile-performance-symbolic", "Power", "power"),
    ("fan-symbolic", "Thermal", "thermal"),
    ("keyboard-brightness-symbolic", "Lighting", "lighting"),
    ("battery-symbolic", "Battery", "battery"),
    ("emblem-system-symbolic", "System", "system"),
    ("help-about-symbolic", "About", "about"),
];

/// Which backend to use. The sysfs backend works on real hardware; the mock
/// backend is for developing and screenshotting without it.
fn select_backend() -> Box<dyn Backend> {
    if std::env::var_os("LEGIONTOOLKIT_MOCK").is_some() {
        tracing::info!("using mock backend (LEGIONTOOLKIT_MOCK is set)");
        return Box::new(MockBackend);
    }
    let backend = SysfsBackend::new();
    if backend.name().contains("driver not loaded") {
        tracing::warn!(
            "legion_laptop is not loaded; only ideapad-backed controls will be available"
        );
    }
    Box::new(backend)
}

/// A dismissible banner. `AdwBanner` carries no body text, so the actionable
/// part goes in the title and the button only silences it.
fn warning_banner(title: &str) -> adw::Banner {
    let banner = adw::Banner::builder()
        .title(title)
        .button_label("Dismiss")
        .revealed(true)
        .build();
    banner.connect_button_clicked(|b| b.set_revealed(false));
    banner
}

pub struct MainWindow {
    window: ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        let backend = select_backend();

        let warnings = legiontoolkit::services::system_warnings(
            backend.as_ref(),
            legiontoolkit::services::privilege::is_escalation_available(),
        );
        for warning in &warnings {
            tracing::warn!("{warning}");
        }

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Legion Toolkit")
            .default_width(1250)
            .default_height(780)
            .build();

        let dashboard_cards: Rc<RefCell<Vec<FeatureCard>>> = Rc::new(RefCell::new(Vec::new()));
        let mut pages = Pages::new();

        // ------------------------------------------------------------------
        // Pages
        // ------------------------------------------------------------------
        let stack = Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

        // The dashboard is generated from the capability report. Capabilities
        // are consumed as they are placed so we never need to clone them.
        {
            let scroll = ScrolledWindow::builder()
                .hscrollbar_policy(gtk4::PolicyType::Never)
                .build();

            let page = GtkBox::new(Orientation::Vertical, 24);
            page.set_margin_top(12);
            page.set_margin_bottom(12);
            page.set_margin_start(12);
            page.set_margin_end(12);

            let mut capabilities = backend.capabilities();
            let mut empty = true;

            for group in GROUPS {
                let mut i = 0;
                let mut any = false;
                while i < capabilities.len() {
                    if capabilities[i].group != group {
                        i += 1;
                        continue;
                    }
                    let cap = capabilities.remove(i);
                    if !any {
                        let title = Label::new(Some(cap.group.title()));
                        title.add_css_class("llt-group-title");
                        title.set_halign(gtk4::Align::Start);
                        page.append(&title);
                        any = true;
                    }
                    let card = FeatureCard::new(cap);
                    page.append(card.gtk_widget());
                    card.connect_controls(page.clone().upcast::<gtk4::Widget>());
                    dashboard_cards.borrow_mut().push(card);
                }
                empty &= !any;
            }

            if empty {
                let note = Label::new(Some("No supported hardware was detected on this machine."));
                note.add_css_class("llt-unavailable");
                note.set_wrap(true);
                page.append(&note);
            }

            scroll.set_child(Some(&page));
            stack.add_titled(&scroll, Some("dashboard"), "Dashboard");
        }

        // Capability-driven pages, built once and refreshed in place.
        pages.rebuild(backend.as_ref());
        for (id, widget, title) in pages.stack_pages() {
            stack.add_titled(widget, Some(id), title);
        }

        // ------------------------------------------------------------------
        // Sidebar
        // ------------------------------------------------------------------
        let sidebar_list = ListBox::new();
        sidebar_list.add_css_class("navigation-sidebar");
        sidebar_list.set_selection_mode(gtk4::SelectionMode::Single);

        for (icon, title, _page_id) in NAV {
            let row = adw::ActionRow::builder()
                .title(title)
                .activatable(true)
                .build();
            row.add_prefix(&gtk4::Image::from_icon_name(icon));
            row.set_widget_name(_page_id);
            sidebar_list.append(&row);
        }

        if let Some(first) = sidebar_list.row_at_index(0) {
            sidebar_list.select_row(Some(&first));
        }

        sidebar_list.connect_row_activated({
            let stack = stack.clone();
            move |_, row| {
                let name = row.widget_name();
                stack.set_visible_child_name(name.as_str());
            }
        });

        let sidebar_page = adw::NavigationPage::builder()
            .title("Legion Toolkit")
            .build();
        let sidebar_box = GtkBox::new(Orientation::Vertical, 0);
        sidebar_box.append(
            &ScrolledWindow::builder()
                .hscrollbar_policy(gtk4::PolicyType::Never)
                .vexpand(true)
                .child(&sidebar_list)
                .build(),
        );
        sidebar_page.set_child(Some(&sidebar_box));

        // ------------------------------------------------------------------
        // Warnings, above the whole page stack
        // ------------------------------------------------------------------
        let content_box = GtkBox::new(Orientation::Vertical, 0);
        for warning in &warnings {
            content_box.append(&warning_banner(warning));
        }
        content_box.append(&stack);

        let content_page = adw::NavigationPage::builder()
            .title("Dashboard")
            .child(&content_box)
            .build();

        // Keep the header bar's title in step with the section on screen.
        // Wired after the page exists so the closure can borrow it.
        let page_for_title = content_page.clone();
        stack.connect_visible_child_notify(move |stack| {
            let name = stack.visible_child_name().unwrap_or_default();
            if name.is_empty() {
                return;
            }
            let title = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &name[c.len_utf8()..])
                .unwrap_or(name.to_string());
            page_for_title.set_title(&title);
        });

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .min_sidebar_width(200.0)
            .build();

        // ------------------------------------------------------------------
        // Header bar
        // ------------------------------------------------------------------
        let header = adw::HeaderBar::new();

        let theme_toggle = gtk4::Button::from_icon_name("weather-clear-night-symbolic");
        theme_toggle.set_tooltip_text(Some("Toggle light / dark"));
        theme_toggle.connect_clicked(|_| crate::design::cycle_color_scheme());
        header.pack_end(&theme_toggle);

        let cards_for_refresh = dashboard_cards.clone();
        let refresh = gtk4::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh now"));
        refresh.connect_clicked(move |_| {
            for card in cards_for_refresh.borrow().iter() {
                card.refresh();
            }
        });
        header.pack_end(&refresh);

        let toolbar = adw::ToolbarView::builder().content(&split).build();
        toolbar.add_top_bar(&header);
        window.set_content(Some(&toolbar));

        // Warm the values before the first paint.
        for card in dashboard_cards.borrow().iter() {
            card.refresh();
        }

        // ------------------------------------------------------------------
        // Refresh loop
        // ------------------------------------------------------------------
        // One second is the cadence the reference's battery and sensor pages
        // use. Readings are cheap sysfs reads, so this stays on the main thread
        // rather than paying for a worker and the channel that would come with
        // it.
        glib::timeout_add_seconds_local(1, move || {
            for card in dashboard_cards.borrow().iter() {
                card.refresh();
            }
            pages.refresh();
            glib::ControlFlow::Continue
        });

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
