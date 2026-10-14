# Data sources — where every reading actually comes from

This is the document that matters most in this repository. It exists because we
spent a long time building a backend that reads **one** source — the sysfs tree
the out-of-tree `legion_laptop` module creates — and reported everything else as
*"Not supported by this model"*.

That was wrong. On this machine, with the out-of-tree module **not loaded**, the
kernel's own in-tree drivers and standard Linux interfaces already expose
almost everything the application wants to show.

## The correction

Verified on 2026-10-13 on a Legion 5 15IRX10 (83LY, QNCN38WW), `legion_laptop`
**not** loaded:

| Reading | We showed | Actually available from | Writable |
|---|---|---|---|
| Power mode | `—` unavailable | `/sys/firmware/acpi/platform_profile` → `performance` | ✅ `0644` |
| Power mode choices | `—` | `platform_profile_choices` → low-power, balanced, performance, max-power, custom | — |
| Fan 1 RPM | `—` | `lenovo_wmi_other` hwmon `fan1_input` → 4000 | read |
| Fan 2 RPM | `—` | `lenovo_wmi_other` hwmon `fan2_input` → 4000 | read |
| Fan target / min / max | not exposed | same hwmon device | read |
| GPU temperature | `—` unavailable | `nvidia-smi` → 63 °C | read |
| GPU utilization | not exposed | `nvidia-smi` → 1 % | read |
| GPU VRAM | not exposed | `nvidia-smi` → 4242 / 8151 MiB | read |
| GPU SM / graphics clock | not exposed | `nvidia-smi` → 690 / 690 MHz | read |
| GPU power draw | not exposed | `nvidia-smi` → 16.26 W | read |
| **CPU PL1 (long-term)** | *"not exposed by this model's firmware"* | `firmware-attributes/ppt_pl1_spl` → 70 W | ✅ `0644` |
| **CPU PL2 (short-term)** | *"not exposed"* | `ppt_pl2_sppt` → 125 W | ✅ `0644` |
| CPU PL1 tau | not exposed | `ppt_pl1_tau` → 56 | ✅ `0644` |
| CPU cross-loading | not exposed | `ppt_cpu_cl` → 65 W | ✅ `0644` |
| **GPU cTGP** | not exposed | `gpu_nv_ctgp` → 65 W | ✅ `0644` |
| **GPU PPAB / dynamic boost** | not exposed | `gpu_nv_ppab` → 15 W | ✅ `0644` |
| GPU load offset | not exposed | `gpu_nv_ac_offset` → 55 | ✅ `0644` |
| GPU->CPU boost | not exposed | `gpu_nv_cpu_boost` → 10 | ✅ `0644` |
| CPU temperature limit | not exposed | `cpu_temp` → 97 °C | ✅ `0644` |
| GPU temperature limit | not exposed | `gpu_temp` → 87 °C | ✅ `0644` |

Every one of those shows as unsupported in the application today.

## Why this happened

`src/paths.rs` resolves `power_profile` by reading
`<legion>/platform-profile/*/profile` — a path that exists **only** when the
out-of-tree module is loaded. The standard path has been
`/sys/firmware/acpi/platform_profile` for years, and the kernel's own
`lenovo_wmi_*` drivers populate it.

The same mistake in three more places: fans are read only from the legion
hwmon device, the GPU is not read at all, and the firmware power limits are not
read at all.

**The lesson:** a capability is "unsupported" only after every source for it has
been tried. Our `unavailable_reason` was asserting "this model cannot" when the
truth was "this backend does not look".

## The sources, ranked

When several sources can serve one reading, prefer in this order. Earlier
sources are more specific and more accurate; later ones are universal.

1. **The out-of-tree `legion_laptop` module** — richest, model-specific, and
   what the reference implementation targets. Not required for most readings.
2. **In-tree kernel drivers** — `lenovo_wmi_*` (hwmon sensors, firmware
   attributes), `ideapad_acpi` (conservation, Fn-lock, touchpad, camera, USB),
   `acpi` platform profile. Available on any current kernel.
3. **Standard Linux interfaces** — `/proc/stat`, `/proc/meminfo`,
   `/proc/diskstats`, `cpufreq`, `statvfs`, `/sys/class/hwmon/*`,
   `/sys/class/power_supply/*` (including `charge_types`, the native three-way
   battery charge mode).
4. **Vendor tools** — `nvidia-smi` for NVIDIA (the only practical route to GPU
   telemetry), `rocm-smi` for AMD.

Sources 2 and 3 need no installation and no out-of-tree module, so our
capability detection should treat them as the baseline, not the fallback.

## Where the firmware attributes live

```
/sys/class/firmware-attributes/lenovo-wmi-other-0/attributes/<name>/
├── current_value      # read and write
├── default_value
├── min_value          # exactly what Capability::limits needs
├── max_value
├── scalar_increment   # exactly what Capability::limits::step needs
├── display_name
└── type
```

This is a **generic kernel interface** — `firmware-attributes` is a kernel
subsystem, not something Lenovo-specific in shape. The per-attribute
`min_value` / `max_value` / `scalar_increment` files are precisely the
`Limits { min, max, step }` a slider card needs, which means the power-limit
controls can be generated from directory listings rather than hardcoded per
model.

## What Vitals taught us

`Reference/Vitals` (a GNOME Shell extension, GPL-2.0 — read for method, not
copied) reads 26 paths and 2 subprocesses to report the same categories we do:

- `/proc/stat`, `/proc/cpuinfo`, `cpufreq/scaling_cur_freq` — CPU
- `/proc/meminfo`, `/proc/diskstats`, `/proc/mounts`, `statvfs` — memory and disk
- `/sys/class/hwmon/*` — **every** hwmon device, for temperature *and fan*
- `/sys/class/net/*/statistics/*` — network throughput
- `/sys/class/power_supply/*` — battery
- `nvidia-smi` — GPU on NVIDIA
- `/sys/class/drm/card*/device/mem_info_vram_used` — GPU on AMD

The lesson is narrow and useful: it sweeps `hwmon` globally rather than by
device name. Our fan reading failed for exactly that reason — we looked for
fans under the legion device and nowhere else.

## Implemented *(2026-10-13)*

All four sources are now read, and the capability report on this machine went
from **14 available readings to 36** — with nothing installed:

| Change | Result |
|---|---|
| Read `platform_profile` from the standard ACPI path | **Power Mode**: `performance`, five choices, writable |
| Sweep `/sys/class/hwmon/*` for fans | **CPU Fan 4200 rpm, GPU Fan 4200 rpm** |
| GPU via `nvidia-smi`, background poller | **54 °C, 202 MHz, 8.5 W, 3804 MiB** |
| Generate controls from `firmware-attributes` | **10 power limits**, 6 writable |

### What was actually wrong

Three separate bugs, each of which alone would have hidden the data:

1. **The key never matched the path.** The capability is keyed `powermode`; the
   path resolver matched `platform_profile`. They were never joined up, so the
   flagship control read as unavailable even when the file was present.
2. **Fans were looked for in one device.** Only the legion hwmon was consulted,
   where a global sweep finds them in `lenovo_wmi_other`.
3. **Availability was decided from an unprimed cache.** The GPU cards asked the
   poller whether a GPU existed before its first poll had returned, and
   therefore reported a present GPU as absent. The poller now primes
   synchronously; a test asserts it, because the failure is invisible — it looks
   exactly like a machine without the hardware.

### The security model changed with it

A fixed allowlist of key *names* cannot cover the firmware attributes: the class
exposes whatever the BIOS has, under names that differ per model. The helper
therefore validates the **resolved destination** against a set of allowed roots,
with the name allowlist kept as defence in depth. Attribute names are validated
as plain identifiers before being used as a path component.

The obvious version of that check — canonicalising both ends — is wrong, and a
test caught it: sysfs presents one device at two paths
(`/sys/class/firmware-attributes/…` resolves to `/sys/devices/virtual/…`), so
canonicalising one end and not the other rejects a legitimate path. The check is
lexical now, with an explicit no-traversal rule and a symlink rejection.

## Next

Ordered by how much of the application stops being empty.

1. **Read `platform_profile` from the standard ACPI path.** One line in
   `paths.rs`. Turns the flagship control from unavailable to live on every
   modern kernel.
2. **Sweep `/sys/class/hwmon/*` for fans** rather than the legion device only.
   Live fan RPM with no driver.
3. **GPU telemetry from `nvidia-smi`**, with the AMD sysfs path as alternate.
   Fills a card that is currently a permanent dash.
4. **Power limits from `firmware-attributes`**, generating sliders from
   `min_value` / `max_value` / `scalar_increment`. Writable through the existing
   pkexec helper — the helper's allowlist needs the new keys.
5. Then the previously listed work: Battery Mode as a three-way, the automation
   page, the keyboard page.

Items 1–4 together move the application from "most cards say requires the
driver" to "most cards are live", without installing anything — which is what
was asked for several turns ago and what the design should have done from the
start.


---

# What the Windows toolkit teaches

Read properly rather than skimmed, `Reference/LenovoLegionToolkit` answers three
questions this project kept getting wrong.

## 1. Its icon vocabulary

`DashboardItemExtensions.GetIcon` maps all 23 dashboard items to `SymbolRegular`
glyphs — `Gauge24` for power mode, `BatteryCharge24` for battery mode,
`WeatherMoon24` for overnight charging, `DeveloperBoard24` for the discrete GPU.
120 distinct glyphs are used across the application.

Those glyphs come from `wpfui`, which is **MIT** — the one part of the Windows
toolkit we could lift freely. We use the system icon theme instead, which is the
native choice on Linux.

**And that is where we had a real bug.** Seven of the icon names in this
application did not exist in the Adwaita theme at all:

```
air-conditioner-symbolic   cpu-symbolic        emblem-ok-symbolic
fan-symbolic               memory-symbolic     motherboard-symbolic
usb-symbolic
```

GTK draws a placeholder rather than failing, so nothing caught it and several
cards were showing a generic glyph. The theme has **701** symbolic icons and no
fan, cpu, chip, memory, thermometer, gauge or speedometer among them — so the
equivalents have to be semantic rather than literal:

| Concept | Reference | Ours (what exists) |
|---|---|---|
| Fan | `Fan24` | `weather-windy-symbolic` |
| CPU | `DeveloperBoard24` | `org.gnome.Settings-device-diagnostics-symbolic` |
| Memory | `Memory16` | `media-flash-symbolic` |
| Chipset | — | `application-x-firmware-symbolic` |
| USB | `UsbStick24` | `drive-harddisk-usb-symbolic` |
| Cooling | — | `weather-windy-symbolic` |
| Success | `Checkmark24` | `object-select-symbolic` |

`src/icons.rs` now enumerates the installed theme, and a test asserts every icon
a capability declares can actually be drawn.

## 2. Custom mode is a preset of capability values

`GodModePreset` is a struct with one optional field per power tunable: PL1,
PL2, PL2 tau, cross-loading, the CPU and GPU temperature caps, GPU TGP, PPAB,
dynamic boost, the AC offset, fan behaviour.

That is almost a one-to-one map of the firmware attributes we now discover. So
a preset here is a **named map of capability key to value**, applying one writes
each entry through the same privilege path a slider uses, and capturing one
reads the current values back. The reference needs 51 hand-written step classes
to do the equivalent; this needs one, because the capability list does the work.

The one reference field we cannot express is `FanTableInfo` — a fan curve —
because this model's firmware has none.

## 3. Refresh must not trigger a write

The reference guards every control with an `IsRefreshing` field, and the reason
is now demonstrated here. Setting a `SpinButton` fires the same `changed` signal
a user gesture does, so a refresh that writes 55 into a spin button looks like
an edit. Our first guard compared the widget's value against the capability's
and skipped if equal — which is unreliable for floats: a spin button rounding
55.0 to 54.999 read as a change and **issued a write on every refresh**. It
showed up as a power limit drifting 55 → 54 with an error row underneath.

`FeatureCard` now carries an `updating` flag held for the duration of a refresh,
cleared by a drop guard. That is the reference's pattern, and it is the right
one — the value comparison was never going to be reliable.

## Taken from the toolkit, and how

- **Battery Mode as a three-way.** Done, but *not* through the driver's
  `rapidcharge`/`conservation_mode` pair. Newer kernels expose the battery's
  accepted charge types directly as `charge_types` on the `power_supply` class —
  `Fast [Standard] Long_Life`, with the current one bracketed — and the kernel
  enforces the Fast/Long_Life exclusivity itself. `battery_mode` is built from
  that attribute, so it needs no Legion driver and no software interlock, and it
  works on this machine today with the module unloaded. The `conservation_mode`
  toggle survives only as the fallback where `charge_types` is absent.
- **GPU Working Mode** (`igpumode`): a three-way enum
  (Hybrid / Integrated Only / Hybrid Auto), the values the Windows toolkit's
  `IGPUModeState` uses for the same GameZone method. Module-gated, and gated on
  `issupportigpumode` so a model whose firmware does not back it says so.
- **Also taken**, all module-gated and support-probed: `winkey`, `overdrive`,
  `gsync` — the attribute LLL itself drives as "Hybrid Mode (sometimes also
  G-Sync)" — and the two `instant_boot_ac` / `instant_boot_usb_pd` toggles.

## Still to take from the toolkit

- **Automation** (33 triggers, 51 steps; ours derives steps from capabilities).
- **The keyboard backlight page** with its effect editor.
- **Boot logo.** Deliberately not attempted yet: it is not a sysfs control at
  all. LLL rewrites the `LBLDESP`/`LBLDVC` EFI variables (with a CRC32 and a
  fixed-size image) and copies the image into the ESP. That is a boot-affecting
  feature with a different risk profile from a hardware toggle, and it deserves
  its own design rather than a rushed one.


---

# Why a password is needed, and how to stop it

Changing the power mode prompts for the administrator password. That is not a
bug in the application — it is the kernel's permissions showing through:

```
$ stat -c '%a %U' /sys/firmware/acpi/platform_profile
644 root
```

Every writable hardware file is mode `0644` owned by root, so the write needs
root, so `pkexec` authenticates. There is no udev rule in LenovoLegionLinux that
would grant a group access, and no polkit action that grants the right to write
sysfs — only the right to *run a binary*.

Three options, in increasing convenience and decreasing strictness:

1. **Leave it.** `auth_admin_keep` means one prompt per session, not per write.
2. **`sudo make install LEGIONTOOLKIT_NO_PASSWORD=1`** installs
   `packaging/49-legiontoolkit-no-password.rules`, which lets an `active` and
   `local` session use the helper without authenticating. The helper's own
   allowlist and path-root checks still apply, so this grants nothing beyond
   the capabilities this application already exposes. This is what Tuxedo and
   the Windows toolkit effectively do.
3. **A udev rule** granting group write on the specific attributes. More work,
   and per-attribute: the class and hwmon files are created by three different
   drivers.

Option 2 is what `install.sh` supports because it is one file and is trivially
reversible (`rm /etc/polkit-1/rules.d/49-legiontoolkit.rules`).

# Where "Requires the legion_laptop driver" comes from

`src/paths.rs` resolves each capability to a file on disk. When none of the
candidate paths exists, `reason_for` reports the most useful explanation it can:

- **"Requires the legion_laptop driver"** — the file only exists once the
  out-of-tree module is loaded. Six capabilities are in this group:
  `fan_fullspeed`, `fancurve`, `ic_temp`, `kbd_backlight`, `ylogo_light`,
  `touchpad`.
- **"Not supported by this model"** — the module is loaded and the file still
  is not there, which means the hardware lacks it.

Thirteen of fifty-four capabilities are unavailable on this machine, and the
first version of the application drew all thirteen as cards. That was wrong, and
it contradicted the design note this project had already written — the reference
*hides* unsupported controls, it does not list them. Thirteen dead cards bury
the thirty-eight that work.

Pages now show only what this machine has, with one line at the foot naming how
many are missing and why, and the About page carries the full report. The
information is preserved; the repetition is gone.
