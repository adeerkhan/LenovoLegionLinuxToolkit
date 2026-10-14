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
    "ioport_light",
    "battery_mode",
    "rapid_charge",
    "igpumode",
    "gsync",
    "winkey",
    "overdrive",
    "instant_boot_ac",
    "instant_boot_usb_pd",
];

/// True if the key is one the helper will honour.
pub fn is_writable(key: &str) -> bool {
    HELPER_WRITABLE.contains(&key)
}

/// Directories the helper is permitted to write into.
///
/// The key allowlist below is a list of *names*; this is a list of
/// *destinations*, and it is the stronger of the two. Names can be discovered
/// at runtime — the firmware-attributes class exposes whatever tunables the
/// BIOS has, under names that differ per model — so a fixed list of names
/// cannot cover them. Every one of those attributes still resolves to a path
/// under `/sys/class/firmware-attributes/`, and checking that is what stops a
/// crafted key from reaching anywhere else.
pub const WRITABLE_ROOTS: &[&str] = &[
    "/sys/class/firmware-attributes/",
    "/sys/firmware/acpi/",
    "/sys/class/leds/",
    // The battery's `charge_types` — the native three-way charging mode — lives
    // in the power_supply class, not under any Lenovo-specific tree.
    "/sys/class/power_supply/",
    "/sys/module/legion_laptop/",
    "/sys/bus/platform/drivers/ideapad",
];

/// True when `path` lies inside one of [`WRITABLE_ROOTS`].
///
/// Deliberately a lexical check rather than `canonicalize`, and the reason is
/// worth recording because the obvious version is wrong. sysfs presents one
/// device under several paths: `/sys/class/firmware-attributes/…` resolves to
/// `/sys/devices/virtual/firmware-attributes/…`. Canonicalising one end of the
/// comparison and not the other makes a legitimate path look like an escape —
/// which is exactly what a test caught here.
///
/// What actually guarantees safety:
/// - **No `..` component**, so a name cannot climb out of its directory. The
///   only caller-supplied component is a firmware attribute name, already
///   validated to be alphanumeric.
/// - **A prefix of an allowlisted root**, so the destination is one of a fixed
///   set of sysfs subtrees even if the key were crafted.
/// - **Not a symlink itself**, so the final component cannot redirect the write.
pub fn is_allowed_path(path: &std::path::Path) -> bool {
    use std::path::Component;

    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return false;
    }
    if !WRITABLE_ROOTS
        .iter()
        .any(|root| path.starts_with(std::path::Path::new(root)))
    {
        return false;
    }
    !std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

/// The capability key prefix used for discovered firmware attributes.
pub const FIRMWARE_PREFIX: &str = "firmware:";

/// Resolve a capability key to the file to write.
///
/// Returns `None` for an unknown key, or for a firmware attribute that does not
/// exist on this machine.
pub fn resolve_target(key: &str) -> Option<std::path::PathBuf> {
    if let Some(name) = key.strip_prefix(FIRMWARE_PREFIX) {
        return crate::system::firmware_attribute_path(name);
    }
    crate::paths::discover().file(key)
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
            if !cap.can_write() {
                continue;
            }
            // A capability is permitted if its key is on the name allowlist, or
            // if it resolves to a path inside an allowed root. Discovered
            // firmware attributes take the second route, which is why the
            // destination check exists at all.
            let named = is_writable(&cap.key);
            let resolved_ok = resolve_target(&cap.key)
                .map(|path| is_allowed_path(&path))
                .unwrap_or(false);
            assert!(
                named || resolved_ok,
                "capability {:?} is writable but neither named nor inside an allowed root",
                cap.key
            );
        }
    }

    /// The destination check is the boundary that a crafted key cannot cross.
    #[test]
    fn paths_outside_the_roots_are_refused() {
        assert!(!is_allowed_path(std::path::Path::new("/etc/passwd")));
        assert!(!is_allowed_path(std::path::Path::new("/tmp/anything")));
        // A traversal component is refused even when it starts out inside a root.
        assert!(!is_allowed_path(std::path::Path::new(
            "/sys/class/leds/../../../etc/passwd"
        )));
    }

    /// Every real firmware attribute the kernel exposes must pass the check.
    /// This is the assertion that caught the canonicalisation mistake.
    #[test]
    fn real_firmware_attributes_are_allowed() {
        let attributes = crate::system::firmware_attributes();
        for attribute in &attributes {
            let Some(path) = crate::system::firmware_attribute_path(&attribute.name) else {
                continue;
            };
            assert!(
                is_allowed_path(&path),
                "{} resolves to {} which the helper would refuse",
                attribute.name,
                path.display()
            );
        }
    }

    /// A firmware attribute name is used as a path component, so it must not be
    /// able to climb out of its directory.
    #[test]
    fn traversal_in_an_attribute_name_is_refused() {
        assert!(crate::system::firmware_attribute_path("../../etc/shadow").is_none());
        assert!(crate::system::firmware_attribute_path("a/b").is_none());
        assert!(crate::system::firmware_attribute_path("").is_none());
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
