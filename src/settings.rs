//! User settings, persisted to disk.
//!
//! The reference application has a Settings page with eight categories, most of
//! which are application behaviour rather than hardware: theme, sensor refresh
//! interval, temperature unit, compact mode, and so on. This is the store those
//! need.
//!
//! Two deliberate choices:
//!
//! - **A missing or corrupt file is not an error.** A settings file is a cache
//!   of preferences; losing it should restore defaults, never prevent the
//!   application from starting.
//! - **Every field has a default**, and the struct is `#[serde(default)]`, so a
//!   file written by an older version still loads after a field is added. That
//!   is the whole reason to use a struct rather than a dictionary here.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// Which scale temperatures are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

/// Which theme to use. `System` follows the desktop's preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    System,
    Light,
    Dark,
}

/// Everything the Settings page owns that is not hardware state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub temperature_unit: TemperatureUnit,
    pub color_scheme: ColorScheme,
    /// How often sensor readings are re-read, in seconds.
    pub refresh_interval_secs: u32,
    /// Render cards without their subtitle and status rows.
    pub compact_mode: bool,
    /// Start hidden rather than showing the window.
    pub start_minimized: bool,
    /// Start automatically when you log in.
    pub autostart: bool,
    /// Hide the window instead of quitting when it is closed, so the
    /// application keeps running in the background.
    pub minimize_on_close: bool,
    /// Show desktop notifications for events such as an automation run.
    pub notifications: bool,
    /// Re-open at the last window size.
    ///
    /// Size, not position: GTK 4 removed programmatic window placement, so the
    /// setting is named for what it can actually honour.
    pub remember_window_size: bool,
    /// The last window size, in logical pixels.
    pub window_width: Option<i32>,
    pub window_height: Option<i32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            temperature_unit: TemperatureUnit::Celsius,
            color_scheme: ColorScheme::System,
            // The reference uses two seconds for its battery page and one for
            // its dashboard. One is brisk without being wasteful: these are
            // sysfs reads measured in microseconds.
            refresh_interval_secs: 1,
            compact_mode: false,
            start_minimized: false,
            autostart: false,
            minimize_on_close: false,
            notifications: true,
            remember_window_size: true,
            // Deliberately `None`: a first run should use the toolkit's own
            // default rather than one shipped in a defaults block.
            window_width: None,
            window_height: None,
        }
    }
}

/// The lowest refresh interval offered. Below one second the UI updates faster
/// than a value can be read from the page it is displaying.
pub const MIN_REFRESH_SECS: u32 = 1;
/// The highest interval offered before a control starts to look frozen.
pub const MAX_REFRESH_SECS: u32 = 30;

impl Settings {
    /// Where the settings file lives, honouring `XDG_CONFIG_HOME`.
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("legion-toolkit").join("settings.json"))
    }

    /// Read from disk, falling back to defaults on any problem.
    ///
    /// A corrupt file is logged and replaced on the next save rather than
    /// raising: refusing to start over a bad preference would be a worse
    /// failure than losing it.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(settings) => settings.sanitised(),
            Err(e) => {
                tracing::warn!("{} is unreadable ({e}); using defaults", path.display());
                Self::default()
            }
        }
    }

    /// Write to disk, creating the directory if needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Clamp anything a hand-edited file could get wrong.
    fn sanitised(mut self) -> Self {
        self.refresh_interval_secs = self
            .refresh_interval_secs
            .clamp(MIN_REFRESH_SECS, MAX_REFRESH_SECS);
        self
    }

    /// The size to restore, if there is one and the setting is on.
    pub fn restored_size(&self) -> Option<(i32, i32)> {
        if !self.remember_window_size {
            return None;
        }
        match (self.window_width, self.window_height) {
            (Some(w), Some(h)) if w > 200 && h > 200 => Some((w, h)),
            _ => None,
        }
    }

    /// Convert a Celsius reading for display, returning `(value, unit)`.
    pub fn temperature(&self, celsius: f64, celsius_label: &'static str) -> (f64, &'static str) {
        const FAHRENHEIT: &str = "°F";
        if self.temperature_unit == TemperatureUnit::Fahrenheit {
            (celsius * 9.0 / 5.0 + 32.0, FAHRENHEIT)
        } else {
            (celsius, celsius_label)
        }
    }
}

/// The process-wide settings, loaded once.
static CURRENT: OnceLock<Mutex<Settings>> = OnceLock::new();

fn cell() -> &'static Mutex<Settings> {
    CURRENT.get_or_init(|| Mutex::new(Settings::load()))
}

/// A copy of the current settings.
/// A copy of the current settings. `Settings` is `Copy`, so this allocates
/// nothing — it is called for every sensor row on every refresh.
pub fn current() -> Settings {
    *cell()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Change settings and persist them.
pub fn update(change: impl FnOnce(&mut Settings)) -> anyhow::Result<Settings> {
    // One lock for the whole operation. Taking it twice left a window where a
    // concurrent `current()` could read a value that had not been sanitised yet
    // — and the clamp is exactly the part that makes the value safe to use.
    let updated = {
        let mut guard = cell()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        change(&mut guard);
        let sanitised = guard.sanitised();
        *guard = sanitised;
        sanitised
    };

    updated.save()?;
    Ok(updated)
}

/// Convert a reading for display, given the unit the capability declared.
///
/// Only temperatures convert; every other unit passes through untouched. This
/// lives here rather than in the capability because the stored value should stay
/// in the unit the hardware reported — converting on read would mean a card
/// showing Fahrenheit while its capability claims Celsius.
pub fn for_display(value: f64, unit: &'static str) -> (f64, &'static str) {
    if unit == "°C" {
        current().temperature(value, unit)
    } else {
        (value, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let s = Settings::default();
        assert_eq!(s.temperature_unit, TemperatureUnit::Celsius);
        assert_eq!(s.color_scheme, ColorScheme::System);
        assert!((MIN_REFRESH_SECS..=MAX_REFRESH_SECS).contains(&s.refresh_interval_secs));
    }

    #[test]
    fn round_trips_through_json() {
        let s = Settings {
            temperature_unit: TemperatureUnit::Fahrenheit,
            color_scheme: ColorScheme::Dark,
            refresh_interval_secs: 5,
            compact_mode: true,
            start_minimized: true,
            autostart: true,
            minimize_on_close: true,
            notifications: false,
            remember_window_size: false,
            window_width: Some(1024),
            window_height: Some(768),
        };
        let text = serde_json::to_string(&s).expect("serialise");
        let back: Settings = serde_json::from_str(&text).expect("deserialise");
        assert_eq!(s, back);
    }

    /// A file written before a field existed must still load, so that adding a
    /// setting is not a breaking change for anyone who already has one.
    #[test]
    fn a_partial_file_loads_with_defaults_for_missing_fields() {
        let back: Settings =
            serde_json::from_str(r#"{"temperature_unit":"fahrenheit"}"#).expect("partial load");
        assert_eq!(back.temperature_unit, TemperatureUnit::Fahrenheit);
        assert_eq!(back.color_scheme, Settings::default().color_scheme);
        assert_eq!(
            back.refresh_interval_secs,
            Settings::default().refresh_interval_secs
        );
    }

    #[test]
    fn a_nonsense_refresh_interval_is_clamped() {
        let s: Settings = serde_json::from_str(r#"{"refresh_interval_secs":0}"#).expect("parse");
        assert_eq!(s.sanitised().refresh_interval_secs, MIN_REFRESH_SECS);

        let s: Settings = serde_json::from_str(r#"{"refresh_interval_secs":9999}"#).expect("parse");
        assert_eq!(s.sanitised().refresh_interval_secs, MAX_REFRESH_SECS);
    }

    /// A file written by a *newer* build must not be rejected wholesale. With
    /// `deny_unknown_fields` it would be, and every setting would silently
    /// revert to its default — a worse outcome than ignoring a key we do not
    /// understand yet.
    #[test]
    fn an_unknown_field_is_ignored_rather_than_rejected() {
        let back: Settings =
            serde_json::from_str(r#"{"temperature_unit":"celsius","a_future_setting":42}"#)
                .expect("a newer file must still load");
        assert_eq!(back.temperature_unit, TemperatureUnit::Celsius);
        assert_eq!(
            back.refresh_interval_secs,
            Settings::default().refresh_interval_secs
        );
    }

    #[test]
    fn garbage_does_not_parse() {
        assert!(serde_json::from_str::<Settings>("not json at all").is_err());
    }

    /// A saved size is only restored when the setting is on, and a nonsensical
    /// one is ignored rather than applied — a 1x1 window would be unrecoverable.
    #[test]
    fn restored_size_is_guarded() {
        let on = Settings {
            window_width: Some(1024),
            window_height: Some(768),
            ..Settings::default()
        };
        assert_eq!(on.restored_size(), Some((1024, 768)));

        let off = Settings {
            remember_window_size: false,
            ..on
        };
        assert_eq!(off.restored_size(), None);

        let nonsense = Settings {
            window_width: Some(1),
            window_height: Some(1),
            ..Settings::default()
        };
        assert_eq!(nonsense.restored_size(), None);

        let unset = Settings::default();
        assert_eq!(
            unset.restored_size(),
            None,
            "a first run must use the default"
        );
    }

    #[test]
    fn fahrenheit_conversion_is_correct() {
        let c = Settings::default();
        assert_eq!(c.temperature(100.0, "°C"), (100.0, "°C"));

        let f = Settings {
            temperature_unit: TemperatureUnit::Fahrenheit,
            ..Settings::default()
        };
        let (value, unit) = f.temperature(100.0, "°C");
        assert!((value - 212.0).abs() < 0.001, "got {value}");
        assert_eq!(unit, "°F");

        // Freezing, and a sub-zero reading, must both convert without a sign
        // or offset mistake.
        assert!((f.temperature(0.0, "°C").0 - 32.0).abs() < 0.001);
        assert!((f.temperature(-40.0, "°C").0 + 40.0).abs() < 0.001);
    }
}
