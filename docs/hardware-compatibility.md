# Hardware compatibility — verified capability matrix

This is the factual grounding for the whole project. Everything here was verified
on 2026-10-13 against the running system and against the vendored source of
LenovoLegionLinux (LLL). No capability in this table is assumed.

## The machine

```
$ cat /sys/devices/virtual/dmi/id/product_name
Legion 5 15IRX10
$ cat /sys/devices/virtual/dmi/id/product_version
83LY
$ cat /sys/devices/virtual/dmi/id/bios_version
QNCN38WW
$ uname -r
7.2.9-200.fc44.x86_64
```

Identified in the driver's DMI allowlist:

```c
// Reference/LenovoLegionLinux/kernel_module/legion-laptop.c:2897-2905
{
    // e.g. Legion 5 15IRX10 (83LY)
    .ident = "QNCN",
    .matches = {
        DMI_MATCH(DMI_SYS_VENDOR, "LENOVO"),
        DMI_MATCH(DMI_BIOS_VERSION, "QNCN"),
    },
    .driver_data = (void *)&model_qncn
},
```

Config, verified at `kernel_module/legion-laptop.c:1102-1122`:

```c
static const struct model_config model_qncn = {
    .registers                = &ec_register_offsets_v0,
    .check_embedded_controller_id = true,
    .embedded_controller_id   = 0x5508,
    .has_custom_powermode     = true,
    .access_method_powermode  = ACCESS_METHOD_WMI,
    .access_method_keyboard   = ACCESS_METHOD_WMI,
    .access_method_temperature= ACCESS_METHOD_WMI3,
    .access_method_fanspeed   = ACCESS_METHOD_WMI3,
    .access_method_fancurve   = ACCESS_METHOD_NO_ACCESS,   /* <-- */
    .access_method_fanfullspeed = ACCESS_METHOD_WMI,
    .skip_ic_temp             = true,
    .skip_fan_maxspeed        = true,
    .skip_lockfancontroller   = true,
    .wmi_fancurve_speed_only  = true,
    /* no .access_method_powerlimits  ->  NO_ACCESS              <-- */
    /* no .acpi_paths            ->  no rapidcharge/conservation here  */
};
```

**The two decisive facts:**

1. `access_method_fancurve = ACCESS_METHOD_NO_ACCESS` — **this laptop has no fan
   curve.** Not "through a different interface": none. The whole hwmon
   `pwmY_auto_point*` family is absent.
2. `access_method_powerlimits` is unset, so it defaults to `NO_ACCESS` — **no CPU
   or GPU power limits.**

Anything the UI shows for these two must be suppressed.

## Capability matrix

| Capability | Kernel sysfs / path | Read | Write | This laptop |
|---|---|---|---|---|
| Power mode / platform profile | `<legion>/platform-profile/*/profile` + `choices` | ✅ | ✅ | **YES** incl. `custom` |
| Max fan speed | `<legion>/fan_fullspeed` | ✅ | ✅ | **YES** |
| Full-speed flag | `<legion>/fan_fullspeed` | ✅ | ✅ | **YES** (WMI) |
| CPU temperature | hwmon `temp1_input` (milli-°C) | ✅ | – | **YES** |
| GPU temperature | hwmon `temp2_input` (milli-°C) | ✅ | – | **YES** |
| IC/chipset temperature | hwmon `temp3_input` | ✅ | – | **YES** (label "IC Temperature") |
| Fan 1–n RPM | hwmon `fanN_input` | ✅ | – | **YES** |
| Fan target RPM | hwmon `fanN_target` | ✅ | – | model-dependent |
| Fan curve points | hwmon `pwmN_auto_point{M=1..10}_{pwm,temp,temp_hyst,accel,decel}` | ✅ | ✅ | **NO** |
| `auto_points_size` | hwmon | ✅ | | **NO** |
| Fan max-speed query | `<legion>/fan_maxspeed` | ✅ | ✅ | **NO** (`skip_fan_maxspeed`) |
| Fan controller lock | `<legion>/lockfancontroller` | ✅ | ✅ | **NO** (`skip_lockfancontroller`) |
| Fan unlock | `<legion>/fan_unlock` | ✅ | ✅ | model-dependent |
| Battery conservation | **ideapad** `VPC2004:00/conservation_mode` | ✅ | ✅ | **YES** |
| Battery charge mode (normal / conservation / rapid) | **power_supply** `charge_types` (`Fast [Standard] Long_Life`) | ✅ | ✅ | **YES**, native |
| Rapid charge | `<legion>/rapidcharge` (ACPI) | ✅ | ✅ | model-dependent |
| Fn-lock | **ideapad** `VPC2004:00/fn_lock` | ✅ | ✅ | **YES** |
| Touchpad | **legion** `<legion>/touchpad` (WMI); harmless `ideapad` copy where present | ✅ | ✅ | **YES** via the module |
| Camera power | **ideapad** `VPC2004:00/camera_power` | ✅ | ✅ | **YES** |
| Always-on USB charging | **ideapad** `VPC2004:00/usb_charging` | ✅ | ✅ | **YES** |
| Fan mode (ideapad) | **ideapad** `VPC2004:00/fan_mode` | ✅ | – | present (value `0`) |
| Keyboard backlight | `leds` `platform::kbd_backlight[_1]` (max_brightness 2) | ✅ | ✅ | **YES**, 3-state |
| Y-logo LED | `/sys/class/leds/platform::ylogo/brightness` | ✅ | ✅ | **YES** |
| IO-port LED | `/sys/class/leds/platform::ioport/brightness` | ✅ | ✅ | **YES** |
| Overdrive | `<legion>/overdrive` | ✅ | ✅ | model-dependent |
| G-Sync | `<legion>/gsync` | ✅ | ✅ (reboot) | model-dependent |
| GPU working mode | `<legion>/igpumode` (+ `issupportigpumode`) | ✅ | ✅ (reboot) | model-dependent |
| Windows-key lock | `<legion>/winkey` | ✅ | ✅ | model-dependent |
| Instant Boot (AC / USB-PD) | `<legion>/instant_boot_ac`, `<legion>/instant_boot_usb_pd` | ✅ | ✅ | model-dependent |
| CPU/GPU power limits | `cpu_{short,long,peak,apu_sppt,cross_loading}_powerlimit` etc. | ✅ | ✅ | **NO** |
| CPU/GPU overclock | `<legion>/cpu_oc`, `gpu_oc` | ✅ | ✅ | **NO** |
| Battery status | `/sys/class/power_supply/BAT0/{status,capacity,...}` | ✅ | – | **YES** |
| AC adapter | `/sys/class/power_supply/ADP0/online` | ✅ | – | **YES** |
| USB-C PD sources | `/sys/class/power_supply/ucsi-source-psy-USBC000:00{1,2}` | ✅ | – | present |

### Verified with the module loaded (2026-10-14)

`legion_laptop` was built, loaded (`sudo make reloadmodule`) and the capability
report re-read on the reference laptop. It went from **38 to 47** available
readings. Newly live: `fan_fullspeed`, `kbd_backlight`, `ylogo_light`,
`ioport_light`, `touchpad`, `winkey`, `igpumode`, `gsync`, and `ic_temp`.
The driver bounded driver `DMI_PRODUCT_NAME: 83LY`, took config `QNCN`, read EC
`0x5508`, and initialised the keyboard / Y-logo / IO-port LED drivers.

Two corrections this produced:

- **Touchpad is module-gated here, not ideapad.** The `ideapad_acpi` directory
  has no `touchpad` file on this machine; the control comes from the module's
  WMI attribute. It read as "not supported by this model" — wrongly — until the
  module was loaded.
- **`issupport…` probes return non-zero, not `1`.** `issupportigpumode` reads
  `3` and `issupportgsync` reads `2` here, so testing `== 1` hid both controls.
  The check is `> 0`, matching the Windows reference's `IsSupportedAsync`.

Still absent on this unit, and correctly hidden: `overdrive`,
`instant_boot_ac`, `instant_boot_usb_pd` (the module creates none of those
attributes here) and `fancurve` (no fan curve on QNCN). `ppt_pl1_tau`,
`gpu_nv_ctgp`, `gpu_nv_ppab` and `gpu_nv_cpu_boost` report `min == max == 0` in
performance mode, so they appear only once custom mode accepts writes.

### nVidia GPU present?

`/sys/bus/pci/devices/0000:01:00.0/power/runtime_status` tells us whether the
dGPU is awake. `lsmod` confirms `nvidia_modeset` is loaded. Useful for the GPU
sensor card, which should hide itself when only the iGPU exists.

## What works without the kernel module

Confirmed live today, `legion_laptop` **not** loaded:

```
/sys/bus/platform/drivers/ideapad_acpi/VPC2004:00/
    camera_power  conservation_mode  fan_mode  fn_lock  input  leds  usb_charging
/sys/class/power_supply/{ADP0,BAT0,ucsi-source-psy-USBC000:001,ucsi-source-psy-USBC000:002}
/sys/class/hwmon/
    hwmon0 ADP0            hwmon1 BAT0           hwmon2 nvme
    hwmon3 ucsi_source_psy_usbc000:001
    hwmon4 ucsi_source_psy_usbc000:002
    hwmon5 lenovo_wmi_other   hwmon6 coretemp
/sys/class/leds/   # no keyboard backlight device yet
```

So M1 (read-only dashboard) can begin against live data **before** the driver is
loaded — a real advantage that is easy to miss.

## The path that the backend must resolve

This is the single most likely source of "it doesn't work on my machine" bugs.
LLL resolves the platform path by kernel version
(`Reference/LenovoLegionLinux/python/legion_linux/legion_linux/legion.py:31-38`):

```python
LEGION_SYS_BASEPATH = "/sys/module/legion_laptop/drivers/platform:legion/legion"   # kernel >= 7.0
                    or "/sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00"
```

We are on 7.2.9, so the first branch applies — but a hardcoded path is wrong for
anyone on 6.x. Resolve it by kernel version, then fall back to the other.

hwmon is discovered by glob, not by name:
`/sys/module/legion_laptop/drivers/platform:legion/legion/hwmon/hwmon*`

And remember that **several controls live under `ideapad_acpi`, not legion**.
Probing only the legion tree will report conservation mode, Fn-lock, touchpad,
camera and USB charging as "unsupported" on a machine where they work fine.

## The permission model

- Every writable sysfs/hwmon attribute is mode `0644`. **Reads are unprivileged;
  writes require root.**
- LLL ships **no udev rule at all** (verified: zero matches for `udev` in the
  whole tree).
- Its polkit actions grant the right to run a binary as root, nothing more. So
  "write this sysfs file" can only be done by (a) running the GUI as root — which
  breaks tray icons and notifications — or (b) a pkexec helper binary.
- `/sys/module/legion_laptop/parameters/ec_readonly` disables all EC writes
  kernel-wide. The UI must surface this.

## Keyboard RGB — corrected 2026-10-14: it *is* 24-zone RGB

**This section's earlier conclusion was wrong.** The keyboard is not a 3-state
white backlight. `048d:c195` is a **Lenovo Legion Gen10 24-zone RGB** keyboard,
and its colour is driven over **USB**, not the WMI brightness path — the kernel
module's own source says "RGB keyboard is controlled over USB", and
[keyRGB](https://github.com/Rainexn0b/keyRGB)'s documentation lists
`048d:c195` (24-zone) and `048d:c197` (per-key + chassis) explicitly.

The mistake: `platform::kbd_backlight` (`max_brightness = 2`) is only the WMI
on/off/brightness control that the module exposes, and it was read as "the
keyboard is 3-state". It is the *brightness* that is 3-state, not the colour.

**What this means for us:** keyRGB already implements the ITE 24-zone
controller, and two programs writing one keyboard interfere (keyRGB warns about
this). So this application keeps the WMI on/off/brightness and **delegates
colour and effects to keyRGB**, offering an *Open keyRGB* action on the Lighting
page. `src/services/keyrgb.rs` detects the launcher; `system::lenovo_rgb_keyboard`
detects the device. Reimplementing the 24-zone protocol in Rust is a possible
later step but is deliberately not attempted while keyRGB exists.

### The earlier (incorrect) probe, kept for the record

`model_qncn` uses `access_method_keyboard = ACCESS_METHOD_WMI`, and the kernel
module exposes this as an `leds` class device with `max_brightness = 2` — i.e. a
**3-state (off / low / high) white backlight**, not an RGB lamp array
(`kernel_module/legion-laptop.c:10211-10213`).

Four-zone RGB would require the third-party `4JX/L5P-Keyboard-RGB` project over
USB HID. Its documented compatibility list does **not** include the Legion 5
15IRX10.

**Open action:** probe the keyboard's USB HID ID
(`lsusb`, `/sys/bus/hid/devices/`) and compare against the project's supported
list before planning any RGB work.

### Probe result (2026-10-13) — RGB is NOT available on this machine

The upstream project `4JX/L5P-Keyboard-RGB` publishes an explicit table of
supported USB product IDs (vendor is always `048d`, Integrated Technology
Express):

| Year | Pro | Regular + Slim | LOQ |
|------|-----|----------------|-----|
| 2024 | `c995` | `c994` | `c993` |
| 2023 | `c985` | `c984` | `c983` |
| 2022 | `c975` | — | — |
| 2021 | `c965` | — | — |
| 2020 | `c955` | — | — |

Plus Ideapad Gaming `c963` (2021) and `c973` (2022).

**This machine's keyboard is `048d:c195`, which is not in that table** — `c195`
is not in the `c955`/`c965`/`c975`/`c985`/`c995` family. The project also
states explicitly, under "How about X model":

> **Any variant with a white backlight:** Haven't figured out how to talk to
> this one yet, but given the limited number of states (off, low, high) there's
> not many effects I'd be able to add anyways.

That is this machine. Three independent confirmations:

1. The kernel module exposes the backlight as a `leds` class device with
   `max_brightness = 2` — a 3-state white backlight.
2. `048d:c195` is outside the upstream project's supported ID set.
3. The upstream project states white-backlight variants are unsupported.

The full hardware detail, for the record:

```
$ lsusb
Bus 001 Device 003: ID 048d:c195 Integrated Technology Express, Inc. ITE Device(8258)
Bus 001 Device 004: ID 04f2:b83e Chicony Electronics Co., Ltd. Integrated Camera
Bus 001 Device 005: ID 0bda:d922 Realtek Semiconductor Corp. Bluetooth Radio
Bus 001 Device 006: ID 046d:c092 Logitech, Inc. G102/G203 LIGHTSYNC Gaming Mouse

$ lsusb -v -d 048d:c195   (USB 1-6, bcdDevice 0x18)
      bInterfaceClass         3 Human Interface Device
      bInterfaceSubClass      1 Boot Interface Subclass
      bInterfaceProtocol     1 Keyboard      <-- interface 0, boot keyboard
      bInterfaceClass         3 Human Interface Device
      bInterfaceSubClass     0
      bInterfaceProtocol     0              <-- interface 1, vendor
```

`/dev/hidraw2` (interface 0) is 231 bytes of descriptor and carries a
vendor-defined usage page (`0xFF89`) with a single 16-byte **Feature** report
(Report ID `0x5A`) — the shape a firmware interface would take, but on a
boot-keyboard-class device, which is exactly where an ITE keyboard's own HID
descriptor puts it. `/dev/hidraw3` (interface 1) is 327 bytes with 38 usages.

**Conclusion: the supported path on this machine is the 3-state `leds`
backlight driven by `legion_laptop`.** Per-key RGB requires a keyboard whose
product ID is in the upstream table.

Because this is the kind of claim that is worth verifying on the hardware
rather than on documentation, `src/bin/legiontoolkit-keyboard-probe.rs` exists.
It issues a **read-only** `HIDIOCGFEATURE` request against the vendor page and
reports what comes back, so the question can be settled empirically without
writing anything to the keyboard.

## Known upstream quirks that affect this hardware

From LLL's own docs (these are not our bugs, but users will report them as ours):

- **Fan curve size varies by model *and power mode***; a point is disabled by
  setting its temperature limits to 127. (Inapplicable here — no curve at all.)
- **A fan-full-speed write through the wrong path can wedge the fans at maximum
  speed until reboot** on this 0x5508 EC generation. Never put it in an
  automation default.
- `wmi_fancurve_speed_only = true` means WMI3 fan tables transmit speed only;
  temperatures, fan-2 speed and acceleration/decelation read back as 0.
- Enabling battery conservation silently clears rapid charge, and vice versa —
   the two controls must be mutually exclusive in the UI.

## Reported machine: Legion 7 16IAX7 (82TD, BIOS K1CN48WW) — 2026-10-25

A compatibility report against toolkit `0.1.0` (built from `74ef4913`), Fedora 44,
kernel 7.2.11, KDE Plasma 6.8, NVIDIA 615 open, BIOS GPU mode Hybrid, Secure Boot
on. Driver: LLL main plus the pending K1CN PR (stock main reads 0 RPM / 0 °C on
this BIOS — the PR adds/fixes the sensor readouts). Every item below was
cross-checked against the driver source before it was acted on.

The DMI entry (`kernel_module/legion-laptop.c`, `.ident = "K1CN"`) maps to:

```c
static const struct model_config model_k1cn = {
    .embedded_controller_id     = 0x5263,
    .has_minifancurve           = false,
    .has_custom_powermode       = true,     /* custom platform profile exists */
    .access_method_powermode    = ACCESS_METHOD_WMI,
    .access_method_keyboard     = ACCESS_METHOD_WMI,
    .access_method_fanspeed     = ACCESS_METHOD_WMI3,
    .access_method_temperature  = ACCESS_METHOD_WMI3,
    .access_method_fancurve     = ACCESS_METHOD_WMI3,  /* <-- a curve exists */
    .access_method_fanfullspeed = ACCESS_METHOD_WMI,
    .acpi_check_dev             = true,
};
```

**Decisive differences from the QNCN reference machine:** the fan curve *is*
exposed (`ACCESS_METHOD_WMI3`, the hwmon `pwm*_auto_point*` family), and the
keyboard is per-key RGB (ITE `048d:c978`) — the driver's
`legion_kbd_bl_init()` aborts LED registration when the initial WMI brightness
read fails, so no `platform::kbd_backlight` appears even with the module
loaded. The About-page reasons for both cases were corrected accordingly
(2026-10-25).

**Reported working:** all sensor cards (values match `sensors`/`nvidia-smi`/
`free`/`df`), power modes + Fn+Q sync, maximum fan speed, battery modes, the
whole Battery page vs `upower`, IO-port light, touchpad, Windows key, display
overdrive, Fn Lock *read*. **Correctly unavailable:** chipset temp, aux/4th fan,
Y-logo light (RGB-controller-driven), Instant Boot, power-limit sliders, CPU
package power, charge limit, zone RGB keyboard.

**Reported broken, with the fixes shipped 2026-10-25:**

1. *Fn Lock / Camera / Always-On USB writes refused* — the helper's writable
   root was spelled `/sys/bus/platform/drivers/ideapad`, which matches neither
   `ideapad_acpi` nor `ideapad_laptop` component-wise (`Path::starts_with`
   compares whole components). Affects every machine, including the QNCN
   reference. Roots corrected in `src/policy.rs`; the drift test now checks the
   resolved path, not just the key name.
2. *Custom power mode EINVAL* — kernel ≥ 6.14 rejects `custom` on the legacy
   `/sys/firmware/acpi/platform_profile`; it must go to the class device
   (`/sys/class/platform-profile/platform-profile-N/profile`) of a handler
   whose `choices` include it (on this machine, lenovo-wmi-gamezone / the
   legion module's own "lenovo-legion" handler). `resolve_target` is now
   value-aware.
3. *G-Sync toggle snapped back with no helper call* — a click landing inside
   the 1-second refresh window was swallowed by the "updating" guard with no
   write and no feedback. The guard now distinguishes programmatic echoes from
   user gestures by value, failed writes revert the control immediately, and
   every failed helper call is logged.
4. *Misleading About reasons* — see the keyboard/fan-curve notes above.

**Not yet validated on this machine** (retest checklist in the issue reply):
Presets, Settings toggles, suspend/resume, Always-On USB while asleep, and the
new `custom` write path (needs `cat /sys/class/platform-profile/platform-profile-*/choices`
to confirm which handler offers `custom` on kernel 7.2).

**Fan curve:** the driver exposes it, and writing works on this model, but the
WMI readback returns an empty buffer — the planned editor must handle a missing
readback (default-curve loader, write-first flow). Tracked in
[docs/PLAN.md](PLAN.md) as its own milestone.


---

# Keyboard backlight — why it cannot be set, and what would change that

The laptop has a backlight — `Fn+Space` works — and the application reports it
unavailable. That is not a mistake in the application; it is what the kernel
currently exposes. Checked on 2026-10-14:

```
$ ls /sys/class/leds/
enp8s0-0::lan  enp8s0-1::lan  enp8s0-2::lan
input3::capslock  input3::numlock  input3::scrolllock
input8::capslock  input8::compose  input8::kana  input8::numlock
input8::scrolllock  platform::fnlock        <- no kbd_backlight

$ ls /sys/class/backlight/
nvidia_0                                    <- the screen, not the keyboard
```

Nothing in the kernel drives it. The firmware *does* expose it, over WMI:

```
$ ls /sys/bus/wmi/devices/ | grep 8C5B9127
8C5B9127-ECD4-4657-980F-851019F99CA5-6      <- present
```

`8C5B9127-ECD4-4657-980F-851019F99CA5` is exactly the GUID the LenovoLegionLinux
driver uses for the keyboard backlight on this model
(`kernel_module/legion-laptop.c:3635`, with
`access_method_keyboard = ACCESS_METHOD_WMI` in `model_qncn`). But **no in-tree
driver claims that device** — `ls -la` on it shows no `driver` symlink, and the
loaded `lenovo_wmi_*` drivers bind to other GUIDs:

| GUID | Driver |
|---|---|
| `DC2A8805-…` | `lenovo_wmi_other` (hwmon fans) |
| `887B54E3-…` | `lenovo_wmi_gamezone` |
| `CE6C0974-…` | `lenovo_wmi_hotkey_utilities` |
| `362A3AFE-…`, `7A8F5407-…`, `642801B-…` | `lenovo_wmi_capdata` |
| **`8C5B9127-…`** | **unbound** |

So the routes and why each fails:

- **The out-of-tree `legion_laptop` module** — implements the GUID and registers
  `platform::kbd_backlight`. **Not loaded.** This is the supported path and the
  module builds cleanly on this machine (verified: `legion-laptop.ko`, 1.7 MB).
- **In-tree drivers** — none handles the keyboard GUID.
- **`acpi_call`** — not loaded and not installed; this is how `extra/smartfan`
  reaches the EC directly on Legion 7 Gen 10+.
- **`hidraw` / `4JX/L5P-Keyboard-RGB`** — the keyboard is `048d:c195`, which is
  outside that project's supported product-ID table, and the upstream project
  states white-backlight variants (which this is) are unsupported.

## What to run

The full sequence — load, read-only checks, then writes safest-first — is in
**[docs/hardware-validation.md](hardware-validation.md)**. The short version:

```sh
cd Reference/LenovoLegionLinux/kernel_module
make                          # verified to build on 7.2.9-200.fc44
sudo make reloadmodule        # loads it and prints dmesg
```

`QNCN` is in the driver's DMI allowlist, so it should bind. Loading it would add
these controls, all of which the application already knows how to show:

| Capability | Source |
|---|---|
| `kbd_backlight` | WMI, 3-state |
| `ylogo_light` | Y-logo LED |
| `fan_fullspeed` | WMI |
| `winkey`, `gsync`, `igpumode`, `overdrive`, `notify_dgpu` | always exposed by the driver |

Eight controls, and the backlight among them. Two caveats from upstream's own
docs, both of which are why we do not enable them by default: `fan_fullspeed`
can latch the fans at maximum until reboot on this EC generation, and
`max-power` is deliberately withheld from `platform_profile` on this hardware
because it cuts power instantly.
