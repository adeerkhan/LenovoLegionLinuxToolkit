//! Pages — one per sidebar entry, each built from live data.
//!
//! The reference application's pages are the thing we were missing: the shell
//! and dashboard were in place, but four of five sidebar entries were a
//! placeholder sentence. This module fills them.
//!
//! Two patterns are lifted directly from the reference:
//!
//! - **Sensor cards** (`SensorsControlV2`): a grid of cards with semibold
//!   titles and rows of label + value + a 40×6 accent meter, each meter's
//!   ceiling taken from the sensor's range rather than its current reading.
//! - **Battery page**: a hero icon and large percentage, then a stack of stat
//!   cards. We surface what sysfs actually reports instead of inventing
//!   placeholders for the rest.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, FlowBox, Label, Orientation, ScrolledWindow};

use legiontoolkit::backends::{driver_status, Backend};
use legiontoolkit::capability::{Capability, Value};
use legiontoolkit::paths;

use crate::design::{FeatureCard, SensorCard};

/// A scrolled page whose contents are rebuilt on refresh.
pub struct Page {
    root: ScrolledWindow,
    contents: GtkBox,
    build: Box<dyn Fn(&mut Page)>,
}

impl Page {
    fn new(build: impl Fn(&mut Page) + 'static) -> Self {
        let contents = GtkBox::new(Orientation::Vertical, 8);
        contents.set_margin_top(12);
        contents.set_margin_bottom(12);
        contents.set_margin_start(12);
        contents.set_margin_end(12);

        let root = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .child(&contents)
            .build();

        Self {
            root,
            contents,
            build: Box::new(build),
        }
    }

    /// Re-run the page's constructor, replacing its contents.
    ///
    /// The closure is moved out for the duration of the call, because
    /// `build(self)` would otherwise borrow `self` immutably while
    /// `&mut self` holds it.
    fn render(&mut self) {
        self.clear();
        let build = std::mem::replace(&mut self.build, Box::new(|_| {}));
        build(self);
        self.build = build;
    }

    fn clear(&mut self) {
        while let Some(child) = self.contents.first_child() {
            self.contents.remove(&child);
        }
    }

    fn append(&mut self, widget: &impl gtk4::prelude::IsA<gtk4::Widget>) {
        self.contents.append(widget);
    }

    /// A large section heading, matching the reference's 24px group titles.
    fn heading(&mut self, text: &str) {
        let label = Label::new(Some(text));
        label.add_css_class("llt-group-title");
        label.set_halign(Align::Start);
        self.append(&label);
    }

    fn widget(&self) -> &ScrolledWindow {
        &self.root
    }
}

/// A page whose contents come from a set of capabilities, refreshed on a timer.
pub struct CapabilityPage {
    page: Page,
    cards: Rc<RefCell<Vec<FeatureCard>>>,
}

impl CapabilityPage {
    fn new(build: impl Fn(&mut Page) + 'static) -> Self {
        Self {
            page: Page::new(build),
            cards: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Re-render from the backend. Called on every refresh tick.
    fn rebuild(&mut self, backend: &dyn Backend, filter: impl Fn(&Capability) -> bool) {
        self.page.clear();
        self.page.render();

        self.cards.borrow_mut().clear();
        let capabilities: Vec<Capability> = backend
            .capabilities()
            .into_iter()
            .filter(|c| filter(c))
            .collect();

        for cap in capabilities {
            if matches!(cap.kind, legiontoolkit::capability::Kind::ReadOnly) && !cap.is_available()
            {
                // A read-only sensor that is unavailable is already explained
                // on the dashboard; repeating it here adds noise.
                continue;
            }
            let card = FeatureCard::new(cap);
            self.page.append(card.gtk_widget());
            card.connect_controls(self.page.contents.clone().upcast::<gtk4::Widget>());
            self.cards.borrow_mut().push(card);
        }

        // Warm the values immediately so the page is not blank for a second.
        for card in self.cards.borrow().iter() {
            card.refresh();
        }
    }

    fn refresh(&self) {
        for card in self.cards.borrow().iter() {
            card.refresh();
        }
    }

    fn widget(&self) -> &ScrolledWindow {
        self.page.widget()
    }
}

/// All pages, so the refresh loop can drive them from one place.
pub struct Pages {
    pub thermal: Page,
    pub battery: BatteryPage,
    pub power: CapabilityPage,
    pub lighting: CapabilityPage,
    pub system: CapabilityPage,
    pub about: Page,
    /// The capability set, held once and shared by every page and the thermal
    /// grid. `Capability` is deliberately not cloneable — it owns its read and
    /// write closures — so pages share it through this rather than copying it.
    caps: Rc<RefCell<Vec<Capability>>>,
    /// The sensor cards on the thermal page, refreshed directly because their
    /// values come from sources the capability model does not describe one row
    /// per file (coretemp and nvme).
    thermal_cards: Rc<RefCell<Option<Rc<ThermalCards>>>>,
}

impl Pages {
    pub fn new() -> Self {
        Self {
            thermal: thermal_page(&Rc::new(RefCell::new(None))),
            battery: BatteryPage::new(),
            power: CapabilityPage::new(|page| {
                page.heading("Power");
            }),
            lighting: CapabilityPage::new(|page| {
                page.heading("Lighting");
            }),
            system: CapabilityPage::new(|page| {
                page.heading("System");
            }),
            about: about_page(),
            caps: Rc::new(RefCell::new(Vec::new())),
            thermal_cards: Rc::new(RefCell::new(None)),
        }
    }

    /// Rebuild every page from the backend.
    pub fn rebuild(&mut self, backend: &dyn Backend) {
        *self.caps.borrow_mut() = backend.capabilities();
        self.thermal = thermal_page(&self.thermal_cards);
        self.battery.rebuild(backend);

        self.power.rebuild(backend, |c| {
            c.group == legiontoolkit::capability::Group::Power
        });
        self.lighting.rebuild(backend, |c| {
            c.group == legiontoolkit::capability::Group::Lighting
        });
        self.system.rebuild(backend, |c| {
            c.group == legiontoolkit::capability::Group::System
        });

        self.about = about_page_with(backend);
    }

    /// Tick every live value.
    pub fn refresh(&self) {
        self.battery.refresh();
        self.power.refresh();
        self.lighting.refresh();
        self.system.refresh();
        refresh_thermal(&self.caps, &self.thermal_cards);
    }

    pub fn stack_pages(&self) -> Vec<(&'static str, &ScrolledWindow, &'static str)> {
        vec![
            ("dashboard", self.thermal.widget(), "Thermal"),
            ("power", self.power.widget(), "Power"),
            ("lighting", self.lighting.widget(), "Lighting"),
            ("battery", self.battery.widget(), "Battery"),
            ("system", self.system.widget(), "System"),
            ("about", self.about.widget(), "About"),
        ]
    }
}

impl Default for Pages {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Thermal — the sensor grid
// ---------------------------------------------------------------------------

/// The live sensor cards, refreshed in place.
struct ThermalCards {
    cpu: Rc<SensorCard>,
    cpu_rows: Vec<(usize, &'static str)>,
    gpu: Rc<SensorCard>,
    gpu_rows: Vec<(usize, &'static str)>,
    fans: Rc<SensorCard>,
    fan_rows: Vec<(usize, &'static str)>,
    disk: Rc<SensorCard>,
    disk_rows: Vec<(usize, &'static str)>,
}

/// Build the thermal page.
///
/// Every row starts blank and is filled from whichever source actually has the
/// reading, exactly as the reference's sensor controller hides cards for
/// sensors a machine does not have.
fn thermal_page(out: &Rc<RefCell<Option<Rc<ThermalCards>>>>) -> Page {
    let out = Rc::clone(out);
    Page::new(move |page| {
        page.heading("Thermal");

        let grid = FlowBox::new();
        grid.add_css_class("llt-sensor-grid");
        grid.set_selection_mode(gtk4::SelectionMode::None);
        grid.set_min_children_per_line(1);
        grid.set_max_children_per_line(4);
        grid.set_homogeneous(true);
        grid.set_valign(gtk4::Align::Start);

        let mut cpu = SensorCard::new("CPU");
        let cpu_rows = vec![(
            cpu.add_row("Temperature", "\u{00b0}C", 0, 110.0),
            "cpu_temp",
        )];

        let mut gpu = SensorCard::new("GPU");
        let gpu_rows = vec![(
            gpu.add_row("Temperature", "\u{00b0}C", 0, 110.0),
            "gpu_temp",
        )];

        let mut fans = SensorCard::new("Fans");
        let mut fan_rows = Vec::new();
        for (i, (key, label)) in [
            ("fan1_rpm", "CPU Fan"),
            ("fan2_rpm", "GPU Fan"),
            ("fan3_rpm", "Auxiliary Fan"),
            ("fan4_rpm", "Fourth Fan"),
        ]
        .into_iter()
        .enumerate()
        {
            fan_rows.push((fans.add_row(label, "rpm", i, 6000.0), key));
        }

        let mut disk = SensorCard::new("Disk");
        let disk_rows = vec![(
            disk.add_row("Temperature", "\u{00b0}C", 0, 90.0),
            "disk_temp",
        )];

        let cards = Rc::new(ThermalCards {
            cpu: Rc::new(cpu),
            cpu_rows,
            gpu: Rc::new(gpu),
            gpu_rows,
            fans: Rc::new(fans),
            fan_rows,
            disk: Rc::new(disk),
            disk_rows,
        });

        for card in [&cards.cpu, &cards.gpu, &cards.fans, &cards.disk] {
            grid.insert(card.widget().upcast_ref::<gtk4::Widget>(), -1);
        }

        page.append(&grid);
        *out.borrow_mut() = Some(Rc::clone(&cards));
    })
}

/// Re-read every sensor row from the capability set.
fn refresh_thermal(
    caps: &Rc<RefCell<Vec<Capability>>>,
    thermal: &Rc<RefCell<Option<Rc<ThermalCards>>>>,
) {
    // Clone the Rc out of the cell so the borrow is not held while the cards
    // are read back.
    let borrowed = thermal.borrow();
    let Some(cards) = borrowed.as_ref().map(Rc::clone) else {
        return;
    };
    drop(borrowed);
    let caps = caps.borrow();

    let value = |key: &str| {
        caps.iter()
            .find(|c| c.key == key)
            .and_then(|c| c.read().ok())
            .and_then(|v| match v {
                Value::Float(f) => Some(f),
                Value::Int(i) => Some(i as f64),
                _ => None,
            })
    };

    for (row, key) in cards.cpu_rows.iter() {
        cards.cpu.set_value(*row, value(key));
    }
    for (row, key) in cards.gpu_rows.iter() {
        cards.gpu.set_value(*row, value(key));
    }
    for (row, key) in cards.fan_rows.iter() {
        cards.fans.set_value(*row, value(key));
    }
    for (row, key) in cards.disk_rows.iter() {
        cards.disk.set_value(*row, value(key));
    }
}

// ---------------------------------------------------------------------------
// Battery — hero plus stat cards
// ---------------------------------------------------------------------------

pub struct BatteryPage {
    page: Page,
    hero_percent: Label,
    hero_status: Label,
    hero_icon: gtk4::Image,
    rows: Rc<RefCell<Vec<(Label, String)>>>,
    backend: Rc<RefCell<Vec<Capability>>>,
}

impl BatteryPage {
    fn new() -> Self {
        Self {
            page: Page::new(|page| {
                page.heading("Battery");
            }),
            hero_percent: Label::new(None),
            hero_status: Label::new(None),
            hero_icon: gtk4::Image::new(),
            rows: Rc::new(RefCell::new(Vec::new())),
            backend: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn rebuild(&mut self, backend: &dyn Backend) {
        self.page.clear();
        self.backend.borrow_mut().clear();
        self.backend.borrow_mut().extend(backend.capabilities());

        // --- Hero ---------------------------------------------------------
        let hero = GtkBox::new(Orientation::Horizontal, 24);
        hero.set_halign(Align::Center);
        hero.set_margin_top(8);
        hero.set_margin_bottom(24);
        hero.add_css_class("llt-card");

        self.hero_icon
            .set_icon_name(Some("battery-level-100-symbolic"));
        self.hero_icon.set_pixel_size(96);
        hero.append(&self.hero_icon);

        let text = GtkBox::new(Orientation::Vertical, 4);
        text.set_valign(Align::Center);
        self.hero_percent.add_css_class("llt-hero-percent");
        self.hero_percent.set_label("—");
        text.append(&self.hero_percent);
        self.hero_status.add_css_class("llt-hero-status");
        self.hero_status.set_label("—");
        text.append(&self.hero_status);
        hero.append(&text);

        self.page.append(&hero);

        // --- Stat cards ---------------------------------------------------
        self.rows.borrow_mut().clear();
        for key in [
            "battery_capacity",
            "battery_state",
            "battery_health",
            "battery_charge_now",
            "battery_power",
            "battery_voltage",
            "battery_cycles",
        ] {
            // Bind the borrow: a temporary would be dropped while `cap` is
            // still alive.
            let backend = self.backend.borrow();
            let Some(cap) = backend.iter().find(|c| c.key == key) else {
                continue;
            };
            let value = Label::new(None);
            value.add_css_class("llt-value");
            value.set_halign(Align::End);

            let card = stat_card(&cap.label, &value);
            self.page.append(&card);
            self.rows.borrow_mut().push((value, key.to_string()));
        }

        self.refresh();
    }

    fn refresh(&self) {
        let lookup = |key: &str| {
            self.backend
                .borrow()
                .iter()
                .find(|c| c.key == key)
                .and_then(|c| c.read().ok())
        };

        // Hero percentage drives the stepped battery icon, matching the
        // reference's Battery024 … Battery1024 ladder.
        if let Some(Value::Float(percent)) = lookup("battery_capacity") {
            let bucket = ((percent / 10.0).round() as i64).clamp(0, 10) * 10;
            self.hero_icon
                .set_icon_name(Some(&format!("battery-level-{bucket}-symbolic")));
            self.hero_percent.set_label(&format!("{percent:.0}%"));
        }

        if let Some(Value::Text(state)) = lookup("battery_state") {
            self.hero_status.set_label(&state);
        }

        for (label, key) in self.rows.borrow().iter() {
            let backend = self.backend.borrow();
            let unit = backend
                .iter()
                .find(|c| c.key == key)
                .and_then(|c| c.unit)
                .unwrap_or("");
            label.set_label(&match lookup(key) {
                Some(Value::Float(f)) if !unit.is_empty() => format!("{f:.1} {unit}"),
                Some(Value::Float(f)) => format!("{f:.1}"),
                Some(Value::Int(i)) if !unit.is_empty() => format!("{i} {unit}"),
                Some(Value::Int(i)) => format!("{i}"),
                Some(Value::Text(t)) => t,
                _ => "—".to_string(),
            });
        }
    }

    fn widget(&self) -> &ScrolledWindow {
        self.page.widget()
    }
}

/// A one-line stat card, matching the reference's `CardControl` +
/// `CardHeaderControl` plus a 14px value.
fn stat_card(title: &str, value: &Label) -> GtkBox {
    let card = GtkBox::new(Orientation::Horizontal, 12);
    card.add_css_class("llt-card");
    card.set_margin_bottom(8);

    let label = Label::new(Some(title));
    label.add_css_class("llt-card-title");
    label.set_halign(Align::Start);
    label.set_hexpand(true);
    card.append(&label);

    card.append(value);
    card
}

// ---------------------------------------------------------------------------
// About
// ---------------------------------------------------------------------------

fn about_page() -> Page {
    about_page_impl(None)
}

fn about_page_with(backend: &dyn Backend) -> Page {
    about_page_impl(Some((
        backend.name().to_string(),
        driver_status(),
        paths::machine_summary(),
    )))
}

fn about_page_impl(info: Option<(String, String, String)>) -> Page {
    Page::new(move |page| {
        page.heading("About");

        let (backend_name, driver, machine) = info
            .clone()
            .unwrap_or_else(|| ("—".into(), driver_status(), paths::machine_summary()));

        for (label, value) in [
            ("Application", "Legion Toolkit"),
            ("Machine", machine.as_str()),
            ("Driver", driver.as_str()),
            ("Backend", backend_name.as_str()),
        ] {
            let row = GtkBox::new(Orientation::Horizontal, 12);
            row.add_css_class("llt-card");
            row.set_margin_bottom(8);
            let key = Label::new(Some(label));
            key.add_css_class("llt-card-title");
            key.set_halign(Align::Start);
            key.set_hexpand(true);
            let val = Label::new(Some(value));
            val.add_css_class("llt-card-subtitle");
            val.set_wrap(true);
            row.append(&key);
            row.append(&val);
            page.append(&row);
        }

        let note = Label::new(Some(
            "Built on GTK 4 and libadwaita. Hardware access via LenovoLegionLinux and the Linux kernel.",
        ));
        note.add_css_class("llt-card-note");
        note.set_wrap(true);
        note.set_margin_top(16);
        page.append(&note);
    })
}
