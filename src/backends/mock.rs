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

        vec![
            // ---- Power ----------------------------------------------------
            Builder::new("powermode", Kind::Enum, Group::Power)
                .label("Power Mode")
                .icon("power-profile-performance-symbolic")
                .values(power_modes())
                .read(|| Ok(Value::Text("1".into())))
                .write(|_| Ok(()))
                .build(),
            Builder::new("fan_fullspeed", Kind::Toggle, Group::Power)
                .label("Maximum Fan Speed")
                .icon("fan-symbolic")
                .note("Requires custom power mode on this model")
                .read(|| Ok(Value::Bool(false)))
                .write(|_| Ok(()))
                .build(),
            // ---- Thermal --------------------------------------------------
            // Absent on QNCN: access_method_fancurve == NO_ACCESS.
            Builder::new("fancurve", Kind::Custom, Group::Thermal)
                .label("Fan Curve")
                .icon("air-conditioner-symbolic")
                .unavailable("Fan curves are not supported by this model's firmware")
                .read(|| Ok(Value::Text("—".into())))
                .build(),
            Builder::new("cpu_temp", Kind::ReadOnly, Group::Thermal)
                .label("CPU Temperature")
                .icon("cpu-symbolic")
                .unit("°C")
                .read(|| Ok(Value::Float(48.0)))
                .build(),
            Builder::new("gpu_temp", Kind::ReadOnly, Group::Thermal)
                .label("GPU Temperature")
                .icon("video-display-symbolic")
                .unit("°C")
                .read(|| Ok(Value::Float(41.0)))
                .build(),
            Builder::new("fan1_rpm", Kind::ReadOnly, Group::Thermal)
                .label("CPU Fan")
                .icon("fan-symbolic")
                .unit("rpm")
                .read(|| Ok(Value::Int(0)))
                .build(),
            Builder::new("fan2_rpm", Kind::ReadOnly, Group::Thermal)
                .label("GPU Fan")
                .icon("fan-symbolic")
                .unit("rpm")
                .read(|| Ok(Value::Int(0)))
                .build(),
            // ---- Lighting -------------------------------------------------
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
            Builder::new("ylogo_light", Kind::Toggle, Group::Lighting)
                .label("Y-Logo Light")
                .icon("starred-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
            // ---- Battery --------------------------------------------------
            Builder::new("battery_capacity", Kind::ReadOnly, Group::Battery)
                .label("Battery Level")
                .icon("battery-symbolic")
                .unit("%")
                .read(|| Ok(Value::Float(68.0)))
                .build(),
            Builder::new("battery_state", Kind::ReadOnly, Group::Battery)
                .label("Battery Status")
                .icon("battery-symbolic")
                .read(|| Ok(Value::Text("Charging".into())))
                .build(),
            Builder::new("battery_conservation", Kind::Toggle, Group::Battery)
                .label("Battery Conservation")
                .icon("battery-symbolic")
                .note("Disables rapid charging")
                .read(|| Ok(Value::Bool(false)))
                .write(|_| Ok(()))
                .build(),
            // ---- System ---------------------------------------------------
            Builder::new("fn_lock", Kind::Toggle, Group::System)
                .label("Fn Lock")
                .icon("input-keyboard-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
            Builder::new("touchpad", Kind::Toggle, Group::System)
                .label("Touchpad")
                .icon("input-touchpad-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
            Builder::new("camera_power", Kind::Toggle, Group::System)
                .label("Camera")
                .icon("camera-photo-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
            Builder::new("usb_charging", Kind::Toggle, Group::System)
                .label("Always-On USB Charging")
                .icon("usb-symbolic")
                .read(|| Ok(Value::Bool(false)))
                .write(|_| Ok(()))
                .build(),
            // ---- Power limits: absent on QNCN, kept for the model matrix ----
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
        ]
    }
}
