//! The security boundary.
//!
//! The helper binary runs as root and writes to sysfs. It must therefore refuse
//! anything it does not recognise. This allowlist is the whole of the helper's
//! authority, and the test in this module asserts that the capability report and
//! the allowlist cannot drift apart — a capability that is writable in the GUI
//! but missing here would look to the user like the feature "doesn't work",
//! which is the worst possible failure mode for a control panel.

/// Capability keys the helper will write.
///
/// Keys are stable identifiers from [`crate::capability::Capability::key`].
/// Deliberately does not include read-only keys — being on this list means
/// "root may write a file on the user's behalf", nothing else.
pub const HELPER_WRITABLE: &[&str] = &[
    "powermode",
    "fan_fullspeed",
    "battery_conservation",
    "fn_lock",
    "camera_power",
    "usb_charging",
    "touchpad",
    "kbd_backlight",
    "ylogo_light",
];

/// True if the key is one the helper will honour.
pub fn is_writable(key: &str) -> bool {
    HELPER_WRITABLE.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{Backend, SysfsBackend};

    /// Every capability that offers a write must be covered by the helper's
    /// allowlist.
    ///
    /// Writes are driven by [`crate::capability::Capability::write`], which the
    /// sysfs backend installs only for paths that exist. So this checks the
    /// installed closures: the `write` field is `Some` exactly when the GUI
    /// believes it can write the key.
    #[test]
    fn every_writable_capability_is_allowed() {
        let backend = SysfsBackend::new();
        for cap in backend.capabilities() {
            if cap.can_write() {
                assert!(
                    is_writable(cap.key),
                    "capability {:?} is writable but not in HELPER_WRITABLE",
                    cap.key
                );
            }
        }
    }

    // Note the converse is *not* asserted: the allowlist may legitimately cover
    // keys for hardware that is not present, which is the point of a static
    // allowlist rather than a probed one.

    /// A sanity check that the allowlist is not accidentally empty.
    #[test]
    fn allowlist_is_populated() {
        assert!(!HELPER_WRITABLE.is_empty());
    }
}
