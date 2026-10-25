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

/// Whether this process is running inside the Flatpak sandbox.
///
/// Under Flatpak neither `pkexec` nor the helper is in the sandbox: both live on
/// the host and are reached through `flatpak-spawn --host`.
fn in_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// Run a command on the host, for the Flatpak case. `true` when it succeeded.
fn on_host(args: &[&str]) -> bool {
    Command::new("flatpak-spawn")
        .arg("--host")
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Whether the pkexec + helper path can work at all on this system.
pub fn is_escalation_available() -> bool {
    if in_flatpak() {
        // Sandboxed: ask the host, where both actually live.
        on_host(&["sh", "-c", "command -v pkexec >/dev/null"])
            && on_host(&["test", "-x", HELPER_PATH])
    } else {
        which("pkexec") && std::path::Path::new(HELPER_PATH).exists()
    }
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

/// This crate targets Linux, so there is no non-unix case to handle.
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
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

    // The output is captured rather than inherited, and this matters more than
    // it looks. A GUI launched from the desktop has no terminal, so an
    // inherited stderr goes nowhere and the user sees only "it didn't work".
    // pkexec's own diagnostics — no polkit agent, not authorised, action not
    // found — are the whole explanation when a password prompt succeeds but the
    // write still fails.
    let output = if in_flatpak() {
        // The helper is on the host; `flatpak-spawn --host` runs it there, and
        // the host's pkexec raises it to root.
        Command::new("flatpak-spawn")
            .args(["--host", "pkexec", HELPER_PATH, "set", key, value])
            .output()?
    } else {
        Command::new("pkexec")
            .arg(HELPER_PATH)
            .arg("set")
            .arg(key)
            .arg(value)
            .output()?
    };

    if output.status.success() {
        tracing::debug!(key, value, "helper write succeeded");
        return Ok(());
    }

    let explanation = explain(key, &output);
    // The failure is shown in red under the control and nowhere else, so a
    // debug log without this line cannot explain a report like "the toggle
    // snaps back" — the refusal never reached the log.
    tracing::warn!(key, value, "helper write failed: {explanation}");
    Err(anyhow::anyhow!("{explanation}"))
}

/// Turn a failed `pkexec` run into something a user can act on.
fn explain(key: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    let detail = if stderr.is_empty() {
        String::new()
    } else {
        format!(" — {stderr}")
    };

    // pkexec's own exit codes mean specific things, and each needs a different
    // action from the user.
    let code = output.status.code();
    let lead = match code {
        Some(126) => {
            "Authentication failed or was dismissed, or no polkit agent is running in this session"
                .to_string()
        }
        Some(127) => format!(
            "pkexec could not run the helper. Is it installed at {HELPER_PATH} and is a polkit \
             action declaring that exact path present in /usr/share/polkit-1/actions or \
             /usr/local/share/polkit-1/actions?"
        ),
        // The helper's own codes, from its source.
        Some(64) => "the helper was called incorrectly".to_string(),
        Some(65) => "the helper rejected the value as implausible".to_string(),
        Some(68) => format!("this machine has no backing file for {key:?}"),
        Some(69) => "the backing file disappeared before it could be written".to_string(),
        Some(74) => format!("the file exists but could not be written for {key:?}"),
        // The helper distinguishes "not an allowed capability" from "outside
        // the writable roots"; its stderr — appended below — says which. Do
        // not paraphrase it here, or the lead contradicts the detail.
        Some(77) => format!("the helper refused to write {key:?}").to_string(),
        Some(other) => format!("pkexec exited {other}"),
        None => "pkexec was killed by a signal".to_string(),
    };

    format!("{lead}{detail}")
}

/// What is wrong with the escalation wiring, if anything.
///
/// Returned rather than logged so the caller can put it in front of the user:
/// this class of failure is invisible otherwise, and every cause has a
/// different fix.
pub fn diagnose() -> Vec<String> {
    if in_flatpak() {
        return diagnose_flatpak();
    }

    let mut findings = Vec::new();

    let helper = std::path::Path::new(HELPER_PATH);
    if !helper.exists() {
        findings.push(format!(
            "the helper is not installed at {HELPER_PATH} (run `sudo make install`, then \
             relaunch the installed binary rather than ./target/release)"
        ));
    } else if !is_executable(helper) {
        findings.push(format!("{HELPER_PATH} is not executable"));
    }

    if !which("pkexec") {
        findings.push("`pkexec` is not on PATH (install polkit)".to_string());
    }

    // The action must declare this exact path, and polkit only reads a couple of
    // fixed directories.
    let policy_hint =
        format!("<annotate key=\"org.freedesktop.policykit.exec.path\">{HELPER_PATH}</annotate>");
    let dirs = [
        "/usr/share/polkit-1/actions",
        "/usr/local/share/polkit-1/actions",
        "/etc/polkit-1/actions",
    ];
    let mut found_policy = false;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            if text.contains(&policy_hint) {
                found_policy = true;
                break;
            }
        }
    }
    if !found_policy {
        findings.push(format!(
            "no polkit action in {} declares exec.path {HELPER_PATH}. pkexec will not \
             authorise the helper without it",
            dirs.join(", ")
        ));
    }

    findings
}

/// The Flatpak case: the helper, `pkexec` and the polkit action are all on the
/// host, so nothing in the sandbox's own filesystem can be checked here.
fn diagnose_flatpak() -> Vec<String> {
    let mut findings = Vec::new();

    if !on_host(&["sh", "-c", "command -v pkexec >/dev/null"]) {
        findings.push("`pkexec` is not on the host's PATH (install polkit)".to_string());
    }
    if !on_host(&["test", "-x", HELPER_PATH]) {
        findings.push(format!(
            "the helper is not installed on the host at {HELPER_PATH} — install the RPM, DEB \
             or AUR package (or run `sudo make install`) so privileged writes work from the \
             Flatpak"
        ));
    }

    findings
}
