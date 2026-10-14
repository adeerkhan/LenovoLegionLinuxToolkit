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
use crate::system;

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
        key: &str,
        label: &str,
        icon: &'static str,
        group: Group,
        fallback: &[(&str, &str)],
    ) -> Capability {
        let path = self.paths.file(key);
        let available = path.as_ref().map(|p| p.exists()).unwrap_or(false);

        let mut values: Vec<NamedValue> = choices_for(key)
            .and_then(|c| read_trim(&c).ok())
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
        key: &str,
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
        key: &str,
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
            "Needs the legion_laptop kernel module (see About)"
        } else {
            "Not supported by this model"
        }
    }

    /// Whether the module's support probe for a control says yes.
    ///
    /// `issupportigpumode` / `issupportgsync` exist on every model but report
    /// `0` where the firmware has no such control, so probing them keeps a dead
    /// toggle off a machine that cannot use it. A missing or unreadable probe
    /// does not suppress the control.
    fn legion_supports(&self, attr: &str) -> bool {
        match self.paths.file(attr).and_then(|p| read_trim(&p).ok()) {
            Some(value) => support_value_says_yes(&value),
            None => true,
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
        {
            let mut power = self.enum_cap(
                "powermode",
                "Power Mode",
                "power-profile-performance-symbolic",
                Group::Power,
                &[("0", "Quiet"), ("1", "Balanced"), ("2", "Performance")],
            );
            power
                .infos
                .push("Custom mode unlocks the fan and power-limit controls.");
            caps.push(power);
        }
        // Upstream documents that a fan-full-speed write through the wrong path
        // can hold the fans at maximum until reboot on this EC generation, so
        // it asks for confirmation first.
        let mut max_fan = self.toggle_cap(
            "fan_fullspeed",
            "Maximum Fan Speed",
            "weather-windy-symbolic",
            Group::Power,
            None,
        );
        // The reference communicates a consequence on the card itself, before
        // the user touches the control. This is the one write in the app that
        // can leave hardware in a state the UI cannot undo.
        max_fan
            .warnings
            .push("A write on the wrong path can hold the fans at maximum until reboot.");
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
                .icon("org.gnome.Settings-device-diagnostics-symbolic")
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

        caps.push(self.sensor_cap(
            "ic_temp",
            "Chipset Temperature",
            "application-x-firmware-symbolic",
            Group::Thermal,
            "°C",
            true,
        ));

        for (key, label) in [
            ("fan1_rpm", "CPU Fan"),
            ("fan2_rpm", "GPU Fan"),
            ("fan3_rpm", "Auxiliary Fan"),
            ("fan4_rpm", "Fourth Fan"),
        ] {
            caps.push(self.sensor_cap(
                key,
                label,
                "weather-windy-symbolic",
                Group::Thermal,
                "rpm",
                false,
            ));
        }

        // ---- Firmware attributes, discovered -------------------------------
        //
        // The kernel's `firmware-attributes` class exposes the BIOS's own
        // tunables, and the in-tree `lenovo-wmi-other` driver populates it. Each
        // attribute carries min, max and increment, so these controls are
        // *generated* rather than written out per model — which is the only way
        // to cover machines whose BIOS names them differently.
        //
        // Ten controls that used to read "not exposed by this model's firmware"
        // come from here.
        for attribute in system::firmware_attributes() {
            let writable = attribute.is_writable();
            let name = attribute.name.clone();
            let key = format!("{}{}", crate::policy::FIRMWARE_PREFIX, name);

            let mut b = Builder::new(key.clone(), Kind::Slider, Group::Processors)
                .label(if attribute.description.is_empty() {
                    name.clone()
                } else {
                    attribute.description.clone()
                })
                .icon("power-profile-performance-symbolic")
                .limits(Limits {
                    min: attribute.min,
                    max: attribute.max,
                    step: attribute.step,
                });

            if let Some(unit) = attribute.unit {
                b = b.unit(unit);
            }

            if writable {
                let read_path = system::firmware_attribute_path(&name);
                b = b.read(move || {
                    let value = read_path
                        .as_ref()
                        .and_then(|p| read_trim(p).ok())
                        .and_then(|raw| raw.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    Ok(Value::Float(value))
                });
                b = b.write(move |value| {
                    crate::services::privilege::request(&key, &value.to_string())?;
                    Ok(())
                });
            } else {
                // `min == max` means the firmware is refusing writes in the
                // current power mode. A slider with no range is a broken
                // control, so the card says why instead.
                b = b.unavailable(
                    "The firmware is not accepting changes to this in the current power mode",
                );
            }

            caps.push(b.build());
        }

        // ---- GPU, via nvidia-smi ------------------------------------------
        //
        // No sysfs equivalent exists for GPU telemetry on NVIDIA hardware, and
        // the legion module does not expose it either. `system::gpu()` reads a
        // cache filled by a background poller, so nothing here forks a process.
        if let Some(gpu) = system::gpu() {
            for (key, label, unit, friendly, value) in [
                (
                    "gpu_temp",
                    "GPU Temperature",
                    "°C",
                    "GPU",
                    gpu.temperature_c,
                ),
                (
                    "gpu_utilization",
                    "GPU Utilization",
                    "%",
                    "GPU",
                    gpu.utilization_percent,
                ),
                ("gpu_clock", "GPU Core Clock", "MHz", "GPU", gpu.clock_mhz),
                ("gpu_power", "GPU Power", "W", "GPU", gpu.power_w),
            ] {
                let mut b = Builder::new(key, Kind::ReadOnly, Group::Processors)
                    .label(label)
                    .icon("video-display-symbolic")
                    .unit(unit);
                if value.is_none() {
                    // The card exists but this sensor does not report on this
                    // card — distinct from the GPU being absent entirely.
                    b = b.unavailable("Not reported by this GPU");
                } else {
                    b = b.read(move || {
                        let reading = system::gpu();
                        let value = match key {
                            "gpu_temp" => reading.and_then(|g| g.temperature_c),
                            "gpu_utilization" => reading.and_then(|g| g.utilization_percent),
                            "gpu_clock" => reading.and_then(|g| g.clock_mhz),
                            "gpu_power" => reading.and_then(|g| g.power_w),
                            _ => None,
                        };
                        Ok(Value::Float(value.unwrap_or(0.0)))
                    });
                }
                let _ = friendly;
                caps.push(b.build());
            }

            if let (Some(used), Some(total)) = (gpu.memory_used_mib, gpu.memory_total_mib) {
                caps.push(
                    Builder::new("gpu_memory", Kind::ReadOnly, Group::Processors)
                        .label("GPU Memory")
                        .icon("video-display-symbolic")
                        .unit("MiB")
                        .note(format!("{used:.0} of {total:.0} MiB"))
                        .read(|| {
                            Ok(Value::Float(
                                system::gpu().and_then(|g| g.memory_used_mib).unwrap_or(0.0),
                            ))
                        })
                        .build(),
                );
            }
        } else {
            // No NVIDIA GPU, or `nvidia-smi` is not installed. Say which by
            // naming the tool, so the fix is obvious.
            caps.push(
                Builder::new("gpu_temp", Kind::ReadOnly, Group::Processors)
                    .label("GPU Temperature")
                    .icon("video-display-symbolic")
                    .unit("°C")
                    .unavailable("Needs nvidia-smi or the legion_laptop kernel module")
                    .read(|| Ok(Value::Float(0.0)))
                    .build(),
            );
        }

        // ---- Generic telemetry, needing no Legion driver -------------------
        //
        // The reference's sensor cards show utilization and clocks alongside
        // temperatures. Everything below comes from /proc and cpufreq, so it
        // works with legion_laptop unloaded — which is the difference between a
        // grid that reads as "unsupported machine" and one that reads as a
        // monitoring panel.
        caps.push(
            Builder::new("cpu_utilization", Kind::ReadOnly, Group::Processors)
                .label("CPU Utilization")
                .icon("org.gnome.Settings-device-diagnostics-symbolic")
                .unit("%")
                .read(|| Ok(Value::Float(system::cpu_percent().unwrap_or(0.0))))
                .build(),
        );
        // CPU package power, from RAPL. Absent where the counter is root-only,
        // which is the honest outcome rather than a fixed zero.
        if system::cpu_power_available() {
            caps.push(
                Builder::new("cpu_power", Kind::ReadOnly, Group::Processors)
                    .label("CPU Package Power")
                    .icon("org.gnome.Settings-device-diagnostics-symbolic")
                    .unit("W")
                    .read(|| Ok(Value::Float(system::cpu_power_w().unwrap_or(0.0))))
                    .build(),
            );
        }
        if system::cpu_clock_mhz().is_some() {
            caps.push(
                Builder::new("cpu_clock", Kind::ReadOnly, Group::Processors)
                    .label("CPU Core Clock")
                    .icon("org.gnome.Settings-device-diagnostics-symbolic")
                    .unit("MHz")
                    .read(|| Ok(Value::Float(system::cpu_clock_mhz().unwrap_or(0.0))))
                    .build(),
            );
        }
        caps.push(
            Builder::new("memory_utilization", Kind::ReadOnly, Group::Thermal)
                .label("Memory Utilization")
                .icon("media-flash-symbolic")
                .unit("%")
                .read(|| Ok(Value::Float(system::memory().map_or(0.0, |m| m.percent))))
                .build(),
        );
        if system::swap_percent().is_some() {
            caps.push(
                Builder::new("swap_utilization", Kind::ReadOnly, Group::Thermal)
                    .label("Swap Utilization")
                    .icon("media-flash-symbolic")
                    .unit("%")
                    .read(|| Ok(Value::Float(system::swap_percent().unwrap_or(0.0))))
                    .build(),
            );
        }
        if let Some((percent, used, total)) = system::disk_usage("/") {
            caps.push(
                Builder::new("disk_usage", Kind::ReadOnly, Group::Thermal)
                    .label("Disk Usage")
                    .icon("drive-harddisk-symbolic")
                    .unit("%")
                    .note(format!("{used:.0} GiB of {total:.0} GiB"))
                    .read(|| {
                        Ok(Value::Float(
                            system::disk_usage("/").map(|d| d.0).unwrap_or(0.0),
                        ))
                    })
                    .build(),
            );
            let _ = percent;
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
                .icon("weather-windy-symbolic")
                .unavailable("Fan curves are not supported by this model's firmware")
                .info(
                    "A fan curve maps temperature to fan speed, so the fans track \
                     the workload instead of the firmware's fixed steps.",
                )
                .read(|| Ok(Value::Text("\u{2014}".into())))
                .build(),
        );

        // ---- Lighting -----------------------------------------------------
        {
            // The keyboard backlight is a 3-state leds-class device
            // (off / low / high) with max_brightness = 2.
            let path = self.paths.file("kbd_backlight");
            let exists = path.as_ref().map(|p| p.exists()).unwrap_or(false);
            // On/off only. The three WMI brightness levels do nothing on this
            // keyboard — the 24-zone controller owns it — and a switch that
            // changes nothing is worse than no switch.
            let mut b = Builder::new("kbd_backlight", Kind::Toggle, Group::Lighting)
                .label("Keyboard Backlight")
                .icon("keyboard-brightness-symbolic")
                .note("Switches the backlight on or off. Colour and effects are on the Keyboard RGB card.");
            if exists {
                let read_path = path.unwrap();
                b = b.read(move || Ok(Value::Bool(read_trim(&read_path)? != "0")));
                b = b.write(|v| {
                    let level = if matches!(v, Value::Bool(true)) {
                        "2"
                    } else {
                        "0"
                    };
                    crate::services::privilege::request("kbd_backlight", level)?;
                    Ok(())
                });
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

        // The rear IO-port ambient light. `paths.rs` has resolved its leds path
        // all along; it was simply never exposed as a control.
        caps.push(self.toggle_cap(
            "ioport_light",
            "IO-Port Light",
            "drive-harddisk-usb-symbolic",
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
        // Time remaining, derived rather than reported: sysfs on this machine
        // exposes no `time_to_empty_now`, but energy and rate are both present
        // and the quotient is the same number.
        caps.push(
            Builder::new("battery_time", Kind::ReadOnly, Group::Battery)
                .label("Time Remaining")
                .icon("preferences-system-time-symbolic")
                .unit("h")
                .read(|| {
                    Ok(Value::Float(
                        paths::battery_time_hours().map(|t| t.0).unwrap_or(0.0),
                    ))
                })
                .build(),
        );

        // The reference shows discharge extremes and time on battery. sysfs
        // reports only the instantaneous rate, so these are observed over time
        // by the refresh tick that already runs.
        caps.push(
            Builder::new("battery_discharge_min", Kind::ReadOnly, Group::Battery)
                .label("Min Discharge Rate")
                .icon("battery-symbolic")
                .unit("W")
                .note("Lowest seen this session")
                // `0.0 W` would assert a measurement that was never taken. Say
                // so instead.
                .read(|| {
                    Ok(match system::discharge_min_w() {
                        Some(watts) => Value::Float(watts),
                        None => Value::Text("None yet".into()),
                    })
                })
                .build(),
        );
        caps.push(
            Builder::new("battery_discharge_max", Kind::ReadOnly, Group::Battery)
                .label("Max Discharge Rate")
                .icon("battery-symbolic")
                .unit("W")
                .note("Highest seen this session")
                .read(|| {
                    Ok(match system::discharge_max_w() {
                        Some(watts) => Value::Float(watts),
                        None => Value::Text("None yet".into()),
                    })
                })
                .build(),
        );
        caps.push(
            Builder::new("battery_on_battery_since", Kind::ReadOnly, Group::Battery)
                .label("On Battery Since")
                .icon("preferences-system-time-symbolic")
                .note("Counted since this session first saw the battery discharging")
                .read(|| {
                    // "On adapter" rather than a dash. While plugged in there is
                    // genuinely no duration, and saying so is more useful than a
                    // placeholder that reads as "no data".
                    Ok(Value::Text(match system::on_battery_seconds() {
                        Some(secs) => system::format_duration(secs),
                        None => "On adapter".into(),
                    }))
                })
                .build(),
        );

        if let Some(home) = paths::battery_dir() {
            let full_dir = home.clone();
            let design_dir = home.clone();
            caps.push(
                Builder::new("battery_full_capacity", Kind::ReadOnly, Group::Battery)
                    .label("Full Charge Capacity")
                    .icon("battery-symbolic")
                    .unit("Wh")
                    .read(move || {
                        let wh = paths::battery_float_in(&full_dir, "energy_full")
                            .or_else(|| paths::battery_float_in(&full_dir, "charge_full"))
                            .unwrap_or(0.0);
                        Ok(Value::Float(wh / 1_000_000.0))
                    })
                    .build(),
            );
            caps.push(
                Builder::new("battery_design_capacity", Kind::ReadOnly, Group::Battery)
                    .label("Design Capacity")
                    .icon("battery-symbolic")
                    .unit("Wh")
                    .read(move || {
                        let wh = paths::battery_float_in(&design_dir, "energy_full_design")
                            .or_else(|| paths::battery_float_in(&design_dir, "charge_full_design"))
                            .unwrap_or(0.0);
                        Ok(Value::Float(wh / 1_000_000.0))
                    })
                    .build(),
            );
        }

        // ---- Battery charging mode ---------------------------------------
        //
        // Prefer the kernel's native `charge_types` interface: it lists every
        // accepted charge type with the current one bracketed, and the kernel
        // enforces the Fast/Long_Life exclusivity, so the three-way control
        // needs no software interlock. Only when that interface is absent do we
        // fall back to the ideapad conservation toggle and, if the module is
        // loaded, its rapid-charge toggle.
        let charge_types = paths::battery_charge_types();
        let parsed = charge_types
            .as_ref()
            .and_then(|p| read_trim(p).ok())
            .map(|raw| paths::parse_charge_types(&raw));

        match (charge_types, parsed) {
            (Some(path), Some(parsed)) if !parsed.values.is_empty() => {
                let values: Vec<NamedValue> = parsed
                    .values
                    .iter()
                    .map(|(value, label)| NamedValue {
                        value: value.clone(),
                        label: label.clone(),
                    })
                    .collect();
                let read_path = path.clone();
                caps.push(
                    Builder::new("battery_mode", Kind::Enum, Group::Battery)
                        .label("Battery Mode")
                        .icon("battery-symbolic")
                        .note(
                            "Normal charges to full; Conservation holds around 60%; \
                             Rapid charges fastest.",
                        )
                        .values(values)
                        .read(move || {
                            let raw = read_trim(&read_path)?;
                            Ok(Value::Text(
                                paths::parse_charge_types(&raw).current.unwrap_or_default(),
                            ))
                        })
                        .write(|v| {
                            crate::services::privilege::request("battery_mode", &v.to_string())?;
                            Ok(())
                        })
                        .build(),
                );
            }
            _ => {
                caps.push(self.toggle_cap(
                    "battery_conservation",
                    "Battery Conservation",
                    "battery-symbolic",
                    Group::Battery,
                    Some("Disables rapid charging"),
                ));

                // The module adds a rapid-charge toggle, mutually exclusive with
                // conservation. The driver clears one when the other is set, so
                // the card says so rather than the UI pretending to enforce it.
                let mut rapid = self.toggle_cap(
                    "rapid_charge",
                    "Rapid Charge",
                    "battery-symbolic",
                    Group::Battery,
                    Some("Enabling this clears battery conservation"),
                );
                if rapid.is_available() {
                    rapid
                        .warnings
                        .push("Enabling rapid charge clears battery conservation, and vice versa.");
                }
                caps.push(rapid);
            }
        }

        // A stop-charging threshold, where the kernel exposes one. Absent on
        // machines whose firmware offers only charge types (this one), so the
        // control is simply not shown there rather than faked.
        if let Some(path) = paths::battery_charge_limit() {
            let read_path = path.clone();
            caps.push(
                Builder::new("battery_charge_limit", Kind::Slider, Group::Battery)
                    .label("Charge Limit")
                    .icon("battery-symbolic")
                    .unit("%")
                    .limits(Limits {
                        min: 50.0,
                        max: 100.0,
                        step: 5.0,
                    })
                    .note("Stop charging at this level.")
                    .read(move || {
                        Ok(Value::Float(
                            read_trim(&read_path)?.parse().unwrap_or(100.0),
                        ))
                    })
                    .write(|v| {
                        crate::services::privilege::request(
                            "battery_charge_limit",
                            &v.to_string(),
                        )?;
                        Ok(())
                    })
                    .build(),
            );
        }

        // ---- System -------------------------------------------------------
        for (key, label, icon) in [
            ("fn_lock", "Fn Lock", "input-keyboard-symbolic"),
            ("touchpad", "Touchpad", "input-touchpad-symbolic"),
            ("camera_power", "Camera", "camera-photo-symbolic"),
            (
                "usb_charging",
                "Always-On USB Charging",
                "drive-harddisk-usb-symbolic",
            ),
        ] {
            caps.push(self.toggle_cap(key, label, icon, Group::System, None));
        }

        // ---- GPU working mode and hybrid/G-Sync ---------------------------
        //
        // These live only in the out-of-tree module's sysfs tree. Without the
        // module they read "Needs the legion_laptop kernel module"; with it, a
        // model whose firmware does not back the control says so rather than
        // offering a toggle that would fail.
        {
            let path = self.paths.file("igpumode").filter(|p| p.exists());
            let mut b = Builder::new("igpumode", Kind::Enum, Group::Processors)
                .label("GPU Working Mode")
                .icon("video-display-symbolic")
                .note("Takes effect after a reboot.")
                .values(vec![
                    NamedValue {
                        value: "0".into(),
                        label: "Hybrid".into(),
                    },
                    NamedValue {
                        value: "1".into(),
                        label: "Integrated Only".into(),
                    },
                    NamedValue {
                        value: "2".into(),
                        label: "Hybrid Auto".into(),
                    },
                ]);
            match path {
                Some(p) if self.legion_supports("issupportigpumode") => {
                    let read_path = p.clone();
                    b = b.read(move || Ok(Value::Text(read_trim(&read_path)?)));
                    b = b.write(|v| {
                        crate::services::privilege::request("igpumode", &v.to_string())?;
                        Ok(())
                    });
                }
                Some(_) => b = b.unavailable("Not supported by this model's firmware"),
                None => b = b.unavailable(self.reason_for("igpumode")),
            }
            caps.push(b.build());
        }

        // `gsync` is the attribute LLL drives as "Hybrid Mode (sometimes also
        // G-Sync)", so the label names both.
        let mut gsync = self.toggle_cap(
            "gsync",
            "G-Sync / Hybrid Mode",
            "video-display-symbolic",
            Group::Processors,
            Some("Takes effect after a reboot."),
        );
        if gsync.is_available() && !self.legion_supports("issupportgsync") {
            gsync.unavailable_reason = Some("Not supported by this model's firmware");
        }
        caps.push(gsync);

        // ---- Module-gated system toggles ----------------------------------
        for (key, label, icon, note) in [
            (
                "winkey",
                "Windows Key",
                "input-keyboard-symbolic",
                Some("Enable or disable the Windows key."),
            ),
            (
                "overdrive",
                "Display Overdrive",
                "video-display-symbolic",
                None,
            ),
            (
                "instant_boot_ac",
                "Instant Boot (AC Adapter)",
                "power-profile-performance-symbolic",
                Some("Powers the laptop on when the AC adapter is connected."),
            ),
            (
                "instant_boot_usb_pd",
                "Instant Boot (USB-PD)",
                "power-profile-performance-symbolic",
                Some("Powers the laptop on when a USB-PD charger is connected."),
            ),
        ] {
            caps.push(self.toggle_cap(key, label, icon, Group::System, note));
        }

        caps
    }
}

/// Where an enum capability's list of choices lives.
///
/// Not simply the sibling `choices` file: the standard ACPI platform profile
/// keeps its list in `platform_profile_choices`, a different name in a
/// different place than the legion module's copy.
fn choices_for(key: &str) -> Option<std::path::PathBuf> {
    match key {
        "powermode" | "platform_profile" => paths::platform_profile_choices(),
        // The legion module writes a plain `choices` beside the attribute.
        _ => None,
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

/// Interpret an `issupport…` probe.
///
/// The probe returns non-zero when the firmware backs a control, and the value
/// is not always `1`: on a Legion 5 15IRX10 `issupportigpumode` reads `3` and
/// `issupportgsync` reads `2`. The Windows reference tests `result > 0`; testing
/// for `1` hid both controls on a machine that supports them.
fn support_value_says_yes(raw: &str) -> bool {
    raw.trim().parse::<i64>().map(|n| n > 0).unwrap_or(true)
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

    /// The two cards the reference uses status rows for must carry them: fan
    /// control warns about the consequence of a wrong-path write, and the fan
    /// curve explains what it would do on a machine that supports it. Neither
    /// is visible in a widget dump, so it is asserted here.
    #[test]
    fn status_rows_are_populated_where_the_reference_has_them() {
        let caps = SysfsBackend::new().capabilities();

        let fan = caps
            .iter()
            .find(|c| c.key == "fan_fullspeed")
            .expect("fan_fullspeed capability");
        assert!(
            !fan.warnings.is_empty(),
            "fan control must carry its fan-latching warning row"
        );

        let curve = caps
            .iter()
            .find(|c| c.key == "fancurve")
            .expect("fancurve capability");
        assert!(
            !curve.infos.is_empty(),
            "the fan-curve card must carry an info row"
        );
    }

    /// Item 5: the reference's battery card set, including the rates that can
    /// only come from sampling.
    #[test]
    fn battery_page_capabilities_are_present() {
        let caps = SysfsBackend::new().capabilities();
        let has = |key: &str| caps.iter().any(|c| c.key == key);

        for key in [
            "battery_state",
            "battery_capacity",
            "battery_time",
            "battery_power",
            "battery_discharge_min",
            "battery_discharge_max",
            "battery_charge_now",
            "battery_full_capacity",
            "battery_design_capacity",
            "battery_health",
            "battery_on_battery_since",
            "battery_voltage",
            "battery_cycles",
        ] {
            assert!(has(key), "battery page is missing {key}");
        }
    }

    /// When the kernel's native charge-type interface is present, the three-way
    /// Battery Mode replaces the conservation toggle: the control belongs to the
    /// interface the firmware actually enforces the Fast/Long_Life exclusivity
    /// on, and having both would be two controls for one state.
    #[test]
    fn battery_mode_replaces_conservation_when_charge_types_exists() {
        let caps = SysfsBackend::new().capabilities();
        if paths::battery_charge_types().is_none() {
            return;
        }
        let mode = caps
            .iter()
            .find(|c| c.key == "battery_mode")
            .expect("charge_types exists but there is no battery_mode capability");
        assert!(mode.is_available(), "battery_mode should be available");
        assert!(
            !mode.values.is_empty(),
            "battery_mode has no choices parsed from charge_types"
        );
        assert!(
            !caps.iter().any(|c| c.key == "battery_conservation"),
            "the conservation toggle must not duplicate battery_mode"
        );
        if let Ok(Value::Text(current)) = mode.read() {
            assert!(
                mode.values.iter().any(|v| v.value == current),
                "battery_mode read {current:?}, which is not one of its choices"
            );
        }
    }

    /// The module-gated controls must always be declared, so that with the
    /// module loaded they appear, and without it they explain the driver
    /// dependency rather than vanishing.
    #[test]
    fn module_gated_controls_are_declared() {
        let caps = SysfsBackend::new().capabilities();
        for key in [
            "igpumode",
            "gsync",
            "winkey",
            "overdrive",
            "instant_boot_ac",
            "instant_boot_usb_pd",
        ] {
            assert!(
                caps.iter().any(|c| c.key == key),
                "module-gated control {key} is not declared"
            );
        }
    }

    /// The support probe is non-zero when supported, and its value is not
    /// always `1`: a Legion 5 15IRX10 reports `3` for igpumode and `2` for
    /// gsync. Reading it as `== 1` hid both on a machine that supports them.
    #[test]
    fn support_probes_are_read_as_non_zero() {
        assert!(support_value_says_yes("1"));
        assert!(support_value_says_yes("2"));
        assert!(support_value_says_yes("3"));
        assert!(!support_value_says_yes("0"));
        // An unreadable probe must not suppress the control.
        assert!(support_value_says_yes(""));
        assert!(support_value_says_yes("nonsense"));
    }

    /// A capability that renders as a control must be able to write.
    ///
    /// A switch or dropdown that draws but does nothing is the worst failure a
    /// control panel has, and `kbd_backlight` was exactly that: it declared a
    /// read closure and no write, so the Lighting page drew a working-looking
    /// dropdown whose handler was never connected. This test is the guard.
    #[test]
    fn available_controls_are_writable() {
        for cap in SysfsBackend::new().capabilities() {
            if !cap.is_available() {
                continue;
            }
            if matches!(cap.kind, Kind::Toggle | Kind::Enum | Kind::Slider) {
                assert!(
                    cap.can_write(),
                    "{} renders a control but cannot write",
                    cap.key
                );
            }
        }
    }

    /// Every icon a capability declares must be one the theme can draw.
    ///
    /// GTK renders a missing icon as a placeholder rather than failing, so this
    /// is invisible to every other test — seven names were wrong and the cards
    /// were drawing placeholders. No-op on a machine with no icon theme.
    #[test]
    fn every_capability_icon_exists() {
        for cap in SysfsBackend::new().capabilities() {
            assert!(
                crate::icons::exists(cap.icon),
                "capability {:?} declares icon {:?}, which the theme cannot draw",
                cap.key,
                cap.icon
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
