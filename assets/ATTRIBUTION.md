# Asset attribution

`sounds/letters/*.wav` (the spoken letters c, h, k, l, q, r, s, t) are taken
unmodified from Brain Workshop:

- Source: https://github.com/brain-workshop/brainworkshop
  (`res/sounds/letters/`)
- License: GNU General Public License, version 2 (see the upstream
  `LICENSE.md`)

These files are embedded into the binary at compile time (`src/audio.rs`).
Because of this, distributing this project including these assets requires a
GPL-compatible license; the crate is marked `GPL-2.0-or-later` accordingly.
If you replace the recordings with your own, the Rust source itself carries
no obligation to Brain Workshop's license — it is an independent
reimplementation.
