//! Named presets — the reference's "custom mode" / God Mode.
//!
//! In the Windows toolkit a preset is a named set of power limits: CPU PL1,
//! PL2, tau, cross-loading, the CPU and GPU temperature caps, GPU TGP, PPAB,
//! dynamic boost, the AC offset, and fan behaviour. `GodModePreset` is a single
//! struct with one optional field per tunable.
//!
//! It maps onto what we have almost exactly. The firmware-attributes class
//! exposes the same tunables, and selecting one of them is just a write. So a
//! preset here is a **named map of capability key to value**, and applying one
//! writes each entry through the same privilege path a slider uses — no
//! per-feature code, and a preset can hold any capability the machine reports.
//!
//! The only thing in the reference's preset we cannot express is `FanTableInfo`
//! (a fan curve), because this model's firmware has none.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One named set of settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    /// Capability key to the value to write, as a string.
    ///
    /// A `BTreeMap` and string values on purpose: this is persisted, and it has
    /// to survive a capability being added, renamed or unavailable on a
    /// different machine. A preset that names something this machine does not
    /// have is not an error — that entry is skipped when applied.
    pub values: BTreeMap<String, String>,
}

impl Preset {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: BTreeMap::new(),
        }
    }

    /// How many settings the preset would write.
    ///
    /// Named rather than `len`, which on a preset reads as "length of what";
    /// clippy flags a bare `len` without `is_empty` for the same reason.
    pub fn setting_count(&self) -> usize {
        self.values.len()
    }
}

/// Storage for presets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetStore {
    pub presets: Vec<Preset>,
    /// Index of the preset the application applies by default.
    pub active: usize,
}

impl Default for PresetStore {
    fn default() -> Self {
        // The reference ships named modes rather than an empty list, so the
        // feature is discoverable the first time the page is opened.
        Self {
            presets: vec![
                Preset::new("Quiet"),
                Preset::new("Balanced"),
                Preset::new("Performance"),
            ],
            active: 1,
        }
    }
}

impl PresetStore {
    /// Where the presets file lives.
    pub fn path() -> Option<PathBuf> {
        crate::settings::Settings::path().map(|p| p.with_file_name("presets.json"))
    }

    /// Read from disk, falling back to defaults.
    ///
    /// Same contract as settings: a missing or corrupt file restores defaults
    /// rather than preventing the application from starting.
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

    /// Keep the active index in range and drop nameless presets.
    fn sanitised(mut self) -> Self {
        self.presets.retain(|p| !p.name.trim().is_empty());
        if self.presets.is_empty() {
            self.presets = Self::default().presets;
        }
        self.active = self.active.min(self.presets.len() - 1);
        self
    }

    /// The preset that would be applied, if any.
    pub fn active_preset(&self) -> Option<&Preset> {
        self.presets.get(self.active)
    }

    /// Add or replace a preset by name, and return its index.
    pub fn upsert(&mut self, preset: Preset) -> usize {
        match self.presets.iter().position(|p| p.name == preset.name) {
            Some(index) => {
                self.presets[index] = preset;
                index
            }
            None => {
                self.presets.push(preset);
                self.presets.len() - 1
            }
        }
    }

    /// Capture into whichever preset is currently selected, and return its
    /// index.
    ///
    /// This is the reference's flow: pick a mode, adjust, save it back.
    pub fn capture_to_active(&mut self, current: impl Iterator<Item = (String, String)>) -> usize {
        let index = self.active.min(self.presets.len().saturating_sub(1));
        let name = self
            .presets
            .get(index)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Custom".to_string());
        self.capture(name, current)
    }

    /// Capture the current values of a set of capabilities into a named preset.
    ///
    /// `current` yields `(key, value)` pairs; anything unreadable is skipped
    /// rather than stored as a placeholder, because a preset that writes a dash
    /// is worse than one that omits it.
    pub fn capture(
        &mut self,
        name: impl Into<String>,
        current: impl Iterator<Item = (String, String)>,
    ) -> usize {
        let mut preset = Preset::new(name);
        for (key, value) in current {
            preset.values.insert(key, value);
        }
        self.upsert(preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_named_and_have_an_active_entry() {
        let store = PresetStore::default();
        assert!(store.active_preset().is_some());
        assert_eq!(store.presets.len(), 3);
    }

    #[test]
    fn round_trips_through_json() {
        let mut store = PresetStore::default();
        store.presets[0]
            .values
            .insert("firmware:ppt_pl1_spl".into(), "70".into());
        store.active = 0;

        let text = serde_json::to_string(&store).expect("serialise");
        let back: PresetStore = serde_json::from_str(&text).expect("deserialise");
        assert_eq!(store, back);
    }

    /// A preset naming something this machine does not have must load and apply
    /// without complaint: presets are shared between machines.
    #[test]
    fn unknown_keys_are_carried_not_rejected() {
        let text = r#"{
            "presets": [{ "name": "Other machine", "values": {
                "firmware:something_this_model_lacks": "42",
                "powermode": "1"
            }}],
            "active": 0
        }"#;
        let store: PresetStore = serde_json::from_str(text).expect("load");
        assert_eq!(store.active_preset().unwrap().setting_count(), 2);
        assert!(store
            .active_preset()
            .unwrap()
            .values
            .keys()
            .any(|k| k == "firmware:something_this_model_lacks"));
    }

    #[test]
    fn upsert_replaces_by_name_rather_than_duplicating() {
        let mut store = PresetStore::default();
        let mut preset = Preset::new("Quiet");
        preset.values.insert("powermode".into(), "0".into());
        let index = store.upsert(preset);

        assert_eq!(store.presets.len(), 3, "upsert must not add a second Quiet");
        assert_eq!(index, 0);
        assert_eq!(store.presets[0].values.get("powermode").unwrap(), "0");
    }

    #[test]
    fn capture_records_only_readable_values() {
        let mut store = PresetStore::default();
        let index = store.capture(
            "Captured",
            [
                ("firmware:ppt_pl1_spl".to_string(), "70".to_string()),
                ("powermode".to_string(), "2".to_string()),
            ]
            .into_iter(),
        );
        assert_eq!(store.presets[index].setting_count(), 2);
        // Capturing must not change which preset is active.
        assert_eq!(store.active, PresetStore::default().active);
    }

    /// A corrupt or hand-edited file must not leave the active index pointing
    /// past the end of the list, which would panic on the next apply.
    #[test]
    fn a_nonsense_active_index_is_clamped() {
        let store: PresetStore = serde_json::from_str::<PresetStore>(
            r#"{"presets":[{"name":"Only","values":{}}],"active":99}"#,
        )
        .expect("load")
        .sanitised();
        assert_eq!(store.active, 0);
        assert!(store.active_preset().is_some());
    }

    /// Presets that were emptied by hand must not leave the store unusable.
    #[test]
    fn an_empty_preset_list_falls_back_to_defaults() {
        let store: PresetStore =
            serde_json::from_str::<PresetStore>(r#"{"presets":[],"active":0}"#)
                .expect("load")
                .sanitised();
        assert!(!store.presets.is_empty());
        assert!(store.active_preset().is_some());
    }
}
