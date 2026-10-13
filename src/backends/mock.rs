//! A backend with no hardware dependency, so the UI can be developed and
//! screenshotted anywhere.
//!
//! It mirrors the *capability set of a Legion 5 15IRX10* — including the
//! capabilities that are absent on that machine — so what you see here is what
//! a user of that laptop sees. That is deliberate: if a control shows up in
//! the mock that should be suppressed on real hardware, the mock is lying and
//! the bug is in the capability model.

use crate::backends::Backend;
use crate::capability::{Builder, Group, Kind, Limits, NamedValue, Value};

pub struct MockBackend;

impl Backend for MockBackend {
    fn name(&self) -> &str {
        "mock (legion 5 15IRX10, QNCN38WW)"
    }

    fn capabilities(&self) -> Vec<crate::capability::Capability> {
        let power_modes = || {
            vec![
                NamedValue {
                    value: "0".into(),
                    label: "Quiet".into(),
                },
                NamedValue {
                    value: "1".into(),
                    label: "Balanced".into(),
                },
                NamedValue {
                    value: "2".into(),
                    label: "Performance".into(),
                },
                NamedValue {
                    value: "3".into(),
                    label: "Custom".into(),
                },
            ]
        };

        let mut caps: Vec<crate::capability::Capability> = Vec::new();

        // ---- Power --------------------------------------------------------
        caps.push(
            Builder::new("powermode", Kind::Enum, Group::Power)
                .label("Power Mode")
                .icon("power-profile-performance-symbolic")
                .values(power_modes())
                .read(|| Ok(Value::Text("1".into())))
                .write(|_| Ok(()))
                .build(),
        );

        // Marked dangerous so the confirmation-dialog path can be exercised
        // without the hardware that actually requires it.
        let mut max_fan = Builder::new("fan_fullspeed", Kind::Toggle, Group::Power)
            .label("Maximum Fan Speed")
            .icon("fan-symbolic")
            .note("Requires custom power mode on this model")
            .read(|| Ok(Value::Bool(false)))
            .write(|_| Ok(()))
            .build();
        max_fan.dangerous = true;
        caps.push(max_fan);

        // ---- Thermal ------------------------------------------------------
        // Absent on QNCN: access_method_fancurve == NO_ACCESS.
        caps.push(
            Builder::new("fancurve", Kind::Custom, Group::Thermal)
                .label("Fan Curve")
                .icon("air-conditioner-symbolic")
                .unavailable("Fan curves are not supported by this model's firmware")
                .read(|| Ok(Value::Text("\u{2014}".into())))
                .build(),
        );
        for (key, label, icon, value, unit) in [
            (
                "cpu_temp",
                "CPU Temperature",
                "cpu-symbolic",
                48.0,
                "\u{00b0}C",
            ),
            (
                "gpu_temp",
                "GPU Temperature",
                "video-display-symbolic",
                41.0,
                "\u{00b0}C",
            ),
            ("fan1_rpm", "CPU Fan", "fan-symbolic", 0.0, "rpm"),
            ("fan2_rpm", "GPU Fan", "fan-symbolic", 0.0, "rpm"),
        ] {
            caps.push(
                Builder::new(key, Kind::ReadOnly, Group::Thermal)
                    .label(label)
                    .icon(icon)
                    .unit(unit)
                    .read(move || Ok(Value::Float(value)))
                    .build(),
            );
        }

        // ---- Lighting -----------------------------------------------------
        caps.push(
            Builder::new("kbd_backlight", Kind::Enum, Group::Lighting)
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
                ])
                .read(|| Ok(Value::Text("1".into())))
                .write(|_| Ok(()))
                .build(),
        );
        caps.push(
            Builder::new("ylogo_light", Kind::Toggle, Group::Lighting)
                .label("Y-Logo Light")
                .icon("starred-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
        );

        // ---- Battery ------------------------------------------------------
        caps.push(
            Builder::new("battery_capacity", Kind::ReadOnly, Group::Battery)
                .label("Battery Level")
                .icon("battery-symbolic")
                .unit("%")
                .read(|| Ok(Value::Float(68.0)))
                .build(),
        );
        caps.push(
            Builder::new("battery_state", Kind::ReadOnly, Group::Battery)
                .label("Battery Status")
                .icon("battery-symbolic")
                .read(|| Ok(Value::Text("Charging".into())))
                .build(),
        );
        caps.push(
            Builder::new("battery_conservation", Kind::Toggle, Group::Battery)
                .label("Battery Conservation")
                .icon("battery-symbolic")
                .note("Disables rapid charging")
                .read(|| Ok(Value::Bool(false)))
                .write(|_| Ok(()))
                .build(),
        );

        // ---- System -------------------------------------------------------
        for (key, label, icon, on) in [
            ("fn_lock", "Fn Lock", "input-keyboard-symbolic", true),
            ("touchpad", "Touchpad", "input-touchpad-symbolic", true),
            ("camera_power", "Camera", "camera-photo-symbolic", true),
            (
                "usb_charging",
                "Always-On USB Charging",
                "usb-symbolic",
                false,
            ),
        ] {
            caps.push(
                Builder::new(key, Kind::Toggle, Group::System)
                    .label(label)
                    .icon(icon)
                    .read(move || Ok(Value::Bool(on)))
                    .write(|_| Ok(()))
                    .build(),
            );
        }

        // ---- Power limits: absent on QNCN, kept for the model matrix -------
        caps.push(
            Builder::new("cpu_longterm_powerlimit", Kind::Slider, Group::Power)
                .label("CPU Long-Term Power Limit")
                .icon("power-profile-performance-symbolic")
                .unit("W")
                .limits(Limits {
                    min: 5.0,
                    max: 200.0,
                    step: 1.0,
                })
                .unavailable("Power limits are not exposed by this model's firmware")
                .read(|| Ok(Value::Int(0)))
                .build(),
        );

        caps
    }
}
