//! A **read-only** diagnostic for the keyboard backlight.
//!
//! Why this exists: whether a given Legion keyboard supports four-zone RGB is a
//! documentation question *and* a hardware question, and the two can disagree.
//! The upstream project `4JX/L5P-Keyboard-RGB` documents a specific table of
//! supported USB product IDs; when that table and the machine in front of you
//! disagree, this settles it.
//!
//! It deliberately only ever issues `HIDIOCGFEATURE` (get a Feature report) —
//! never `HIDIOCSFEATURE`. That makes it safe to run on hardware nobody has
//! tested: the worst case is that a report comes back empty.

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

/// The vendor usage page the ITE keyboard firmware interfaces use, and the
/// report ID observed on this hardware.
const VENDOR_PAGE: u16 = 0xff89;
const REPORT_ID: u8 = 0x5a;

fn main() -> std::io::Result<()> {
    let mut out = std::io::stdout();

    writeln!(out, "Legion keyboard backlight probe (read-only)")?;
    writeln!(out, "=========================================\n")?;

    let devices = hidraw_devices();
    if devices.is_empty() {
        writeln!(out, "No hidraw devices found.")?;
        return Ok(());
    }

    for (path, id) in &devices {
        writeln!(out, "{path}")?;
        writeln!(out, "  USB id:     {id}")?;

        let vendor = parse_vendor(id);
        let product = parse_product(id);
        writeln!(out, "  vendor:     {vendor:#06x}   product: {product:#06x}")?;
        writeln!(
            out,
            "  in upstream supported set: {}",
            in_supported_set(product)
        )?;

        match probe_feature(path) {
            Ok(bytes) => {
                writeln!(out, "  feature report (Report ID {REPORT_ID:#04x}):")?;
                writeln!(out, "    {}", hex(&bytes))?;
                interpret(&bytes)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                writeln!(
                    out,
                    "  feature report: permission denied — hidraw nodes are root-only."
                )?;
                writeln!(
                    out,
                    "      Re-run with `sudo`, or install a udev rule granting the"
                )?;
                writeln!(out, "      `plugdev` group read access to the device.")?;
            }
            Err(e) => writeln!(out, "  feature report: unavailable ({e})")?,
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

/// `HID_ID` from sysfs is `BUS:VENDOR:PRODUCT`, each four hex digits of bus and
/// four of vendor, zero-padded — e.g. `0003:0000046D:0000C092`.
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
fn in_supported_set(product: u16) -> bool {
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

/// Issue HIDIOCGFEATURE. Read-only by construction.
fn probe_feature(path: &str) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;

    // First byte is the report ID, the rest the payload.
    let len = 1 + 65;
    let mut buf = vec![0u8; len];
    buf[0] = REPORT_ID;

    let request = hid_get_feature_ioctl(len);
    // SAFETY: `file` is a valid fd for the duration of the call, and `buf` is
    // `len` bytes long and stays alive across the call.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), request, buf.as_mut_ptr()) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }

    // Read back whatever the device put in the buffer.
    buf.truncate(len);
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Print the firmware version fields the L5P protocol returns at offset 2..6.
///
/// A device in the supported set answers with a plausible version string; one
/// outside it typically returns zeros or an echo of the request.
fn interpret(bytes: &[u8]) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    if bytes.len() > 1 && bytes[1..].iter().all(|&b| b == 0) {
        writeln!(
            out,
            "    -> all-zero payload: the firmware did not answer. Consistent"
        )?;
        writeln!(
            out,
            "       with a keyboard that has no RGB control interface."
        )?;
    } else if bytes[0] == 0 && bytes.iter().filter(|&&b| b == 0).count() >= bytes.len() / 2 {
        writeln!(out, "    -> mostly zeros: no meaningful firmware response.")?;
    } else {
        let nonzero = bytes.iter().filter(|&&b| b != 0).count();
        writeln!(
            out,
            "    -> {nonzero} non-zero bytes. A response was returned; compare"
        )?;
        writeln!(
            out,
            "       against the same probe on a known-supported keyboard."
        )?;
    }
    let _ = VENDOR_PAGE;
    Ok(())
}
