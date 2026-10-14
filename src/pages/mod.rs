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
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, FlowBox, Label, Orientation, ScrolledWindow};

use legiontoolkit::backends::{driver_status, Backend};
use legiontoolkit::capability::{Capability, Value};
use legiontoolkit::paths;
use legiontoolkit::system;

use crate::design::card::{LltCard, Severity};
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

        // `vexpand` is essential: without it the ScrolledWindow measures to its
        // natural height, which for a tall page is a sliver, and everything
        // below the fold is clipped away rather than scrolled to.
        let root = ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .child(&contents)
            .build();

        let mut page = Self {
            root,
            contents,
            build: Box::new(build),
        };
        // Render immediately. A `Page` that stores a constructor and never runs
        // it looks exactly like an empty page, which is a footgun: the thermal
        // page was silently blank because its builder was never called.
        page.render();
        page
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

    /// A smaller heading, for grouping cards within one page.
    fn subheading(&mut self, text: &str) {
        let label = Label::new(Some(text));
        label.add_css_class("llt-subheading");
        label.set_halign(Align::Start);
        self.append(&label);
    }

    /// A page with no contents, for a slot that will be filled by `rebuild`.
    fn empty() -> Self {
        Self::new(|_| {})
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
/// A section drawn around the capability cards, for content the capability
/// model does not describe — the presets editor is the only one so far.
type ExtraSection = Box<dyn Fn(&mut Page, &Rc<RefCell<Vec<Arc<Capability>>>>) + 'static>;

/// One titled section's predicate: the capabilities it contains.
type SectionFilter = Rc<dyn Fn(&Capability) -> bool>;

/// The landing page's sensor cards, shared between the page builder and the
/// refresh loop. Named because the nested form is unreadable inline.
type ThermalGrid = Rc<RefCell<Option<Rc<ThermalCards>>>>;

pub struct CapabilityPage {
    page: Page,
    cards: Rc<RefCell<Vec<FeatureCard>>>,
    /// The same list `Pages` holds, so an extra section can read and write
    /// capabilities without its own discovery pass.
    caps: Rc<RefCell<Vec<Arc<Capability>>>>,
    prelude: Option<ExtraSection>,
    extra: Option<ExtraSection>,
    /// When set, the page renders these titled sections instead of one flat
    /// list. Each section is a title and a predicate over the capability, so a
    /// section can be a fixed set of keys or a class like "the power limits".
    sections: Option<Vec<(&'static str, SectionFilter)>>,
}

impl CapabilityPage {
    fn new(build: impl Fn(&mut Page) + 'static, caps: Rc<RefCell<Vec<Arc<Capability>>>>) -> Self {
        Self {
            page: Page::new(build),
            cards: Rc::new(RefCell::new(Vec::new())),
            caps,
            prelude: None,
            extra: None,
            sections: None,
        }
    }

    /// Render the page as titled sections rather than one flat list.
    fn with_sections(mut self, sections: Vec<(&'static str, SectionFilter)>) -> Self {
        self.sections = Some(sections);
        self
    }

    /// Add a section drawn *before* the capability cards.
    ///
    /// The landing page needs this: the reference puts its sensor grid above
    /// the controls, not below them.
    fn with_prelude(
        mut self,
        prelude: impl Fn(&mut Page, &Rc<RefCell<Vec<Arc<Capability>>>>) + 'static,
    ) -> Self {
        self.prelude = Some(Box::new(prelude));
        self
    }

    /// Add a section drawn after the capability cards.
    fn with_extra(
        mut self,
        extra: impl Fn(&mut Page, &Rc<RefCell<Vec<Arc<Capability>>>>) + 'static,
    ) -> Self {
        self.extra = Some(Box::new(extra));
        self
    }

    /// Re-render from the backend. Called on every refresh tick.
    /// Re-render from the shared capability list.
    ///
    /// The list is shared rather than re-derived so that a section drawn beside
    /// the cards — the presets editor — can read and write the same
    /// capabilities the cards control.
    fn rebuild(&mut self, filter: impl Fn(&Capability) -> bool) {
        // `render` clears as well, so this is one pass, not two.
        self.page.render();

        if let Some(prelude) = &self.prelude {
            prelude(&mut self.page, &self.caps);
        }

        self.cards.borrow_mut().clear();

        // A page shows the controls this machine actually has. The reference
        // hides unsupported controls entirely, and the reason is now obvious:
        // with thirteen permanently-unavailable capabilities, listing them all
        // buries the ones that work. What is missing is stated once, at the
        // bottom, and enumerated in full on the About page.
        let mut unavailable: Vec<Arc<Capability>> = Vec::new();

        if let Some(sections) = self.sections.clone() {
            for (title, predicate) in sections {
                let grid = self.section_grid();
                let mut shown = 0i32;
                let caps: Vec<Arc<Capability>> = self
                    .caps
                    .borrow()
                    .iter()
                    .filter(|c| predicate(c))
                    .cloned()
                    .collect();
                for cap in caps {
                    if !cap.is_available() {
                        unavailable.push(cap);
                        continue;
                    }
                    let card = FeatureCard::new(Arc::clone(&cap));
                    card.gtk_widget().set_hexpand(true);
                    grid.attach(card.gtk_widget(), shown % 2, shown / 2, 1, 1);
                    card.connect_controls(self.page.contents.clone().upcast::<gtk4::Widget>());
                    self.cards.borrow_mut().push(card);
                    shown += 1;
                }
                // Only title a section that has something in it.
                if shown > 0 {
                    self.page.heading(title);
                    self.page.append(&grid);
                }
            }
        } else {
            let capabilities: Vec<Arc<Capability>> = self
                .caps
                .borrow()
                .iter()
                .filter(|cap| filter(cap))
                .cloned()
                .collect();
            for cap in &capabilities {
                if !cap.is_available() {
                    unavailable.push(Arc::clone(cap));
                    continue;
                }
                self.push_card(cap);
            }
        }

        if !unavailable.is_empty() {
            let reasons: std::collections::BTreeSet<&str> = unavailable
                .iter()
                .filter_map(|c| c.unavailable_reason)
                .collect();
            let note = Label::new(Some(&format!(
                "{} control{} unavailable on this machine ({}) — see About for the full list.",
                unavailable.len(),
                if unavailable.len() == 1 {
                    " is"
                } else {
                    "s are"
                },
                reasons.into_iter().collect::<Vec<_>>().join("; ")
            )));
            note.add_css_class("llt-page-footnote");
            note.set_halign(Align::Start);
            note.set_wrap(true);
            self.page.append(&note);
        }

        // A page with nothing on it should say so rather than look broken.
        if self.cards.borrow().is_empty() && unavailable.is_empty() {
            let note = Label::new(Some("Nothing to show in this section."));
            note.add_css_class("llt-page-footnote");
            note.set_halign(Align::Start);
            self.page.append(&note);
        }

        if let Some(extra) = &self.extra {
            extra(&mut self.page, &self.caps);
        }

        // Warm the values immediately so the page is not blank for a second.
        for card in self.cards.borrow().iter() {
            card.refresh();
        }
    }

    /// A two-column grid for a section's cards, matching the reference's
    /// landing-page layout.
    fn section_grid(&self) -> gtk4::Grid {
        let grid = gtk4::Grid::new();
        grid.set_column_homogeneous(true);
        grid.set_column_spacing(8);
        grid.set_row_spacing(8);
        grid.set_valign(gtk4::Align::Start);
        grid
    }

    /// Append one capability's card to the page's own column and register it
    /// for refresh.
    fn push_card(&mut self, cap: &Arc<Capability>) {
        let card = FeatureCard::new(Arc::clone(cap));
        self.page.append(card.gtk_widget());
        card.connect_controls(self.page.contents.clone().upcast::<gtk4::Widget>());
        self.cards.borrow_mut().push(card);
    }

    /// Re-read the cards, but only while the page is on screen.
    fn refresh_if_mapped(&self) {
        if !self.page.widget().is_mapped() {
            return;
        }
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
    /// The landing page: readings and controls together.
    pub home: CapabilityPage,
    pub battery: BatteryPage,
    pub power: CapabilityPage,
    pub processors: CapabilityPage,
    pub lighting: CapabilityPage,
    pub system: CapabilityPage,
    pub about: Page,
    pub settings: Page,
    /// The refresh interval in seconds, shared with the Settings control so a
    /// change takes effect without a restart.
    pub refresh_seconds: Rc<RefCell<u32>>,
    landing_grid: ThermalGrid,
    /// The capability set, held once and shared by every page and the thermal
    /// grid. `Capability` is deliberately not cloneable — it owns its read and
    /// write closures — so pages share it through this rather than copying it.
    caps: Rc<RefCell<Vec<Arc<Capability>>>>,
}

impl Pages {
    pub fn new() -> Self {
        let caps: Rc<RefCell<Vec<Arc<Capability>>>> = Rc::new(RefCell::new(Vec::new()));
        let landing_grid: ThermalGrid = Rc::new(RefCell::new(None));
        Self {
            landing_grid: Rc::clone(&landing_grid),
            // The landing page leads with the sensor grid and then shows every
            // group, which is the reference's dashboard layout. The grid's cards
            // are registered into `thermal_cards` so the refresh loop reaches
            // them without knowing which page they are on.
            // The landing page leads with the sensor grid, then the reference's
            // dashboard sections — the headline controls only, not every
            // capability. The firmware power limits and the rest live on their
            // own pages.
            home: CapabilityPage::new(|_| {}, Rc::clone(&caps))
                .with_prelude({
                    let out = Rc::clone(&landing_grid);
                    move |page, _caps| append_sensor_grid(page, &out)
                })
                .with_sections(vec![
                    (
                        "Power",
                        Rc::new(|c: &Capability| {
                            matches!(
                                c.key.as_str(),
                                "powermode" | "battery_mode" | "fan_fullspeed" | "usb_charging"
                            )
                        }),
                    ),
                    (
                        "Graphics",
                        Rc::new(|c: &Capability| matches!(c.key.as_str(), "igpumode" | "gsync")),
                    ),
                    (
                        "Lighting",
                        Rc::new(|c: &Capability| {
                            matches!(
                                c.key.as_str(),
                                "kbd_backlight" | "ylogo_light" | "ioport_light"
                            )
                        }),
                    ),
                ]),
            battery: BatteryPage::new(),
            power: CapabilityPage::new(
                |page| {
                    page.heading("Power");
                },
                Rc::clone(&caps),
            )
            .with_extra(|page, caps| {
                // The presets editor belongs on the Power page, next to the
                // limits it stores.
                let card = presets_section(Rc::clone(caps));
                page.append(card.widget());
            }),
            // The Processors page is the controls only, grouped. The telemetry
            // (temperatures, clocks, utilization) lives in the Home sensor row;
            // repeating it here is exactly the duplication to avoid.
            processors: CapabilityPage::new(
                |page| {
                    page.heading("Processors & GPU");
                },
                Rc::clone(&caps),
            )
            .with_sections(vec![
                (
                    "Power Limits",
                    Rc::new(|c: &Capability| c.key.starts_with("firmware:")),
                ),
                (
                    "Graphics",
                    Rc::new(|c: &Capability| matches!(c.key.as_str(), "igpumode" | "gsync")),
                ),
            ]),
            lighting: CapabilityPage::new(
                |page| {
                    page.heading("Lighting");
                },
                Rc::clone(&caps),
            )
            .with_extra(|page, _caps| {
                if let Some(card) = rgb_section() {
                    page.append(card.widget());
                }
            }),
            system: CapabilityPage::new(
                |page| {
                    page.heading("System");
                },
                Rc::clone(&caps),
            ),
            about: about_page(),
            // Built once, in `rebuild`, like every other page. Constructing it
            // here as well built and discarded a full widget tree.
            settings: Page::empty(),
            refresh_seconds: Rc::new(RefCell::new(
                legiontoolkit::settings::current().refresh_interval_secs,
            )),
            caps,
        }
    }

    /// Rebuild every page from the backend.
    pub fn rebuild(&mut self, backend: &dyn Backend) {
        *self.caps.borrow_mut() = backend.capabilities().into_iter().map(Arc::new).collect();
        self.settings = settings_page(Rc::clone(&self.refresh_seconds));
        self.battery.rebuild(backend);

        // The landing page shows every group, sensors first.
        self.home.rebuild(|_| true);
        // Group pages show controls only. The read-only telemetry belongs in
        // the Home sensor row, not repeated as a card on every page.
        self.power.rebuild(|c| {
            c.group == legiontoolkit::capability::Group::Power
                && !matches!(c.kind, legiontoolkit::capability::Kind::ReadOnly)
        });
        // The Processors page renders sections, so its filter is unused.
        self.processors.rebuild(|_| false);
        self.lighting.rebuild(|c| {
            c.group == legiontoolkit::capability::Group::Lighting
                && !matches!(c.kind, legiontoolkit::capability::Kind::ReadOnly)
        });
        self.system.rebuild(|c| {
            c.group == legiontoolkit::capability::Group::System
                && !matches!(c.kind, legiontoolkit::capability::Kind::ReadOnly)
        });

        self.about = about_page_with(backend, &self.caps.borrow().clone());

        // Warm the sensor grid once, here rather than in the tick. The tick only
        // refreshes a page that is mapped, and at startup nothing is mapped yet
        // — so gating the warm-up on visibility left the landing page showing
        // dashes until the first second had passed.
        refresh_thermal(&self.caps, &self.landing_grid);
    }

    /// The configured refresh interval, in seconds.
    pub fn refresh_seconds(&self) -> u32 {
        *self.refresh_seconds.borrow()
    }

    /// Tick the values on the page that is actually on screen.
    ///
    /// Every capability read is a file open, a read and a parse, and the
    /// application has around fifty of them across eight pages. Refreshing all
    /// eight every second did all of that work for seven pages nobody was
    /// looking at — including five GPU cache locks — which is exactly the kind
    /// of background churn that costs a laptop its battery.
    ///
    /// A page that is not the stack's visible child is not mapped, so that is
    /// the test.
    pub fn refresh(&self) {
        self.home.refresh_if_mapped();
        self.battery.refresh_if_mapped();
        self.power.refresh_if_mapped();
        self.processors.refresh_if_mapped();
        self.lighting.refresh_if_mapped();
        self.system.refresh_if_mapped();
        if self.home.widget().is_mapped() {
            refresh_thermal(&self.caps, &self.landing_grid);
        }
    }

    pub fn stack_pages(&self) -> Vec<(&'static str, &ScrolledWindow, &'static str)> {
        vec![
            ("home", self.home.widget(), "Home"),
            ("power", self.power.widget(), "Power"),
            ("processors", self.processors.widget(), "Processors & GPU"),
            ("lighting", self.lighting.widget(), "Lighting"),
            ("battery", self.battery.widget(), "Battery"),
            ("system", self.system.widget(), "System"),
            ("settings", self.settings.widget(), "Settings"),
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
    board: Rc<SensorCard>,
    board_rows: Vec<(usize, &'static str)>,
    storage: Rc<SensorCard>,
    storage_rows: Vec<(usize, &'static str)>,
}

/// Append the sensor grid to a page and register its cards for refreshing.
///
/// Four cards, as the reference lays them out: CPU, GPU, Motherboard + Battery
/// (one card, two sections) and Memory + Disk. Every row starts blank and is
/// filled from whichever source has the reading.
fn append_sensor_grid(page: &mut Page, out: &ThermalGrid) {
    let out = Rc::clone(out);
    {
        let grid = FlowBox::new();
        grid.add_css_class("llt-sensor-grid");
        grid.set_selection_mode(gtk4::SelectionMode::None);
        grid.set_min_children_per_line(1);
        grid.set_max_children_per_line(4);
        grid.set_homogeneous(true);
        grid.set_valign(gtk4::Align::Start);

        let mut cpu = SensorCard::new("CPU");
        let cpu_rows = vec![
            (cpu.add_row("Utilization", "%", 0, 100.0), "cpu_utilization"),
            // The meter ceiling is the CPU's own rated maximum, so the bar
            // means "fraction of this CPU's range" rather than of a guess.
            (
                cpu.add_row(
                    "Core Clock",
                    "MHz",
                    0,
                    legiontoolkit::system::cpu_clock_max_mhz().unwrap_or(5000.0),
                ),
                "cpu_clock",
            ),
            (
                cpu.add_row("Temperature", "\u{00b0}C", 0, 110.0),
                "cpu_temp",
            ),
            // Blank where the RAPL counter is root-only.
            (cpu.add_row("Power", "W", 1, 60.0), "cpu_power"),
        ];

        let mut gpu = SensorCard::new("GPU");
        // The model goes on the card once, not on every row.
        if let Some(reading) = system::gpu() {
            gpu.set_detail(&reading.name);
        }
        let gpu_rows = vec![
            (gpu.add_row("Utilization", "%", 0, 100.0), "gpu_utilization"),
            (
                gpu.add_row("Temperature", "\u{00b0}C", 0, 110.0),
                "gpu_temp",
            ),
            (gpu.add_row("Core Clock", "MHz", 0, 3000.0), "gpu_clock"),
            (gpu.add_row("Power", "W", 1, 175.0), "gpu_power"),
            (gpu.add_row("Memory", "MiB", 0, 8192.0), "gpu_memory"),
        ];

        // Motherboard and Battery share a card, separated by a rule, so the
        // sensor row stays four cards on a wide window.
        let mut board = SensorCard::new("Motherboard");
        let mut board_rows = vec![
            (board.add_row("CPU Fan", "rpm", 0, 6000.0), "fan1_rpm"),
            (board.add_row("GPU Fan", "rpm", 0, 6000.0), "fan2_rpm"),
            (
                board.add_row("IC Temperature", "\u{00b0}C", 0, 110.0),
                "ic_temp",
            ),
        ];
        board.divider();
        board.subheading("Battery");
        board_rows.push((board.add_row("State", "", 0, 100.0), "battery_state"));
        board_rows.push((board.add_row("Level", "%", 0, 100.0), "battery_capacity"));

        let mut storage = SensorCard::new("Memory");
        let mut storage_rows = vec![
            (
                storage.add_row("Utilization", "%", 0, 100.0),
                "memory_utilization",
            ),
            (storage.add_row("Swap", "%", 0, 100.0), "swap_utilization"),
        ];
        storage.divider();
        storage.subheading("Disk");
        storage_rows.push((storage.add_row("Usage", "%", 0, 100.0), "disk_usage"));
        storage_rows.push((
            storage.add_row("Temperature", "\u{00b0}C", 0, 90.0),
            "disk_temp",
        ));

        let cards = Rc::new(ThermalCards {
            cpu: Rc::new(cpu),
            cpu_rows,
            gpu: Rc::new(gpu),
            gpu_rows,
            board: Rc::new(board),
            board_rows,
            storage: Rc::new(storage),
            storage_rows,
        });

        for card in [&cards.cpu, &cards.gpu, &cards.board, &cards.storage] {
            grid.insert(card.widget().upcast_ref::<gtk4::Widget>(), -1);
        }

        page.append(&grid);
        *out.borrow_mut() = Some(Rc::clone(&cards));
    }
}

/// Re-read every sensor row from the capability set.
fn refresh_thermal(
    caps: &Rc<RefCell<Vec<Arc<Capability>>>>,
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
            .find(|c| c.key == *key)
            .and_then(|c| c.read().ok())
            .and_then(|v| match v {
                Value::Float(f) => Some(f),
                Value::Int(i) => Some(i as f64),
                _ => None,
            })
    };
    let text = |key: &str| {
        caps.iter()
            .find(|c| c.key == *key)
            .and_then(|c| c.read().ok())
            .and_then(|v| match v {
                Value::Text(t) => Some(t),
                _ => None,
            })
    };

    for (row, key) in cards.cpu_rows.iter() {
        cards.cpu.set_value(*row, value(key));
    }
    for (row, key) in cards.gpu_rows.iter() {
        cards.gpu.set_value(*row, value(key));
    }
    for (row, key) in cards.board_rows.iter() {
        // "Charging" is a word, not a number, so it goes through the text path.
        if *key == "battery_state" {
            if let Some(state) = text(key) {
                cards.board.set_text(*row, 0.0, &state);
            }
            continue;
        }
        cards.board.set_value(*row, value(key));
    }
    // The reference's RAM card reads "Utilization  10.8/15.7 GB": the absolute
    // in the label, the percentage in the meter.
    for (row, key) in cards.storage_rows.iter() {
        if *key == "memory_utilization" {
            if let Some(m) = system::memory() {
                cards.storage.set_text(
                    *row,
                    m.percent / 100.0,
                    &format!("{:.1} / {:.1} GB", m.used_gib, m.total_gib),
                );
                continue;
            }
        }
        cards.storage.set_value(*row, value(key));
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
    backend: Rc<RefCell<Vec<Arc<Capability>>>>,
    /// The charging control (the three-way Battery Mode, or the conservation
    /// fallback), drawn below the hero so it sits with the readings it changes.
    control: Rc<RefCell<Option<FeatureCard>>>,
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
            control: Rc::new(RefCell::new(None)),
        }
    }

    fn rebuild(&mut self, backend: &dyn Backend) {
        self.page.clear();
        self.backend.borrow_mut().clear();
        self.backend
            .borrow_mut()
            .extend(backend.capabilities().into_iter().map(Arc::new));

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

        // --- Charging control ---------------------------------------------
        // The three-way Battery Mode (or the conservation fallback) belongs
        // here, beside the readings it changes, rather than only on Home.
        *self.control.borrow_mut() = None;
        let control_cap = self
            .backend
            .borrow()
            .iter()
            .find(|c| c.key == "battery_mode" || c.key == "battery_conservation")
            .cloned();
        if let Some(cap) = control_cap {
            let card = FeatureCard::new(cap);
            self.page.append(card.gtk_widget());
            card.connect_controls(self.page.contents.clone().upcast::<gtk4::Widget>());
            card.refresh();
            *self.control.borrow_mut() = Some(card);
        }

        // --- Stat cards ---------------------------------------------------
        self.rows.borrow_mut().clear();
        // Ordered as the reference orders BatteryPage: state, then capacity and
        // health, then the electrical detail.
        for key in [
            "battery_state",
            "battery_capacity",
            "battery_time",
            "battery_power",
            "battery_discharge_min",
            "battery_discharge_max",
            "battery_charge_now",
            "battery_full_capacity",
            "battery_design_capacity",
            "battery_health",
            "battery_on_battery_since",
            "battery_voltage",
            "battery_cycles",
        ] {
            // Bind the borrow: a temporary would be dropped while `cap` is
            // still alive.
            let backend = self.backend.borrow();
            let Some(cap) = backend.iter().find(|c| c.key == *key) else {
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

    /// Re-read the battery cards, but only while the page is on screen.
    fn refresh_if_mapped(&self) {
        if !self.page.widget().is_mapped() {
            return;
        }
        self.refresh();
    }

    fn refresh(&self) {
        // The charging control reads its own capability state.
        if let Some(card) = self.control.borrow().as_ref() {
            card.refresh();
        }

        let lookup = |key: &str| {
            self.backend
                .borrow()
                .iter()
                .find(|c| c.key == *key)
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
                .find(|c| c.key == *key)
                .and_then(|c| c.unit)
                .unwrap_or("");
            label.set_label(&match lookup(key) {
                // Hours-and-minutes reads better than a decimal for a duration.
                Some(Value::Float(hours)) if key == "battery_time" => {
                    if hours <= 0.0 {
                        "—".to_string()
                    } else {
                        format!("{}h {:02}m", hours as u64, ((hours.fract()) * 60.0) as u64)
                    }
                }
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
    about_page_body(None, Vec::new(), Vec::new())
}

fn about_page_with(backend: &dyn Backend, caps: &[Arc<Capability>]) -> Page {
    let info = Some((
        backend.name().to_string(),
        driver_status(),
        paths::machine_summary(),
    ));
    // Rebuilt with the full capability list so the compatibility report is
    // always current.
    let working: Vec<(String, String)> = caps
        .iter()
        .filter(|c| c.is_available())
        .map(|c| (c.label.clone(), c.key.clone()))
        .collect();
    let missing: Vec<(String, &'static str)> = caps
        .iter()
        .filter(|c| !c.is_available())
        .map(|c| {
            (
                c.label.clone(),
                c.unavailable_reason.unwrap_or("Unavailable"),
            )
        })
        .collect();
    about_page_body(info, working, missing)
}

fn about_page_body(
    info: Option<(String, String, String)>,
    working: Vec<(String, String)>,
    missing: Vec<(String, &'static str)>,
) -> Page {
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
            val.set_xalign(1.0);
            row.append(&key);
            row.append(&val);

            // Each value can be copied — machine id, driver, backend — because
            // these are exactly the strings a user pastes into a bug report.
            let copy = gtk4::Button::from_icon_name("edit-copy-symbolic");
            copy.add_css_class("flat");
            copy.set_tooltip_text(Some("Copy"));
            copy.set_valign(Align::Center);
            let text = value.to_string();
            copy.connect_clicked(move |button| {
                button.clipboard().set_text(&text);
            });
            row.append(&copy);

            page.append(&row);
        }

        // The full compatibility report sits behind a button, so About reads as
        // a summary rather than a wall of rows.
        let summary = format!(
            "Supported on this machine ({}); {} not available",
            working.len(),
            missing.len()
        );
        let toggle = gtk4::ToggleButton::with_label(&format!("Show all controls — {summary}"));
        toggle.add_css_class("flat");
        toggle.set_halign(Align::Start);
        toggle.set_margin_top(16);
        page.append(&toggle);

        let report = GtkBox::new(Orientation::Vertical, 0);
        report.set_margin_top(8);

        let supported = Label::new(Some(&format!("Supported ({})", working.len())));
        supported.add_css_class("llt-subheading");
        supported.set_halign(Align::Start);
        report.append(&supported);
        for (label, key) in &working {
            report.append(&compat_row(label, key, "llt-ok"));
        }

        if !missing.is_empty() {
            let absent = Label::new(Some(&format!("Not available ({})", missing.len())));
            absent.add_css_class("llt-subheading");
            absent.set_halign(Align::Start);
            absent.set_margin_top(12);
            report.append(&absent);
            for (label, reason) in &missing {
                report.append(&compat_row(label, reason, "llt-missing"));
            }
        }

        let revealer = gtk4::Revealer::new();
        revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
        revealer.set_reveal_child(false);
        revealer.set_child(Some(&report));

        toggle.connect_toggled({
            let revealer = revealer.clone();
            let summary = summary.clone();
            move |button| {
                let active = button.is_active();
                revealer.set_reveal_child(active);
                button.set_label(&if active {
                    "Hide controls".to_string()
                } else {
                    format!("Show all controls — {summary}")
                });
            }
        });
        page.append(&revealer);

        let note = Label::new(Some(
            "Built on GTK 4 and libadwaita. Hardware access via LenovoLegionLinux and the Linux kernel.",
        ));
        note.add_css_class("llt-card-note");
        note.set_wrap(true);
        note.set_margin_top(16);
        page.append(&note);
    })
}

/// One line of the compatibility report.
fn compat_row(label: &str, detail: &str, class: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_start(4);
    row.set_margin_bottom(2);

    let name = Label::new(Some(label));
    name.add_css_class("llt-compat-name");
    name.set_halign(Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&name);

    let detail = Label::new(Some(detail));
    detail.add_css_class(class);
    detail.set_halign(Align::End);
    detail.set_xalign(1.0);
    row.append(&detail);

    row
}

// ---------------------------------------------------------------------------
// Settings — the reference's categories that are application behaviour
// ---------------------------------------------------------------------------

/// A settings card: title, explanatory note, and a control in the accessory
/// slot, using the same anatomy as every other card in the application.
fn setting_card(
    label: &str,
    note: &str,
    control: &impl gtk4::prelude::IsA<gtk4::Widget>,
) -> LltCard {
    let card = LltCard::new();
    card.set_header("", label, Some(note), &[]);
    card.set_accessory(control);
    card
}

/// A combo bound to one setting, rendered as text choices.
fn setting_combo(
    label: &str,
    note: &str,
    choices: &[(&str, &str)],
    selected: &str,
) -> (LltCard, gtk4::DropDown) {
    let ids: Vec<&str> = choices.iter().map(|(id, _)| *id).collect();
    let names: Vec<&str> = choices.iter().map(|(_, name)| *name).collect();

    let dropdown = gtk4::DropDown::from_strings(&names);
    dropdown.set_valign(Align::Center);
    if let Some(index) = ids.iter().position(|id| *id == selected) {
        dropdown.set_selected(index as u32);
    }

    let card = setting_card(label, note, &dropdown);
    (card, dropdown)
}

fn setting_switch(label: &str, note: &str, active: bool) -> (LltCard, gtk4::Switch) {
    let toggle = gtk4::Switch::new();
    toggle.set_valign(Align::Center);
    toggle.set_active(active);
    let card = setting_card(label, note, &toggle);
    (card, toggle)
}

/// Build the settings page, and connect each control to the store.
fn settings_page(refresh_seconds: Rc<RefCell<u32>>) -> Page {
    use legiontoolkit::settings::{self, ColorScheme, TemperatureUnit};
    use legiontoolkit::settings::{MAX_REFRESH_SECS, MIN_REFRESH_SECS};

    Page::new(move |page| {
        let current = settings::current();
        page.heading("Settings");

        // --- Appearance -------------------------------------------------
        page.subheading("Appearance");
        let (card, scheme) = setting_combo(
            "Theme",
            "Follow the desktop, or force light or dark.",
            &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
            match current.color_scheme {
                ColorScheme::System => "system",
                ColorScheme::Light => "light",
                ColorScheme::Dark => "dark",
            },
        );
        scheme.connect_selected_notify(move |dropdown| {
            let scheme = match dropdown.selected() {
                1 => ColorScheme::Light,
                2 => ColorScheme::Dark,
                _ => ColorScheme::System,
            };
            let _ = settings::update(|s| s.color_scheme = scheme);
            crate::design::apply_color_scheme(scheme);
        });
        page.append(card.widget());

        // --- Units ------------------------------------------------------
        page.subheading("Units");
        let (card, unit) = setting_combo(
            "Temperature",
            "Applies to every temperature in the application.",
            &[("celsius", "Celsius"), ("fahrenheit", "Fahrenheit")],
            match current.temperature_unit {
                TemperatureUnit::Celsius => "celsius",
                TemperatureUnit::Fahrenheit => "fahrenheit",
            },
        );
        unit.connect_selected_notify(|dropdown| {
            let unit = if dropdown.selected() == 1 {
                TemperatureUnit::Fahrenheit
            } else {
                TemperatureUnit::Celsius
            };
            let _ = settings::update(|s| s.temperature_unit = unit);
        });
        page.append(card.widget());

        // --- Sensors ----------------------------------------------------
        page.subheading("Sensors");
        let intervals: Vec<u32> = [1, 2, 5, 10, 30]
            .into_iter()
            .filter(|s| (MIN_REFRESH_SECS..=MAX_REFRESH_SECS).contains(s))
            .collect();
        let labels: Vec<String> = intervals.iter().map(|s| format!("{s} s")).collect();
        let names: Vec<&str> = labels.iter().map(String::as_str).collect();
        let dropdown = gtk4::DropDown::from_strings(&names);
        dropdown.set_valign(Align::Center);
        if let Some(index) = intervals
            .iter()
            .position(|s| *s == current.refresh_interval_secs)
        {
            dropdown.set_selected(index as u32);
        }
        let selected_refresh = Rc::clone(&refresh_seconds);
        let intervals_for_signal = intervals.clone();
        dropdown.connect_selected_notify(move |dropdown| {
            let Some(seconds) = intervals_for_signal.get(dropdown.selected() as usize) else {
                return;
            };
            *selected_refresh.borrow_mut() = *seconds;
            let _ = settings::update(|s| s.refresh_interval_secs = *seconds);
        });
        page.append(
            setting_card(
                "Refresh Interval",
                "How often readings are re-read. Each one is a sysfs read.",
                &dropdown,
            )
            .widget(),
        );

        // --- Behaviour --------------------------------------------------
        page.subheading("Behaviour");

        let (card, minimize) = setting_switch(
            "Start Minimized",
            "Open without showing the window.",
            current.start_minimized,
        );
        minimize.connect_state_set(|_, active| {
            let _ = settings::update(|s| s.start_minimized = active);
            glib::Propagation::Proceed
        });
        page.append(card.widget());

        let (card, remember) = setting_switch(
            "Remember Window Size",
            "Re-open at the size you left it. GTK 4 cannot place a window, so \
             only the size is kept.",
            current.remember_window_size,
        );
        remember.connect_state_set(|_, active| {
            let _ = settings::update(|s| s.remember_window_size = active);
            glib::Propagation::Proceed
        });
        page.append(card.widget());

        let (card, compact) = setting_switch(
            "Compact Mode",
            "Hide card subtitles and status rows. Takes effect on the next launch.",
            current.compact_mode,
        );
        compact.connect_state_set(|_, active| {
            let _ = settings::update(|s| s.compact_mode = active);
            glib::Propagation::Proceed
        });
        page.append(card.widget());

        // --- Background ------------------------------------------------
        page.subheading("Background");

        // The autostart entry is the source of truth: the switch reflects the
        // file on disk, so removing it by hand is picked up on the next launch.
        let (card, autostart) = setting_switch(
            "Start on Login",
            "Launch Legion Toolkit automatically when you log in.",
            legiontoolkit::services::autostart::is_enabled(),
        );
        autostart.connect_state_set(|_, active| {
            if let Err(e) = legiontoolkit::services::autostart::set(active) {
                tracing::warn!("could not update start-on-login: {e}");
            }
            let _ = settings::update(|s| s.autostart = active);
            glib::Propagation::Proceed
        });
        page.append(card.widget());

        let (card, minimize_close) = setting_switch(
            "Minimize on Close",
            "Keep running in the background when the window is closed. Reopen it by \
             launching Legion Toolkit again, or quit from the menu.",
            current.minimize_on_close,
        );
        minimize_close.connect_state_set(|_, active| {
            let _ = settings::update(|s| s.minimize_on_close = active);
            glib::Propagation::Proceed
        });
        page.append(card.widget());

        // --- About the project ------------------------------------------
        page.subheading("About");
        page.append(
            setting_card(
                "Legion Toolkit",
                "Built on GTK 4 and libadwaita. Hardware access via LenovoLegionLinux \
                 and the Linux kernel.",
                &gtk4::Box::new(Orientation::Horizontal, 0),
            )
            .widget(),
        );
    })
}

// ---------------------------------------------------------------------------
// Keyboard RGB — delegated to keyRGB
// ---------------------------------------------------------------------------

/// A card that controls the keyboard's RGB lighting directly.
///
/// `None` when this machine has no ITE RGB keyboard. The protocol is our own
/// implementation of the controller (see [`legiontoolkit::rgb`]); this card is
/// the UI over it.
#[allow(deprecated)] // gtk4::ColorButton is the swatch widget; still the simplest fit.
fn rgb_section() -> Option<LltCard> {
    use legiontoolkit::rgb::{Effect, Keyboard};
    use legiontoolkit::rgb_presets::RgbPresetStore;

    if !Keyboard::available() {
        return None;
    }

    /// The card's live selection.
    /// Which zones a picked colour applies to.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum ZoneScope {
        All,
        Row,
        /// The zones stacked below the one picked.
        Column,
        Zone,
    }

    struct Ui {
        off: bool,
        effect: Effect,
        zones: [[u8; 3]; legiontoolkit::rgb::NUM_ZONES],
        brightness: u32,
        speed: u32,
        forward: bool,
        scope: ZoneScope,
    }

    let state = Rc::new(RefCell::new(Ui {
        off: false,
        effect: Effect::Static,
        zones: [[255, 32, 32]; legiontoolkit::rgb::NUM_ZONES],
        brightness: 40,
        speed: 5,
        forward: true,
        scope: ZoneScope::Zone,
    }));

    let card = LltCard::new();
    card.set_header(
        "keyboard-brightness-symbolic",
        "Keyboard RGB",
        Some("24-zone RGB. Set the colour and effect here."),
        &[],
    );

    // Presets sit at the top, as the reference lays them out. The buttons are
    // built further down (they need the controls), but the row is placed here.
    let preset_row = GtkBox::new(Orientation::Horizontal, 4);
    preset_row.set_valign(Align::Center);
    card.append(&preset_row);

    // --- Effect ---------------------------------------------------------
    let effects = [
        ("Off", None),
        ("Static", Some(Effect::Static)),
        ("Rainbow (spiral)", Some(Effect::ScrewRainbow)),
        ("Rainbow wave", Some(Effect::RainbowWave)),
        ("Colour change", Some(Effect::ColorChange)),
        ("Colour pulse", Some(Effect::ColorPulse)),
        ("Colour wave", Some(Effect::ColorWave)),
        ("Smooth", Some(Effect::Smooth)),
    ];
    let names: Vec<&str> = effects.iter().map(|(label, _)| *label).collect();
    let effect_dd = gtk4::DropDown::from_strings(&names);
    effect_dd.set_selected(1);
    effect_dd.set_valign(Align::Center);
    effect_dd.set_size_request(200, -1);
    effect_dd.connect_selected_notify({
        let state = Rc::clone(&state);
        move |dropdown| {
            let selected = effects
                .get(dropdown.selected() as usize)
                .and_then(|(_, effect)| *effect);
            let mut state = state.borrow_mut();
            match selected {
                Some(effect) => {
                    state.off = false;
                    state.effect = effect;
                }
                None => state.off = true,
            }
        }
    });

    // --- Zones ----------------------------------------------------------
    // The keyboard's 24 zones as a 4x6 grid of colour swatches — the same
    // idea as the reference's 24-zone view. Each swatch is a GTK colour
    // button, so a click opens the system colour chooser.
    let zone_grid = gtk4::Grid::new();
    zone_grid.add_css_class("llt-keyboard");
    zone_grid.set_margin_top(8);
    zone_grid.set_margin_bottom(8);
    zone_grid.set_row_spacing(4);
    zone_grid.set_column_spacing(4);
    zone_grid.set_halign(Align::Start);
    let mut swatches: Vec<gtk4::ColorButton> = Vec::new();
    for row in 0..legiontoolkit::rgb::NUM_ROWS {
        for col in 0..legiontoolkit::rgb::NUM_COLS {
            let index = row * legiontoolkit::rgb::NUM_COLS + col;
            let button = gtk4::ColorButton::new();
            button.set_size_request(58, 22);
            button.set_rgba(&to_rgba(state.borrow().zones[index]));
            // Stagger the rows, as keyboard rows are staggered.
            if row % 2 == 1 {
                button.set_margin_start(16);
            }
            if row == 3 {
                button.set_margin_end(24);
            }
            zone_grid.attach(&button, col as i32, row as i32, 1, 1);
            swatches.push(button);
        }
    }
    let swatches = Rc::new(swatches);

    // Picking a colour applies it to every zone in the current scope: all 24, the
    // row, the column (the four zones below), or just the one picked.
    for (index, swatch) in swatches.iter().enumerate() {
        let state = Rc::clone(&state);
        let swatches = Rc::clone(&swatches);
        swatch.connect_color_set(move |button| {
            let rgba = button.rgba();
            let rgb = [
                (rgba.red() * 255.0).round() as u8,
                (rgba.green() * 255.0).round() as u8,
                (rgba.blue() * 255.0).round() as u8,
            ];
            let row = index / legiontoolkit::rgb::NUM_COLS;
            let col = index % legiontoolkit::rgb::NUM_COLS;
            let cols = legiontoolkit::rgb::NUM_COLS;
            let rows = legiontoolkit::rgb::NUM_ROWS;
            let scope = state.borrow().scope;
            let indices: Vec<usize> = match scope {
                ZoneScope::All => (0..cols * rows).collect(),
                ZoneScope::Row => (0..cols).map(|c| row * cols + c).collect(),
                ZoneScope::Column => (0..rows).map(|r| r * cols + col).collect(),
                ZoneScope::Zone => vec![index],
            };
            for i in indices {
                state.borrow_mut().zones[i] = rgb;
                swatches[i].set_rgba(&to_rgba(rgb));
            }
        });
    }

    // --- Brightness and speed ------------------------------------------
    // Sliders, not steppers: brightness is a percentage, speed a 0-10 scale.
    let brightness = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 5.0);
    brightness.set_size_request(240, -1);
    brightness.set_value(40.0);
    brightness.set_valign(Align::Center);
    brightness.set_draw_value(true);
    brightness.connect_value_changed({
        let state = Rc::clone(&state);
        move |scale| state.borrow_mut().brightness = scale.value() as u32
    });

    let speed = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 10.0, 1.0);
    speed.set_size_request(240, -1);
    speed.set_value(5.0);
    speed.set_valign(Align::Center);
    speed.set_draw_value(true);
    speed.connect_value_changed({
        let state = Rc::clone(&state);
        move |scale| state.borrow_mut().speed = scale.value() as u32
    });

    // --- What a colour applies to ---------------------------------------
    let scopes = [
        ("All zones", ZoneScope::All),
        ("This row", ZoneScope::Row),
        ("This column (4 zones)", ZoneScope::Column),
        ("This zone", ZoneScope::Zone),
    ];
    let scope_names: Vec<&str> = scopes.iter().map(|(label, _)| *label).collect();
    let scope_dd = gtk4::DropDown::from_strings(&scope_names);
    scope_dd.set_selected(3);
    scope_dd.set_valign(Align::Center);
    scope_dd.connect_selected_notify({
        let state = Rc::clone(&state);
        move |dropdown| {
            let scope = scopes
                .get(dropdown.selected() as usize)
                .map(|(_, scope)| *scope)
                .unwrap_or(ZoneScope::Zone);
            state.borrow_mut().scope = scope;
        }
    });

    // --- Rows -----------------------------------------------------------
    let grid = gtk4::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(8);
    for (index, (label, widget)) in [
        ("Effect", effect_dd.clone().upcast::<gtk4::Widget>()),
        ("Apply colour to", scope_dd.clone().upcast::<gtk4::Widget>()),
        ("Zones", zone_grid.clone().upcast::<gtk4::Widget>()),
        ("Brightness", brightness.clone().upcast::<gtk4::Widget>()),
        ("Speed", speed.clone().upcast::<gtk4::Widget>()),
    ]
    .into_iter()
    .enumerate()
    {
        let name = Label::new(Some(label));
        name.add_css_class("llt-card-title");
        name.set_halign(Align::Start);
        name.set_hexpand(true);
        grid.attach(&name, 0, index as i32, 1, 1);
        grid.attach(&widget, 1, index as i32, 1, 1);
    }
    card.append(&grid);

    // --- Actions, presets ----------------------------------------------
    let status = Label::new(None);
    status.add_css_class("llt-card-note");
    status.set_halign(Align::Start);
    status.set_wrap(true);

    // Push the current controls to the keyboard. Shared by Apply and by
    // selecting a preset.
    let apply_current: Rc<dyn Fn()> = {
        let state = Rc::clone(&state);
        let status = status.clone();
        Rc::new(move || {
            let state = state.borrow();
            let outcome = Keyboard::open().and_then(|keyboard| {
                if state.off {
                    keyboard.turn_off()
                } else if state.effect == Effect::Static {
                    keyboard.set_zone_colors(&state.zones, state.brightness)
                } else {
                    keyboard.set_effect(
                        state.effect,
                        state.speed,
                        None,
                        state.forward,
                        state.brightness,
                    )
                }
            });
            status.set_label(&match outcome {
                Ok(()) => "Applied.".to_string(),
                Err(e) => format!("{e}"),
            });
        })
    };

    // Six numbered presets, as the reference lays them out. Selecting a slot
    // loads it into the controls and applies it; Save writes the current look
    // back into the selected slot.
    let store = Rc::new(RefCell::new(RgbPresetStore::load()));
    let mut group: Option<gtk4::ToggleButton> = None;
    for index in 0..store.borrow().presets.len() {
        let name = store.borrow().presets[index].name.clone();
        let button = gtk4::ToggleButton::with_label(&name);
        if let Some(previous) = &group {
            button.set_group(Some(previous));
        }
        button.set_active(index == store.borrow().active);
        button.connect_toggled({
            let store = Rc::clone(&store);
            let state = Rc::clone(&state);
            let effect_dd = effect_dd.clone();
            let brightness = brightness.clone();
            let speed = speed.clone();
            let swatches = swatches.clone();
            let apply_current = Rc::clone(&apply_current);
            move |button| {
                if !button.is_active() {
                    return;
                }
                store.borrow_mut().active = index;
                let _ = store.borrow().save();
                let Some(preset) = store.borrow().active_preset().cloned() else {
                    return;
                };
                let effect = preset.effect();
                let zones = preset.zones();
                {
                    let mut state = state.borrow_mut();
                    state.off = false;
                    state.effect = effect;
                    state.zones = zones;
                    state.brightness = preset.brightness;
                    state.speed = preset.speed;
                }
                effect_dd.set_selected(effect_index(effect));
                brightness.set_value(preset.brightness as f64);
                speed.set_value(preset.speed as f64);
                for (slot, swatch) in swatches.iter().enumerate() {
                    swatch.set_rgba(&to_rgba(zones[slot]));
                }
                apply_current();
            }
        });
        preset_row.append(&button);
        group = Some(button);
    }

    let save = gtk4::Button::with_label("Save preset");
    save.connect_clicked({
        let store = Rc::clone(&store);
        let state = Rc::clone(&state);
        let status = status.clone();
        move |_| {
            let state = state.borrow();
            store
                .borrow_mut()
                .capture(state.effect, state.zones, state.brightness, state.speed);
            let _ = store.borrow().save();
            status.set_label("Saved.");
        }
    });
    preset_row.append(&save);
    card.append(&preset_row);

    let apply = gtk4::Button::with_label("Apply");
    apply.add_css_class("suggested-action");
    let off = gtk4::Button::with_label("Turn off");
    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.append(&off);
    actions.append(&apply);
    card.append(&actions);
    card.append(&status);

    apply.connect_clicked({
        let apply_current = Rc::clone(&apply_current);
        move |_| apply_current()
    });
    off.connect_clicked({
        let status = status.clone();
        move |_| {
            status.set_label(&match Keyboard::open().and_then(|k| k.turn_off()) {
                Ok(()) => "Lighting off.".to_string(),
                Err(e) => format!("{e}"),
            });
        }
    });

    Some(card)
}

/// The effect dropdown index for an effect (`0` is Off).
fn effect_index(effect: legiontoolkit::rgb::Effect) -> u32 {
    use legiontoolkit::rgb::Effect;
    match effect {
        Effect::Static => 1,
        Effect::ScrewRainbow => 2,
        Effect::RainbowWave => 3,
        Effect::ColorChange => 4,
        Effect::ColorPulse => 5,
        Effect::ColorWave => 6,
        Effect::Smooth => 7,
    }
}

/// A colour as a GTK RGBA.
fn to_rgba(rgb: [u8; 3]) -> gtk4::gdk::RGBA {
    gtk4::gdk::RGBA::new(
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    )
}

// ---------------------------------------------------------------------------
// Custom mode — named presets of power settings
// ---------------------------------------------------------------------------

/// A section for named presets, matching the reference's custom mode.
///
/// The reference's `GodModeSettingsWindow` edits a `GodModePreset`: a name plus
/// one value per tunable. This is the same idea with the capability list doing
/// the work — a preset is a map of capability key to value, applying one writes
/// each entry through the same path a slider uses, and capturing one reads the
/// current values back.
fn presets_section(caps: Rc<RefCell<Vec<Arc<Capability>>>>) -> LltCard {
    use legiontoolkit::presets::PresetStore;

    let store = Rc::new(RefCell::new(PresetStore::load()));
    let names: Vec<String> = store
        .borrow()
        .presets
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let labels: Vec<&str> = names.iter().map(String::as_str).collect();

    let dropdown = gtk4::DropDown::from_strings(&labels);
    dropdown.set_valign(Align::Center);
    dropdown.set_selected(store.borrow().active as u32);

    let apply = gtk4::Button::with_label("Apply");
    apply.add_css_class("suggested-action");

    let capture = gtk4::Button::with_label("Capture current");
    capture.set_tooltip_text(Some(
        "Save the current values of every configurable setting into this preset",
    ));

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.append(&capture);
    actions.append(&apply);

    let card = LltCard::new();
    card.set_header(
        "",
        "Custom Presets",
        Some(
            "A named set of power limits. Applying one switches to custom power mode, \
             then writes every value in the preset.",
        ),
        &[],
    );
    card.set_accessory(&dropdown);
    card.append(&actions);

    let status = Label::new(None);
    status.add_css_class("llt-card-note");
    status.set_halign(Align::Start);
    status.set_wrap(true);
    card.append(&status);

    // Selecting a preset makes it active, and persists that.
    dropdown.connect_selected_notify({
        let store = Rc::clone(&store);
        move |dropdown| {
            let mut store = store.borrow_mut();
            store.active = dropdown.selected() as usize;
            let _ = store.save();
        }
    });

    // Apply: custom mode first, then each value.
    apply.connect_clicked({
        let store = Rc::clone(&store);
        let caps = Rc::clone(&caps);
        let status = status.clone();
        let card = card.clone();
        move |_| {
            let preset = match store.borrow().active_preset() {
                Some(p) => p.clone(),
                None => return,
            };
            apply_preset(&preset, &caps, &card, &status);
        }
    });

    // Capture: read every writable capability's current value into the preset.
    capture.connect_clicked({
        let store = Rc::clone(&store);
        let caps = Rc::clone(&caps);
        let status = status.clone();
        move |_| {
            // Bind the borrow: a temporary would be dropped while the
            // iterator over it is still alive.
            let capabilities = caps.borrow();
            let current: Vec<(String, String)> = capabilities
                .iter()
                .filter(|cap| cap.can_write())
                .filter_map(|cap| {
                    let value = cap.read().ok()?;
                    Some((cap.key.clone(), display_value(&value)))
                })
                .collect();
            drop(capabilities);
            let mut store = store.borrow_mut();
            store.active = store.capture_to_active(current.into_iter());
            let count = store
                .active_preset()
                .map(|p| p.setting_count())
                .unwrap_or(0);
            let _ = store.save();
            status.set_label(&format!("Captured {count} settings into this preset."));
        }
    });

    card
}

/// The string a preset stores for a value.
///
/// Text values are stored verbatim; numbers lose their formatting, because the
/// file being written wants `70`, not `70.0 W`.
fn display_value(value: &Value) -> String {
    match value {
        Value::Bool(true) => "1".into(),
        Value::Bool(false) => "0".into(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 {
                format!("{f:.0}")
            } else {
                f.to_string()
            }
        }
        Value::Text(t) => t.clone(),
    }
}

/// Apply a preset: switch to custom power mode, then write each value.
///
/// The mode switch comes first and deliberately: on this firmware several of
/// the power limits report no writable range outside custom mode, so writing
/// them first would fail.
fn apply_preset(
    preset: &legiontoolkit::presets::Preset,
    caps: &Rc<RefCell<Vec<Arc<Capability>>>>,
    card: &LltCard,
    status: &Label,
) {
    let capabilities = caps.borrow();

    // 1. Custom power mode, if the machine offers one.
    if let Some(mode) = capabilities.iter().find(|c| c.key == "powermode") {
        let custom = mode
            .values
            .iter()
            .find(|v| v.value == "custom" || v.label.eq_ignore_ascii_case("custom"));
        if let Some(custom) = custom {
            if let Err(e) = mode.write(Value::Text(custom.value.clone())) {
                card.set_outcome(
                    Severity::Error,
                    &format!("Could not enter custom mode: {e}"),
                );
                return;
            }
        }
    }

    // 2. Every value the preset holds that this machine actually has.
    let mut written = 0;
    let mut skipped = 0;
    for (key, value) in &preset.values {
        match capabilities.iter().find(|c| c.key == *key) {
            Some(cap) if cap.can_write() => {
                let parsed = legiontoolkit::capability::parse_stored(cap.kind, value);
                match cap.write(parsed) {
                    Ok(()) => written += 1,
                    Err(e) => {
                        card.set_outcome(Severity::Error, &format!("{key}: {e}"));
                        return;
                    }
                }
            }
            // A preset may name settings this machine does not have. That is
            // not an error — the same preset is meant to work elsewhere.
            _ => skipped += 1,
        }
    }

    let message = if skipped > 0 {
        format!("Applied {written} settings ({skipped} not present on this machine).")
    } else {
        format!("Applied {written} settings.")
    };
    status.set_label(&message);
}
