use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the audio channel plays for each stimulus.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioSet {
    /// Spoken letters (c, h, k, l, q, r, s, t) — the classic dual n-back.
    #[default]
    Letters,
    /// Synthesized piano-like tones.
    Tones,
}

/// User settings plus the small bit of adaptive state (current level,
/// fallback streak) that has to survive restarts.
///
/// Defaults mirror Brain Workshop's standard mode.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Config {
    /// Current n-back level.
    pub n: usize,
    /// Automatically raise/lower n based on session scores.
    pub adaptive: bool,
    /// Length of one trial in milliseconds.
    pub trial_ms: u32,
    /// How long the square stays visible within a trial, in milliseconds.
    pub stim_ms: u32,
    /// Base trial count; the session total is `base_trials + n²`.
    pub base_trials: usize,
    /// Level up when the session score reaches this percentage.
    pub threshold_advance: u32,
    /// Level down after `fallback_sessions` consecutive sessions below this percentage.
    pub threshold_fallback: u32,
    /// How many consecutive below-fallback sessions trigger a level down.
    pub fallback_sessions: u32,
    /// Show green/red feedback as soon as a match key is pressed.
    pub feedback: bool,
    /// Master volume, 0.0–1.0.
    pub volume: f32,
    pub audio_set: AudioSet,
    /// Consecutive sessions below the fallback threshold at the current level.
    pub fallback_streak: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            n: 2,
            adaptive: true,
            trial_ms: 3000,
            stim_ms: 500,
            base_trials: 20,
            threshold_advance: 80,
            threshold_fallback: 50,
            fallback_sessions: 3,
            feedback: true,
            volume: 0.8,
            audio_set: AudioSet::Letters,
            fallback_streak: 0,
        }
    }
}

impl Config {
    /// Total trials for a session at level `n` (Brain Workshop: 20 + n²).
    pub fn total_trials(&self) -> usize {
        self.base_trials + self.n * self.n
    }

    pub fn load() -> Self {
        let path = config_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let dir = data_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("dual-nback: could not create {}: {e}", dir.display());
            return;
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = fs::write(config_path(), json) {
                    eprintln!("dual-nback: could not save config: {e}");
                }
            }
            Err(e) => eprintln!("dual-nback: could not serialize config: {e}"),
        }
    }
}

/// Per-user data directory (`~/Library/Application Support/dual-nback` on macOS).
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dual-nback")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips() {
        let cfg = Config {
            n: 4,
            audio_set: AudioSet::Tones,
            volume: 0.25,
            ..Config::default()
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn config_tolerates_missing_fields() {
        let cfg: Config = serde_json::from_str(r#"{ "n": 5 }"#).unwrap();
        assert_eq!(cfg.n, 5);
        assert_eq!(cfg.trial_ms, Config::default().trial_ms);
    }

    #[test]
    fn total_trials_follows_brainworkshop_formula() {
        let mut cfg = Config { n: 2, ..Config::default() };
        assert_eq!(cfg.total_trials(), 24);
        cfg.n = 3;
        assert_eq!(cfg.total_trials(), 29);
    }
}
