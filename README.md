# Dual N-Back

A modern, native dual n-back working-memory trainer written in Rust — a
reimplementation of the classic [Brain Workshop](https://github.com/brain-workshop/brainworkshop)
dual n-back mode with a cleaner interface, adaptive difficulty, and built-in
progress tracking. Single self-contained binary; the letter recordings are
embedded at compile time.

## How to play

Each trial a blue square appears in one of 8 grid positions while a letter is
spoken. Your job is to spot repeats from **n trials ago**, per channel:

- Press **A** (or click the Position chip) when the square's position matches
  the position from n trials back.
- Press **L** (or click the Sound chip) when the letter matches the letter
  from n trials back.

Both, one, or neither can match on any trial. The first n trials of a session
are warm-up and are never scored.

**Other keys:** `Space` pause/resume (the game also auto-pauses when the
window loses focus) · `Esc` end the session · `←`/`→` change level in the
menu · `S` stats.

## Mechanics (faithful to Brain Workshop's standard mode)

- 8 positions (3×3 grid, center reserved for the fixation cross) and 8 spoken
  letters (c, h, k, l, q, r, s, t).
- Session length is `20 + n²` trials (24 at Dual 2-Back), 3.0 s per trial,
  square visible for 0.5 s — all adjustable in Settings.
- Stimulus generation: each trial has a 12.5% chance of a forced n-back match
  and a 12.5% chance of a forced interference lure (a repeat from n−1, n+1,
  or 2n trials back) on top of uniform randomness.
- Scoring: per channel, a press on a match is a *hit*; a missed match or a
  press without a match is *wrong*. Correct rejections aren't counted. The
  session score is `hits / (hits + wrong)` combined across both channels.
- Adaptive leveling: score **≥ 80%** → n goes up; score **< 50%** earns a
  strike (mid-range scores don't clear strikes), and three strikes drop n by
  one. Disable *Adaptive level* to train at a fixed n.

## Modern additions

- Adaptive **or** manual level, countdown lead-in, pause, click/touch input.
- Instant right/wrong/missed feedback on the input chips (can be turned off).
- Stats screen: best level, sessions today, 7-day average, a chart of score
  and n-level over your last 40 sessions, and a recent-session table.
- Synthesized tone set as an alternative audio channel, plus volume control
  and gentle end-of-session / level-up cues.
- Config and history are plain JSON in your platform data directory
  (`~/Library/Application Support/dual-nback` on macOS), shown at the bottom
  of the Settings screen.

## Build & run

```sh
cargo run --release
```

Requires Rust 1.85+ (edition 2024). Tests: `cargo test`.

## License & attribution

The letter recordings in `assets/sounds/letters/` come from
[Brain Workshop](https://github.com/brain-workshop/brainworkshop), which is
licensed under the **GPL-2.0**; this project is therefore distributed under
**GPL-2.0-or-later** (see `assets/ATTRIBUTION.md`). The Rust code is a
clean-room reimplementation based on Brain Workshop's documented behavior.
