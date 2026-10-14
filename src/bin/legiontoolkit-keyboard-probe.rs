//! A **read-only** diagnostic for the keyboard backlight / RGB controller.
//!
//! Whether a Legion keyboard is a 3-state white one or a four-zone RGB one is a
//! hardware question, and the answer differs by SKU even within a model line.
//! This probe settles it on the machine in front of you: it issues only
//! `HIDIOCGFEATURE` (get a Feature report) and never a set, so the worst case is
//! an empty report.
//!
//! It reads two protocols:
//!
//! - **`0xcc`** — the ITE four-zone RGB state, as read by LegionAura and the
//!   L5P family: `[id, 0x16, effect, speed, brightness, 4×RGB, unused, rtl, ltr]`.
//! - **`0x5a`** — the vendor firmware Feature report the `4JX/L5P-Keyboard-RGB`
//!   project uses.
//!
//! A device that answers `0xcc` with a plausible state is a four-zone RGB
//! keyboard; one that leaves the feature report empty is not.

use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;

/// `_IOC(_IOC_READ, 'H', 0x07, len)` — get a Feature report of `len` bytes.
fn hid_get_feature_ioctl(len: usize) -> libc::c_ulong {
    const IOC_READ: libc::c_ulong = 2;
    const IOC_NRBITS: libc::c_ulong = 8;
    const IOC_TYPEBITS: libc::c_ulong = 8;
    const IOC_SIZEBITS: libc::c_ulong = 14;
    const IOC_NRSHIFT: libc::c_ulong = 0;
    const IOC_TYPESHIFT: libc::c_ulong = IOC_NRSHIFT + IOC_NRBITS;
    const IOC_SIZESHIFT: libc::c_ulong = IOC_TYPESHIFT + IOC_TYPEBITS;
    const IOC_DIRSHIFT: libc::c_ulong = IOC_SIZESHIFT + IOC_SIZEBITS;

    ((IOC_READ << IOC_DIRSHIFT)
        | ((len as libc::c_ulong) << IOC_SIZESHIFT)
        | ((b'H' as libc::c_ulong) << IOC_TYPESHIFT)
        | (0x07u64 << IOC_NRSHIFT)) as libc::c_ulong
}

/// The ITE four-zone RGB state report (LegionAura / L5P family).
const REPORT_ID_ITE_RGB: u8 = 0xcc;
/// The vendor firmware Feature report used by `4JX/L5P-Keyboard-RGB`.
const REPORT_ID_L5P: u8 = 0x5a;

const ITE_VENDOR: u16 = 0x048d;

fn main() -> std::io::Result<()> {
    let mut out = std::io::stdout();

    writeln!(out, "Legion keyboard RGB probe (read-only)")?;
    writeln!(out, "====================================\n")?;

    let devices: Vec<(String, String)> = hidraw_devices()
        .into_iter()
        .filter(|(_, id)| parse_vendor(id) == ITE_VENDOR)
        .collect();

    if devices.is_empty() {
        writeln!(out, "No ITE ({ITE_VENDOR:#06x}) hidraw devices found.")?;
        return Ok(());
    }

    for (path, id) in &devices {
        let product = parse_product(id);
        writeln!(out, "{path}   (ITE {ITE_VENDOR:#06x}:{product:#06x})")?;
        writeln!(
            out,
            "  in the 4JX/L5P documented set: {}",
            in_4jx_set(product)
        )?;

        let mut denied = false;
        for &report in &[REPORT_ID_ITE_RGB, REPORT_ID_L5P] {
            match probe_feature(path, report) {
                Ok(bytes) => {
                    writeln!(out, "  Feature report {report:#04x}: {}", hex(&bytes))?;
                    if report == REPORT_ID_ITE_RGB {
                        interpret_ite_state(&bytes)?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    denied = true;
                    break;
                }
                Err(e) => writeln!(out, "  Feature report {report:#04x}: unavailable ({e})")?,
            }
        }
        if denied {
            writeln!(
                out,
                "  permission denied — hidraw nodes are root-only. Re-run with `sudo`."
            )?;
        }
        writeln!(out)?;
    }

    Ok(())
}

/// Every `/dev/hidrawN` with its USB id from sysfs.
fn hidraw_devices() -> Vec<(String, String)> {
    let mut found = Vec::new();
    let entries = match std::fs::read_dir("/sys/class/hidraw") {
        Ok(e) => e,
        Err(_) => return found,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let uevent = entry.path().join("device/uevent");
        let id = std::fs::read_to_string(&uevent)
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| l.starts_with("HID_ID="))
                    .map(|l| l["HID_ID=".len()..].to_string())
            })
            .unwrap_or_else(|| "unknown".into());
        found.push((format!("/dev/{name}"), id));
    }
    found.sort();
    found
}

/// `HID_ID` is `BUS:VENDOR:PRODUCT`, each zero-padded hex.
fn parse_vendor(id: &str) -> u16 {
    id.split(':')
        .nth(1)
        .and_then(|s| u16::from_str_radix(s, 16).ok())
        .unwrap_or(0)
}

fn parse_product(id: &str) -> u16 {
    id.split(':')
        .nth(2)
        .and_then(|s| u16::from_str_radix(s, 16).ok())
        .unwrap_or(0)
}

/// The table published by `4JX/L5P-Keyboard-RGB`.
fn in_4jx_set(product: u16) -> bool {
    matches!(
        product,
        0xc955
            | 0xc965
            | 0xc975
            | 0xc985
            | 0xc995
            | 0xc963
            | 0xc973
            | 0xc983
            | 0xc984
            | 0xc993
            | 0xc994
    )
}

/// Issue HIDIOCGFEATURE for one report ID. Read-only by construction.
fn probe_feature(path: &str, report: u8) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;

    // First byte is the report ID, the rest the payload.
    let len = 1 + 64;
    let mut buf = vec![0u8; len];
    buf[0] = report;

    let request = hid_get_feature_ioctl(len);
    // SAFETY: `file` is a valid fd for the duration of the call, and `buf` is
    // `len` bytes long and stays alive across the call.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), request, buf.as_mut_ptr()) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decode the `0xcc` ITE RGB state, per the LegionAura/L5P layout.
fn interpret_ite_state(bytes: &[u8]) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    if bytes.len() < 20 {
        writeln!(out, "    -> too short to be an RGB state.")?;
        return Ok(());
    }
    if bytes.iter().all(|&b| b == 0) {
        writeln!(
            out,
            "    -> all zero: the controller did not answer 0xcc (likely not a \
             four-zone RGB keyboard)."
        )?;
        return Ok(());
    }

    let effect = match bytes[2] {
        0x00 => "brightness-only",
        0x01 => "Static",
        0x03 => "Breath",
        0x04 => "Wave",
        0x06 => "Hue",
        _ => "unknown",
    };
    writeln!(
        out,
        "    -> FOUR-ZONE RGB STATE: effect={effect} speed={} brightness={}",
        bytes[3], bytes[4]
    )?;
    for zone in 0..4 {
        let base = 5 + zone * 3;
        let (r, g, b) = (bytes[base], bytes[base + 1], bytes[base + 2]);
        writeln!(
            out,
            "       zone {zone}: rgb({r}, {g}, {b}) #{r:02x}{g:02x}{b:02x}"
        )?;
    }
    Ok(())
}
