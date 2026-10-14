//! Resolution of the sysfs paths the toolkit reads.
//!
//! This is the single most likely source of "it does not work on my machine"
//! bugs, so it deliberately mirrors what LenovoLegionLinux does rather than
//! inventing anything.
//!
//! Two things are easy to get wrong:
//!
//! 1. The legion platform directory moved in kernel 7.0. Resolve by kernel
//!    version, then fall back to the legacy path.
//! 2. Several controls live under `ideapad_acpi`, not legion. Probing only
//!    the legion tree reports them as unsupported on machines where they work.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the driver's sysfs attributes live, once discovered.
#[derive(Debug, Default, Clone)]
pub struct SysPaths {
    /// The legion platform directory (level 2 attribute group).
    pub legion: Option<PathBuf>,
    /// The legion hwmon directory (sensors + fan curve).
    pub hwmon: Option<PathBuf>,
    /// The ideapad_acpi directory (conservation, Fn-lock, touchpad, camera, USB).
    pub ideapad: Option<PathBuf>,
    /// True when the module was loaded with `ec_readonly=1`.
    pub ec_readonly: bool,
}

/// A single, stable identifier for what this machine is, for logs and the
/// About page.
pub fn machine_summary() -> String {
    let dmi = |name: &str| {
        std::fs::read_to_string(format!("/sys/devices/virtual/dmi/id/{name}"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let product = dmi("product_name");
    let version = dmi("product_version");
    let bios = dmi("bios_version");
    match (product.is_empty(), version.is_empty()) {
        (true, _) => "unknown machine".into(),
        (false, true) => format!("{product} (BIOS {bios})"),
        (false, false) => format!("{product} {version} (BIOS {bios})"),
    }
}

/// Kernel release, as `uname -r`.
fn kernel_release() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn kernel_major_release() -> u32 {
    kernel_release()
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// The legion platform directory, or `None` when the module is not loaded.
fn legion_platform_dir() -> Option<PathBuf> {
    // Kernel >= 7.0 renamed the driver bus from PNP0C09:00 to `legion`.
    let candidates: &[&str] = if kernel_major_release() < 7 {
        &["/sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00"]
    } else {
        &[
            "/sys/module/legion_laptop/drivers/platform:legion/legion",
            "/sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00",
        ]
    };

    candidates.iter().map(PathBuf::from).find(|p| p.is_dir())
}

/// The ideapad_acpi directory, which carriers many of the toggles.
fn ideapad_dir() -> Option<PathBuf> {
    let candidates = [
        "/sys/bus/platform/drivers/ideapad_acpi/VPC2004:00",
        // Older and newer boards use different ACPI device ids.
        "/sys/bus/platform/drivers/ideapad_laptop/VPC2004:00",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.is_dir())
}

/// Probe the running system. Every capability is gated on one of these paths.
pub fn discover() -> SysPaths {
    let legion = legion_platform_dir();

    // hwmon is discovered by glob beneath the platform directory; the dir name
    // is `hwmonN` and is not stable across boots.
    let hwmon = legion.as_ref().and_then(|base| {
        std::fs::read_dir(base.join("hwmon"))
            .ok()?
            .flatten()
            .find_map(|e| {
                let p = e.path();
                p.is_dir().then_some(p)
            })
    });

    let ec_readonly = std::fs::read_to_string("/sys/module/legion_laptop/parameters/ec_readonly")
        .map(|s| s.trim() == "1" || s.trim() == "Y")
        .unwrap_or(false);

    SysPaths {
        legion,
        hwmon,
        ideapad: ideapad_dir(),
        ec_readonly,
    }
}

impl SysPaths {
    /// First existing path among the candidates, for a key that can live in
    /// more than one place. The touchpad is the real case: `ideapad_acpi`
    /// exposes it on some boards and the legion driver on others.
    pub fn first_existing(&self, candidates: &[Option<PathBuf>]) -> Option<PathBuf> {
        candidates.iter().flatten().find(|p| p.exists()).cloned()
    }

    /// Map a stable capability key onto its backing file, if the key is known
    /// and the owning subsystem is present.
    pub fn file(&self, key: &str) -> Option<PathBuf> {
        let legion = |name: &str| self.legion.as_ref().map(|b| b.join(name));
        let ideapad = |name: &str| self.ideapad.as_ref().map(|b| b.join(name));

        let path = match key {
            // --- power mode -------------------------------------------------
            // `powermode` is the capability key the UI and the privilege
            // allowlist use; `platform_profile` is what the kernel calls it.
            // They were never matched up, so the flagship control read as
            // unavailable even when the file was present.
            "powermode" | "platform_profile" => platform_profile()?,

            // --- fans -------------------------------------------------------
            // Resolved by sweeping hwmon rather than assuming the legion device
            // owns them, so they report even with the out-of-tree module absent.
            "fan1_rpm" => fan_path(1, self.hwmon.as_deref())?,
            "fan2_rpm" => fan_path(2, self.hwmon.as_deref())?,
            "fan3_rpm" => fan_path(3, self.hwmon.as_deref())?,
            "fan4_rpm" => fan_path(4, self.hwmon.as_deref())?,
            "fan_fullspeed" => legion("fan_fullspeed")?,

            // --- hwmon sensors ----------------------------------------------
            // Only the legion module's own CPU and chipset channels are read by
            // name here; CPU temperature additionally falls back to coretemp,
            // the fans are swept from hwmon, and the GPU comes from nvidia-smi.
            "cpu_temp" => self.hwmon.as_ref().map(|h| h.join("temp1_input"))?,
            "ic_temp" => self.hwmon.as_ref().map(|h| h.join("temp3_input"))?,

            // --- ideapad toggles (NOT under the legion tree) ----------------
            "battery_conservation" => ideapad("conservation_mode")?,
            "fn_lock" => ideapad("fn_lock")?,
            "camera_power" => ideapad("camera_power")?,
            "usb_charging" => ideapad("usb_charging")?,
            "touchpad" => self.first_existing(&[ideapad("touchpad"), legion("touchpad")])?,
            "fan_mode" => ideapad("fan_mode")?,

            // --- lighting, via the standard leds class ----------------------
            "kbd_backlight" => leds_path("kbd_backlight")?,
            "ylogo_light" => PathBuf::from("/sys/class/leds/platform::ylogo/brightness"),
            "ioport_light" => PathBuf::from("/sys/class/leds/platform::ioport/brightness"),

            _ => return None,
        };
        Some(path)
    }
}

/// The platform-profile control, preferring the standard ACPI interface.
///
/// `/sys/firmware/acpi/platform_profile` has been the standard way to read and
/// set the power mode for years, and the kernel's own in-tree drivers populate
/// it. Reading only the legion module's copy reported "unsupported" on every
/// machine without the out-of-tree module loaded — including this one, where
/// the ACPI file is present and the module is not.
pub fn platform_profile() -> Option<PathBuf> {
    const STANDARD: &str = "/sys/firmware/acpi/platform_profile";
    if Path::new(STANDARD).exists() {
        return Some(PathBuf::from(STANDARD));
    }

    // Fallback: the legion module's layout, one directory per profile handler.
    let base = Path::new("/sys/module/legion_laptop/drivers/platform:legion");
    let candidates = [base.join("legion"), base.join("PNP0C09:00")];
    candidates.into_iter().find_map(|dir| {
        std::fs::read_dir(dir.join("platform-profile"))
            .ok()?
            .flatten()
            .map(|e| e.path().join("profile"))
            .find(|p| p.exists())
    })
}

/// The list of platform-profile choices.
///
/// Its name differs between the two interfaces — `platform_profile_choices`
/// beside the ACPI file, `choices` beside the legion one — so it cannot be
/// found by looking for a sibling named `choices`.
pub fn platform_profile_choices() -> Option<PathBuf> {
    const STANDARD: &str = "/sys/firmware/acpi/platform_profile_choices";
    if Path::new(STANDARD).exists() {
        return Some(PathBuf::from(STANDARD));
    }
    let profile = platform_profile()?;
    let sibling = profile.parent()?.join("choices");
    sibling.exists().then_some(sibling)
}

/// The `leds` class renames the keyboard backlight to `kbd_backlight_1` when a
/// name collision with another driver is detected, so it must be globbed.
fn leds_path(function: &str) -> Option<PathBuf> {
    let dir = Path::new("/sys/class/leds");
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        (name == format!("platform::{function}") || name == format!("platform::{function}_1"))
            .then(|| e.path().join("brightness"))
    })
}

/// Find an hwmon directory by its `name` file — `coretemp`, `nvme`, and so on.
///
/// The legion driver's own sensors live under its platform directory and are
/// resolved separately; this is for the generic thermal and storage monitoring
/// the kernel already provides with no driver of ours involved.
pub fn find_hwmon_by_name(name: &str) -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .find_map(|e| {
            let path = e.path();
            (std::fs::read_to_string(path.join("name")).ok()?.trim() == name).then_some(path)
        })
}

/// The hottest coretemp reading, in °C.
///
/// `coretemp` exposes one input per core plus a package sensor, and which index
/// is the package varies by CPU generation. Reading the maximum is the honest
/// proxy for "CPU temperature" — it is what matters thermally, and it never
/// reports a colder number than any real sensor.
pub fn coretemp_max_c() -> Option<f64> {
    let dir = find_hwmon_by_name("coretemp")?;
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .fold(None, |acc: Option<f64>, e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("temp") && name.ends_with("_input")) {
                return acc;
            }
            let value: f64 = std::fs::read_to_string(e.path())
                .ok()?
                .trim()
                .parse()
                .ok()?;
            let celsius = value / 1000.0;
            if value <= 0.0 {
                return acc;
            }
            Some(acc.map_or(celsius, |a: f64| a.max(celsius)))
        })
}

/// The battery's power_supply directory.
pub fn battery_dir() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/power_supply")
        .ok()?
        .flatten()
        .filter_map(|e| {
            let dir = e.path();
            let is_battery = std::fs::read_to_string(dir.join("type")).ok()?.trim() == "Battery";
            let present = std::fs::read_to_string(dir.join("present"))
                .ok()
                .map(|s| s.trim() == "1")
                .unwrap_or(true);
            (is_battery && present).then_some(dir)
        })
        .next()
}

/// A numeric attribute of a specific power_supply directory, or `None`.
pub fn battery_float_in(dir: &Path, attr: &str) -> Option<f64> {
    std::fs::read_to_string(dir.join(attr))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// A numeric attribute of the battery, or `None`.
pub fn battery_float(attr: &str) -> Option<f64> {
    battery_float_in(&battery_dir()?, attr)
}

/// A textual attribute of the battery, or an empty string.
pub fn battery_text(attr: &str) -> String {
    battery_dir()
        .and_then(|d| std::fs::read_to_string(d.join(attr)).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Remaining or required time on the current charge, in hours.
///
/// Returns `(hours, is_charging)`. `power_now` is a rate in µW, so the quotient
/// is in hours directly. Discharge is reported by some drivers as a negative
/// rate and others as a positive one, so the magnitude is taken rather than
/// trusting the sign.
pub fn battery_time_hours() -> Option<(f64, bool)> {
    let dir = battery_dir()?;
    let status = std::fs::read_to_string(dir.join("status")).ok()?;
    let charging = status.trim() == "Charging";

    let now =
        battery_float_in(&dir, "energy_now").or_else(|| battery_float_in(&dir, "charge_now"))?;

    // A rate of zero means the firmware has no estimate to give, not "forever".
    let rate = battery_float_in(&dir, "power_now")
        .or_else(|| battery_float_in(&dir, "current_now"))?
        .abs();
    if rate < 1.0 {
        return None;
    }

    let energy = if charging {
        let full = battery_float_in(&dir, "energy_full")
            .or_else(|| battery_float_in(&dir, "charge_full"))?;
        (full - now).max(0.0)
    } else {
        now
    };

    Some((energy / rate, charging))
}

/// Every hwmon directory, with its device `name`.
///
/// The kernel exposes sensors through `/sys/class/hwmon/*` regardless of which
/// driver owns them, and the directory names are not stable across boots. The
/// only reliable way to find a sensor is to sweep them all and read each
/// device's `name` — which is what Vitals does, and the reason it finds fans on
/// machines where a driver-specific lookup finds none.
pub fn hwmon_devices() -> Vec<(String, PathBuf)> {
    std::fs::read_dir("/sys/class/hwmon")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = std::fs::read_to_string(path.join("name")).ok()?;
            Some((name.trim().to_string(), path))
        })
        .collect()
}

/// The path for fan `index` (1-based), preferring the most specific source.
///
/// Order matters: the legion module's own sensor is the most accurate where it
/// exists, then the in-tree `lenovo_wmi_other` driver — which on this machine
/// already reports both fans at 4000 RPM — then any hwmon device that happens
/// to expose the channel.
pub fn fan_path(index: u32, legion_hwmon: Option<&Path>) -> Option<PathBuf> {
    let attribute = format!("fan{index}_input");

    // 1. The out-of-tree module's own hwmon.
    if let Some(hwmon) = legion_hwmon {
        let candidate = hwmon.join(&attribute);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // 2. In-tree Lenovo WMI sensors.
    for (name, path) in hwmon_devices() {
        if name.starts_with("lenovo_wmi") {
            let candidate = path.join(&attribute);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. Any other device exposing the channel.
    let mut devices = hwmon_devices();
    devices.sort_by(|a, b| a.0.cmp(&b.0));
    devices
        .into_iter()
        .map(|(_, path)| path.join(&attribute))
        .find(|candidate| candidate.exists())
}
