//! Privileged writes.
//!
//! Reading hardware needs no privilege at all. Writing needs root, because
//! every writable sysfs attribute is mode `0644` and LenovoLegionLinux ships no
//! udev rule granting group write. Rather than run the whole GUI as root —
//! which breaks tray icons and desktop notifications — we escalate *per write*
//! to a small helper binary through polkit.
//!
//! The helper is the only privileged code we own. It validates the capability
//! key against a fixed allowlist and writes the file, so a compromised GUI
//! cannot be talked into writing somewhere else.

use std::process::Command;

/// Name of the polkit action, which must match the `org.freedesktop.policykit.
/// exec.path` annotation in the shipped `.policy` file exactly.
pub const HELPER_ACTION: &str = "org.legiontoolkit.helper";

/// Absolute path of the helper binary.
///
/// Baked in at build time by the Makefile, which passes
/// `LEGIONTOOLKIT_HELPER_PATH` derived from `HELPER_PATH`. This exists so the
/// binary and the polkit policy cannot disagree: the policy declares the same
/// path, and polkit matches it exactly, so a mismatch would refuse every write
/// in a way that looks like the feature is broken.
///
/// The fallback is the location a distro package would use.
pub const HELPER_PATH: &str = match option_env!("LEGIONTOOLKIT_HELPER_PATH") {
    Some(path) => path,
    None => "/usr/libexec/legiontoolkit-helper",
};

/// Whether the pkexec + helper path can work at all on this system.
pub fn is_escalation_available() -> bool {
    which("pkexec") && std::path::Path::new(HELPER_PATH).exists()
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(bin);
                candidate.is_file() && is_executable(&candidate)
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_: &std::path::Path) -> bool {
    true
}

/// Escalate a single write.
///
/// Runs synchronously: a polkit prompt is a blocking user interaction by
/// nature, and the caller is expected to be on a short-lived worker or to
/// accept a modal pause. For the read-only first milestone this is never
/// reached.
pub fn request(key: &str, value: &str) -> anyhow::Result<()> {
    if !is_escalation_available() {
        return Err(anyhow::anyhow!(
            "privileged writes need `pkexec` and {HELPER_PATH}"
        ));
    }

    let status = Command::new("pkexec")
        .arg(HELPER_PATH)
        .arg("set")
        .arg(key)
        .arg(value)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "the helper refused to write {key} (exit {status})"
        ))
    }
}
