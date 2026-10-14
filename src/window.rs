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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::ApplicationWindow;
use gtk4::{Box as GtkBox, ListBox, Orientation, ScrolledWindow, Stack};

use crate::pages::Pages;
use legiontoolkit::backends::{Backend, MockBackend, SysfsBackend};

/// Main sidebar entries, in the order the reference uses.
const MAIN_NAV: [(&str, &str, &str); 6] = [
    ("go-home-symbolic", "Home", "home"),
    ("power-profile-performance-symbolic", "Power", "power"),
    (
        "org.gnome.Settings-device-diagnostics-symbolic",
        "Processors & GPU",
        "processors",
    ),
    ("keyboard-brightness-symbolic", "Lighting", "lighting"),
    ("battery-symbolic", "Battery", "battery"),
    ("emblem-system-symbolic", "System", "system"),
];

/// Pinned to the bottom of the sidebar, as the reference does.
const BOTTOM_NAV: [(&str, &str, &str); 2] = [
    ("preferences-system-symbolic", "Settings", "settings"),
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

#[derive(Clone)]
pub struct MainWindow {
    pub(crate) window: ApplicationWindow,
    /// The page stack, so `show_page` can select a section without walking the
    /// widget tree.
    stack: Stack,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        let backend = select_backend();

        let warnings = legiontoolkit::services::system_warnings(
            backend.as_ref(),
            legiontoolkit::services::privilege::is_escalation_available(),
            &legiontoolkit::services::privilege::diagnose(),
        );
        for warning in &warnings {
            tracing::warn!("{warning}");
        }

        let current = legiontoolkit::settings::current();
        let (width, height) = current.restored_size().unwrap_or((1250, 780));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Legion Toolkit")
            .default_width(width)
            .default_height(height)
            .build();

        // Persist the size on close. GTK 4 cannot read a window's position, so
        // only the size is kept — see `Settings::restored_size`.
        //
        // `force_quit` lets the menu's Quit close the window even when
        // minimize-on-close would otherwise hide it.
        let force_quit = Rc::new(Cell::new(false));
        window.connect_close_request({
            let force_quit = Rc::clone(&force_quit);
            move |window| {
                let (width, height) = (window.width(), window.height());
                if width > 200 && height > 200 {
                    let _ = legiontoolkit::settings::update(|s| {
                        s.window_width = Some(width);
                        s.window_height = Some(height);
                    });
                }
                // Keep running in the background: hide rather than destroy, so
                // relaunching the application activates this instance and shows
                // the window again.
                if legiontoolkit::settings::current().minimize_on_close && !force_quit.get() {
                    window.set_visible(false);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });

        // Shared: the toolbar's refresh button, the refresh loop and the
        // struct all tick the same page set. `Pages` is not `Copy`, so they
        // borrow one handle rather than taking ownership.
        let pages = Rc::new(RefCell::new(Pages::new()));

        // ------------------------------------------------------------------
        // Pages
        // ------------------------------------------------------------------
        let stack = Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);

        // Capability-driven pages, built once and refreshed in place.
        pages.borrow_mut().rebuild(backend.as_ref());
        for (id, widget, title) in pages.borrow().stack_pages() {
            stack.add_titled(widget, Some(id), title);
        }
        // ------------------------------------------------------------------
        // Sidebar — main entries, then Settings and About pinned at the bottom
        // ------------------------------------------------------------------
        // A navigation row: an icon and a title, tagged with its page id.
        fn nav_row(icon: &str, title: &str, page_id: &str) -> adw::ActionRow {
            let row = adw::ActionRow::builder()
                .title(title)
                // Adwaita interprets a title as Pango markup unless told
                // otherwise, and "Processors & GPU" is not valid markup — the
                // row rendered with an icon and *no text at all*.
                .use_markup(false)
                .activatable(true)
                .build();
            row.add_prefix(&gtk4::Image::from_icon_name(icon));
            row.set_widget_name(page_id);
            row
        }

        let main_list = ListBox::new();
        main_list.add_css_class("navigation-sidebar");
        main_list.set_selection_mode(gtk4::SelectionMode::Single);
        for (icon, title, page_id) in MAIN_NAV {
            main_list.append(&nav_row(icon, title, page_id));
        }

        let bottom_list = ListBox::new();
        bottom_list.add_css_class("navigation-sidebar");
        bottom_list.set_selection_mode(gtk4::SelectionMode::Single);
        for (icon, title, page_id) in BOTTOM_NAV {
            bottom_list.append(&nav_row(icon, title, page_id));
        }

        // Selected rows are per-list, so activating a row in one list clears the
        // other; otherwise two rows would read as selected at once.
        main_list.connect_row_activated({
            let stack = stack.clone();
            let bottom = bottom_list.clone();
            move |_, row| {
                stack.set_visible_child_name(row.widget_name().as_str());
                bottom.unselect_all();
            }
        });
        bottom_list.connect_row_activated({
            let stack = stack.clone();
            let main = main_list.clone();
            move |_, row| {
                stack.set_visible_child_name(row.widget_name().as_str());
                main.unselect_all();
            }
        });

        if let Some(first) = main_list.row_at_index(0) {
            main_list.select_row(Some(&first));
        }

        let sidebar_page = adw::NavigationPage::builder()
            .title("Legion Toolkit")
            .build();
        let sidebar_box = GtkBox::new(Orientation::Vertical, 0);
        sidebar_box.append(
            &ScrolledWindow::builder()
                .hscrollbar_policy(gtk4::PolicyType::Never)
                .vexpand(true)
                .child(&main_list)
                .build(),
        );
        sidebar_box.append(&gtk4::Separator::new(Orientation::Horizontal));

        // The bottom entries are outside the scroll view, so they stay visible
        // at the foot of the panel however long the main list gets.
        let bottom_holder = GtkBox::new(Orientation::Vertical, 0);
        bottom_holder.set_margin_top(6);
        bottom_holder.set_margin_bottom(6);
        bottom_holder.append(&bottom_list);
        sidebar_box.append(&bottom_holder);

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

        let refresh = gtk4::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh now"));
        refresh.connect_clicked({
            let pages = Rc::clone(&pages);
            move |_| pages.borrow().refresh()
        });
        header.pack_end(&refresh);

        // A menu button. Its only entry today is Quit, which matters because
        // minimize-on-close otherwise leaves no way to end the process from the
        // window.
        let menu = gtk4::gio::Menu::new();
        menu.append(Some("Quit"), Some("win.quit"));
        let menu_button = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Menu")
            .menu_model(&menu)
            .build();

        let quit = gtk4::gio::SimpleAction::new("quit", None);
        quit.connect_activate({
            let window = window.clone();
            let force_quit = Rc::clone(&force_quit);
            move |_, _| {
                force_quit.set(true);
                window.close();
            }
        });
        window.add_action(&quit);
        header.pack_end(&menu_button);

        let toolbar = adw::ToolbarView::builder().content(&split).build();
        toolbar.add_top_bar(&header);
        window.set_content(Some(&toolbar));

        // Warm the values before the first paint. Without this a page shows
        // dashes until the one-second refresh loop first fires, which reads as
        // a broken page rather than a pending one.
        pages.borrow().refresh();

        // ------------------------------------------------------------------
        // Refresh loop
        // ------------------------------------------------------------------
        // One second is the cadence the reference's battery and sensor pages
        // use. Readings are cheap sysfs reads, so this stays on the main thread
        // rather than paying for a worker and the channel that would come with
        // it.
        // The tick is a fixed one second so that a change to the configured
        // interval takes effect immediately, without rescheduling the timer.
        // Readings are skipped until the interval has actually elapsed.
        let pages_for_loop = Rc::clone(&pages);
        let seconds_since_refresh = std::cell::Cell::new(0u32);
        glib::timeout_add_seconds_local(1, move || {
            seconds_since_refresh.set(seconds_since_refresh.get() + 1);
            let interval = pages_for_loop.borrow().refresh_seconds().max(1);
            if seconds_since_refresh.get() < interval {
                return glib::ControlFlow::Continue;
            }
            seconds_since_refresh.set(0);

            pages_for_loop.borrow().refresh();
            glib::ControlFlow::Continue
        });

        Self { window, stack }
    }

    pub fn present(&self) {
        self.window.present();
    }

    /// Log the visible page's widget tree, for checking content a capture
    /// cannot reach.
    pub fn log_page(&self) {
        crate::shot::log_visible_page(&self.stack);
    }

    /// Select a page by name. Used by `--shot` to capture a specific section.
    ///
    /// The transition is disabled for the switch. A stack crossfades between
    /// children, and an offscreen capture taken mid-transition paints *both*
    /// pages, which looks exactly like a layout bug: two sets of cards
    /// interleaved. Capturing the settled state needs no animation.
    pub fn show_page(&self, page: &str) {
        let previous = self.stack.transition_type();
        self.stack
            .set_transition_type(gtk4::StackTransitionType::None);
        self.stack.set_visible_child_name(page);
        self.stack.set_transition_type(previous);
    }
}
