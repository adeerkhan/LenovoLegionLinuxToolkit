//! Generic Linux telemetry — the readings that need no Legion driver at all.
//!
//! The reference's sensor card shows CPU *utilization* and *core clock*, memory
//! utilization, and disk activity alongside temperatures. Those come from
//! `/proc` and the standard `cpufreq` interface, which every Linux box has.
//!
//! This matters here specifically: on this machine `legion_laptop` is not
//! loaded, so a sensor grid built only from Legion sources is mostly dashes.
//! Everything in this module works regardless, which is the difference between
//! a grid that reads as "unsupported machine" and one that reads as a
//! monitoring panel.

use std::path::Path;
use std::sync::Mutex;

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
fn read_cpufreq(attr: &str) -> Vec<f64> {
    let cpu_root = Path::new("/sys/devices/system/cpu");
    let Ok(entries) = std::fs::read_dir(cpu_root) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("cpu"))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Skip `cpufreq`, `cpuidle`, and anything that is not `cpuN`.
            if name.len() < 4 || !name[3..].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let value: f64 = std::fs::read_to_string(e.path().join("cpufreq").join(attr))
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
pub fn cpu_clock_max_mhz() -> Option<f64> {
    let clocks = read_cpufreq("cpuinfo_max_freq");
    clocks.into_iter().fold(None, |acc: Option<f64>, v| {
        Some(acc.map_or(v, |a| a.max(v)))
    })
}

/// A `/proc/meminfo` value in kB.
fn meminfo_kb(key: &str) -> Option<f64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    text.lines()
        .find(|l| l.starts_with(key))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// Memory utilization as a percentage of total.
pub fn memory_percent() -> Option<f64> {
    let total = meminfo_kb("MemTotal:")?;
    let available = meminfo_kb("MemAvailable:")?;
    if total <= 0.0 {
        return None;
    }
    Some((total - available) / total * 100.0)
}

/// Used and total memory, in GiB.
pub fn memory_used_gib() -> Option<(f64, f64)> {
    let total = meminfo_kb("MemTotal:")?;
    let available = meminfo_kb("MemAvailable:")?;
    const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
    Some(((total - available) / KIB_PER_GIB, total / KIB_PER_GIB))
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

/// Load average over the last minute.
pub fn load_average() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// The number of logical CPUs, for context against the load average.
pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
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
        let percent = memory_percent().expect("/proc/meminfo is always readable");
        assert!(
            (0.0..=100.0).contains(&percent),
            "memory out of range: {percent}"
        );
        let (used, total) = memory_used_gib().expect("meminfo");
        assert!(used <= total * 1.05, "used {used} exceeds total {total}");
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
    fn core_count_is_at_least_one() {
        assert!(cpu_count() >= 1);
    }
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

#[cfg(test)]
mod battery_history_tests {
    use super::*;

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
}
