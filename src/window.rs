//! The main window: sidebar navigation plus capability-driven pages.
//!
//! The dashboard is not written per model. It is generated from the backend's
//! capability report and grouped, which is why the same window works on a
//! Legion 5 15IRX10 (no fan curve, no power limits) and a model that has both.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::ApplicationWindow;
use gtk4::{Box as GtkBox, Label, ListBox, Orientation, ScrolledWindow, Stack};

use crate::design::FeatureCard;
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

/// Sidebar entries: icon, title, stack page id.
const PAGES: [(&str, &str, &str); 5] = [
    ("go-home-symbolic", "Dashboard", "dashboard"),
    ("power-profile-performance-symbolic", "Power", "power"),
    ("keyboard-brightness-symbolic", "Lighting", "lighting"),
    ("battery-symbolic", "Battery", "battery"),
    ("emblem-system-symbolic", "System", "system"),
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

/// Build a one-line placeholder page.
fn placeholder_page(title: &str, body: &str) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 12);
    page.set_valign(gtk4::Align::Center);
    page.set_halign(gtk4::Align::Center);

    let heading = Label::new(Some(title));
    heading.add_css_class("llt-group-title");
    let detail = Label::new(Some(body));
    detail.add_css_class("llt-card-subtitle");

    page.append(&heading);
    page.append(&detail);
    page
}

pub struct MainWindow {
    window: ApplicationWindow,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        let backend = select_backend();
        let mut capabilities = backend.capabilities();

        if let Some(warning) = backend.global_warning() {
            tracing::warn!("{warning}");
        }

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Legion Toolkit")
            .default_width(1250)
            .default_height(780)
            .build();

        let cards: Rc<RefCell<Vec<FeatureCard>>> = Rc::new(RefCell::new(Vec::new()));

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
                    cards.borrow_mut().push(card);
                }
            }

            scroll.set_child(Some(&page));
            stack.add_titled(&scroll, Some("dashboard"), "Dashboard");
        }

        stack.add_titled(
            &placeholder_page("Power", "Power modes, power limits and fan control."),
            Some("power"),
            "Power",
        );
        stack.add_titled(
            &placeholder_page("Lighting", "Keyboard backlight and logo lighting."),
            Some("lighting"),
            "Lighting",
        );
        stack.add_titled(
            &placeholder_page("Battery", "Charge thresholds and battery health."),
            Some("battery"),
            "Battery",
        );
        stack.add_titled(
            &placeholder_page("System", "Input toggles, boot logo and about."),
            Some("system"),
            "System",
        );

        // ------------------------------------------------------------------
        // Sidebar
        // ------------------------------------------------------------------
        let sidebar_list = ListBox::new();
        sidebar_list.add_css_class("navigation-sidebar");
        sidebar_list.set_selection_mode(gtk4::SelectionMode::Single);

        for (icon, title, page_id) in PAGES {
            let row = adw::ActionRow::builder()
                .title(title)
                .activatable(true)
                .build();
            row.add_prefix(&gtk4::Image::from_icon_name(icon));
            row.set_widget_name(page_id);
            sidebar_list.append(&row);
        }

        // Select the dashboard row so the first page is visible immediately.
        if let Some(first) = sidebar_list.row_at_index(0) {
            sidebar_list.select_row(Some(&first));
        }

        sidebar_list.connect_row_activated({
            let stack = stack.clone();
            move |_, row| {
                // `widget_name` hands back an owned `GString`; bind it so the
                // borrow outlives the call.
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

        let content_page = adw::NavigationPage::builder()
            .title("Dashboard")
            .child(&stack)
            .build();

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

        let cards_for_refresh = cards.clone();
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
        for card in cards.borrow().iter() {
            card.refresh();
        }

        // ------------------------------------------------------------------
        // Refresh loop
        // ------------------------------------------------------------------
        // One second is the cadence the reference's battery page uses. Readings
        // are cheap sysfs reads, so this stays on the main thread rather than
        // paying for a worker and the channel that would come with it.
        glib::timeout_add_seconds_local(1, move || {
            for card in cards.borrow().iter() {
                card.refresh();
            }
            glib::ControlFlow::Continue
        });

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}
