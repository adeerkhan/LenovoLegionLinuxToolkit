//! The privileged helper.
//!
//! This is the only code we own that runs as root, so its contract is
//! deliberately tiny and its authority is deliberately narrow:
//!
//! ```text
//! legiontoolkit-helper set <capability-key> <value>
//! ```
//!
//! It validates `<key>` against [`legiontoolkit::policy`], resolves the backing
//! sysfs file, writes the value, and exits. It does not read configuration, does
//! not follow input from anywhere except argv, and does not accept a path from
//! the caller — a compromised GUI therefore cannot be talked into writing a file
//! of its choosing, only into writing a value to a key that is already allowed.
//!
//! Invoked through `pkexec` under the `org.legiontoolkit.helper` polkit action.

use std::io::Write;
use std::process::ExitCode;

fn fail(code: u8, message: &str) -> ExitCode {
    // Errors go to stderr; the GUI surfaces them verbatim underneath the
    // control that failed, so they are written to be read by a user.
    let _ = writeln!(std::io::stderr(), "legiontoolkit-helper: {message}");
    ExitCode::from(code)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn run() -> Result<(), ExitCode> {
    let args: Vec<String> = std::env::args().collect();
    let usage = || {
        fail(
            64,
            "usage: legiontoolkit-helper set <capability-key> <value>\n       \
             legiontoolkit-helper --help",
        )
    };

    // Handled before anything is parsed, so a user poking at the binary on the
    // command line gets an answer rather than an error.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", HELP);
        return Ok(());
    }

    match args.get(1).map(String::as_str) {
        // Run as root via pkexec, this proves the whole chain: polkit
        // authorised the helper, the helper reached the file, and root can
        // write it. Everything short of that is guesswork.
        Some("--check") => {
            match legiontoolkit::policy::resolve_target("powermode", None) {
                Some(path) => {
                    println!(
                        "helper      {}",
                        legiontoolkit::services::privilege::HELPER_PATH
                    );
                    println!("target      {}", path.display());
                    match std::fs::OpenOptions::new().write(true).open(&path) {
                        Ok(_) => println!("writable    yes (running as uid {})", unsafe {
                            libc::geteuid()
                        }),
                        Err(e) => println!("writable    NO — {e}"),
                    }
                }
                None => println!("no platform-profile file found on this machine"),
            }
            Ok(())
        }
        Some("set") => {
            let key = args.get(2).ok_or_else(usage)?;
            let value = args.get(3).ok_or_else(usage)?;
            write_capability(key, value)
        }
        _ => Err(usage()),
    }
}

const HELP: &str = "\
legiontoolkit-helper — write one hardware setting, as root, on behalf of the GUI.

Usage:
  legiontoolkit-helper set <capability-key> <value>
  legiontoolkit-helper --check
  legiontoolkit-helper --help

This binary is invoked by the application through `pkexec` under the
org.legiontoolkit.helper polkit action. It is not meant to be run directly:
the key must be in the allowlist, and the value must be a plausible sysfs
token. Anything else is refused.

Capability keys it will accept are those the application considers writable;
run the application to see what this machine supports.
";

fn write_capability(key: &str, value: &str) -> Result<(), ExitCode> {
    // 1. Reject a value that is not a plausible sysfs value. Length bound first,
    //    so a hostile argument cannot exhaust memory in the formatting path.
    if value.is_empty() || value.len() > 64 {
        return Err(fail(65, &format!("refusing implausible value for {key:?}")));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':')
    {
        return Err(fail(65, &format!("refusing non-sysfs value for {key:?}")));
    }

    // 2. Resolve through the same logic the GUI uses, so the two cannot
    //    disagree about which file a key means. The value is part of the
    //    resolution: since kernel 6.14 the legacy platform-profile file
    //    rejects `custom`, which must go to a handler's class device.
    let path = legiontoolkit::policy::resolve_target(key, Some(value))
        .ok_or_else(|| fail(68, &format!("no backing file for {key:?} on this machine")))?;

    // 3. The destination check. This is the boundary that matters, and it is a
    //    *path* check rather than a key check for a concrete reason: the
    //    firmware-attributes class exposes whatever tunables the BIOS has,
    //    under names that differ per model, so no fixed list of key names can
    //    cover them. Every one still resolves under a known root, and refusing
    //    anything else is what stops a crafted key reaching elsewhere.
    //
    //    The name allowlist is still consulted, as defence in depth: a key must
    //    be either named or resolved into an allowed root.
    let named = legiontoolkit::policy::is_writable(key);
    if !named && !legiontoolkit::policy::is_allowed_path(&path) {
        return Err(fail(
            77,
            &format!(
                "refusing {key:?}: not an allowed capability and {} is outside the writable roots",
                path.display()
            ),
        ));
    }
    if named && !legiontoolkit::policy::is_allowed_path(&path) {
        return Err(fail(
            77,
            &format!(
                "refusing {key:?}: {} is outside the writable roots",
                path.display()
            ),
        ));
    }

    if !path.exists() {
        return Err(fail(
            69,
            &format!("{} does not exist on this machine", path.display()),
        ));
    }

    // 4. Some values are meaningful only relative to what the file already
    //    allows; let the kernel's own validation be the last word rather than
    //    duplicating its rules here.
    std::fs::write(&path, format!("{value}\n")).map_err(|e| {
        // `EBUSY` from the firmware-attributes class is the common case and is
        // not a permissions problem: the driver is refusing the value in the
        // current power mode. Saying so saves the user from chasing permissions.
        let hint = match e.raw_os_error() {
            Some(16) => {
                " — the firmware is refusing this value in the current power mode \
                         (several limits are only accepted in custom mode)"
            }
            Some(1) => " — the kernel rejected the value as out of range",
            _ => "",
        };
        fail(74, &format!("cannot write {}: {e}{hint}", path.display()))
    })?;

    Ok(())
}
