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
    /// more than one place (touchpad does: ideapad *or* legion).
    pub fn first_existing(&self, candidates: &[Option<PathBuf>]) -> Option<PathBuf> {
        candidates.iter().flatten().find(|p| p.exists()).cloned()
    }

    /// True when a path both resolves and currently exists on disk. This is the
    /// Rust equivalent of LenovoLegionLinux's `Feature.exists()`.
    pub fn has(&self, key: &str) -> bool {
        self.file(key).map(|p| p.exists()).unwrap_or(false)
    }

    /// Map a stable capability key onto its backing file, if the key is known
    /// and the owning subsystem is present.
    pub fn file(&self, key: &str) -> Option<PathBuf> {
        let legion = |name: &str| self.legion.as_ref().map(|b| b.join(name));
        let ideapad = |name: &str| self.ideapad.as_ref().map(|b| b.join(name));

        let path = match key {
            // --- power mode -------------------------------------------------
            "platform_profile" => std::fs::read_dir(self.legion.as_ref()?.join("platform-profile"))
                .ok()?
                .flatten()
                .find(|e| e.path().join("profile").exists())
                .map(|e| e.path().join("profile"))?,

            // --- fans -------------------------------------------------------
            "fan_fullspeed" => legion("fan_fullspeed")?,

            // --- hwmon sensors ----------------------------------------------
            "cpu_temp" => self.hwmon.as_ref().map(|h| h.join("temp1_input"))?,
            "gpu_temp" => self.hwmon.as_ref().map(|h| h.join("temp2_input"))?,
            "ic_temp" => self.hwmon.as_ref().map(|h| h.join("temp3_input"))?,
            "fan1_rpm" => self.hwmon.as_ref().map(|h| h.join("fan1_input"))?,
            "fan2_rpm" => self.hwmon.as_ref().map(|h| h.join("fan2_input"))?,
            "fan3_rpm" => self.hwmon.as_ref().map(|h| h.join("fan3_input"))?,
            "fan4_rpm" => self.hwmon.as_ref().map(|h| h.join("fan4_input"))?,

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

/// Find a battery attribute file, following the `power_supply` class rather
/// than assuming `BAT0`. Some boards have several USB-C PD sources that also
/// look like batteries; the real one reports a non-zero `present`.
pub fn find_battery_file(attr: &str) -> Option<PathBuf> {
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
            (is_battery && present).then(|| dir.join(attr))
        })
        .find(|p| p.exists())
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
