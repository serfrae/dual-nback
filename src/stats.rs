//! Persistent session history and the aggregates shown on the stats screen.

use std::fs;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Serialize};

use crate::config::history_path;
use crate::game::Outcome;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SessionRecord {
    pub ts: DateTime<Local>,
    pub n: usize,
    pub position_pct: u32,
    pub audio_pct: u32,
    pub score: u32,
    pub trials: usize,
    pub outcome: Outcome,
    #[serde(default)]
    pub manual: bool,
}

#[derive(Default)]
pub struct History {
    pub records: Vec<SessionRecord>,
}

impl History {
    pub fn load() -> Self {
        let records = fs::read_to_string(history_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { records }
    }

    pub fn save(&self) {
        if let Err(e) = fs::create_dir_all(crate::config::data_dir()) {
            eprintln!("dual-nback: could not create data dir: {e}");
            return;
        }
        match serde_json::to_string_pretty(&self.records) {
            Ok(json) => {
                if let Err(e) = fs::write(history_path(), json) {
                    eprintln!("dual-nback: could not save history: {e}");
                }
            }
            Err(e) => eprintln!("dual-nback: could not serialize history: {e}"),
        }
    }

    pub fn push(&mut self, record: SessionRecord) {
        self.records.push(record);
        self.save();
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.save();
    }

    /// Highest level reached: the n of each session, plus one if it advanced.
    pub fn best_level(&self) -> Option<usize> {
        self.records
            .iter()
            .map(|r| r.n + usize::from(r.outcome == Outcome::Advance))
            .max()
    }

    pub fn sessions_today(&self) -> usize {
        let today = Local::now().date_naive();
        self.records.iter().filter(|r| r.ts.date_naive() == today).count()
    }

    /// Mean session score over the past `days` days, if any sessions exist.
    pub fn avg_score_recent(&self, days: i64) -> Option<u32> {
        let cutoff = Local::now() - Duration::days(days);
        let recent: Vec<u32> =
            self.records.iter().filter(|r| r.ts > cutoff).map(|r| r.score).collect();
        if recent.is_empty() {
            None
        } else {
            Some(recent.iter().sum::<u32>() / recent.len() as u32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_level_counts_advances() {
        let rec = |n, outcome| SessionRecord {
            ts: Local::now(),
            n,
            position_pct: 50,
            audio_pct: 50,
            score: 50,
            trials: 24,
            outcome,
            manual: false,
        };
        let history = History {
            records: vec![rec(2, Outcome::Stay), rec(3, Outcome::Advance), rec(3, Outcome::Stay)],
        };
        assert_eq!(history.best_level(), Some(4));
        assert_eq!(History::default().best_level(), None);
    }

    #[test]
    fn record_round_trips_through_json() {
        let record = SessionRecord {
            ts: Local::now(),
            n: 3,
            position_pct: 80,
            audio_pct: 75,
            score: 77,
            trials: 29,
            outcome: Outcome::Advance,
            manual: true,
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: SessionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.n, 3);
        assert_eq!(back.outcome, Outcome::Advance);
        assert!(back.manual);
    }
}
