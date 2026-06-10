//! Audio output: the embedded letter recordings, a synthesized tone set,
//! and small end-of-session cues. All playback is fire-and-forget through
//! one rodio mixer; if no output device is available the game runs silent.

use std::num::NonZero;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Source};

use crate::config::AudioSet;
use crate::game::STIM_VALUES;

/// Letter recordings from Brain Workshop (GPL-2.0), embedded so the binary
/// is self-contained. Order matches `game::LETTERS`.
const LETTER_WAVS: [&[u8]; STIM_VALUES as usize] = [
    include_bytes!("../assets/sounds/letters/c.wav"),
    include_bytes!("../assets/sounds/letters/h.wav"),
    include_bytes!("../assets/sounds/letters/k.wav"),
    include_bytes!("../assets/sounds/letters/l.wav"),
    include_bytes!("../assets/sounds/letters/q.wav"),
    include_bytes!("../assets/sounds/letters/r.wav"),
    include_bytes!("../assets/sounds/letters/s.wav"),
    include_bytes!("../assets/sounds/letters/t.wav"),
];

/// Pentatonic-ish pitches so the tone set stays pleasant over long sessions.
const TONE_HZ: [f32; STIM_VALUES as usize] =
    [261.63, 293.66, 329.63, 392.00, 440.00, 523.25, 587.33, 659.25];

const SYNTH_RATE: u32 = 44_100;

struct Clip {
    channels: u16,
    rate: u32,
    samples: Vec<f32>,
}

pub struct Audio {
    device: Option<MixerDeviceSink>,
    letters: Vec<Clip>,
    tones: Vec<Clip>,
    advance_cue: Clip,
    fallback_cue: Clip,
    end_cue: Clip,
    pub volume: f32,
}

impl Audio {
    pub fn new(volume: f32) -> Self {
        let device = match DeviceSinkBuilder::open_default_sink() {
            Ok(mut d) => {
                d.log_on_drop(false);
                Some(d)
            }
            Err(e) => {
                eprintln!("dual-nback: audio unavailable, playing silent: {e}");
                None
            }
        };
        let letters: Vec<Clip> = LETTER_WAVS.iter().filter_map(|bytes| decode_wav(bytes)).collect();
        let letters = if letters.len() == STIM_VALUES as usize { letters } else { Vec::new() };
        let tones = TONE_HZ.iter().map(|&hz| melody(&[(hz, 0.42)])).collect();
        Self {
            device,
            letters,
            tones,
            advance_cue: melody(&[(523.25, 0.12), (659.25, 0.12), (783.99, 0.26)]),
            fallback_cue: melody(&[(440.00, 0.14), (349.23, 0.24)]),
            end_cue: melody(&[(392.00, 0.20)]),
            volume,
        }
    }

    pub fn has_letters(&self) -> bool {
        !self.letters.is_empty()
    }

    /// Play the audio stimulus for one trial.
    pub fn play_stimulus(&self, set: AudioSet, index: usize) {
        let clip = match set {
            AudioSet::Letters if self.has_letters() => &self.letters[index],
            _ => &self.tones[index],
        };
        self.play(clip, 1.0);
    }

    pub fn play_advance_cue(&self) {
        self.play(&self.advance_cue, 0.8);
    }

    pub fn play_fallback_cue(&self) {
        self.play(&self.fallback_cue, 0.6);
    }

    pub fn play_end_cue(&self) {
        self.play(&self.end_cue, 0.6);
    }

    fn play(&self, clip: &Clip, gain: f32) {
        let Some(device) = &self.device else { return };
        let (Some(channels), Some(rate)) = (NonZero::new(clip.channels), NonZero::new(clip.rate))
        else {
            return;
        };
        let source = SamplesBuffer::new(channels, rate, clip.samples.clone());
        device.mixer().add(source.amplify(self.volume * gain));
    }
}

fn decode_wav(bytes: &'static [u8]) -> Option<Clip> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(bytes)).ok()?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / scale)
                .collect()
        }
        hound::SampleFormat::Float => {
            reader.samples::<f32>().filter_map(Result::ok).collect()
        }
    };
    if samples.is_empty() {
        return None;
    }
    Some(Clip { channels: spec.channels, rate: spec.sample_rate, samples })
}

/// Synthesize a soft sine voice (fundamental plus a quiet octave) with a
/// short attack and release so notes never click.
fn tone_samples(hz: f32, seconds: f32) -> Vec<f32> {
    let count = (SYNTH_RATE as f32 * seconds) as usize;
    let attack = (SYNTH_RATE as f32 * 0.012) as usize;
    let release = (SYNTH_RATE as f32 * 0.090) as usize;
    (0..count)
        .map(|i| {
            let t = i as f32 / SYNTH_RATE as f32;
            let mut env = 1.0f32;
            if i < attack {
                env = i as f32 / attack as f32;
            }
            let remaining = count - i;
            if remaining < release {
                env = env.min(remaining as f32 / release as f32);
            }
            let tau = std::f32::consts::TAU;
            ((t * hz * tau).sin() * 0.34 + (t * hz * 2.0 * tau).sin() * 0.07) * env
        })
        .collect()
}

fn melody(notes: &[(f32, f32)]) -> Clip {
    let mut samples = Vec::new();
    for &(hz, seconds) in notes {
        samples.extend(tone_samples(hz, seconds));
    }
    Clip { channels: 1, rate: SYNTH_RATE, samples }
}
