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
        caps.push(self.toggle_cap(
            "fan_fullspeed",
            "Maximum Fan Speed",
            "fan-symbolic",
            Group::Power,
            Some("Apply sparingly; on this EC a wrong-path write can latch the fans."),
        ));

        // ---- Thermal ------------------------------------------------------
        for (key, label, icon) in [
            ("cpu_temp", "CPU Temperature", "cpu-symbolic"),
            ("gpu_temp", "GPU Temperature", "video-display-symbolic"),
            ("ic_temp", "Chipset Temperature", "motherboard-symbolic"),
            ("fan1_rpm", "CPU Fan", "fan-symbolic"),
            ("fan2_rpm", "GPU Fan", "fan-symbolic"),
        ] {
            let unit = match key {
                "cpu_temp" | "gpu_temp" | "ic_temp" => "\u{00b0}C",
                _ => "rpm",
            };
            caps.push(self.sensor_cap(
                key,
                label,
                icon,
                Group::Thermal,
                unit,
                key.ends_with("_temp"),
            ));
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
        caps.push(
            Builder::new("battery_capacity", Kind::ReadOnly, Group::Battery)
                .label("Battery Level")
                .icon("battery-symbolic")
                .unit("%")
                .read(|| {
                    let cap = paths::find_battery_file("capacity")
                        .and_then(|p| read_trim(&p).ok())
                        .unwrap_or_default();
                    Ok(Value::Float(cap.parse().unwrap_or(0.0)))
                })
                .build(),
        );
        caps.push(
            Builder::new("battery_state", Kind::ReadOnly, Group::Battery)
                .label("Battery Status")
                .icon("battery-symbolic")
                .read(|| {
                    let st = paths::find_battery_file("status")
                        .and_then(|p| read_trim(&p).ok())
                        .unwrap_or_else(|| "Unknown".into());
                    Ok(Value::Text(st))
                })
                .build(),
        );
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
