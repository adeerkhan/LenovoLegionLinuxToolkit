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

**M2 — Writable cards + privilege.** `AbstractFeatureCard`, the pkexec helper,
suppressed-unavailable cards, and the QNCN "fan curve unsupported" notice.

**M3 — Profiles & automation.** Named profiles, AC↔battery switching, the small
trigger/step engine.

**M4 — Lighting & battery pages.** 3-state backlight via `leds`; separately probe
the keyboard's USB HID ID against `4JX/L5P-Keyboard-RGB` for 4-zone RGB. Battery
page with hero percentage and stat cards.

**M5 — Packaging.** RPM spec / COPR for Fedora, desktop entry, polkit policy,
icon, `meson` or `just` build recipe.

---

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
