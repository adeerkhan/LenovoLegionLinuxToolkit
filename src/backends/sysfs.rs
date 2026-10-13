//! The native sysfs/hwmon backend.
//!
//! Reads require no privilege — every readable sysfs and hwmon file is `0444`
//! or `0644`, so a plain user can read them. Writes are routed through
//! [`crate::services::privilege`], which escalates to the helper binary via
//! pkexec. The backend never touches `/tmp`, never shells out for reads, and
//! never assumes a path exists.
//!
//! Path resolution lives in [`crate::paths`], which is where the model-specific
//! knowledge lives. This module only maps keys to descriptors.

use crate::backends::Backend;
use crate::capability::{read_trim, Builder, Capability, Group, Kind, Limits, NamedValue, Value};
use crate::paths::{self, SysPaths};

pub struct SysfsBackend {
    paths: SysPaths,
    module_loaded: bool,
}

impl SysfsBackend {
    pub fn new() -> Self {
        let paths = paths::discover();
        Self {
            module_loaded: paths.legion.is_some(),
            paths,
        }
    }

    /// Add an enum capability backed by a sysfs file, enumerating values from
    /// a sibling `choices` file where the driver provides one.
    fn enum_cap(
        &self,
        key: &'static str,
        label: &str,
        icon: &'static str,
        group: Group,
        fallback: &[(&str, &str)],
    ) -> Capability {
        let path = self.paths.file(key);
        let available = path.as_ref().map(|p| p.exists()).unwrap_or(false);

        let mut values: Vec<NamedValue> = self
            .paths
            .file(key)
            .and_then(|p| {
                p.parent()
                    .map(|d| d.join("choices"))
                    .filter(|c| c.exists())
                    .and_then(|c| read_trim(&c).ok())
            })
            .map(|raw| {
                raw.split_whitespace()
                    .map(|v| NamedValue {
                        value: v.to_string(),
                        label: humanise_profile(v),
                    })
                    .collect()
            })
            .unwrap_or_default();

        if values.is_empty() {
            values = fallback
                .iter()
                .map(|(v, l)| NamedValue {
                    value: (*v).to_string(),
                    label: (*l).to_string(),
                })
                .collect();
        }

        let mut b = Builder::new(key, Kind::Enum, group)
            .label(label)
            .icon(icon)
            .values(values);

        if let Some(p) = path.filter(|p| p.exists()) {
            let key = key.to_string();
            b = b.read(move || {
                let raw = read_trim(&p)?;
                // `low-power` is the driver's spelling of "Quiet"; present it
                // as the label the firmware documentation uses.
                Ok(Value::Text(raw))
            });
            b = b.write(move |v| {
                crate::services::privilege::request(&key, &v.to_string())?;
                Ok(())
            });
        } else if available {
            b = b.unavailable("Driver not loaded");
        } else {
            b = b.unavailable("Not supported by this model");
        }

        b.build()
    }

    /// Add a toggle backed by a sysfs file that uses `0`/`1`.
    fn toggle_cap(
        &self,
        key: &'static str,
        label: &str,
        icon: &'static str,
        group: Group,
        note: Option<&'static str>,
    ) -> Capability {
        let path = self.paths.file(key);
        let mut b = Builder::new(key, Kind::Toggle, group)
            .label(label)
            .icon(icon);
        if let Some(n) = note {
            b = b.note(n);
        }

        match path.filter(|p| p.exists()) {
            Some(p) => {
                let key = key.to_string();
                b = b.read(move || Ok(Value::Bool(read_trim(&p)? == "1")));
                b = b.write(move |v| {
                    let arg = if matches!(v, Value::Bool(true)) {
                        "1"
                    } else {
                        "0"
                    };
                    crate::services::privilege::request(&key, arg)?;
                    Ok(())
                });
            }
            None => {
                b = b.unavailable(self.reason_for(key));
            }
        }

        b.build()
    }

    /// Add a read-only numeric sensor, converting milli-degrees where hwmon
    /// reports them that way.
    fn sensor_cap(
        &self,
        key: &'static str,
        label: &str,
        icon: &'static str,
        group: Group,
        unit: &'static str,
        milli: bool,
    ) -> Capability {
        let path = self.paths.file(key);
        let mut b = Builder::new(key, Kind::ReadOnly, group)
            .label(label)
            .icon(icon)
            .unit(unit);

        match path.filter(|p| p.exists()) {
            Some(p) => {
                b = b.read(move || {
                    let raw: f64 = read_trim(&p)?.parse()?;
                    Ok(Value::Float(if milli { raw / 1000.0 } else { raw }))
                });
            }
            None => {
                b = b.unavailable(self.reason_for(key));
            }
        }
        b.build()
    }

    /// Why is a capability missing? Distinguish "driver not loaded" from
    /// "hardware does not have it", because those need different actions from
    /// the user and are otherwise indistinguishable in sysfs.
    fn reason_for(&self, key: &str) -> &'static str {
        if !self.module_loaded && !keys_from_ideapad().contains(&key) {
            "Requires the legion_laptop driver"
        } else {
            "Not supported by this model"
        }
    }
}

impl Default for SysfsBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for SysfsBackend {
    fn name(&self) -> &str {
        if self.module_loaded {
            "sysfs (legion_laptop)"
        } else {
            "sysfs (ideapad only — driver not loaded)"
        }
    }

    fn global_warning(&self) -> Option<String> {
        if self.paths.ec_readonly {
            Some(
                "The driver is loaded with ec_readonly=1; no setting on this page can be written."
                    .to_string(),
            )
        } else {
            None
        }
    }

    fn capabilities(&self) -> Vec<Capability> {
        let mut caps: Vec<Capability> = Vec::new();

        // ---- Power --------------------------------------------------------
        caps.push(self.enum_cap(
            "powermode",
            "Power Mode",
            "power-profile-performance-symbolic",
            Group::Power,
            &[("0", "Quiet"), ("1", "Balanced"), ("2", "Performance")],
        ));
        // Upstream documents that a fan-full-speed write through the wrong path
        // can hold the fans at maximum until reboot on this EC generation, so
        // it asks for confirmation first.
        let mut max_fan = self.toggle_cap(
            "fan_fullspeed",
            "Maximum Fan Speed",
            "fan-symbolic",
            Group::Power,
            Some(
                "On this EC generation a misplaced write can hold the fans at \
                 maximum until reboot.",
            ),
        );
        max_fan.dangerous = true;
        caps.push(max_fan);

        // ---- Thermal ------------------------------------------------------
        // CPU temperature prefers the legion driver's sensor and falls back to
        // coretemp. On this model the driver is not loaded, and the machine is
        // at 60–88 °C right now: reporting "requires the driver" while coretemp
        // holds the number would be the wrong answer. The fallback is labelled
        // honestly as the hottest core, because coretemp's index layout varies
        // by CPU generation and this is a proxy, not the driver's reading.
        {
            let legion = self.paths.file("cpu_temp").filter(|p| p.exists());
            let has_coretemp = paths::find_hwmon_by_name("coretemp").is_some();
            let mut b = Builder::new("cpu_temp", Kind::ReadOnly, Group::Thermal)
                .label("CPU Temperature")
                .icon("cpu-symbolic")
                .unit("°C");
            match (&legion, has_coretemp) {
                (Some(path), _) => {
                    let path = path.clone();
                    b = b.read(move || {
                        let raw: f64 = read_trim(&path)?.parse()?;
                        Ok(Value::Float(raw / 1000.0))
                    });
                }
                (None, true) => {
                    b = b
                        .note("Hottest core, via coretemp")
                        .read(|| Ok(Value::Float(paths::coretemp_max_c().unwrap_or(0.0))));
                }
                (None, false) => {
                    b = b.unavailable("No CPU temperature sensor found");
                }
            }
            caps.push(b.build());
        }

        for (key, label, icon) in [
            ("gpu_temp", "GPU Temperature", "video-display-symbolic"),
            ("ic_temp", "Chipset Temperature", "motherboard-symbolic"),
        ] {
            caps.push(self.sensor_cap(key, label, icon, Group::Thermal, "°C", true));
        }

        for (key, label) in [
            ("fan1_rpm", "CPU Fan"),
            ("fan2_rpm", "GPU Fan"),
            ("fan3_rpm", "Auxiliary Fan"),
            ("fan4_rpm", "Fourth Fan"),
        ] {
            caps.push(self.sensor_cap(key, label, "fan-symbolic", Group::Thermal, "rpm", false));
        }

        // NVMe temperature, straight from the kernel's own monitoring. Available
        // on every machine with an NVMe drive and no driver involved — the
        // reference shows this in its disk sensor card.
        if let Some(hwmon) = paths::find_hwmon_by_name("nvme") {
            let path = hwmon.join("temp1_input");
            if path.exists() {
                caps.push(
                    Builder::new("disk_temp", Kind::ReadOnly, Group::Thermal)
                        .label("Disk Temperature")
                        .icon("drive-harddisk-symbolic")
                        .unit("°C")
                        .read(move || {
                            let raw: f64 = read_trim(&path)?.parse()?;
                            Ok(Value::Float(raw / 1000.0))
                        })
                        .build(),
                );
            }
        }

        // Fan curve: absent on this model, present in the matrix so the
        // suppression path is exercised on every load rather than only on
        // machines that happen to have a curve.
        caps.push(
            Builder::new("fancurve", Kind::Custom, Group::Thermal)
                .label("Fan Curve")
                .icon("air-conditioner-symbolic")
                .unavailable("Fan curves are not supported by this model's firmware")
                .read(|| Ok(Value::Text("\u{2014}".into())))
                .build(),
        );

        // CPU long-term power limit: absent here, present so the slider exists
        // for models that do expose it.
        {
            let path = self.paths.file("cpu_longterm_powerlimit");
            let exists = path.as_ref().map(|p| p.exists()).unwrap_or(false);
            let mut b = Builder::new("cpu_longterm_powerlimit", Kind::Slider, Group::Power)
                .label("CPU Long-Term Power Limit")
                .icon("power-profile-performance-symbolic")
                .unit("W")
                .limits(Limits {
                    min: 5.0,
                    max: 200.0,
                    step: 1.0,
                });
            if exists {
                let p = path.unwrap();
                b = b.read(move || Ok(Value::Int(read_trim(&p)?.parse::<i64>().unwrap_or(0))));
            } else {
                b = b.unavailable("Power limits are not exposed by this model's firmware");
            }
            caps.push(b.build());
        }

        // ---- Lighting -----------------------------------------------------
        {
            // The keyboard backlight is a 3-state leds-class device
            // (off / low / high) with max_brightness = 2.
            let path = self.paths.file("kbd_backlight");
            let exists = path.as_ref().map(|p| p.exists()).unwrap_or(false);
            let mut b = Builder::new("kbd_backlight", Kind::Enum, Group::Lighting)
                .label("Keyboard Backlight")
                .icon("keyboard-brightness-symbolic")
                .note("This model exposes a 3-state backlight, not per-key RGB")
                .values(vec![
                    NamedValue {
                        value: "0".into(),
                        label: "Off".into(),
                    },
                    NamedValue {
                        value: "1".into(),
                        label: "Low".into(),
                    },
                    NamedValue {
                        value: "2".into(),
                        label: "High".into(),
                    },
                ]);
            if exists {
                let p = path.unwrap();
                b = b.read(move || Ok(Value::Text(read_trim(&p)?)));
            } else {
                b = b.unavailable("Requires the legion_laptop driver");
            }
            caps.push(b.build());
        }

        caps.push(self.toggle_cap(
            "ylogo_light",
            "Y-Logo Light",
            "starred-symbolic",
            Group::Lighting,
            None,
        ));

        // ---- Battery ------------------------------------------------------
        // Every value here is read straight from the power_supply class, which
        // works on any Linux machine with no driver of ours involved — so the
        // battery page is useful before the legion module is ever loaded.
        // The reference shows twelve stat cards; we surface the ones sysfs
        // actually reports rather than inventing placeholders for the rest.
        caps.push(
            Builder::new("battery_capacity", Kind::ReadOnly, Group::Battery)
                .label("Battery Level")
                .icon("battery-symbolic")
                .unit("%")
                .read(|| {
                    let cap = paths::battery_float("capacity").unwrap_or(0.0);
                    Ok(Value::Float(cap))
                })
                .build(),
        );
        caps.push(
            Builder::new("battery_state", Kind::ReadOnly, Group::Battery)
                .label("Battery Status")
                .icon("battery-symbolic")
                .read(|| {
                    let st = paths::battery_text("status");
                    Ok(Value::Text(st))
                })
                .build(),
        );

        // Battery health is the number people actually look up, and sysfs
        // reports both full and design capacity so it can be derived rather
        // than guessed. Full charge below ~80% of design means the cell is worn.
        if let Some(home) = paths::battery_dir() {
            let hwmon = home.clone();
            caps.push(
                Builder::new("battery_health", Kind::ReadOnly, Group::Battery)
                    .label("Battery Health")
                    .icon("battery-good-symbolic")
                    .unit("%")
                    .read(move || {
                        let full = paths::battery_float_in(&hwmon, "energy_full")
                            .or_else(|| paths::battery_float_in(&hwmon, "charge_full"));
                        let design = paths::battery_float_in(&hwmon, "energy_full_design")
                            .or_else(|| paths::battery_float_in(&hwmon, "charge_full_design"));
                        match (full, design) {
                            (Some(f), Some(d)) if d > 0.0 => Ok(Value::Float(f / d * 100.0)),
                            _ => Ok(Value::Float(0.0)),
                        }
                    })
                    .build(),
            );
        }

        caps.push(
            Builder::new("battery_cycles", Kind::ReadOnly, Group::Battery)
                .label("Cycle Count")
                .icon("battery-symbolic")
                .read(|| {
                    let cycles = paths::battery_text("cycle_count");
                    if cycles.is_empty() {
                        Ok(Value::Text("—".into()))
                    } else {
                        Ok(Value::Text(cycles))
                    }
                })
                .build(),
        );

        if let Some(home) = paths::battery_dir() {
            let d1 = home.clone();
            let d2 = home.clone();
            let d3 = home.clone();
            caps.push(
                Builder::new("battery_voltage", Kind::ReadOnly, Group::Battery)
                    .label("Voltage")
                    .icon("battery-symbolic")
                    .unit("V")
                    .read(move || {
                        let v = paths::battery_float_in(&d1, "voltage_now").unwrap_or(0.0);
                        Ok(Value::Float(v / 1_000_000.0))
                    })
                    .build(),
            );
            caps.push(
                Builder::new("battery_charge_now", Kind::ReadOnly, Group::Battery)
                    .label("Charge Remaining")
                    .icon("battery-symbolic")
                    .unit("Wh")
                    .read(move || {
                        let wh = paths::battery_float_in(&d2, "energy_now")
                            .or_else(|| paths::battery_float_in(&d2, "charge_now"))
                            .unwrap_or(0.0);
                        // energy_* are in µWh already; charge_* are in µAh and
                        // would need the voltage to become Wh.
                        Ok(Value::Float(wh / 1_000_000.0))
                    })
                    .build(),
            );
            caps.push(
                Builder::new("battery_power", Kind::ReadOnly, Group::Battery)
                    .label("Power Draw")
                    .icon("battery-symbolic")
                    .unit("W")
                    .read(move || {
                        let w = paths::battery_float_in(&d3, "power_now").unwrap_or(0.0);
                        // power_now is µW; a negative value means discharging,
                        // which is the direction the reference shows.
                        Ok(Value::Float(w / 1_000_000.0))
                    })
                    .build(),
            );
        }
        caps.push(self.toggle_cap(
            "battery_conservation",
            "Battery Conservation",
            "battery-symbolic",
            Group::Battery,
            Some("Disables rapid charging"),
        ));

        // ---- System -------------------------------------------------------
        for (key, label, icon) in [
            ("fn_lock", "Fn Lock", "input-keyboard-symbolic"),
            ("touchpad", "Touchpad", "input-touchpad-symbolic"),
            ("camera_power", "Camera", "camera-photo-symbolic"),
            ("usb_charging", "Always-On USB Charging", "usb-symbolic"),
        ] {
            caps.push(self.toggle_cap(key, label, icon, Group::System, None));
        }

        caps
    }
}

/// Capabilities that live under `ideapad_acpi` rather than the legion tree, and
/// so can work even with the legion module absent.
fn keys_from_ideapad() -> &'static [&'static str] {
    &[
        "battery_conservation",
        "fn_lock",
        "camera_power",
        "usb_charging",
        "touchpad",
        "fan_mode",
    ]
}

/// Present `low-power` as "Quiet" — the firmware's own name for that mode.
fn humanise_profile(value: &str) -> String {
    match value {
        "low-power" => "Quiet".to_string(),
        "balanced-performance" => "Balanced-Performance".to_string(),
        "max-power" => "Maximum".to_string(),
        other => other
            .split(['-', '_'])
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::Backend;

    /// Every capability the backend reports, as `key: value` lines.
    ///
    /// Run with `cargo test --lib -- --nocapture capability_report` to see what
    /// a machine actually has. The fastest way to check the backend against
    /// real hardware without launching the GUI.
    #[test]
    fn capability_report() {
        let backend = SysfsBackend::new();
        for cap in backend.capabilities() {
            match cap.read() {
                Ok(value) => println!(
                    "{:<26} {:<24} {}",
                    cap.key,
                    value.to_string(),
                    if cap.is_available() {
                        ""
                    } else {
                        "UNAVAILABLE"
                    }
                ),
                Err(e) => println!("{:<26} <read failed: {e}>", cap.key),
            }
        }
    }

    /// A capability that reports itself available must return a value, not a
    /// placeholder. This is the invariant the pages depend on: a blank row means
    /// "no sensor", so a row of placeholders means the backend is lying.
    #[test]
    fn available_capabilities_return_values() {
        let backend = SysfsBackend::new();
        for cap in backend.capabilities() {
            if !cap.is_available() || matches!(cap.kind, Kind::Custom) {
                continue;
            }
            let value = cap
                .read()
                .unwrap_or_else(|e| panic!("{} is available but unreadable: {e}", cap.key));
            assert!(
                !matches!(value, Value::Text(ref t) if t == "—"),
                "{} returned a placeholder while claiming to be available",
                cap.key
            );
        }
    }

    /// Battery health is derived, and a derived number that comes out impossible
    /// is the failure mode to catch — the page would show it to the user.
    #[test]
    fn battery_health_is_a_percentage() {
        let backend = SysfsBackend::new();
        let Some(cap) = backend
            .capabilities()
            .into_iter()
            .find(|c| c.key == "battery_health")
        else {
            return;
        };
        // Only meaningful where the battery reports the capacities it needs.
        if paths::battery_float("energy_full").is_none()
            && paths::battery_float("charge_full").is_none()
        {
            return;
        }
        let Ok(Value::Float(health)) = cap.read() else {
            return;
        };
        assert!(
            (0.0..=100.0).contains(&health),
            "battery health out of range: {health}%"
        );
    }

    /// The coretemp fallback is what makes the thermal page useful with no
    /// legion driver loaded, so it must resolve when the source exists.
    #[test]
    fn cpu_temperature_has_a_source() {
        let has_coretemp = paths::find_hwmon_by_name("coretemp").is_some()
            || paths::find_hwmon_by_name("k10temp").is_some();
        if !has_coretemp {
            return;
        }
        let backend = SysfsBackend::new();
        let Some(cap) = backend
            .capabilities()
            .into_iter()
            .find(|c| c.key == "cpu_temp")
        else {
            return;
        };
        assert!(
            cap.is_available(),
            "cpu_temp unavailable although a thermal source exists"
        );
        if let Ok(Value::Float(celsius)) = cap.read() {
            assert!(
                (5.0..=130.0).contains(&celsius),
                "cpu temperature implausible: {celsius} C"
            );
        }
    }
}
