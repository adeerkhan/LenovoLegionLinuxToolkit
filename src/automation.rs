//! Automation — triggers that run a set of capability writes.
//!
//! The Windows reference has 33 trigger types and 51 step types, each written
//! out by hand. We take the idea and lean on the capability list instead: a
//! *pipeline* is a trigger plus an ordered list of `(capability key, value)`
//! steps, and applying it writes each step through the same path a slider uses.
//! A capability the machine cannot write can never be a step, because the step
//! is looked up in the same report the UI is built from.
//!
//! This module is deliberately free of GTK: it is pure state and evaluation, so
//! the trigger edges are unit-tested without a display. The window drives it
//! from its existing one-second tick.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What causes a pipeline to run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// The AC adapter is connected.
    OnAc,
    /// The AC adapter is disconnected.
    OnBattery,
    /// A wall-clock interval, checked against the tick.
    Interval { secs: u64 },
    /// The power mode becomes `value`.
    PowerMode { value: String },
}

impl Trigger {
    /// The wire id used by the UI's trigger dropdown.
    pub fn id(&self) -> String {
        match self {
            Self::OnAc => "ac".into(),
            Self::OnBattery => "battery".into(),
            Self::Interval { secs } => format!("interval:{secs}"),
            Self::PowerMode { value } => format!("powermode:{value}"),
        }
    }

    /// Rebuild a trigger from a dropdown id. `None` for an id this build does
    /// not know, so a file from a newer version cannot invent behaviour.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "ac" => Some(Self::OnAc),
            "battery" => Some(Self::OnBattery),
            other => {
                let (kind, arg) = other.split_once(':')?;
                match kind {
                    "interval" => Some(Self::Interval {
                        secs: arg.parse().ok()?,
                    }),
                    "powermode" if !arg.is_empty() => Some(Self::PowerMode {
                        value: arg.to_string(),
                    }),
                    _ => None,
                }
            }
        }
    }

    /// A sentence for the card subtitle.
    pub fn describe(&self) -> String {
        match self {
            Self::OnAc => "When the AC adapter is connected".into(),
            Self::OnBattery => "When running on battery".into(),
            Self::Interval { secs } => {
                if secs % 60 == 0 {
                    format!("Every {} minutes", secs / 60)
                } else {
                    format!("Every {secs} seconds")
                }
            }
            Self::PowerMode { value } => format!("When power mode becomes {value}"),
        }
    }
}

/// One capability write.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub key: String,
    pub value: String,
}

/// A trigger and the steps it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pipeline {
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub trigger: Trigger,
    #[serde(default)]
    pub steps: Vec<Step>,
}

fn default_enabled() -> bool {
    true
}

impl Pipeline {
    pub fn new(name: impl Into<String>, trigger: Trigger) -> Self {
        Self {
            name: name.into(),
            enabled: true,
            trigger,
            steps: Vec::new(),
        }
    }

    /// A one-line summary of what the pipeline does, for the card.
    pub fn summary(&self) -> String {
        if self.steps.is_empty() {
            "No steps yet — use Capture current to record the settings to apply.".into()
        } else {
            format!("{} step(s)", self.steps.len())
        }
    }
}

/// Persisted set of pipelines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutomationStore {
    pub pipelines: Vec<Pipeline>,
}

impl AutomationStore {
    /// Where the automation file lives, beside settings and presets.
    pub fn path() -> Option<PathBuf> {
        crate::settings::Settings::path().map(|p| p.with_file_name("automation.json"))
    }

    /// Read from disk, falling back to defaults. Same contract as settings and
    /// presets: a missing or corrupt file restores defaults rather than
    /// preventing the application from starting.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str::<Self>(&text).unwrap_or_else(|e| {
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

    /// Add or replace a pipeline by name, returning its index.
    pub fn upsert(&mut self, pipeline: Pipeline) -> usize {
        match self.pipelines.iter().position(|p| p.name == pipeline.name) {
            Some(index) => {
                self.pipelines[index] = pipeline;
                index
            }
            None => {
                self.pipelines.push(pipeline);
                self.pipelines.len() - 1
            }
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.pipelines.retain(|p| p.name != name);
    }
}

/// What the engine is told about the machine each tick.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub on_ac: bool,
    pub power_mode: Option<String>,
}

/// Edge detection and interval timing.
///
/// Kept as state beside the store rather than inside it: what has *already*
/// happened is not something to persist, and a restart should re-arm the edges
/// instead of firing immediately on the first observation.
#[derive(Debug)]
pub struct Engine {
    last_ac: Option<bool>,
    last_power_mode: Option<String>,
    /// When each interval pipeline last fired, in seconds since start.
    last_interval: HashMap<String, u64>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            last_ac: None,
            last_power_mode: None,
            last_interval: HashMap::new(),
        }
    }

    /// The pipelines that should run now, by index into `store.pipelines`.
    ///
    /// `now_secs` is monotonic seconds since start, passed in rather than read
    /// from a clock so the edges and intervals are testable.
    pub fn evaluate(
        &mut self,
        env: &Environment,
        now_secs: u64,
        store: &AutomationStore,
    ) -> Vec<usize> {
        let previous_ac = self.last_ac;
        let previous_mode = self.last_power_mode.clone();
        let mut fire = Vec::new();

        for (index, pipeline) in store.pipelines.iter().enumerate() {
            if !pipeline.enabled {
                continue;
            }
            let triggered = match &pipeline.trigger {
                // An edge, not a level: firing while already on AC would run
                // the pipeline on every tick.
                Trigger::OnAc => matches!(previous_ac, Some(false)) && env.on_ac,
                Trigger::OnBattery => matches!(previous_ac, Some(true)) && !env.on_ac,
                Trigger::PowerMode { value } => {
                    previous_mode.is_some()
                        && env.power_mode.as_deref() == Some(value.as_str())
                        && previous_mode.as_deref() != Some(value.as_str())
                }
                Trigger::Interval { secs } => {
                    match self.last_interval.get(&pipeline.name).copied() {
                        // First sighting arms the timer without firing, so a
                        // restart does not run every periodic pipeline at once.
                        None => {
                            self.last_interval.insert(pipeline.name.clone(), now_secs);
                            false
                        }
                        Some(last) => {
                            if now_secs.saturating_sub(last) >= *secs {
                                self.last_interval.insert(pipeline.name.clone(), now_secs);
                                true
                            } else {
                                false
                            }
                        }
                    }
                }
            };
            if triggered {
                fire.push(index);
            }
        }

        self.last_ac = Some(env.on_ac);
        self.last_power_mode = env.power_mode.clone();
        fire
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipeline(name: &str, trigger: Trigger) -> Pipeline {
        Pipeline::new(name, trigger)
    }

    #[test]
    fn ac_edge_fires_once_on_change_not_on_first_sight() {
        let store = AutomationStore {
            pipelines: vec![pipeline("on-ac", Trigger::OnAc)],
        };
        let mut engine = Engine::new();

        // First observation while on battery: arms, does not fire.
        assert!(engine
            .evaluate(
                &Environment {
                    on_ac: false,
                    power_mode: None
                },
                0,
                &store
            )
            .is_empty());

        // Still on battery: no edge.
        assert!(engine
            .evaluate(
                &Environment {
                    on_ac: false,
                    power_mode: None
                },
                1,
                &store
            )
            .is_empty());

        // Plugged in: fires.
        assert_eq!(
            engine.evaluate(
                &Environment {
                    on_ac: true,
                    power_mode: None
                },
                2,
                &store
            ),
            vec![0]
        );

        // Still plugged in: the level has not changed again.
        assert!(engine
            .evaluate(
                &Environment {
                    on_ac: true,
                    power_mode: None
                },
                3,
                &store
            )
            .is_empty());
    }

    #[test]
    fn interval_fires_only_after_the_period_elapses() {
        let store = AutomationStore {
            pipelines: vec![pipeline("every-5", Trigger::Interval { secs: 5 })],
        };
        let mut engine = Engine::new();
        let env = Environment::default();

        assert!(engine.evaluate(&env, 100, &store).is_empty(), "arms");
        assert!(engine.evaluate(&env, 103, &store).is_empty(), "not yet");
        assert_eq!(engine.evaluate(&env, 105, &store), vec![0], "fires");
        assert!(engine.evaluate(&env, 106, &store).is_empty(), "re-armed");
        assert_eq!(engine.evaluate(&env, 110, &store), vec![0], "fires again");
    }

    #[test]
    fn power_mode_fires_on_change_and_not_for_the_previous_value() {
        let store = AutomationStore {
            pipelines: vec![pipeline(
                "perf",
                Trigger::PowerMode {
                    value: "performance".into(),
                },
            )],
        };
        let mut engine = Engine::new();

        let mode = |m: &str| Environment {
            on_ac: true,
            power_mode: Some(m.to_string()),
        };

        // First sighting is the current mode: arms, does not fire.
        assert!(engine.evaluate(&mode("performance"), 0, &store).is_empty());
        assert!(engine.evaluate(&mode("balanced"), 1, &store).is_empty());
        assert_eq!(engine.evaluate(&mode("performance"), 2, &store), vec![0]);
    }

    #[test]
    fn a_disabled_pipeline_never_fires() {
        let mut disabled = pipeline("on-ac", Trigger::OnAc);
        disabled.enabled = false;
        let store = AutomationStore {
            pipelines: vec![disabled],
        };
        let mut engine = Engine::new();
        engine.evaluate(
            &Environment {
                on_ac: false,
                power_mode: None,
            },
            0,
            &store,
        );
        assert!(engine
            .evaluate(
                &Environment {
                    on_ac: true,
                    power_mode: None
                },
                1,
                &store
            )
            .is_empty());
    }

    #[test]
    fn trigger_ids_round_trip() {
        for trigger in [
            Trigger::OnAc,
            Trigger::OnBattery,
            Trigger::Interval { secs: 900 },
            Trigger::PowerMode {
                value: "custom".into(),
            },
        ] {
            assert_eq!(Trigger::from_id(&trigger.id()), Some(trigger));
        }
        assert_eq!(Trigger::from_id("nonsense"), None);
        assert_eq!(Trigger::from_id("interval:notanumber"), None);
    }

    #[test]
    fn upsert_replaces_by_name_and_remove_drops_it() {
        let mut store = AutomationStore::default();
        store.upsert(pipeline("a", Trigger::OnAc));
        store.upsert(pipeline("a", Trigger::OnBattery));
        assert_eq!(store.pipelines.len(), 1);
        assert_eq!(store.pipelines[0].trigger, Trigger::OnBattery);

        store.remove("a");
        assert!(store.pipelines.is_empty());
    }
}
