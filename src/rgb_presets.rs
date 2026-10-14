//! Named RGB keyboard presets, stored in the application's config directory.
//!
//! The keyboard's own `Fn+Space` modes live in the embedded controller and are
//! not reachable through the documented USB protocol (the c195 controller
//! exposes a single profile), so the toolkit keeps its own presets — the same
//! idea as the Windows reference's numbered presets, but editable: a preset
//! records the effect, the 24 zone colours, and the brightness and speed.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::rgb::{Effect, NUM_ZONES};

/// One saved look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RgbPreset {
    pub name: String,
    /// The effect's stable name (see [`Effect::name`]).
    pub effect: String,
    /// One colour per zone, in zone order.
    pub zones: Vec<[u8; 3]>,
    pub brightness: u32,
    pub speed: u32,
}

impl RgbPreset {
    /// A preset of the given name holding a solid red static look.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            effect: Effect::Static.name().to_string(),
            zones: vec![[255, 32, 32]; NUM_ZONES],
            brightness: 40,
            speed: 5,
        }
    }

    pub fn effect(&self) -> Effect {
        Effect::from_name(&self.effect).unwrap_or(Effect::Static)
    }

    /// The zone colours, padded or truncated to exactly 24.
    pub fn zones(&self) -> [[u8; 3]; NUM_ZONES] {
        let mut zones = [[0u8; 3]; NUM_ZONES];
        for (slot, color) in zones.iter_mut().zip(self.zones.iter()) {
            *slot = *color;
        }
        zones
    }
}

/// The stored presets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RgbPresetStore {
    /// Bumped whenever the built-in defaults change shape, so a file written by
    /// an older build is replaced rather than silently keeping its stale look.
    ///
    /// A missing field must read as `0`, not as the current version: the
    /// container's `#[serde(default)]` would otherwise take the value from
    /// `Default::default()` and defeat the migration.
    #[serde(default = "default_version")]
    pub version: u32,
    pub presets: Vec<RgbPreset>,
    pub active: usize,
}

/// The current presets schema.
pub const VERSION: u32 = 2;

/// What a file without a version counts as: an older build wrote it.
fn default_version() -> u32 {
    0
}

impl Default for RgbPresetStore {
    fn default() -> Self {
        // Six numbered slots, as the reference shows. Deliberately distinct so
        // switching between them visibly changes the keyboard.
        let presets = vec![
            static_preset("1", [255, 32, 32]),
            static_preset("2", [32, 255, 96]),
            static_preset("3", [32, 96, 255]),
            RgbPreset {
                name: "4".into(),
                effect: Effect::ScrewRainbow.name().into(),
                zones: vec![[0, 0, 0]; NUM_ZONES],
                brightness: 40,
                speed: 5,
            },
            RgbPreset {
                name: "5".into(),
                effect: Effect::RainbowWave.name().into(),
                zones: vec![[0, 0, 0]; NUM_ZONES],
                brightness: 40,
                speed: 5,
            },
            RgbPreset {
                name: "6".into(),
                effect: Effect::ColorPulse.name().into(),
                zones: vec![[0, 0, 0]; NUM_ZONES],
                brightness: 40,
                speed: 5,
            },
        ];
        Self {
            version: VERSION,
            presets,
            active: 0,
        }
    }
}

/// A preset whose 24 zones are one solid colour.
fn static_preset(name: &str, rgb: [u8; 3]) -> RgbPreset {
    RgbPreset {
        name: name.into(),
        effect: Effect::Static.name().into(),
        zones: vec![rgb; NUM_ZONES],
        brightness: 40,
        speed: 5,
    }
}

impl RgbPresetStore {
    /// Where the presets live, beside settings.
    pub fn path() -> Option<PathBuf> {
        crate::settings::Settings::path().map(|p| p.with_file_name("rgb_presets.json"))
    }

    /// Read from disk, falling back to defaults. Same contract as settings.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(store) if store.version == VERSION => store.sanitised(),
            // An older build wrote its defaults here, so editing the file would
            // not change them. Discard and write the current ones instead.
            _ => {
                tracing::info!(
                    "{} predates the current presets; using the new defaults",
                    path.display()
                );
                let defaults = Self::default();
                let _ = defaults.save();
                defaults
            }
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Keep the active index in range and every preset 24 zones wide.
    fn sanitised(mut self) -> Self {
        self.version = VERSION;
        if self.presets.is_empty() {
            self.presets = Self::default().presets;
        }
        for preset in &mut self.presets {
            preset.zones.resize(NUM_ZONES, [0, 0, 0]);
        }
        self.active = self.active.min(self.presets.len() - 1);
        self
    }

    pub fn active_preset(&self) -> Option<&RgbPreset> {
        self.presets.get(self.active)
    }

    /// Store the current look in the active slot.
    pub fn capture(
        &mut self,
        effect: Effect,
        zones: [[u8; 3]; NUM_ZONES],
        brightness: u32,
        speed: u32,
    ) {
        if let Some(preset) = self.presets.get_mut(self.active) {
            preset.effect = effect.name().to_string();
            preset.zones = zones.to_vec();
            preset.brightness = brightness;
            preset.speed = speed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_six_numbered_presets() {
        let store = RgbPresetStore::default();
        assert_eq!(store.presets.len(), 6);
        assert_eq!(store.presets[0].name, "1");
        assert!(store.active_preset().is_some());
    }

    #[test]
    fn capture_writes_the_active_slot() {
        let mut store = RgbPresetStore {
            active: 2,
            ..RgbPresetStore::default()
        };
        store.capture(Effect::RainbowWave, [[1, 2, 3]; NUM_ZONES], 20, 7);
        let preset = store.active_preset().unwrap();
        assert_eq!(preset.effect(), Effect::RainbowWave);
        assert_eq!(preset.zones()[0], [1, 2, 3]);
        assert_eq!(preset.brightness, 20);
        assert_eq!(preset.speed, 7);
    }

    #[test]
    fn a_short_zone_list_is_padded() {
        let text = r#"{"presets":[{"name":"x","effect":"static","zones":[[1,2,3]],"brightness":5,"speed":5}],"active":0}"#;
        let store: RgbPresetStore = serde_json::from_str(text).unwrap();
        assert_eq!(store.sanitised().presets[0].zones().len(), NUM_ZONES);
    }

    /// The migration only fires if a missing version reads as *old*. With the
    /// container's `#[serde(default)]` a missing field took the value from
    /// `Default::default()` — the current version — so a legacy file looked
    /// fresh and kept its stale presets forever.
    #[test]
    fn a_file_without_a_version_counts_as_old() {
        let text = r#"{"presets":[{"name":"1","effect":"static","zones":[[1,2,3]],"brightness":5,"speed":5}],"active":0}"#;
        let store: RgbPresetStore = serde_json::from_str(text).unwrap();
        assert_eq!(
            store.version, 0,
            "a missing version must not read as current"
        );
    }

    #[test]
    fn round_trips_through_json() {
        let store = RgbPresetStore::default();
        let text = serde_json::to_string(&store).unwrap();
        assert_eq!(
            serde_json::from_str::<RgbPresetStore>(&text).unwrap(),
            store
        );
    }
}
