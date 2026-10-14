//! A backend with no hardware dependency.
//!
//! Its job is narrow, and it is worth being explicit about what that is:
//!
//! 1. Let the application run and be screenshotted on a machine with no Legion
//!    hardware — every contributor's machine, and CI.
//! 2. Exercise the *unavailable* path, which is the mechanism the whole UI
//!    design rests on.
//!
//! It deliberately does **not** mirror the real capability set. An earlier
//! version did, at 204 lines, on the theory that a drift between the mock and
//! the real backend would be caught. It would not have been: nothing compares
//! the two, so the fidelity was unverifiable duplication. One capability of
//! each kind, plus one unavailable, covers every widget the UI can build.

use crate::backends::Backend;
use crate::capability::{Builder, Capability, Group, Kind, Limits, NamedValue, Value};

pub struct MockBackend;

impl MockBackend {
    fn capabilities_inner() -> Vec<Capability> {
        vec![
            // An enum with a control — the combo path.
            Builder::new("powermode", Kind::Enum, Group::Power)
                .label("Power Mode")
                .icon("power-profile-performance-symbolic")
                .values(vec![
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
                ])
                .info("Custom mode unlocks the fan and power-limit controls.")
                .read(|| Ok(Value::Text("1".into())))
                .write(|_| Ok(()))
                .build(),
            // A switch, with a warning row and the confirmation path.
            {
                let mut cap = Builder::new("fan_fullspeed", Kind::Toggle, Group::Power)
                    .label("Maximum Fan Speed")
                    .icon("weather-windy-symbolic")
                    .warning("A write on the wrong path can hold the fans at maximum.")
                    .read(|| Ok(Value::Bool(false)))
                    .write(|_| Ok(()))
                    .build();
                cap.dangerous = true;
                cap
            },
            // A slider.
            Builder::new("cpu_longterm_powerlimit", Kind::Slider, Group::Power)
                .label("CPU Long-Term Power Limit")
                .icon("power-profile-performance-symbolic")
                .unit("W")
                .limits(Limits {
                    min: 5.0,
                    max: 200.0,
                    step: 1.0,
                })
                .read(|| Ok(Value::Int(65)))
                .write(|_| Ok(()))
                .build(),
            // A reading with a meter.
            Builder::new("cpu_temp", Kind::ReadOnly, Group::Thermal)
                .label("CPU Temperature")
                .icon("org.gnome.Settings-device-diagnostics-symbolic")
                .unit("°C")
                .read(|| Ok(Value::Float(52.0)))
                .build(),
            // A reading with no sensor behind it.
            Builder::new("gpu_temp", Kind::ReadOnly, Group::Thermal)
                .label("GPU Temperature")
                .icon("video-display-symbolic")
                .unit("°C")
                .unavailable("Needs the legion_laptop kernel module")
                .read(|| Ok(Value::Float(0.0)))
                .build(),
            // The unavailability path that matters most: a feature this model
            // does not have at all, rather than one whose driver is missing.
            Builder::new("fancurve", Kind::Custom, Group::Thermal)
                .label("Fan Curve")
                .icon("weather-windy-symbolic")
                .unavailable("Fan curves are not supported by this model's firmware")
                .info("A fan curve maps temperature to fan speed.")
                .read(|| Ok(Value::Text("—".into())))
                .build(),
            // A light and a system toggle, so the remaining groups are non-empty.
            Builder::new("kbd_backlight", Kind::Enum, Group::Lighting)
                .label("Keyboard Backlight")
                .icon("keyboard-brightness-symbolic")
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
            Builder::new("battery_capacity", Kind::ReadOnly, Group::Battery)
                .label("Battery Level")
                .icon("battery-symbolic")
                .unit("%")
                .read(|| Ok(Value::Float(78.0)))
                .build(),
            Builder::new("fn_lock", Kind::Toggle, Group::System)
                .label("Fn Lock")
                .icon("input-keyboard-symbolic")
                .read(|| Ok(Value::Bool(true)))
                .write(|_| Ok(()))
                .build(),
        ]
    }
}

impl Backend for MockBackend {
    fn name(&self) -> &str {
        "mock (no hardware)"
    }

    fn capabilities(&self) -> Vec<Capability> {
        Self::capabilities_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mock exists to exercise the UI without hardware, so it must contain
    /// at least one capability of every kind the UI can render — otherwise the
    /// paths it is meant to stand in for go unexercised.
    #[test]
    fn covers_every_control_kind_and_the_unavailable_path() {
        let caps = MockBackend.capabilities();
        for kind in [Kind::Toggle, Kind::Enum, Kind::Slider, Kind::ReadOnly] {
            assert!(
                caps.iter().any(|c| c.kind == kind),
                "the mock has no {kind:?} capability"
            );
        }
        assert!(
            caps.iter().any(|c| c.unavailable_reason.is_some()),
            "the mock has nothing unavailable, so the suppression path is never rendered"
        );
        assert!(
            caps.iter().any(|c| c.can_write()),
            "the mock has nothing writable"
        );
    }
}
