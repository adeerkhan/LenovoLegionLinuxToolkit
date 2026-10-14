//! 24-zone RGB control for Lenovo Legion Gen10 keyboards (ITE 8258).
//!
//! This is our own implementation of the keyboard's USB protocol, so the
//! toolkit can drive the lighting itself. It was written against the public
//! [keyRGB](https://github.com/Rainexn0b/keyRGB) project (MIT), which reverse
//! engineered the controller; the report layout below is reimplemented from its
//! `ite8258_zones_lenovo_legion` backend, and keyRGB is credited in the README.
//!
//! # The device
//!
//! The keyboard exposes a vendor HID interface; we open the first hidraw node
//! carrying the ITE vendor/product id, as keyRGB does.
//!
//! Three commands matter:
//!
//! - `0xC8` **switch** to a stored profile,
//! - `0xCB` **save** lighting groups into that profile,
//! - `0xCE` set the raw brightness (`0..=9`).
//!
//! A "group" is a set of LEDs sharing a mode (static/rainbow/…) and, for static
//! modes, a colour. Applying a look is therefore: switch profile, write the
//! groups, set brightness.
//!
//! Access is unprivileged when the keyRGB udev rule is installed (it grants
//! `uaccess` on these hidraw nodes), so no polkit round-trip is needed here.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

/// The vendor report id the keyboard expects.
const REPORT_ID: u8 = 0x07;
/// The Feature report length, report id included.
const PACKET_SIZE: usize = 960;

const CMD_SAVE_PROFILE: u8 = 0xcb;
const CMD_SWITCH_PROFILE: u8 = 0xc8;
const CMD_SET_BRIGHTNESS: u8 = 0xce;

/// The profile the keyboard stores our lighting in.
const PROFILE: u8 = 0x01;

/// The 24-zone grid: 4 rows by 6 columns.
pub const NUM_ROWS: usize = 4;
pub const NUM_COLS: usize = 6;
pub const NUM_ZONES: usize = NUM_ROWS * NUM_COLS;

/// LED ids `1..=24`, in row-major order.
fn led_ids() -> Vec<u16> {
    (1..=NUM_ZONES as u16).collect()
}

/// UI brightness runs `0..=50`; the firmware takes `0..=9`.
const UI_BRIGHTNESS_MAX: u32 = 50;
const RAW_BRIGHTNESS_MAX: u8 = 9;
/// UI speed runs `0..=10`; the firmware takes `1..=3`.
const UI_SPEED_MAX: u32 = 10;

/// A lighting effect the keyboard can run in hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Static,
    ScrewRainbow,
    RainbowWave,
    ColorChange,
    ColorPulse,
    ColorWave,
    Smooth,
}

impl Effect {
    /// The mode byte the firmware expects.
    fn mode(self) -> u8 {
        match self {
            Self::ScrewRainbow => 0x01,
            Self::RainbowWave => 0x02,
            Self::ColorChange => 0x03,
            Self::ColorPulse => 0x04,
            Self::ColorWave => 0x05,
            Self::Smooth => 0x06,
            Self::Static => 0x0b,
        }
    }

    /// The effect's label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Static => "Static",
            Self::ScrewRainbow => "Rainbow (spiral)",
            Self::RainbowWave => "Rainbow wave",
            Self::ColorChange => "Colour change",
            Self::ColorPulse => "Colour pulse",
            Self::ColorWave => "Colour wave",
            Self::Smooth => "Smooth",
        }
    }

    /// A stable name, for persistence.
    pub fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::ScrewRainbow => "screw_rainbow",
            Self::RainbowWave => "rainbow_wave",
            Self::ColorChange => "color_change",
            Self::ColorPulse => "color_pulse",
            Self::ColorWave => "color_wave",
            Self::Smooth => "smooth",
        }
    }

    /// Rebuild an effect from its stored name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }

    /// Whether the effect uses the chosen colour, or runs through its own.
    pub fn uses_color(self) -> bool {
        matches!(
            self,
            Self::Static | Self::ColorChange | Self::ColorPulse | Self::ColorWave | Self::Smooth
        )
    }

    /// Whether the effect has a direction.
    pub fn uses_direction(self) -> bool {
        matches!(
            self,
            Self::ScrewRainbow | Self::RainbowWave | Self::ColorWave
        )
    }

    pub const ALL: [Effect; 7] = [
        Effect::Static,
        Effect::ScrewRainbow,
        Effect::RainbowWave,
        Effect::ColorChange,
        Effect::ColorPulse,
        Effect::ColorWave,
        Effect::Smooth,
    ];
}

/// One group of LEDs sharing a mode and colour.
struct Group {
    mode: u8,
    speed: u8,
    spin: u8,
    direction: u8,
    color_mode: u8,
    colors: Vec<[u8; 3]>,
    leds: Vec<u16>,
}

/// A packet with the header set and the payload as zeros.
fn packet(command: u8, payload_len: usize) -> Vec<u8> {
    let mut packet = vec![0u8; PACKET_SIZE];
    packet[0] = REPORT_ID;
    packet[1] = command;
    packet[2] = (payload_len & 0xff) as u8;
    packet[3] = ((payload_len >> 8) & 0xff) as u8;
    packet
}

fn switch_profile_report() -> Vec<u8> {
    let mut packet = packet(CMD_SWITCH_PROFILE, 1);
    packet[4] = PROFILE;
    packet
}

fn brightness_report(raw: u8) -> Vec<u8> {
    let mut packet = packet(CMD_SET_BRIGHTNESS, 1);
    packet[4] = raw.min(RAW_BRIGHTNESS_MAX);
    packet
}

fn turn_off_report() -> Vec<u8> {
    let mut packet = packet(CMD_SAVE_PROFILE, 3);
    packet[4] = PROFILE;
    packet[5] = 0x01;
    packet[6] = 0x01;
    packet
}

/// Encode one group's payload.
fn encode_group(index: usize, group: &Group) -> Vec<u8> {
    let mut p = Vec::new();
    p.push((index + 1) as u8);
    p.push(0x06);
    p.extend_from_slice(&[0x01, group.mode]);
    p.extend_from_slice(&[0x02, group.speed]);
    p.extend_from_slice(&[0x03, group.spin]);
    p.extend_from_slice(&[0x04, group.direction]);
    p.extend_from_slice(&[0x05, group.color_mode]);
    p.extend_from_slice(&[0x06, 0x00]);
    p.push(group.colors.len() as u8);
    for color in &group.colors {
        p.extend_from_slice(color);
    }
    p.push(group.leds.len() as u8);
    for led in &group.leds {
        p.push((*led & 0xff) as u8);
        p.push(((*led >> 8) & 0xff) as u8);
    }
    p
}

/// Build the save-profile packets for a set of groups, packing as many groups
/// as fit into each 960-byte packet.
fn save_profile_reports(groups: &[Group]) -> Vec<Vec<u8>> {
    let mut reports = Vec::new();
    let mut index = 0;
    while index < groups.len() {
        let mut packet = packet(CMD_SAVE_PROFILE, 0);
        let mut offset = 4;
        packet[offset] = PROFILE;
        packet[offset + 1] = 0x01;
        packet[offset + 2] = 0x01;
        offset += 3;

        let mut wrote = false;
        while index < groups.len() {
            let encoded = encode_group(index, &groups[index]);
            if offset + encoded.len() > PACKET_SIZE {
                break;
            }
            packet[offset..offset + encoded.len()].copy_from_slice(&encoded);
            offset += encoded.len();
            wrote = true;
            index += 1;
        }
        if !wrote {
            break;
        }
        let payload_len = offset - 4;
        packet[2] = (payload_len & 0xff) as u8;
        packet[3] = ((payload_len >> 8) & 0xff) as u8;
        reports.push(packet);
    }
    reports
}

fn raw_brightness(ui: u32) -> u8 {
    let level = ui.min(UI_BRIGHTNESS_MAX);
    if level == 0 {
        return 0;
    }
    let scaled = ((level as f32 / UI_BRIGHTNESS_MAX as f32) * RAW_BRIGHTNESS_MAX as f32).round();
    (scaled as u8).clamp(1, RAW_BRIGHTNESS_MAX)
}

fn raw_speed(ui: u32) -> u8 {
    let level = ui.min(UI_SPEED_MAX);
    // 0..=10 → 1..=3
    (1 + ((level as f32 / UI_SPEED_MAX as f32) * 2.0).round() as u8).clamp(1, 3)
}

/// Build the group for a uniform static colour.
fn static_group(rgb: [u8; 3]) -> Group {
    Group {
        mode: Effect::Static.mode(),
        speed: raw_speed(UI_SPEED_MAX / 2),
        spin: 0,
        direction: 0,
        color_mode: 0x02, // custom
        colors: vec![rgb],
        leds: led_ids(),
    }
}

/// Build static groups for per-zone colours, one group per distinct colour
/// (so a solid colour is a single group, and a rainbow of zones is many).
fn static_groups(zones: &[[u8; 3]; NUM_ZONES]) -> Vec<Group> {
    let mut groups: Vec<([u8; 3], Vec<u16>)> = Vec::new();
    for (index, color) in zones.iter().enumerate() {
        if *color == [0, 0, 0] {
            continue; // off zones simply carry no colour
        }
        let led = (index + 1) as u16;
        match groups.iter_mut().find(|(existing, _)| existing == color) {
            Some((_, leds)) => leds.push(led),
            None => groups.push((*color, vec![led])),
        }
    }
    groups
        .into_iter()
        .map(|(color, leds)| Group {
            mode: Effect::Static.mode(),
            speed: raw_speed(UI_SPEED_MAX / 2),
            spin: 0,
            direction: 0,
            color_mode: 0x02,
            colors: vec![color],
            leds,
        })
        .collect()
}

/// Build the group for a hardware effect.
fn effect_group(effect: Effect, speed: u32, color: Option<[u8; 3]>, forward: bool) -> Group {
    let (spin, direction) = match effect {
        Effect::ScrewRainbow => (if forward { 0x01 } else { 0x02 }, 0),
        // 0x04 right, 0x03 left
        Effect::RainbowWave | Effect::ColorWave => (0, if forward { 0x04 } else { 0x03 }),
        _ => (0, 0),
    };
    let (color_mode, colors) = if effect.uses_color() {
        match color {
            Some(rgb) => (0x02, vec![rgb]), // custom
            None => (0x01, Vec::new()),     // random
        }
    } else {
        (0x00, Vec::new())
    };
    Group {
        mode: effect.mode(),
        speed: raw_speed(speed),
        spin,
        direction,
        color_mode,
        colors,
        leds: led_ids(),
    }
}

/// An opened keyboard, or a reason it could not be opened.
pub struct Keyboard {
    file: File,
}

impl Keyboard {
    /// The path of the keyboard's RGB hidraw node, if the keyboard is attached.
    ///
    /// Matches keyRGB: the ITE vendor/product id, taking the first node by name.
    pub fn node() -> Option<PathBuf> {
        let entries = std::fs::read_dir("/sys/class/hidraw").ok()?;
        let mut matches: Vec<(String, PathBuf)> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let uevent =
                std::fs::read_to_string(entry.path().join("device/uevent")).unwrap_or_default();
            let Some(id) = uevent
                .lines()
                .find(|l| l.starts_with("HID_ID="))
                .map(|l| l["HID_ID=".len()..].to_string())
            else {
                continue;
            };
            // HID_ID=0003:0000048D:0000C195
            let mut parts = id.split(':');
            let _bus = parts.next();
            let vendor = parts
                .next()
                .and_then(|v| u16::from_str_radix(v, 16).ok())
                .unwrap_or(0);
            let product = parts
                .next()
                .and_then(|v| u16::from_str_radix(v, 16).ok())
                .unwrap_or(0);
            if vendor == 0x048d && matches!(product, 0xc195 | 0xc197) {
                matches.push((name.clone(), PathBuf::from(format!("/dev/{name}"))));
            }
        }
        // First by name, as keyRGB does.
        matches.sort_by_key(|(name, _)| name.to_lowercase());
        matches.into_iter().next().map(|(_, path)| path)
    }

    /// Whether a controllable RGB keyboard is present.
    pub fn available() -> bool {
        Self::node().is_some()
    }

    /// Open the keyboard. Access is unprivileged when the udev rule is present.
    pub fn open() -> io::Result<Self> {
        let path = Self::node()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no ITE 8258 RGB keyboard"))?;
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self { file })
    }

    fn send(&self, report: &[u8]) -> io::Result<()> {
        let request = hid_set_feature_ioctl(report.len());
        // SAFETY: `self.file` is a valid fd, and `report` is a live buffer of
        // exactly the length the ioctl was built for.
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), request, report.as_ptr()) };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn apply(&self, groups: &[Group], brightness: u32) -> io::Result<()> {
        if brightness == 0 {
            return self.turn_off();
        }
        self.send(&switch_profile_report())?;
        for report in save_profile_reports(groups) {
            self.send(&report)?;
        }
        self.send(&brightness_report(raw_brightness(brightness)))?;
        Ok(())
    }

    /// Fill the keyboard with one colour.
    pub fn set_color(&self, rgb: [u8; 3], brightness: u32) -> io::Result<()> {
        if rgb == [0, 0, 0] {
            return self.turn_off();
        }
        self.apply(&[static_group(rgb)], brightness)
    }

    /// Set each of the 24 zones to its own colour.
    pub fn set_zone_colors(&self, zones: &[[u8; 3]; NUM_ZONES], brightness: u32) -> io::Result<()> {
        if zones.iter().all(|c| *c == [0, 0, 0]) {
            return self.turn_off();
        }
        self.apply(&static_groups(zones), brightness)
    }

    /// Run a hardware effect.
    pub fn set_effect(
        &self,
        effect: Effect,
        speed: u32,
        color: Option<[u8; 3]>,
        forward: bool,
        brightness: u32,
    ) -> io::Result<()> {
        if effect == Effect::Static {
            return self.set_color(color.unwrap_or([255, 0, 0]), brightness);
        }
        self.apply(&[effect_group(effect, speed, color, forward)], brightness)
    }

    /// Change brightness only.
    pub fn set_brightness(&self, brightness: u32) -> io::Result<()> {
        if brightness == 0 {
            return self.turn_off();
        }
        self.send(&switch_profile_report())?;
        self.send(&brightness_report(raw_brightness(brightness)))?;
        Ok(())
    }

    /// Turn the lighting off.
    pub fn turn_off(&self) -> io::Result<()> {
        self.send(&switch_profile_report())?;
        self.send(&turn_off_report())?;
        Ok(())
    }
}

/// Compute a `HIDIOCSFEATURE(len)` request (`_IOC(_IOC_READ|_IOC_WRITE, 'H', 0x06, len)`).
fn hid_set_feature_ioctl(len: usize) -> libc::c_ulong {
    const IOC_WRITE: libc::c_ulong = 1;
    const IOC_READ: libc::c_ulong = 2;
    const IOC_NRBITS: libc::c_ulong = 8;
    const IOC_TYPEBITS: libc::c_ulong = 8;
    const IOC_SIZEBITS: libc::c_ulong = 14;
    const IOC_NRSHIFT: libc::c_ulong = 0;
    const IOC_TYPESHIFT: libc::c_ulong = IOC_NRSHIFT + IOC_NRBITS;
    const IOC_SIZESHIFT: libc::c_ulong = IOC_TYPESHIFT + IOC_TYPEBITS;
    const IOC_DIRSHIFT: libc::c_ulong = IOC_SIZESHIFT + IOC_SIZEBITS;

    (((IOC_READ | IOC_WRITE) << IOC_DIRSHIFT)
        | ((len as libc::c_ulong) << IOC_SIZESHIFT)
        | ((b'H' as libc::c_ulong) << IOC_TYPESHIFT)
        | (0x06u64 << IOC_NRSHIFT)) as libc::c_ulong
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_scales_to_the_firmware_range() {
        assert_eq!(raw_brightness(0), 0);
        assert_eq!(raw_brightness(1), 1, "any non-zero level is at least 1");
        assert_eq!(raw_brightness(50), 9);
        assert_eq!(raw_brightness(25), 5, "half of 9, rounded");
        assert_eq!(raw_brightness(999), 9, "clamped");
    }

    #[test]
    fn speed_scales_to_1_3() {
        assert_eq!(raw_speed(0), 1);
        assert_eq!(raw_speed(5), 2);
        assert_eq!(raw_speed(10), 3);
    }

    #[test]
    fn a_packet_has_the_report_id_and_header() {
        let report = brightness_report(9);
        assert_eq!(report.len(), PACKET_SIZE);
        assert_eq!(report[0], REPORT_ID);
        assert_eq!(report[1], CMD_SET_BRIGHTNESS);
        assert_eq!(report[2], 1, "payload length");
        assert_eq!(report[4], 9);
    }

    #[test]
    fn a_static_group_encodes_the_mode_and_leds() {
        let group = static_group([255, 0, 0]);
        let encoded = encode_group(0, &group);
        assert_eq!(encoded[0], 1, "group index is 1-based");
        assert_eq!(encoded[1], 0x06, "six fields");
        // fields: 01 mode, 02 speed, 03 spin, 04 direction, 05 color_mode, 06 0
        assert_eq!(&encoded[2..4], &[0x01, Effect::Static.mode()]);
        assert_eq!(encoded[11], 0x02, "custom colour");
        assert_eq!(encoded[14], 1, "one colour");
        assert_eq!(&encoded[15..18], &[255, 0, 0]);
        assert_eq!(encoded[18], 24, "all 24 zones");
    }

    #[test]
    fn save_profile_packet_is_well_formed() {
        let reports = save_profile_reports(&[static_group([1, 2, 3])]);
        assert_eq!(reports.len(), 1);
        let packet = &reports[0];
        assert_eq!(packet[0], REPORT_ID);
        assert_eq!(packet[1], CMD_SAVE_PROFILE);
        assert_eq!(packet[4], PROFILE);
        let payload_len = packet[2] as usize | ((packet[3] as usize) << 8);
        assert!(payload_len > 3, "payload includes at least one group");
    }

    /// Per-zone colours become one group per distinct colour, each carrying the
    /// LED ids that share it.
    #[test]
    fn zone_colours_group_by_distinct_colour() {
        let mut zones = [[0u8, 0, 0]; NUM_ZONES];
        zones[0] = [255, 0, 0];
        zones[1] = [255, 0, 0];
        zones[2] = [0, 0, 255];
        let groups = static_groups(&zones);
        assert_eq!(groups.len(), 2, "red and blue; black zones are skipped");
        let red = groups.iter().find(|g| g.colors[0] == [255, 0, 0]).unwrap();
        assert_eq!(red.leds, vec![1, 2]);
        let blue = groups.iter().find(|g| g.colors[0] == [0, 0, 255]).unwrap();
        assert_eq!(blue.leds, vec![3]);
    }

    /// A uniform colour is a single group over all 24 zones.
    #[test]
    fn a_uniform_colour_is_one_group_of_all_zones() {
        let zones = [[10, 20, 30]; NUM_ZONES];
        let groups = static_groups(&zones);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].leds.len(), NUM_ZONES);
    }
}
