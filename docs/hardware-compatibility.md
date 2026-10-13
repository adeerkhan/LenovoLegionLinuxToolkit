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
| IC/chipset temperature | hwmon `temp3_input` | ✅ | – | **NO** (`skip_ic_temp`) |
| Fan 1–n RPM | hwmon `fanN_input` | ✅ | – | **YES** |
| Fan target RPM | hwmon `fanN_target` | ✅ | – | model-dependent |
| Fan curve points | hwmon `pwmN_auto_point{M=1..10}_{pwm,temp,temp_hyst,accel,decel}` | ✅ | ✅ | **NO** |
| `auto_points_size` | hwmon | ✅ | | **NO** |
| Fan max-speed query | `<legion>/fan_maxspeed` | ✅ | ✅ | **NO** (`skip_fan_maxspeed`) |
| Fan controller lock | `<legion>/lockfancontroller` | ✅ | ✅ | **NO** (`skip_lockfancontroller`) |
| Fan unlock | `<legion>/fan_unlock` | ✅ | ✅ | model-dependent |
| Battery conservation | **ideapad** `VPC2004:00/conservation_mode` | ✅ | ✅ | **YES** |
| Rapid charge | `<legion>/rapidcharge` (ACPI) | ✅ | ✅ | model-dependent |
| Fn-lock | **ideapad** `VPC2004:00/fn_lock` | ✅ | ✅ | **YES** |
| Touchpad | **ideapad** `VPC2004:00/touchpad` | ✅ | ✅ | **YES** |
| Camera power | **ideapad** `VPC2004:00/camera_power` | ✅ | ✅ | **YES** |
| Always-on USB charging | **ideapad** `VPC2004:00/usb_charging` | ✅ | ✅ | **YES** |
| Fan mode (ideapad) | **ideapad** `VPC2004:00/fan_mode` | ✅ | – | present (value `0`) |
| Keyboard backlight | `leds` `platform::kbd_backlight[_1]` (max_brightness 2) | ✅ | ✅ | **YES**, 3-state |
| Y-logo LED | `/sys/class/leds/platform::ylogo/brightness` | ✅ | ✅ | **YES** |
| IO-port LED | `/sys/class/leds/platform::ioport/brightness` | ✅ | ✅ | **YES** |
| Overdrive | `<legion>/overdrive` | ✅ | ✅ | model-dependent |
| G-Sync | `<legion>/gsync` | ✅ | ✅ (reboot) | model-dependent |
| CPU/GPU power limits | `cpu_{short,long,peak,apu_sppt,cross_loading}_powerlimit` etc. | ✅ | ✅ | **NO** |
| CPU/GPU overclock | `<legion>/cpu_oc`, `gpu_oc` | ✅ | ✅ | **NO** |
| Battery status | `/sys/class/power_supply/BAT0/{status,capacity,...}` | ✅ | – | **YES** |
| AC adapter | `/sys/class/power_supply/ADP0/online` | ✅ | – | **YES** |
| USB-C PD sources | `/sys/class/power_supply/ucsi-source-psy-USBC000:00{1,2}` | ✅ | – | present |

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

## Keyboard RGB — unverified, needs an early probe

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
