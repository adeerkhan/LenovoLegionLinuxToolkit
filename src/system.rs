//! Generic Linux telemetry — the readings that need no Legion driver at all.
//!
//! The reference's sensor card shows CPU *utilization* and *core clock*, memory
//! utilization, and disk activity alongside temperatures. Those come from
//! `/proc`, the standard `cpufreq` interface, the kernel's `hwmon` class and
//! `nvidia-smi`, all of which every Linux box has.
//!
//! This matters here specifically: on this machine `legion_laptop` is not
//! loaded, and a sensor grid built only from Legion sources is mostly dashes.
//! Everything in this module works regardless, which is the difference between
//! a grid that reads as "unsupported machine" and one that reads as a
//! monitoring panel.

use std::path::Path;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// CPU
// ---------------------------------------------------------------------------

/// The previous CPU time sample, for computing a delta between reads.
///
/// Utilization is inherently a rate, so it needs two samples. The first call
/// returns `None` and the caller shows a blank for one tick rather than a
/// misleading zero.
static CPU_PREV: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// Aggregate CPU time across all cores, as `(idle, total)` in jiffies.
fn cpu_times() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let mut fields = line.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }

    let values: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
    if values.len() < 4 {
        return None;
    }

    // user nice system idle iowait irq softirq steal ...
    let total: u64 = values.iter().sum();
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((idle, total))
}

/// CPU utilization as a percentage, or `None` on the first call after start.
pub fn cpu_percent() -> Option<f64> {
    let (idle, total) = cpu_times()?;
    let mut previous = CPU_PREV.lock().ok()?;

    let percent = previous.map(|(prev_idle, prev_total)| {
        let total_delta = total.saturating_sub(prev_total);
        let idle_delta = idle.saturating_sub(prev_idle);
        if total_delta == 0 {
            0.0
        } else {
            (total_delta - idle_delta.min(total_delta)) as f64 / total_delta as f64 * 100.0
        }
    });

    *previous = Some((idle, total));
    percent
}

/// One `cpufreq` attribute per core.
fn read_cpufreq(attribute: &str) -> Vec<f64> {
    let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip `cpufreq`, `cpuidle`, and anything that is not `cpuN`.
            name.len() > 3
                && name.starts_with("cpu")
                && name[3..].chars().all(|c| c.is_ascii_digit())
        })
        .filter_map(|entry| {
            let value: f64 = std::fs::read_to_string(entry.path().join("cpufreq").join(attribute))
                .ok()?
                .trim()
                .parse()
                .ok()?;
            Some(value / 1000.0) // kHz -> MHz
        })
        .collect()
}

/// Mean current clock across cores, in MHz.
pub fn cpu_clock_mhz() -> Option<f64> {
    let clocks = read_cpufreq("scaling_cur_freq");
    if clocks.is_empty() {
        return None;
    }
    Some(clocks.iter().sum::<f64>() / clocks.len() as f64)
}

/// The CPU's rated maximum clock, in MHz.
///
/// Used as a meter's ceiling, so the bar reads as a fraction of this CPU's
/// range rather than of a number chosen by hand.
pub fn cpu_clock_max_mhz() -> Option<f64> {
    read_cpufreq("cpuinfo_max_freq")
        .into_iter()
        .fold(None, |acc: Option<f64>, value| {
            Some(acc.map_or(value, |a| a.max(value)))
        })
}

// ---------------------------------------------------------------------------
// Memory and disk
// ---------------------------------------------------------------------------

/// A `/proc/meminfo` value in kB.
fn meminfo_kb(key: &str) -> Option<f64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find(|line| line.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// A memory reading.
pub struct Memory {
    pub used_gib: f64,
    pub total_gib: f64,
    pub percent: f64,
}

/// Used and total memory, in GiB, and the used fraction of it.
///
/// One parse of `/proc/meminfo` serves both numbers, because they are the same
/// subtraction: the reference's RAM card shows the absolute while its meter
/// carries the percentage, and computing them separately would parse the file
/// twice for one answer.
pub fn memory() -> Option<Memory> {
    let total_kib = meminfo_kb("MemTotal:")?;
    let available_kib = meminfo_kb("MemAvailable:")?;
    if total_kib <= 0.0 {
        return None;
    }

    const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
    let used_kib = total_kib - available_kib;
    Some(Memory {
        used_gib: used_kib / KIB_PER_GIB,
        total_gib: total_kib / KIB_PER_GIB,
        percent: used_kib / total_kib * 100.0,
    })
}

/// Swap utilization as a percentage. `None` when no swap is configured —
/// a permanently-zero bar would be worse than an absent one.
pub fn swap_percent() -> Option<f64> {
    let total = meminfo_kb("SwapTotal:")?;
    let free = meminfo_kb("SwapFree:")?;
    if total <= 0.0 {
        return None;
    }
    Some((total - free) / total * 100.0)
}

/// Filesystem usage for a mount point, as `(percent_used, used_gib, total_gib)`.
pub fn disk_usage(mount: &str) -> Option<(f64, f64, f64)> {
    use std::ffi::CString;

    let path = CString::new(mount).ok()?;
    // SAFETY: `path` is a valid NUL-terminated string and `stat` is a
    // properly-sized, zeroed `statvfs` for the duration of the call.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }

    let block = stat.f_frsize as f64;
    let total = stat.f_blocks as f64 * block;
    let free = stat.f_bavail as f64 * block;
    if total <= 0.0 {
        return None;
    }

    const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let used = total - free;
    Some((
        used / total * 100.0,
        used / BYTES_PER_GIB,
        total / BYTES_PER_GIB,
    ))
}

// ---------------------------------------------------------------------------
// Battery history
//
// `power_supply` exposes only the *current* discharge rate. The reference shows
// minimum and maximum rates and the time spent on battery, which means tracking
// values over time rather than reading them. Sampling here is a side effect of
// the UI's existing refresh tick, so nothing extra is scheduled.
// ---------------------------------------------------------------------------

/// Discharge-rate extremes in watts, observed since the app started.
static DISCHARGE_RANGE: Mutex<(Option<f64>, Option<f64>)> = Mutex::new((None, None));

/// When the current discharge began, if the machine is discharging.
static ON_BATTERY_SINCE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// The battery's `status` string.
fn battery_status() -> Option<String> {
    let dir = crate::paths::battery_dir()?;
    Some(
        std::fs::read_to_string(dir.join("status"))
            .ok()?
            .trim()
            .to_string(),
    )
}

/// The current discharge rate in watts, or `None` when not discharging.
///
/// A rate is only a "discharge rate" while discharging, so the extremes below
/// deliberately hold their last values rather than decaying to zero when the
/// machine is plugged in.
fn discharge_watts() -> Option<f64> {
    if battery_status()?.as_str() != "Discharging" {
        return None;
    }
    let dir = crate::paths::battery_dir()?;
    let microwatts: f64 = std::fs::read_to_string(dir.join("power_now"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let watts = (microwatts / 1_000_000.0).abs();
    (watts > 0.0).then_some(watts)
}

/// Fold the current reading into the observed range and return it.
fn discharge_range() -> Option<(f64, f64)> {
    if let Some(watts) = discharge_watts() {
        if let Ok(mut range) = DISCHARGE_RANGE.lock() {
            range.0 = Some(range.0.map_or(watts, |min| min.min(watts)));
            range.1 = Some(range.1.map_or(watts, |max| max.max(watts)));
        }
    }
    let range = DISCHARGE_RANGE.lock().ok()?;
    match (range.0, range.1) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    }
}

/// Lowest discharge rate seen this session, in watts.
pub fn discharge_min_w() -> Option<f64> {
    discharge_range().map(|(min, _)| min)
}

/// Highest discharge rate seen this session, in watts.
pub fn discharge_max_w() -> Option<f64> {
    discharge_range().map(|(_, max)| max)
}

/// How long the machine has been on battery, in seconds.
///
/// Tracks the current discharge and resets when the adapter is connected. It
/// does not survive a restart, because nothing persists it yet; the reference
/// stores the timestamp in its settings.
pub fn on_battery_seconds() -> Option<u64> {
    let discharging = battery_status()?.as_str() == "Discharging";
    let mut since = ON_BATTERY_SINCE.lock().ok()?;

    if discharging {
        let start = *since.get_or_insert_with(std::time::Instant::now);
        Some(start.elapsed().as_secs())
    } else {
        *since = None;
        None
    }
}

/// Format a duration as the reference does: `1h 05m`, `12m`, `40s`.
pub fn format_duration(seconds: u64) -> String {
    let (hours, minutes) = (seconds / 3600, (seconds % 3600) / 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}

// ---------------------------------------------------------------------------
// Firmware attributes
//
// The kernel's `firmware-attributes` class exposes the BIOS's own tunables —
// power limits, thermal limits — through a generic interface, and the in-tree
// `lenovo-wmi-other` driver populates it. Every attribute carries the metadata
// a slider card needs, so the controls can be *generated* rather than written
// out per model.
//
// This is why controls the application labelled "not exposed by this model's
// firmware" are in fact present and writable on this machine.
// ---------------------------------------------------------------------------

/// Where the kernel exposes firmware attributes.
const FIRMWARE_ATTRIBUTES: &str = "/sys/class/firmware-attributes";

/// One BIOS tunable.
#[derive(Debug, Clone)]
pub struct FirmwareAttribute {
    /// The driver's name for it, e.g. `ppt_pl1_spl`.
    pub name: String,
    /// The driver's own one-line description, e.g.
    /// "Set the CPU sustained power limit", with the imperative "Set the "
    /// stripped: the card already reads as a control, and the reference's
    /// labels are noun phrases.
    pub description: String,
    pub current: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Physical unit, inferred from the attribute name. The interface does not
    /// state one, and an unlabelled number is not worth showing.
    pub unit: Option<&'static str>,
}

impl FirmwareAttribute {
    /// Whether the attribute can actually be changed right now.
    ///
    /// Several attributes report `min_value == max_value == 0` in the current
    /// power mode, which means the firmware is not accepting writes to them.
    /// Offering a slider with no range would be a broken control.
    pub fn is_writable(&self) -> bool {
        self.max > self.min && self.step > 0.0
    }
}

fn read_number(path: &Path) -> Option<f64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Physical unit for an attribute, from its name.
///
/// The `firmware-attributes` interface carries no unit, and `ppt_pl1_spl`
/// reading `70` is meaningless without one.
fn attribute_unit(name: &str) -> Option<&'static str> {
    if name.starts_with("ppt_") || name.starts_with("gpu_nv") {
        Some("W")
    } else {
        None
    }
}

/// Every firmware attribute the kernel currently exposes, in a stable order.
pub fn firmware_attributes() -> Vec<FirmwareAttribute> {
    let Ok(drivers) = std::fs::read_dir(FIRMWARE_ATTRIBUTES) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for driver in drivers.flatten() {
        let Ok(attributes) = std::fs::read_dir(driver.path().join("attributes")) else {
            continue;
        };
        for attribute in attributes.flatten() {
            let dir = attribute.path();
            let name = attribute.file_name().to_string_lossy().to_string();

            // Only integer attributes map onto a slider.
            let kind = std::fs::read_to_string(dir.join("type")).unwrap_or_default();
            if kind.trim() != "integer" {
                continue;
            }

            let (Some(current), Some(min), Some(max)) = (
                read_number(&dir.join("current_value")),
                read_number(&dir.join("min_value")),
                read_number(&dir.join("max_value")),
            ) else {
                continue;
            };
            // A missing increment means one unit per step.
            let step = read_number(&dir.join("scalar_increment")).unwrap_or(1.0);

            let description = std::fs::read_to_string(dir.join("display_name"))
                .map(|d| {
                    let text = d.trim();
                    // The BIOS phrases these as instructions. A card titled
                    // "Set the CPU sustained power limit" reads as an order;
                    // "CPU sustained power limit" reads as a name.
                    text.strip_prefix("Set the ")
                        .or_else(|| text.strip_prefix("Set "))
                        .unwrap_or(text)
                        .to_string()
                })
                .unwrap_or_default();

            found.push(FirmwareAttribute {
                unit: attribute_unit(&name),
                name,
                description,
                current,
                min,
                max,
                step: if step > 0.0 { step } else { 1.0 },
            });
        }
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// The file a firmware attribute is read from and written to.
///
/// Returns `None` for a name that is not a plain identifier: it is about to be
/// used as a path component, so anything that could traverse is refused here
/// rather than relied on being refused later.
pub fn firmware_attribute_path(name: &str) -> Option<std::path::PathBuf> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }

    std::fs::read_dir(FIRMWARE_ATTRIBUTES)
        .ok()?
        .flatten()
        .map(|driver| {
            driver
                .path()
                .join("attributes")
                .join(name)
                .join("current_value")
        })
        .find(|candidate| candidate.exists())
}

// ---------------------------------------------------------------------------
// GPU telemetry
//
// The only practical source of GPU temperature, utilization, VRAM, clocks and
// power on NVIDIA hardware is `nvidia-smi` — there is no sysfs equivalent. It
// costs tens of milliseconds to fork, so it is *never* run on the UI thread: a
// background poller refreshes a cache and the capability reads that.
//
// Vitals runs it as a long-lived subprocess with `-l` rather than forking per
// poll. A thread plus a cache is the Rust equivalent without taking on async.
// ---------------------------------------------------------------------------

/// The cached reading and when it was taken.
static GPU: Mutex<Option<(std::time::Instant, Gpu)>> = Mutex::new(None);

/// Ensures the cache is primed and the poller started, exactly once.
static GPU_POLLER: std::sync::Once = std::sync::Once::new();

/// How often `nvidia-smi` is re-run. Longer than the UI's refresh interval on
/// purpose: a GPU reading is not worth a process fork every second.
const GPU_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// One GPU's reading. Fields are `Option` because `nvidia-smi` reports `[N/A]`
/// for anything the card does not expose — a laptop GPU has no fan-speed
/// sensor, for instance — and that is not the same as zero.
#[derive(Debug, Clone, Default)]
pub struct Gpu {
    pub name: String,
    pub temperature_c: Option<f64>,
    pub utilization_percent: Option<f64>,
    pub memory_used_mib: Option<f64>,
    pub memory_total_mib: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub power_w: Option<f64>,
}

const GPU_QUERY: &str =
    "name,temperature.gpu,utilization.gpu,memory.used,memory.total,clocks.sm,power.draw";

/// Parse one CSV row of `nvidia-smi --format=csv,noheader,nounits`.
fn parse_gpu_row(row: &str) -> Option<Gpu> {
    let fields: Vec<&str> = row.split(',').map(str::trim).collect();
    if fields.len() < 7 {
        return None;
    }
    // `[N/A]` and `[Not Supported]` are what the driver emits for an absent
    // sensor; anything unparseable is treated as absent rather than as zero.
    let number = |s: &str| s.parse::<f64>().ok();
    let name = fields[0].to_string();
    if name.is_empty() {
        return None;
    }
    Some(Gpu {
        name,
        temperature_c: number(fields[1]),
        utilization_percent: number(fields[2]),
        memory_used_mib: number(fields[3]),
        memory_total_mib: number(fields[4]),
        clock_mhz: number(fields[5]),
        power_w: number(fields[6]),
    })
}

fn poll_gpu() -> Option<Gpu> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            &format!("--query-gpu={GPU_QUERY}"),
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    // One row per GPU; a laptop has one, and the first is the dGPU.
    parse_gpu_row(text.lines().next()?)
}

fn start_gpu_poller() {
    GPU_POLLER.call_once(|| {
        // Prime the cache synchronously before starting the thread. Callers use
        // `gpu()` to decide whether to *build* the GPU cards at all, and an
        // empty cache at that moment reported a present GPU as absent — which
        // is exactly what it did. One blocking poll at startup is the cost of
        // not guessing.
        if let Some(gpu) = poll_gpu() {
            if let Ok(mut cache) = GPU.lock() {
                *cache = Some((std::time::Instant::now(), gpu));
            }
        }

        // A detached thread: it holds the cache for the life of the process and
        // has no shutdown handshake to get wrong.
        std::thread::Builder::new()
            .name("gpu-poller".into())
            .spawn(|| loop {
                let reading = poll_gpu();
                if let Ok(mut cache) = GPU.lock() {
                    // A failed poll clears the cache rather than leaving a stale
                    // reading: a GPU that has gone away should stop being shown.
                    *cache = reading.clone().map(|gpu| (std::time::Instant::now(), gpu));
                }
                std::thread::sleep(GPU_POLL_INTERVAL);
            })
            .ok();
    });
}

/// The most recent GPU reading, starting the poller on first call.
///
/// Returns `None` for a machine with no NVIDIA GPU, or before the first poll
/// completes — the card shows a dash for a second or two at startup, which is
/// honest rather than wrong.
pub fn gpu() -> Option<Gpu> {
    start_gpu_poller();
    let cache = GPU.lock().ok()?;
    cache.as_ref().map(|(_, gpu)| gpu.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_utilization_is_a_percentage_once_warmed() {
        // First call establishes the baseline and may return None; the second
        // must produce a real number.
        let _ = cpu_percent();
        std::thread::sleep(std::time::Duration::from_millis(60));
        if let Some(percent) = cpu_percent() {
            assert!(
                (0.0..=100.0).contains(&percent),
                "cpu utilization out of range: {percent}"
            );
        }
    }

    #[test]
    fn clocks_are_plausible() {
        if let Some(mhz) = cpu_clock_mhz() {
            assert!(
                (100.0..=8000.0).contains(&mhz),
                "cpu clock implausible: {mhz}"
            );
        }
        if let (Some(current), Some(max)) = (cpu_clock_mhz(), cpu_clock_max_mhz()) {
            // Current can briefly exceed the nominal max under boost, but not
            // by an order of magnitude.
            assert!(current <= max * 2.0, "current {current} vs max {max}");
        }
    }

    #[test]
    fn memory_is_a_percentage_within_bounds() {
        let m = memory().expect("/proc/meminfo is always readable");
        assert!(
            (0.0..=100.0).contains(&m.percent),
            "memory out of range: {}",
            m.percent
        );
        assert!(m.used_gib <= m.total_gib * 1.05);
        assert!(m.total_gib > 0.0);
    }

    #[test]
    fn root_filesystem_reports_usage() {
        let (percent, used, total) = disk_usage("/").expect("root is always mounted");
        assert!(
            (0.0..=100.0).contains(&percent),
            "disk out of range: {percent}"
        );
        assert!(used <= total * 1.05, "used {used} exceeds total {total}");
    }

    #[test]
    fn discharge_range_is_ordered_and_plausible() {
        let _ = discharge_range();
        if let Some((min, max)) = discharge_range() {
            assert!(min <= max, "min {min} exceeds max {max}");
            assert!((0.0..=500.0).contains(&min), "min implausible: {min}");
            assert!((0.0..=500.0).contains(&max), "max implausible: {max}");
        }
    }

    #[test]
    fn on_battery_seconds_is_consistent_with_status() {
        let discharging = battery_status()
            .map(|s| s == "Discharging")
            .unwrap_or(false);
        match on_battery_seconds() {
            Some(_) => assert!(
                discharging,
                "reported time on battery while not discharging"
            ),
            None => assert!(!discharging, "on battery but reported no duration"),
        }
    }

    #[test]
    fn durations_format_readably() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(40), "40s");
        assert_eq!(format_duration(605), "10m");
        assert_eq!(format_duration(3900), "1h 05m");
    }

    /// The metadata a slider needs must be self-consistent, and an attribute
    /// the firmware is locking must be recognised as such rather than offered
    /// as a control with no range.
    #[test]
    fn firmware_attributes_are_well_formed() {
        for attribute in firmware_attributes() {
            assert!(!attribute.name.is_empty());
            assert!(attribute.max >= attribute.min, "{}", attribute.name);
            assert!(attribute.step > 0.0, "{}", attribute.name);
            if attribute.is_writable() {
                assert!(
                    (attribute.min..=attribute.max).contains(&attribute.current),
                    "{} reports {} outside {}..={}",
                    attribute.name,
                    attribute.current,
                    attribute.min,
                    attribute.max
                );
            }
        }
    }

    /// An attribute name becomes a path component, so anything that could
    /// climb out of its directory must be refused.
    #[test]
    fn traversal_in_an_attribute_name_is_refused() {
        assert!(firmware_attribute_path("../../etc/shadow").is_none());
        assert!(firmware_attribute_path("a/b").is_none());
        assert!(firmware_attribute_path("").is_none());
    }

    #[test]
    fn parses_a_normal_gpu_row() {
        let gpu =
            parse_gpu_row("NVIDIA GeForce RTX 5060 Laptop GPU, 63, 1, 4242, 8151, 690, 16.26")
                .expect("row should parse");
        assert_eq!(gpu.name, "NVIDIA GeForce RTX 5060 Laptop GPU");
        assert_eq!(gpu.temperature_c, Some(63.0));
        assert_eq!(gpu.memory_total_mib, Some(8151.0));
        assert_eq!(gpu.power_w, Some(16.26));
    }

    /// `nvidia-smi` emits `[N/A]` for sensors the card lacks. Treating that as
    /// zero would show a laptop GPU as 0 °C, which is worse than showing
    /// nothing.
    #[test]
    fn absent_gpu_sensors_are_none_not_zero() {
        let gpu =
            parse_gpu_row("NVIDIA GeForce RTX 5060 Laptop GPU, 63, [N/A], 100, 8151, [N/A], 16.26")
                .expect("row should parse");
        assert_eq!(gpu.utilization_percent, None);
        assert_eq!(gpu.clock_mhz, None);
        assert_eq!(gpu.temperature_c, Some(63.0));
    }

    #[test]
    fn a_short_or_empty_gpu_row_is_rejected() {
        assert!(parse_gpu_row("").is_none());
        assert!(parse_gpu_row("NVIDIA, 63, 1").is_none());
    }

    /// The poller must not be started twice, and a machine without the binary
    /// must simply report no GPU rather than failing.
    #[test]
    fn gpu_polling_is_idempotent_and_absence_is_not_an_error() {
        start_gpu_poller();
        start_gpu_poller();
        let _ = gpu();
    }

    /// The first `gpu()` call must already have an answer, because it is what
    /// decides whether the GPU cards are built. An unprimed cache reported a
    /// present GPU as absent.
    #[test]
    fn the_first_gpu_call_is_primed() {
        let first = gpu();
        let nvidia_smi_works = std::process::Command::new("nvidia-smi")
            .args(["--query-gpu=name", "--format=csv,noheader"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if nvidia_smi_works {
            assert!(
                first.is_some(),
                "nvidia-smi works but the first gpu() call returned nothing"
            );
            assert!(!first.unwrap().name.is_empty());
        }
    }
}
