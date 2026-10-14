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
    pub presets: Vec<RgbPreset>,
    pub active: usize,
}

impl Default for RgbPresetStore {
    fn default() -> Self {
        // Six numbered slots, as the reference shows.
        Self {
            presets: (1..=6).map(|n| RgbPreset::new(n.to_string())).collect(),
            active: 0,
        }
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
        serde_json::from_str::<Self>(&text)
            .map(|store| store.sanitised())
            .unwrap_or_else(|e| {
                tracing::warn!("{} is unreadable ({e}); using defaults", path.display());
                Self::default()
            })
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
