# Legion Linux Toolkit — Plan

**Status:** draft, revised 2026-10-13
**Stack:** Rust · GTK 4 · libadwaita
**Target hardware (verified):** Lenovo Legion 5 15IRX10, model `83LY`, BIOS `QNCN38WW`

---

## 1. What we are building

A polished, native Linux desktop application for controlling Lenovo Legion
laptops — the Linux counterpart to the Windows "Lenovo Legion Toolkit".

We are **not** writing a hardware driver. We are writing the *application*: a
dashboard, a settings surface, profiles and automation, consuming the hardware
layer that LenovoLegionLinux (LLL) already provides through sysfs and hwmon.

The reference material in this repo is vendored read-only and serves two distinct
purposes:

| Reference | License | Role |
|---|---|---|
| `Reference/LenovoLegionToolkit` | GPL-3.0 | **UX reference.** Feature scope, information architecture, visual language. |
| `Reference/LenovoLegionLinux` | GPL-2.0 | **Hardware reference.** Kernel module, sysfs/hwmon surface, capability matrix. |

We port *design and structure* between them. We never copy source files across:
GPL-2.0-only code cannot be relicensed as GPL-3.0, and the two licenses are not
compatible in that direction. Our own code is independently licensed and talks to
LLL across a documented interface.

---

## 2. Decisions

### 2.1 Rust + GTK 4 + libadwaita

The obvious alternative was Python, because LenovoLegionLinux already ships a
PyQt6 GUI (`legion_gui.py`, ~1950 lines) and PyQt6 is already in its dependency
lists. We rejected that path:

- **libadwaita gives us the target look for free.** The GNOME Human Interface
  Guidelines — adaptive layout, dark/light, coordinated accent colours, animated
  transitions — are the closest thing Linux has to a first-class "designed"
  application design system. The Windows toolkit is Fluent 2; Adwaita is the
  native equivalent and is maintained for us rather than by us.
- **No runtime dependency.** A statically linked Rust binary needs no Python
  interpreter, no PyQt6 wheels, no `darkdetect`. One artifact installs cleanly on
  Fedora and beyond.
- **Rust removes a whole class of bug.** Hardware state is shared mutable state
  read from files that can disappear or change under us. Rust's ownership model
  and exhaustive `Result` handling make "file vanished mid-refresh" a compile
  error rather than a traceback.

Costs we accept knowingly:

- **We reimplement the hardware logic.** LLL's `legion.py` encodes years of
  reverse-engineered knowledge: fan-curve RPM↔PWM conversion per backend, the
  level-clamp tables, YAML preset schemas, the conservation/rapid-charge
  interlock. Some of that will be rewritten in Rust. See §4 for how we bound the
  cost.
- **Longer to first pixel** than improving the existing PyQt6 GUI.

### 2.2 Capability-first, not page-first

The single most important design constraint. Feature availability on Legion
hardware varies wildly by model *and power mode*, and the Windows toolkit's
answer to this is one mechanism: a card that fails to refresh hides itself
(`AbstractRefreshingControl`). We adopt that as an architectural rule — **the
UI is generated from a capability report, not hand-written per model.**

This is not a nicety here. On *this specific laptop*, fan curves and power limits
do not exist at all (§3). A page-first UI would show a dead fan-curve editor to
every user of this machine. A capability-first UI shows a working dashboard.

### 2.3 No new privileged daemon

We do not build a background service to own hardware state. The first release is
an unprivileged GUI that escalates *per write* to a small helper binary through
polkit. A daemon is only justified once automation needs persistent, central
permission — and even then, `legiond` already exists for that job.

### 2.4 Import LLL's logic only where it's irreplaceable

A `HardwareBackend` trait with two implementations:

- **`SysfsBackend`** (Rust, direct sysfs/hwmon) — the default. Reads need no
  privilege at all; writes go through the helper. Covers every capability this
  laptop has.
- **`LllCliBackend`** — shells out to `legion_cli` under pkexec, for capabilities
  whose logic we deliberately do not reimplement (fan curves on models that have
  them). Implemented later, only when needed.

---

## 3. What this laptop can actually do

Verified from the running system on 2026-10-13.

```
product_name:   Legion 5 15IRX10      board_name: LNVNB161216
product_version: 83LY                 bios:       QNCN38WW
kernel:         7.2.9-200.fc44.x86_64 (Fedora 44)
```

The driver's DMI allowlist contains a `QNCN` entry
(`Reference/LenovoLegionLinux/kernel_module/legion-laptop.c:2897-2905`) mapping
to `model_qncn` (config at `:1102-1122`). So this machine **is** supported — the
kernel module is simply not loaded yet.

### `model_qncn` capability matrix

| Capability | Access method | Status |
|---|---|---|
| Power mode (quiet / balanced / performance / **custom**) | `WMI` | ✅ |
| Fan full-speed (max) | `WMI` | ✅ |
| CPU + GPU temperature | `WMI3` | ✅ |
| Fan RPM (all fans) | `WMI3` | ✅ |
| IC / chipset temperature | `skip_ic_temp = true` | ❌ hidden |
| Keyboard backlight (3-state, white) | `WMI` → `leds` class | ✅ |
| Y-logo + IO-port LEDs | — | ✅ |
| **Fan curve** | `NO_ACCESS` | ❌ **none** |
| **CPU/GPU power limits** | unset → `NO_ACCESS` | ❌ **none** |
| Fan max-speed query (`fan_maxspeed`) | `skip_fan_maxspeed` | ❌ hidden |
| Fan controller lock | `skip_lockfancontroller` | ❌ hidden |
| CPU/GPU overclock | — | ❌ |

LLL's own documentation already writes this machine's test case
(`doc/FEATURES_AND_TESTING.md:1-8`):

> On a Legion 5 15IRX10 with QNCN firmware, start `legion_gui` with the module
> loaded. Verify that the window opens, fan curve Read/Apply buttons are
> disabled, and the notice explains that custom fan curves are unsupported.

**Consequence for the plan:** the fan-curve editor — the headline feature of the
Windows original — cannot work on this hardware. It stays in the architecture and
in the design system, gated behind capability detection, and it is *not* in the
first milestone. Anyone testing on this laptop must see it correctly suppressed.

### What already works today, with no new module

`ideapad_laptop` is already bound to `VPC2004:00` and exposes:

```
conservation_mode   fan_mode   fn_lock   camera_power   usb_charging
```

`/sys/class/power_supply/` provides `ADP0`, `BAT0` (68%, Charging) plus two
USB-C PD sources. So the first dashboard can be built against *live data before
the kernel module is ever loaded*.

---

## 4. Architecture

```
┌──────────────────────────────────────────────┐
│  app/          GTK4 + libadwaita shell       │  window, navigation,
│                design system, pages          │  dark/light, CSS
├──────────────────────────────────────────────┤
│  capability/   the stable API the UI writes  │  descriptors, availability
│                against — never sysfs paths   │  probes, limits, units
├──────────────────────────────────────────────┤
│  backends/     sysfs · hwmon · leds · power  │  reads are unprivileged;
│                supply · LLL CLI bridge       │  writes escalate
├──────────────────────────────────────────────┤
│  services/     privilege (pkexec) · profiles │
│                · automation · settings       │
└──────────────────────────────────────────────┘
```

### 4.1 The capability descriptor

One type per control, so no page needs to know which backend or which machine it
is running on:

```rust
pub enum Kind { Toggle, Enum, Slider, ReadOnly, Custom }

pub struct Capability {
    pub key:        &'static str,   // stable id, e.g. "powermode"
    pub label:      String,
    pub icon:       &'static str,
    pub kind:       Kind,
    pub group:      Group,          // Power / Thermal / Lighting / Battery / System
    pub available:  bool,           // probed from backend
    pub values:     Vec<NamedValue>,// enums, from sysfs `choices`
    pub limits:     Option<Limits>, // sliders, from *_limits_* / int ranges
    pub unit:       Option<&'static str>,
    pub note:       Option<&'static str>, // e.g. "requires custom power mode"
    pub read:  Box<dyn Fn() -> Result<Value> + Send + Sync>,
    pub write: Box<dyn Fn(Value) -> Result<()> + Send + Sync>,
}
```

`available` is the whole trick. It is probed by checking that the underlying
sysfs/hwmon/led path exists and is readable — the Rust equivalent of LLL's
`Feature.exists()`. A card whose capability is unavailable renders as suppressed
with its `note`, not as an error and not as a disabled mystery.

### 4.2 Backend selection

`SysfsBackend` resolves paths the same way LLL does, which matters: it is the
difference between working on this laptop and not.

- Kernel ≥ 7.0 platform path: `/sys/module/legion_laptop/drivers/platform:legion/legion`
  (we are on 7.2.9, so this branch).
- Legacy path: `…/platform:legion/PNP0C09:00`.
- hwmon: `/sys/devices/platform/legion/hwmon/hwmon*/` (name `legion_hwmon`).
- Several controls live under **`ideapad_acpi`**, not legion:
  `battery_conservation`, `fn_lock`, `touchpad`, `camera_power`, `usb_charging`.
  These must be probed separately or they will read as "unsupported".

### 4.3 Privilege

Every writable sysfs attribute is mode `0644`; reads are unprivileged, writes are
not. There is no udev rule anywhere in LLL, and its polkit actions only grant the
right to *run a binary*, not to write sysfs.

Our approach — the standard polkit helper pattern:

1. The GUI runs unprivileged and does all reads locally.
2. A write builds a request (`{ key, value }`), serialised as JSON.
3. The helper binary `legiontoolkit-helper` is invoked via pkexec under
   `org.legiontoolkit.helper`, whose `.policy` declares
   `org.freedesktop.policykit.exec.path` pointing at the helper's absolute path.
4. The helper validates the key against its own allowlist and performs the write.

Two rules we carry over from LLL that are easy to get wrong:

- **The polkit action id must reference the helper's real absolute path.** A
  helper installed outside the declared path silently fails to authorise.
- **Check `ec_readonly`.** `/sys/module/legion_laptop/parameters/ec_readonly`
  disables every EC write kernel-wide. The UI must read it and say so, or users
  will report "nothing works" on a module loaded in read-only mode.

### 4.4 Profiles and automation

Profiles are named, ordered sets of capability values — the Quiet / Balanced /
Performance idea from the Windows app, plus AC↔battery switching.

We deliberately start small. The Windows app has 27 trigger types and 53 step
types; reproducing that is not the first job. Milestone 3 implements a focused
subset: power-state, AC/battery, and periodic triggers, with a step-per-capability
model that reuses the same descriptors the dashboard uses. That keeps the
automation surface honest — a capability that can't be written can't be a step.

---

## 5. Design system

Extracted from the Windows reference and mapped onto GTK4/libadwaita.

### Tokens

| Element | Windows (Fluent 2) | Ours |
|---|---|---|
| Window | 1250×780, min 650×600 | `AdwWindow` default, adaptive |
| Card radius | 8px | libadwaita `.card` (12px) or 8px override |
| Card padding | `12,10` | 12px 10px |
| Card margin | `0,0,8,8` | 8px gap in a flow-box |
| Card title | 14px SemiBold | `.title-4` |
| Data label | 12px secondary | `dim-label` |
| Page heading | 24px Medium, margin `0,16,0,24` | `.title-1` + 24px bottom |
| Compact bar | 40×6, animated, accent | 40×6 `GtkProgressBar` + CSS animation |
| Accent | `RGB(255,33,33)` | Adwaita accent colour, overridable via CSS |
| Separator | 1px `CardStrokeColorDefault` | `GtkSeparator` |
| Segmented control | pill, radius 4, 12px, 0.15s fade+scale | `GtkStackSwitcher` styled, or `AdwSegmentedButton` |

### Component mapping

| Windows | Ours |
|---|---|
| `CardControl` / `CardExpander` / `CardAction` | one `LltCard` base + three variants; `AdwExpanderRow`, `AdwActionRow` |
| `CardHeaderControl` (title/subtitle/status/accessory) | `LltCardHeader` — title, subtitle, status rows, accessory slot |
| `AbstractRefreshingControl` | `AbstractFeatureCard` — refresh, hide on unsupported |
| `LoadableControl` | spinner placeholder widget |
| `SensorsControlV2` | `SensorGrid` — CPU/GPU/Motherboard+Memory cards |
| `FanCurveControlV2` | `FanCurveEditor` (deferred; see §3) |
| `DashboardGroupControl` | `DashboardGroup` titled stack |
| `NavigationStore` sidebar | `AdwNavigationSplitView` or `GtkStack` + `GtkListBox` sidebar |

### Dashboard groups

Reproduce the four from the Windows reference, populated by capability:

- **Power** — power mode, battery mode, conservation, always-on-USB, instant boot
- **Thermal** — sensors, fan full-speed, fan curve *(suppressed on QNCN)*
- **Lighting** — keyboard backlight, Y-logo, IO-port
- **System** — Fn-lock, touchpad, camera, boot logo

---

## 6. Milestones

**M0 — Environment & driver.** *(blocked on you: `sudo` install)*
Install GTK4/libadwaita, load `legion_laptop` on this machine, and capture the
real sysfs tree as a fixture. Everything downstream is written against observed
reality, not assumptions.

**M1 — Read-only dashboard.** App shell, sidebar, dark/light, the card components
and `SensorGrid`. A mock backend so development does not require hardware.
*Done when it reads like the Windows app and shows live values from this laptop.*

**M2 — Writable cards + privilege.** ✅ *done 2026-10-13*
`FeatureCard` now performs writes: switches, dropdowns and spinners are wired to
their capability, the write runs off the main thread (a polkit prompt is a
blocking interaction and would otherwise freeze the window), and its outcome
reports inline under the control — blank while applying, the helper's message on
failure. Suppressed-unavailable cards and the QNCN "fan curve unsupported"
notice were already in place from M1.

The startup banner for "escalation unavailable" and the `ec_readonly` warning
are now in the UI as well: `services::system_warnings` builds the list from the
privilege diagnosis and `Backend::global_warning`, and the window renders each
as an `AdwBanner` above the page stack.

**M3 next.**

### Pages — the reference UI, ported *(done 2026-10-13)*

### What the references actually teach

The LenovoLegionLinux reference teaches the *hardware*: which sysfs and hwmon
attributes exist, which are read-only, where the paths live, and which
capabilities a given BIOS generation has. Every finding in
`docs/hardware-compatibility.md` traces to it.

The Lenovo Legion Toolkit reference teaches something different, and less
obvious: **how to make an unreliable feature set look deliberate.** Three
concrete techniques, all now used here:

1. **A card abstracts the control.** The reference builds every control in the
   app out of one `CardHeaderControl` — title, subtitle, status rows, an
   accessory slot — with a toggle variant and a combo variant. Ported as
   `LltCard` + `LltCardHeader` + `FeatureCard`.
2. **Unsupported means hidden, not disabled.** `AbstractRefreshingControl`
   throws `NotSupportedException` while refreshing and the card disappears.
   Crucially, it *explains why* when it can: "requires the driver" and "your
   hardware lacks this" need different actions from the user.
3. **A metric reads as a fraction of its range, not its magnitude.** In
   `SensorsControlV2`, `UpdateValue` sets `bar.Maximum = max` — the sensor's own
   ceiling. A 40×6 bar then means "how close to the top of this sensor's range"
   at every row, which is why the bar is informative at a glance.

The `power_supply` class is used for everything, since that's the standard Linux
way to read battery data rather than Legion-specific code, so the battery page
works with no driver loaded at all.

### The bug the references taught us to find

`ScrolledWindow::builder()...build()` with no `vexpand(true)` measures to its
natural height. For a page whose contents are 607px tall, that height was 58px —
so the page laid its children out correctly and then clipped all but the first
58px. The symptom was a page that looked blank while its widget tree was
complete and every child reported `visible=true, opacity=1.00`.

Nothing in the reference GUI's own layout hints at this; WPF does not have this
failure mode. It cost most of the time to diagnose, and it is invisible to
`make check`, clippy and the tests. It is also the reason
`Packages/dashboards` look fine while pages look empty: the dashboard's scroll
happens to be sized by its content, and pages did not.

The lesson written down: **a page measuring 0px tall and a page that is
functionally empty are indistinguishable to a widget-tree dump.** Use
`--shot` (below) to look, not `visible()` flags.

### Tier 1 complete — cards, and the data that fills them *(2026-10-13)*

**Card anatomy.** The reference's cards are not a title and a widget. Its
WPF-UI `CardControl` carries an `Icon`, and `CardHeaderControl` carries a
title, a subtitle, **four status rows** (`Info` / `Warning` / `Error` /
`Success`) and an **accessory slot**. Reproducing that is the single biggest
visual change made so far: `LltCard` now takes an icon, up to four status rows,
and puts the control in the header rather than below it. Write outcomes report
through the same rows, so `Error` and `Success` have real users.

Found while doing it: `build_toggle`, `build_enum` and `build_slider` created
their widgets and **never added them to the card**. Every switch, dropdown and
spinner in the app was invisible.

**Generic telemetry** (`src/system.rs`). The reference's sensor card shows
utilization and clocks, not only temperatures. `/proc/stat`, `cpufreq`,
`/proc/meminfo`, `statvfs` and `/proc/loadavg` provide all of it with no Legion
driver, which matters because on this machine `legion_laptop` is unloaded and a
Legion-only grid is mostly dashes. Added: CPU utilization, core clock, memory,
swap, disk usage, disk temperature, battery time remaining, full and design
capacity. The thermal grid went from 2 populated rows to 9.

**Two bugs found by looking at pixels.** `--shot` paid for itself twice over:

- `Page::new` stored its constructor and never ran it. The thermal page had
  *always* been blank; what looked like a working thermal page was the
  dashboard, shown because of the duplicate stack ID fixed earlier. `Page::new`
  now renders on construction.
- Pages were only refreshed by the one-second timer, so a page read as dashes
  until the first tick. Pages are now warmed at startup, as the dashboard
  already was.

**Meter**: the reference uses a `ProgressBar`, not a slider. Ours was a disabled
`GtkScale`, which drew a slider handle and read as an editable control.

**Battery to the reference's card set.** `power_supply` reports only the
*instantaneous* discharge rate, so the reference's minimum and maximum rates are
obtained by tracking: the refresh tick that already runs folds each reading into
an observed range, and resets the on-battery timer when the adapter connects.
Thirteen cards now, against the reference's twelve — the two it has that sysfs
cannot provide on this machine are battery temperature and manufacture/first-use
dates, and those are absent rather than faked.

Two values needed care. An unobserved discharge rate reports "None yet" rather
than `0.0 W`, because zero asserts a measurement that was never taken — and the
`available_capabilities_return_values` test caught exactly that class of
mistake for a different capability ("On Battery Since" returning a dash), which
is now "On adapter".

### Settings page and a persisted store *(2026-10-13)*

Item 8. The reference's eight Settings categories are mostly application
behaviour, and this is the first part of the app that persists anything.

`src/settings.rs` holds a serde-backed `Settings` written to
`$XDG_CONFIG_HOME/legion-toolkit/settings.json`. Three deliberate properties,
each with a test:

- A **missing or corrupt file is not an error** — defaults replace it, because
  refusing to start over a bad preference is a worse failure than losing it.
- **Every field defaults**, so a file predating a field still loads.
- **An unknown field is ignored**, not rejected. `deny_unknown_fields` was the
  first instinct and is wrong: a file written by a *newer* build would fail to
  parse on an older one and silently revert every setting.

Wired and verified: temperature unit (Celsius/Fahrenheit, applied in the card
and sensor-card display layers rather than in the capability, so stored values
stay in the unit the hardware reported), theme, and refresh interval. Verified
end to end by writing a settings file and confirming the UI both honours it
(CPU 196 °F) and reflects it (Theme: Dark, Temperature: Fahrenheit, 5 s).

### `--shot` — offscreen rendering

`LegionToolkit --shot /path.png` renders the window to a PNG and exits, with no
display required, via `gsk::CairoRenderer`. `LEGIONTOOLKIT_PAGE=<id>` captures
a specific section.

**A flaw in this method, worth recording.** `--shot` captures the window, and
a `GtkStack` crossfades between children — so a capture taken mid-transition
paints *both* pages, which looks exactly like a layout bug. It produced a
convincing screenshot of the Settings page with the dashboard's cards
interleaved. Checking the stack's child count (eight, as expected) rather than
trusting the pixels is what caught it. `show_page` now disables the transition,
so captures show the settled state.

Written because every layout claim in this project was previously inference:
about 2,500 lines of GTK had never been rendered, and the usual screenshot tools
do not work under Wayland. It also gives the README something real to show.


The reference's pages were the visible gap: the shell and dashboard existed,
four of five sidebar entries were a placeholder sentence. Now ported, matching
the reference's own structure:

- **Thermal page** — the `SensorsControlV2` look: a wrapping grid of four cards
  (CPU, GPU, Fans, Disk), each with a semibold title and rows of *label, value,
  and a 40×6 accent meter*. The meter's ceiling comes from the sensor's range,
  not the current reading, exactly as the reference's `UpdateValue` sets
  `bar.Maximum = max`.
- **Battery page** — hero (stepped battery icon + large percentage + status),
  then stat cards, following `BatteryPage.xaml`. Values are derived from
  `power_supply`: level, status, health, charge remaining, power draw, voltage,
  cycle count.
- **Power / Lighting / System pages** — the dashboard's capability cards,
  grouped per section.
- **About page** — machine, driver state, backend.

Verified against this machine, where the driver is *not* loaded:

```
cpu_temp             86 C      ← coretemp fallback
disk_temp            39.85 C   ← nvme
battery_capacity     100 %
battery_health       94.74 %   ← derived, energy_full / energy_full_design
battery_cycles       176
battery_voltage      17.46 V
battery_charge_now   75.79 Wh
```

Six new tests cover this: a printable capability report, "an available
capability must return a real value", and range checks on the derived health
and the coretemp fallback.

**Resolved (2026-10-14):** the dashboard duplicates the thermal readings it also
shows as a group, where the reference puts the sensor grid at the top of the
dashboard itself. The decision is **the reference layout**: Home now leads with
the sensor cards (CPU, GPU, Motherboard, Memory, Disk, Battery) and then the
headline controls under titled, two-column sections (Power, Graphics, Lighting).
The firmware power limits and the rest stay on their own pages, so Home is a
dashboard rather than a dump of every capability.

**M3 — Profiles & automation.** Named profiles, AC↔battery switching, the small
trigger/step engine.

**M4 — Lighting & battery pages.** 3-state backlight via `leds`; separately probe
the keyboard's USB HID ID against `4JX/L5P-Keyboard-RGB` for 4-zone RGB. Battery
page with hero percentage and stat cards.

**M5 — Packaging.** RPM spec / COPR for Fedora, desktop entry, polkit policy,
icon, `meson` or `just` build recipe.

---

### The backend was too narrow — see docs/data-sources.md *(2026-10-13)*

A correction with consequences for the whole plan.

`src/paths.rs` resolves capabilities against the sysfs tree the **out-of-tree**
`legion_laptop` module creates, and reports everything absent from it as
*"Not supported by this model"*. On this machine the out-of-tree module is not
loaded, so that tree does not exist — and the application says the hardware
cannot do things it demonstrably can:

- **Power mode** is live at `/sys/firmware/acpi/platform_profile` (currently
  `performance`, with all five choices populated). We look only under
  `<legion>/platform-profile/`.
- **Fan RPM** is live at `lenovo_wmi_other`'s hwmon — `fan1_input` and
  `fan2_input`, both 4000. We look for fans under the legion device only.
- **Every power limit** — PL1 70 W, PL2 125 W, tau 56, cross-loading 65 W, GPU
  cTGP 65 W, PPAB 15 W, and the CPU/GPU temperature limits — is exposed and
  writable through the kernel's generic `firmware-attributes` class, each with
  `min_value`, `max_value` and `scalar_increment`. We label them *"not exposed
  by this model's firmware"*, which is false.
- **The GPU** reports temperature, utilization, VRAM, clocks and power through
  `nvidia-smi`. We never invoke it, so that card is a permanent dash.

The in-tree drivers for all of this ship with the kernel we are already running.
Nothing needs installing.

This reorders the plan. Loading the out-of-tree module is no longer the
unlock — **widening the backend is**, and it comes first. The revised order is in
`docs/data-sources.md`.

## 7. Risks and open questions

**Repo hygiene.** This repository currently tracks 1066 files of the Windows
toolkit as deleted (`git status` lists every `.cs`/`.xaml` as removed), with
`Reference/` untracked on top. A clean starting commit is needed before we add a
new tree, or history will be permanently confusing.

**Keyboard RGB is unverified for this model.** The upstream 4-zone RGB project
(`4JX/L5P-Keyboard-RGB`) documents support for several four-zone Legion keyboards
but does not list the 2025 Legion 5 15IRX10. This laptop's backlight is a
3-state white LED through the `leds` class, not an RGB lamp array. Before
promising RGB, the keyboard's USB HID ID must be probed. **This is a read-only
check and should be done early** — it decides whether M4 is large or small.

**Fan-curve logic is deferred, not abandoned.** When we reach it, we either
reimplement the RPM↔PWM conversion in Rust or bridge to `legion_cli` via
`LllCliBackend`. The `HardwareBackend` trait exists precisely so this decision
can be made later without rewriting the UI.

**Writes have not been tested on this machine.** The module is not loaded yet.
Nothing in this plan has been validated against real EC writes — and EC writes
are the part that can wedge fans at maximum speed until reboot, as documented for
a sibling model. Writes are tested under load, conservatively, one capability at a
time.

**`fan_fullspeed` is dangerous on this generation.** LLL documents that a
full-speed write through the wrong path can wedge the fans at maximum until
reboot on 0x5508-generation hardware. We expose it, but never inside an
automation default.

---

## 8. Next work — the correctness & honesty pass (2026-10-14)

The read-only dashboard, the capability layer, the privilege path, the pages,
the settings store and the presets editor are all real and working
(`cargo test --lib --no-default-features`: 47/47). What is left divides into a
small set of things that are quietly *wrong*, a set of *missing* features the
reference applications expose, and *housekeeping*.

This section records the pass agreed on 2026-10-14, and the backlog behind it.

### 8.1 What is broken today

1. **The toolbar "Refresh now" button is a no-op.** `src/window.rs` iterates
   `dashboard_cards`, a `Vec` that is created empty and never populated; the
   timed loop does the same. Only `Pages::refresh()` does any work.
2. **`compact_mode` is a dead setting.** It is persisted and has a switch, but
   nothing in the design layer reads it.
3. **`cpu_longterm_powerlimit` is a stale duplicate.** It has no resolver arm
   in `paths.rs`, so it is never available, it has no write closure despite
   being a `Slider`, and it sits in the wrong group. The real PL1/PL2 are
   discovered from `firmware-attributes` and shown under *Processors*.
4. **`ioport_light` is resolved but never exposed.** `paths.rs` knows the path;
   no capability uses it.
5. **Docs describe things the code does not do:** the README's tray-icon and
   desktop-notification claim, the Arch `yay -S legiontoolkit` route, a udev
   rule where the repo ships a polkit rule, a `LEGIONTOOLKIT_NO_PASSWORD=1`
   option `install.sh` never implemented, and a GTK 4.12 requirement when
   `Cargo.toml` gates on GTK 4.22 / libadwaita 1.9. `architecture.md` also
   says the polkit policy uses `auth_admin` when the shipped policy uses
   `auth_admin_keep`.
6. **Repo hygiene:** `docs/` and `Reference/` are untracked; `Reference/Vitals`
   and `Reference/LenovoLegionLinux` are nested git repositories.
7. **CI:** the GUI job runs `cargo build --bins`, which shares one feature
   resolution and links GTK into the root helper — the exact thing the
   Makefile avoids; it does not lint the GUI; and it cannot build on
   `ubuntu-latest`, which has no GTK 4.22.

### 8.2 The fixes

- **Refresh button:** drop `dashboard_cards` and wire the button to
  `Pages::refresh()`.
- **`compact_mode`:** apply it when a `FeatureCard` is built (skip the note and
  status rows) and label the setting honestly — it takes effect on the next
  launch, because pages are built once.
- **Delete `cpu_longterm_powerlimit`;** keep the mock's copy, which is only a
  slider example.
- **Expose `ioport_light`** as a Lighting toggle; add it to the helper
  allowlist for defence in depth.
- **Correct the docs** (README, architecture) so they match the code, and
  implement `LEGIONTOOLKIT_NO_PASSWORD=1` in `install.sh` rather than deleting
  the promise — three files already reference it.
- **Track `docs/`; keep `Reference/` out of history** (nested third-party git
  repositories are vendored read-only reference material).
- **CI:** build each binary the way the Makefile does, lint the GUI, and run
  the GUI job in a Fedora container so GTK 4.22 is actually present.

### 8.3 The backlog behind it

Ordered by how much of the application stops being empty.

1. ✅ **Battery Mode as a three-way** *(done 2026-10-14)* — built on the
   kernel's native `power_supply/charge_types` interface, not the driver's
   `rapidcharge`/`conservation_mode` pair. The kernel enforces the
   Fast/Long_Life exclusivity, so no software interlock is needed, and the
   control works with no module loaded. The conservation toggle remains as the
   fallback where `charge_types` is absent.
2. ✅ **Remaining LLL controls** *(done 2026-10-14)* — GPU Working Mode
   (`igpumode`), Windows-key lock, display overdrive, G-Sync, and the two
   Instant Boot toggles, all module-gated and support-probed. **Boot logo is
   deferred**: it is an EFI-variable/ESP feature in LLL, not a sysfs control,
   and needs its own design.
3. ✅ **Automation (M3)** *(done 2026-10-14)* — a pipeline is a trigger plus
   capability-derived steps, applied through the same privileged path as the
   dashboard controls and persisted like presets. Triggers: AC connected, on
   battery, periodic, power-mode-changed, all edge-detected. Steps are chosen
   from the capabilities the machine can actually write, so an unwritable
   control cannot become a step. `src/automation.rs` is pure and unit-tested;
   the window drives it from its one-second tick.
4. ✅ **Background presence** *(done 2026-10-14, partial)* — Start on Login (an
   XDG autostart entry), Minimize on Close (keep running in the background), a
   Quit menu, and notifications via `notify-send`. **Not done: a system-tray
   icon.** There is no StatusNotifierItem, so the README deliberately makes no
   tray claim; minimise-on-close reopens by relaunching the application.
5. ✅ **Packaging (M5)** *(RPM verified 2026-10-14)* — `packaging/rpm` with a
   `make rpm` target that builds and was checked end-to-end on Fedora 44;
   `packaging/debian` and `packaging/aur` delegating to the same `install.sh`.
   `install.sh` gained `DESTDIR` staging. The no-password polkit option exists
   for a manual install. Submitting to COPR is an operational step left to the
   maintainer.
6. ▶️ **Hardware validation** — the module **loads** on the reference laptop
   (config `QNCN`, EC `0x5508`) and the capability report went from 38 to 47
   readings *(2026-10-14)*. Two bugs were found and fixed by loading it: the
   touchpad was wrongly attributed to ideapad, and `issupport…` probes were
   read as `== 1` instead of non-zero. **Writes are still untested** — they need
   root and touch the EC; the procedure is in `docs/hardware-validation.md`.
