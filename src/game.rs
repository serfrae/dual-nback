//! Core dual n-back logic, independent of UI and audio.
//!
//! Mechanics mirror Brain Workshop's standard (non-Jaeggi) mode:
//! 8 grid positions (3×3 minus center), 8 spoken letters, 12.5% forced
//! matches, 12.5% interference lures, combined right/(right+wrong) scoring
//! that ignores correct rejections, and 80%/50%-with-3-strikes leveling.

use rand::seq::IndexedRandom;
use rand::{Rng, RngExt as _};
use serde::{Deserialize, Serialize};

/// Distinct values per modality: 8 grid cells and 8 letters.
pub const STIM_VALUES: u8 = 8;

/// Letters of the classic dual n-back audio channel, in stimulus order.
/// The embedded recordings in `audio.rs` follow this order; kept here as
/// the single place that documents the mapping.
#[allow(dead_code)]
pub const LETTERS: [char; STIM_VALUES as usize] = ['c', 'h', 'k', 'l', 'q', 'r', 's', 't'];

/// Chance that a trial is forced to match its n-back stimulus.
pub const CHANCE_GUARANTEED_MATCH: f64 = 0.125;
/// Chance that a trial is forced to be an interference lure instead.
pub const CHANCE_INTERFERENCE: f64 = 0.125;

/// Map a position stimulus (0..8) to its cell in the 3×3 grid; the center
/// cell (4) is reserved for the fixation cross.
pub fn position_cell(position: u8) -> usize {
    let p = position as usize;
    if p < 4 { p } else { p + 1 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trial {
    pub position: u8,
    pub letter: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionConfig {
    pub n: usize,
    pub total_trials: usize,
    pub trial_ms: f32,
    pub stim_ms: f32,
}

/// Tallies for one modality. Following Brain Workshop, "wrong" combines
/// misses and false alarms, and correct rejections are not counted.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModalityScore {
    pub hits: u32,
    pub misses: u32,
    pub false_alarms: u32,
    /// How many scored trials actually contained a match.
    pub matches: u32,
}

impl ModalityScore {
    pub fn wrong(&self) -> u32 {
        self.misses + self.false_alarms
    }

    /// Brain Workshop: `int(hits * 100 / (hits + wrong))`, 0 when empty.
    pub fn percent(&self) -> u32 {
        (self.hits * 100).checked_div(self.hits + self.wrong()).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionResult {
    pub n: usize,
    pub trials: usize,
    pub position: ModalityScore,
    pub audio: ModalityScore,
    /// Combined score across both modalities (the value leveling uses).
    pub score: u32,
}

/// What a key press meant, for immediate feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Correct,
    Wrong,
    /// Pressed during the first n trials, which are never scored.
    Unscored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickEvent {
    /// A new trial began: show its square and play its sound.
    TrialStarted(usize),
    /// The trial that just ended had an unanswered position match.
    MissedPosition,
    /// The trial that just ended had an unanswered audio match.
    MissedAudio,
    SessionEnded,
}

pub struct Session {
    pub cfg: SessionConfig,
    pub trials: Vec<Trial>,
    index: usize,
    elapsed_ms: f32,
    started: bool,
    finished: bool,
    pos_pressed: bool,
    aud_pressed: bool,
    pub position: ModalityScore,
    pub audio: ModalityScore,
}

impl Session {
    pub fn new(cfg: SessionConfig, rng: &mut impl Rng) -> Self {
        let trials = generate_trials(cfg.n, cfg.total_trials, rng);
        Self::with_trials(cfg, trials)
    }

    pub fn with_trials(cfg: SessionConfig, trials: Vec<Trial>) -> Self {
        Self {
            cfg,
            trials,
            index: 0,
            elapsed_ms: 0.0,
            started: false,
            finished: false,
            pos_pressed: false,
            aud_pressed: false,
            position: ModalityScore::default(),
            audio: ModalityScore::default(),
        }
    }

    pub fn start(&mut self) -> TickEvent {
        self.started = true;
        TickEvent::TrialStarted(0)
    }

    pub fn started(&self) -> bool {
        self.started
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn total(&self) -> usize {
        self.trials.len()
    }

    pub fn current_trial(&self) -> Trial {
        self.trials[self.index]
    }

    pub fn elapsed_in_trial(&self) -> f32 {
        self.elapsed_ms
    }

    pub fn pos_pressed(&self) -> bool {
        self.pos_pressed
    }

    pub fn aud_pressed(&self) -> bool {
        self.aud_pressed
    }

    /// Fraction of the whole session completed, for the progress bar.
    pub fn progress(&self) -> f32 {
        if self.trials.is_empty() {
            return 0.0;
        }
        let in_trial = (self.elapsed_ms / self.cfg.trial_ms).clamp(0.0, 1.0);
        (self.index as f32 + in_trial) / self.trials.len() as f32
    }

    pub fn is_position_match(&self, i: usize) -> bool {
        i >= self.cfg.n && self.trials[i].position == self.trials[i - self.cfg.n].position
    }

    pub fn is_audio_match(&self, i: usize) -> bool {
        i >= self.cfg.n && self.trials[i].letter == self.trials[i - self.cfg.n].letter
    }

    /// Advance time; trial transitions and the session end land in `events`.
    pub fn tick(&mut self, dt_ms: f32, events: &mut Vec<TickEvent>) {
        if !self.started || self.finished {
            return;
        }
        self.elapsed_ms += dt_ms;
        while self.elapsed_ms >= self.cfg.trial_ms && !self.finished {
            self.elapsed_ms -= self.cfg.trial_ms;
            self.finalize_current(events);
            if self.index + 1 >= self.trials.len() {
                self.finished = true;
                events.push(TickEvent::SessionEnded);
            } else {
                self.index += 1;
                self.pos_pressed = false;
                self.aud_pressed = false;
                events.push(TickEvent::TrialStarted(self.index));
            }
        }
    }

    pub fn press_position(&mut self) -> Option<Press> {
        if !self.started || self.finished || self.pos_pressed {
            return None;
        }
        self.pos_pressed = true;
        if self.index < self.cfg.n {
            return Some(Press::Unscored);
        }
        if self.is_position_match(self.index) {
            self.position.hits += 1;
            Some(Press::Correct)
        } else {
            self.position.false_alarms += 1;
            Some(Press::Wrong)
        }
    }

    pub fn press_audio(&mut self) -> Option<Press> {
        if !self.started || self.finished || self.aud_pressed {
            return None;
        }
        self.aud_pressed = true;
        if self.index < self.cfg.n {
            return Some(Press::Unscored);
        }
        if self.is_audio_match(self.index) {
            self.audio.hits += 1;
            Some(Press::Correct)
        } else {
            self.audio.false_alarms += 1;
            Some(Press::Wrong)
        }
    }

    fn finalize_current(&mut self, events: &mut Vec<TickEvent>) {
        let i = self.index;
        if i < self.cfg.n {
            return; // warm-up trials are never scored
        }
        if self.is_position_match(i) {
            self.position.matches += 1;
            if !self.pos_pressed {
                self.position.misses += 1;
                events.push(TickEvent::MissedPosition);
            }
        }
        if self.is_audio_match(i) {
            self.audio.matches += 1;
            if !self.aud_pressed {
                self.audio.misses += 1;
                events.push(TickEvent::MissedAudio);
            }
        }
    }

    pub fn result(&self) -> SessionResult {
        let right = self.position.hits + self.audio.hits;
        let wrong = self.position.wrong() + self.audio.wrong();
        let score = (right * 100).checked_div(right + wrong).unwrap_or(0);
        SessionResult {
            n: self.cfg.n,
            trials: self.trials.len(),
            position: self.position,
            audio: self.audio,
            score,
        }
    }
}

pub fn generate_trials(n: usize, total: usize, rng: &mut impl Rng) -> Vec<Trial> {
    let positions = generate_stream(n, total, rng);
    let letters = generate_stream(n, total, rng);
    positions
        .into_iter()
        .zip(letters)
        .map(|(position, letter)| Trial { position, letter })
        .collect()
}

fn generate_stream(n: usize, total: usize, rng: &mut impl Rng) -> Vec<u8> {
    let mut seq = Vec::with_capacity(total);
    for _ in 0..total {
        let v = next_stim(&seq, n, rng);
        seq.push(v);
    }
    seq
}

/// Brain Workshop's stimulus choice: a uniformly random value, except a
/// 12.5% chance of forcing the n-back match and a 12.5% chance of forcing an
/// interference lure — a repeat of the (n−1)-, (n+1)- or 2n-back stimulus
/// (the (n−1) lure is skipped for n = 2). Lures must differ from the real
/// n-back value; if no candidate qualifies, the random value stands.
fn next_stim(seq: &[u8], n: usize, rng: &mut impl Rng) -> u8 {
    let i = seq.len();
    let random_value = rng.random_range(0..STIM_VALUES);
    if i < n {
        return random_value;
    }
    let target = seq[i - n];
    let r1: f64 = rng.random();
    let r2: f64 = rng.random();
    if r1 < CHANCE_GUARANTEED_MATCH {
        return target;
    }
    if r2 < CHANCE_INTERFERENCE && n > 1 {
        let distances: &[usize] = if n < 3 { &[n + 1, 2 * n] } else { &[n - 1, n + 1, 2 * n] };
        let lures: Vec<u8> = distances
            .iter()
            .filter(|&&back| back <= i)
            .map(|&back| seq[i - back])
            .filter(|&v| v != target)
            .collect();
        if let Some(&lure) = lures.choose(rng) {
            return lure;
        }
    }
    random_value
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Advance,
    Fallback,
    Stay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelChange {
    pub outcome: Outcome,
    pub n: usize,
    pub streak: u32,
}

/// Brain Workshop leveling. Advance at `threshold_advance` or better. A score
/// below `threshold_fallback` increments a strike counter — which mid-range
/// scores do NOT reset — and the level drops (never below 1) once
/// `fallback_sessions` strikes accumulate. Level changes reset the counter.
pub fn decide_level(
    n: usize,
    score: u32,
    threshold_advance: u32,
    threshold_fallback: u32,
    fallback_sessions: u32,
    streak: u32,
) -> LevelChange {
    if score >= threshold_advance {
        LevelChange { outcome: Outcome::Advance, n: n + 1, streak: 0 }
    } else if n > 1 && score < threshold_fallback {
        if streak + 1 >= fallback_sessions {
            LevelChange { outcome: Outcome::Fallback, n: n - 1, streak: 0 }
        } else {
            LevelChange { outcome: Outcome::Stay, n, streak: streak + 1 }
        }
    } else {
        LevelChange { outcome: Outcome::Stay, n, streak }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn test_cfg(n: usize, total: usize) -> SessionConfig {
        SessionConfig { n, total_trials: total, trial_ms: 100.0, stim_ms: 50.0 }
    }

    fn trial(position: u8, letter: u8) -> Trial {
        Trial { position, letter }
    }

    #[test]
    fn position_cell_skips_center() {
        let cells: Vec<usize> = (0..STIM_VALUES).map(position_cell).collect();
        assert_eq!(cells, vec![0, 1, 2, 3, 5, 6, 7, 8]);
    }

    #[test]
    fn generates_requested_trials_in_range() {
        let mut rng = StdRng::seed_from_u64(7);
        let trials = generate_trials(2, 24, &mut rng);
        assert_eq!(trials.len(), 24);
        assert!(trials.iter().all(|t| t.position < STIM_VALUES && t.letter < STIM_VALUES));
    }

    #[test]
    fn match_rate_is_near_brain_workshop_expectation() {
        // Forced matches (12.5%) plus accidental ones (~7/8 · 7/8 · 1/8)
        // should put the long-run match rate near 22%.
        let mut rng = StdRng::seed_from_u64(42);
        let n = 2;
        let seq = generate_stream(n, 20_000, &mut rng);
        let matches = (n..seq.len()).filter(|&i| seq[i] == seq[i - n]).count();
        let rate = matches as f64 / (seq.len() - n) as f64;
        assert!((0.18..0.27).contains(&rate), "match rate {rate} out of range");
    }

    #[test]
    fn scores_a_scripted_session() {
        let cfg = test_cfg(2, 5);
        let trials = vec![trial(0, 5), trial(1, 5), trial(0, 5), trial(2, 6), trial(2, 7)];
        let mut s = Session::with_trials(cfg, trials);
        let mut events = Vec::new();

        assert_eq!(s.start(), TickEvent::TrialStarted(0));
        // Warm-up press: locked in but never scored.
        assert_eq!(s.press_position(), Some(Press::Unscored));
        assert_eq!(s.press_position(), None);

        s.tick(100.0, &mut events);
        assert_eq!(events, vec![TickEvent::TrialStarted(1)]);
        events.clear();

        s.tick(100.0, &mut events);
        assert_eq!(events, vec![TickEvent::TrialStarted(2)]);
        events.clear();
        // Trial 2: both modalities match. Catch position, ignore audio.
        assert_eq!(s.press_position(), Some(Press::Correct));

        s.tick(100.0, &mut events);
        assert_eq!(events, vec![TickEvent::MissedAudio, TickEvent::TrialStarted(3)]);
        events.clear();
        // Trial 3: no match anywhere; this press is a false alarm.
        assert_eq!(s.press_audio(), Some(Press::Wrong));

        s.tick(100.0, &mut events);
        assert_eq!(events, vec![TickEvent::TrialStarted(4)]);
        events.clear();

        s.tick(100.0, &mut events);
        assert_eq!(events, vec![TickEvent::SessionEnded]);
        assert!(s.finished());

        assert_eq!(s.position, ModalityScore { hits: 1, misses: 0, false_alarms: 0, matches: 1 });
        assert_eq!(s.audio, ModalityScore { hits: 0, misses: 1, false_alarms: 1, matches: 1 });

        let result = s.result();
        assert_eq!(s.position.percent(), 100);
        assert_eq!(s.audio.percent(), 0);
        // Combined: 1 right, 2 wrong → floor(100/3).
        assert_eq!(result.score, 33);
    }

    #[test]
    fn percent_floors_and_handles_empty() {
        let empty = ModalityScore::default();
        assert_eq!(empty.percent(), 0);
        let one_of_three =
            ModalityScore { hits: 1, misses: 1, false_alarms: 1, matches: 2 };
        assert_eq!(one_of_three.percent(), 33);
    }

    #[test]
    fn several_trials_can_elapse_in_one_tick() {
        let cfg = test_cfg(1, 3);
        let trials = vec![trial(0, 0), trial(1, 1), trial(2, 2)];
        let mut s = Session::with_trials(cfg, trials);
        let mut events = Vec::new();
        s.start();
        s.tick(350.0, &mut events);
        assert!(s.finished());
        assert_eq!(
            events,
            vec![
                TickEvent::TrialStarted(1),
                TickEvent::TrialStarted(2),
                TickEvent::SessionEnded
            ]
        );
    }

    #[test]
    fn leveling_follows_brain_workshop_rules() {
        // Advance resets the strike counter.
        let up = decide_level(2, 85, 80, 50, 3, 2);
        assert_eq!(up, LevelChange { outcome: Outcome::Advance, n: 3, streak: 0 });

        // Mid-range scores keep the counter as-is (no reset).
        let mid = decide_level(2, 60, 80, 50, 3, 2);
        assert_eq!(mid, LevelChange { outcome: Outcome::Stay, n: 2, streak: 2 });

        // Below-fallback scores accumulate strikes…
        let strike1 = decide_level(2, 40, 80, 50, 3, 0);
        assert_eq!(strike1, LevelChange { outcome: Outcome::Stay, n: 2, streak: 1 });

        // …and the third strike drops the level.
        let drop = decide_level(2, 40, 80, 50, 3, 2);
        assert_eq!(drop, LevelChange { outcome: Outcome::Fallback, n: 1, streak: 0 });

        // n = 1 never falls further and accrues no strikes.
        let floor = decide_level(1, 10, 80, 50, 3, 1);
        assert_eq!(floor, LevelChange { outcome: Outcome::Stay, n: 1, streak: 1 });

        // fallback_sessions = 1 drops immediately.
        let instant = decide_level(3, 20, 80, 50, 1, 0);
        assert_eq!(instant, LevelChange { outcome: Outcome::Fallback, n: 2, streak: 0 });
    }
}
