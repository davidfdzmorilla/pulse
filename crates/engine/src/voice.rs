//! Voice effects: the Vocoder, and «Voz: tono y formantes» (a voice moved in
//! pitch and in formants, each on its own). Constructors allocate; `process`
//! never does, and everything runs sample by sample, so the sound doesn't
//! depend on how the blocks are cut.
//!
//! **Vocoder.** Two banks of band-pass filters (two TPT state-variable
//! sections in cascade, 24 dB/oct skirts), log-spaced between `low_hz` and
//! `high_hz`. The first hears the modulator (the track's own sound, the
//! voice) and follows each band's level (mean square, with its attack and
//! release); the second splits the carrier (another track by sidechain, or
//! its own saw, super saw or noise) and each of its bands is set to the
//! voice's level there. The carrier's bands are first evened out (each over
//! its own level, at most 26 dB up), so a dark carrier sounds as clear as a
//! bright one, and its overall level gates the result: when the carrier goes
//! quiet, so does the vocoder. `formant` moves the carrier's bank against the
//! voice's. The voice's own highs (a high-pass at 5 kHz) can be mixed in, so
//! its esses and tees are understood.
//!
//! Its own carrier plays `note` and `chord`, or with `midi`, the notes that
//! reach the track (or another, `sidechain`): the engine hands it each
//! block's note events, on their frames ([`Vocoder::feed_notes`]). Up to
//! [`VOCODER_VOICES`] notes at once, each with a short envelope of its own
//! (no click), and the esses come in with the notes: without notes, silence.
//!
//! **VoiceShift.** Pitch-synchronous overlap-add (TD-PSOLA). YIN (the
//! Tuner's detector) hears the voice's period every 5 ms; grains two periods
//! long (Hann), cut a period apart along the voice, are laid out again a
//! period ÷ the pitch ratio apart: the pitch moves and the formants (each
//! grain's shape) stay. Each grain is also read faster or slower (resampled
//! by the formant ratio), which moves the formants and not the pitch.
//! Without a voice (esses, breath, silence) the grains keep their spacing:
//! no buzz is made out of noise. It sounds [`VoiceShift::latency`] late
//! (about 32 ms, the same at any setting), which the mixer makes up for.

use crate::effects::Key;
use crate::engine::{Event, MAX_EVENTS};
use crate::model::{db_to_gain, VocoderCarrier, VocoderParams, VoiceShiftParams, VOCODER_MAX_BANDS, VOCODER_VOICES};
use crate::tuner::Detector;
use std::f32::consts::PI;

/// A setting that glides to where it is set (one pole, per sample).
#[derive(Clone, Copy)]
struct Smooth {
    value: f32,
    coef: f32,
}

impl Smooth {
    /// Settles in about `ms` (time constant).
    fn new(value: f32, ms: f32, sr: f32) -> Self {
        Self { value, coef: (-1.0 / (ms * 0.001 * sr)).exp() }
    }

    #[inline]
    fn next(&mut self, target: f32) -> f32 {
        if self.value != target {
            self.value = target + self.coef * (self.value - target);
            if (self.value - target).abs() <= 1e-6 * target.abs().max(1.0) {
                self.value = target;
            }
        }
        self.value
    }
}

/// Topology-preserving state variable filter.
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy, Default)]
struct Coefs {
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Coefs {
    fn new(cutoff: f32, q: f32, sr: f32) -> Self {
        let g = (PI * cutoff.clamp(10.0, sr * 0.45) / sr).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        Self { a1, a2: g * a1, a3: g * g * a1, k }
    }
}

impl Svf {
    /// (band, high); the band-pass is normalized (1 at its centre).
    #[inline]
    fn tick(&mut self, x: f32, c: &Coefs) -> (f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (c.k * v1, x - c.k * v1 - v2)
    }

    /// Clears a state that has decayed to nothing (no subnormals).
    #[inline]
    fn settle(&mut self) {
        if self.ic1.abs() < 1e-20 && self.ic2.abs() < 1e-20 {
            *self = Svf::default();
        }
    }
}

// ----- Vocoder ---------------------------------------------------------------------------

const MAX: usize = VOCODER_MAX_BANDS;
/// The carrier's bands are evened out at most this far up (−40 dB, as power).
const CARRIER_FLOOR: f32 = 1e-4;
/// A band comes out at most this many times the voice's level there: a chord
/// struck on the carrier, ahead of its level, doesn't jump out.
const BAND_CEILING: f32 = 4.0;
/// Below this carrier level (RMS) the vocoder gets quieter with it.
const CARRIER_REF: f32 = 0.03;
/// How fast the carrier's levels are followed, ms.
const CARRIER_MS: f32 = 10.0;
/// The voice's highs mixed in for its esses, above this (Hz).
const SIBILANCE_HZ: f32 = 5000.0;
/// The vocoder's level against the voice's (makes up for the narrow bands).
const MAKEUP: f32 = 1.0;
/// Each super saw note's saws, and how far apart they go (cents at full
/// detune, the outer ones).
const SAWS: usize = 7;
const SPREAD_CENTS: f32 = 50.0;
/// How fast a note played by MIDI comes in and goes (ms): short, no click.
const NOTE_ATTACK_MS: f32 = 3.0;
const NOTE_RELEASE_MS: f32 = 25.0;
/// Each such note's saws, at full level.
const NOTE_LEVEL: f32 = 0.35;

/// A note its own carrier plays, following MIDI.
#[derive(Clone, Copy, Default)]
struct NoteVoice {
    pitch: u8,
    /// Sounding (held, or for its gate); false: let go, fading out.
    on: bool,
    /// Until its note-off (else for `gate` more samples).
    held: bool,
    gate: u32,
    level: f32,
    /// When it was struck (the oldest is taken for a new note).
    struck: u64,
    /// Each saw's phase and its step.
    phases: [f32; SAWS],
    steps: [f32; SAWS],
}

pub struct Vocoder {
    pub(crate) params: VocoderParams,
    pub(crate) key: Key,
    rate: f32,
    /// What its filters were made for: bands, low, high, formant.
    made: (usize, f32, f32, f32),
    bands: usize,
    analysis: [Coefs; MAX],
    synthesis: [Coefs; MAX],
    voice: [[Svf; 2]; MAX],
    carrier: [[Svf; 2]; MAX],
    /// Each band's level (mean square) of the voice and of the carrier, and
    /// the carrier's in all.
    voice_level: [f32; MAX],
    carrier_level: [f32; MAX],
    total: f32,
    highs: [Svf; 2],
    highs_coefs: Coefs,
    /// Its own carrier: each saw's phase, and the noise's state.
    phases: [f32; 4 * SAWS],
    noise: u32,
    sibilance: Smooth,
    mix: Smooth,
    gain: Smooth,
    /// This block's note events (frame, event), as fed, in order; and the
    /// notes it plays from them.
    notes: Vec<(usize, Event)>,
    voices: [NoteVoice; VOCODER_VOICES],
    struck: u64,
}

impl Vocoder {
    /// Allocates (its sidechain's room); off the audio thread.
    pub fn new(params: VocoderParams, rate: f32) -> Self {
        let mut phases = [0.0; 4 * SAWS];
        for (i, p) in phases.iter_mut().enumerate() {
            *p = (i as f32 * 0.618_034).fract();
        }
        let mut vocoder = Self {
            params,
            key: Key::new(),
            rate,
            made: (0, 0.0, 0.0, f32::NAN),
            bands: 0,
            analysis: [Coefs::default(); MAX],
            synthesis: [Coefs::default(); MAX],
            voice: [[Svf::default(); 2]; MAX],
            carrier: [[Svf::default(); 2]; MAX],
            voice_level: [0.0; MAX],
            carrier_level: [0.0; MAX],
            total: 0.0,
            highs: [Svf::default(); 2],
            highs_coefs: Coefs::new(SIBILANCE_HZ, std::f32::consts::FRAC_1_SQRT_2, rate),
            phases,
            noise: 0x1234_5678,
            sibilance: Smooth::new(params.sibilance, 10.0, rate),
            mix: Smooth::new(params.mix, 10.0, rate),
            gain: Smooth::new(db_to_gain(params.output_db), 10.0, rate),
            notes: Vec::with_capacity(MAX_EVENTS),
            voices: [NoteVoice::default(); VOCODER_VOICES],
            struck: 0,
        };
        for (v, voice) in vocoder.voices.iter_mut().enumerate() {
            for (s, p) in voice.phases.iter_mut().enumerate() {
                *p = ((v * SAWS + s) as f32 * 0.618_034).fract();
            }
        }
        vocoder.make_filters();
        vocoder
    }

    /// Forgets what it heard, without allocating.
    pub fn reset(&mut self) {
        self.voice = [[Svf::default(); 2]; MAX];
        self.carrier = [[Svf::default(); 2]; MAX];
        self.voice_level = [0.0; MAX];
        self.carrier_level = [0.0; MAX];
        self.total = 0.0;
        self.highs = [Svf::default(); 2];
        self.notes.clear();
        self.silence_notes();
    }

    /// Its notes stop at once (they no longer play).
    fn silence_notes(&mut self) {
        for voice in &mut self.voices {
            voice.on = false;
            voice.level = 0.0;
        }
    }

    /// This block's note events (frame, event), in order: with `midi`, its own
    /// carrier plays them. Never allocates (what doesn't fit is left out).
    pub(crate) fn feed_notes(&mut self, notes: &[(usize, u8, Event)]) {
        self.notes.clear();
        let room = self.notes.capacity();
        self.notes.extend(notes.iter().filter(|(_, _, e)| !matches!(e, Event::Control { .. })).map(|&(frame, _, e)| (frame, e)).take(room));
    }

    /// A note event, now.
    fn note_event(&mut self, event: Event) {
        match event {
            Event::NoteOn { pitch, gate, .. } => {
                // The same key again, else a quiet voice, else the one let go
                // that is quietest, else the oldest: it goes on from its level.
                let voices = &self.voices;
                let v = voices
                    .iter()
                    .position(|v| v.pitch == pitch && (v.on || v.level > 0.0))
                    .or_else(|| voices.iter().position(|v| !v.on && v.level == 0.0))
                    .or_else(|| (0..VOCODER_VOICES).filter(|&v| !voices[v].on).min_by(|&a, &b| voices[a].level.total_cmp(&voices[b].level)))
                    .unwrap_or_else(|| (0..VOCODER_VOICES).min_by_key(|&v| voices[v].struck).unwrap_or(0));
                self.struck += 1;
                let steps = self.note_steps(pitch.min(127));
                let voice = &mut self.voices[v];
                voice.pitch = pitch.min(127);
                voice.on = true;
                voice.held = gate == crate::synth::HELD;
                voice.gate = gate;
                voice.struck = self.struck;
                voice.steps = steps;
            }
            Event::NoteOff { pitch } => {
                for voice in self.voices.iter_mut().filter(|v| v.pitch == pitch) {
                    voice.on = false;
                }
            }
            Event::AllNotesOff => {
                for voice in &mut self.voices {
                    voice.on = false;
                }
            }
            Event::Control { .. } => {}
        }
    }

    /// Each saw's phase step for `pitch` (MIDI).
    fn note_steps(&self, pitch: u8) -> [f32; SAWS] {
        let saws = if self.params.carrier == VocoderCarrier::SuperSaw { SAWS } else { 1 };
        let detune = self.params.detune.clamp(0.0, 1.0);
        let mut steps = [0.0; SAWS];
        for (s, step) in steps.iter_mut().enumerate().take(saws) {
            let cents = if saws > 1 { (s as f32 / (saws - 1) as f32 * 2.0 - 1.0) * detune * SPREAD_CENTS } else { 0.0 };
            *step = (440.0 * 2f32.powf((pitch as f32 - 69.0 + cents / 100.0) / 12.0) / self.rate).min(0.5);
        }
        steps
    }

    /// The next sample of its own carrier playing MIDI notes, `saws` a note
    /// (`rise` and `fall`: the envelopes' steps), and the notes' level in all.
    /// The carrier keeps its level as notes come and go (the bands it is
    /// evened out by can't follow a note's attack): the vocoder's sound is
    /// faded by the notes' level instead, so they start and end smoothly.
    #[inline]
    fn played(&mut self, saws: usize, rise: f32, fall: f32) -> (f32, f32) {
        let noise = self.params.carrier == VocoderCarrier::Noise;
        let mut sum = 0.0;
        let mut power = 0.0;
        for voice in &mut self.voices {
            if voice.on && !voice.held {
                voice.gate = voice.gate.saturating_sub(1);
                if voice.gate == 0 {
                    voice.on = false;
                }
            }
            voice.level = if voice.on { (voice.level + rise).min(1.0) } else { (voice.level - fall).max(0.0) };
            if voice.level == 0.0 {
                continue;
            }
            power += voice.level * voice.level;
            if noise {
                continue;
            }
            let mut saws_sum = 0.0;
            for s in 0..saws {
                let dt = voice.steps[s];
                let phase = &mut voice.phases[s];
                saws_sum += saw(*phase, dt);
                *phase += dt;
                if *phase >= 1.0 {
                    *phase -= 1.0;
                }
            }
            sum += saws_sum * voice.level;
        }
        // Noise doesn't depend on the notes: it keeps running between them, so
        // the bands that even it out are settled when a note starts (no burst).
        if power == 0.0 {
            return (if noise { self.white() } else { 0.0 }, 0.0);
        }
        let level = power.sqrt();
        let carrier = if noise { self.white() } else { sum * NOTE_LEVEL / ((saws as f32).sqrt() * level) };
        (carrier, level.min(1.0))
    }

    /// White noise, −0.5..0.5.
    #[inline]
    fn white(&mut self) -> f32 {
        let mut x = self.noise;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.noise = x;
        (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.5
    }

    /// How many bands its settings ask for.
    fn band_count(p: &VocoderParams) -> usize {
        (p.bands.round() as usize).clamp(8, MAX)
    }

    /// The centre of band `b` of `n`, Hz (the voice's).
    pub fn centre(p: &VocoderParams, b: usize, n: usize) -> f32 {
        let (low, high) = Self::range(p);
        low * (high / low).powf(b as f32 / (n - 1).max(1) as f32)
    }

    fn range(p: &VocoderParams) -> (f32, f32) {
        let low = p.low_hz.clamp(20.0, 2000.0);
        (low, p.high_hz.clamp(low * 2.0, 20_000.0))
    }

    /// The two banks for the settings, if they changed (no allocation).
    fn make_filters(&mut self) {
        let p = self.params;
        let n = Self::band_count(&p);
        let (low, high) = Self::range(&p);
        let made = (n, low, high, p.formant);
        if made == self.made {
            return;
        }
        self.made = made;
        self.bands = n;
        // Each section a band wide: neighbouring bands cross about 6 dB down
        // (two in cascade narrow it). Narrow bands ring a little, which
        // spreads a saw's pulse: a low saw comes out a buzz, not a click train
        // (3 dB less crest than a band and a half wide, as clear).
        let spacing = (high / low).log2() / (n - 1) as f32;
        let width = spacing;
        let q = 1.0 / (2.0 * (std::f32::consts::LN_2 / 2.0 * width).sinh());
        let shift = 2f32.powf(p.formant.clamp(-24.0, 24.0) / 12.0);
        for b in 0..n {
            let centre = Self::centre(&p, b, n);
            self.analysis[b] = Coefs::new(centre, q, self.rate);
            self.synthesis[b] = Coefs::new(centre * shift, q, self.rate);
        }
    }

    /// The next sample of its own carrier.
    #[inline]
    /// `steps`: each saw's phase step, `saws` of them a note, `notes` notes.
    fn oscillator(&mut self, steps: &[f32; 4 * SAWS], saws: usize, notes: usize) -> f32 {
        match self.params.carrier {
            VocoderCarrier::Noise => self.white(),
            _ => {
                let mut sum = 0.0;
                for n in 0..notes {
                    for s in 0..saws {
                        let dt = steps[n * SAWS + s];
                        let phase = &mut self.phases[n * SAWS + s];
                        sum += saw(*phase, dt);
                        *phase += dt;
                        if *phase >= 1.0 {
                            *phase -= 1.0;
                        }
                    }
                }
                sum * 0.5 / ((notes * saws) as f32).sqrt()
            }
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.make_filters();
        let p = self.params;
        let n = self.bands;
        let rate = self.rate;
        let coef = |ms: f32| 1.0 - (-1.0 / (ms.max(0.05) * 0.001 * rate)).exp();
        let (attack, release, follow) = (coef(p.attack_ms), coef(p.release_ms), coef(CARRIER_MS));
        // Following MIDI, `sidechain` is whose notes it plays, not its carrier.
        let midi = p.midi;
        let keyed = p.sidechain.is_some() && !midi;
        let fed = self.key.take(l.len()) && keyed;
        if midi {
            // Detune may have moved.
            for v in 0..VOCODER_VOICES {
                if self.voices[v].on || self.voices[v].level > 0.0 {
                    self.voices[v].steps = self.note_steps(self.voices[v].pitch);
                }
            }
        } else if self.voices.iter().any(|v| v.on || v.level > 0.0) {
            self.silence_notes();
        }
        let (rise, fall) = (1.0 / (NOTE_ATTACK_MS * 0.001 * rate), 1.0 / (NOTE_RELEASE_MS * 0.001 * rate));
        let mut next_note = if midi { 0 } else { self.notes.len() };
        // Its own carrier's notes.
        let intervals = p.chord.intervals();
        let root = p.note.round().clamp(0.0, 127.0);
        let notes = intervals.len().min(4);
        let saws = if p.carrier == VocoderCarrier::SuperSaw { SAWS } else { 1 };
        let detune = p.detune.clamp(0.0, 1.0);
        let mut steps = [0.0; 4 * SAWS];
        for (n, i) in intervals.iter().take(notes).enumerate() {
            for s in 0..saws {
                let cents = if saws > 1 { (s as f32 / (saws - 1) as f32 * 2.0 - 1.0) * detune * SPREAD_CENTS } else { 0.0 };
                let freq = 440.0 * 2f32.powf((root + *i as f32 - 69.0 + cents / 100.0) / 12.0);
                steps[n * SAWS + s] = (freq / rate).min(0.5);
            }
        }
        let gain_target = db_to_gain(p.output_db);
        let last = l.len().saturating_sub(1);
        for i in 0..l.len() {
            // The notes that start or end on this frame (the last one takes
            // any later: a block the engine feeds has none).
            while let Some(&(frame, event)) = self.notes.get(next_note) {
                if frame.min(last) > i {
                    break;
                }
                self.note_event(event);
                next_note += 1;
            }
            let (dry_l, dry_r) = (l[i], r[i]);
            let voice = 0.5 * (dry_l + dry_r);
            let (carrier, played) = if keyed {
                (if fed { self.key.mono[i] } else { 0.0 }, 1.0)
            } else if midi {
                self.played(saws, rise, fall)
            } else {
                (self.oscillator(&steps, saws, notes), 1.0)
            };
            self.total += (carrier * carrier - self.total) * follow;
            let floor = (self.total * CARRIER_FLOOR).max(1e-12);
            let mut wet = 0.0;
            for b in 0..n {
                let [v0, v1] = &mut self.voice[b];
                let vb = v1.tick(v0.tick(voice, &self.analysis[b]).0, &self.analysis[b]).0;
                let [c0, c1] = &mut self.carrier[b];
                let cb = c1.tick(c0.tick(carrier, &self.synthesis[b]).0, &self.synthesis[b]).0;
                let level = &mut self.voice_level[b];
                let v2 = vb * vb;
                *level += (v2 - *level) * if v2 > *level { attack } else { release };
                if *level < 1e-12 {
                    *level = 0.0;
                }
                let heard = &mut self.carrier_level[b];
                *heard += (cb * cb - *heard) * follow;
                let ceiling = BAND_CEILING * level.sqrt();
                wet += (cb * (*level / heard.max(floor)).sqrt()).clamp(-ceiling, ceiling);
            }
            // The carrier's own level gates it: quiet carrier, quiet vocoder.
            wet *= MAKEUP * (self.total.sqrt() / CARRIER_REF).min(1.0) * played;
            let [h0, h1] = &mut self.highs;
            let highs = h1.tick(h0.tick(voice, &self.highs_coefs).1, &self.highs_coefs).1;
            h0.settle();
            h1.settle();
            // Following notes, its esses come with them: silence without.
            wet += highs * self.sibilance.next(p.sibilance.clamp(0.0, 1.0)) * played;
            if self.total < 1e-24 {
                self.total = 0.0;
            }
            let mix = self.mix.next(p.mix.clamp(0.0, 1.0));
            let gain = self.gain.next(gain_target);
            l[i] = (dry_l * (1.0 - mix) + wet * mix) * gain;
            r[i] = (dry_r * (1.0 - mix) + wet * mix) * gain;
        }
        // Fed again for the next block.
        self.notes.clear();
    }
}

/// A band-limited saw (PolyBLEP), −1..1, at `phase` (0..1) moving `dt` a sample.
#[inline]
fn saw(phase: f32, dt: f32) -> f32 {
    let mut v = 2.0 * phase - 1.0;
    if phase < dt {
        let t = phase / dt;
        v -= t + t - t * t - 1.0;
    } else if phase > 1.0 - dt {
        let t = (phase - 1.0) / dt;
        v -= t * t + t + t + 1.0;
    }
    v
}

// ----- VoiceShift ------------------------------------------------------------------------

/// The voices it follows (Hz): its longest period sets the latency.
const SHIFT_FMIN: f32 = 70.0;
const SHIFT_FMAX: f32 = 1_000.0;
/// How often it listens (seconds), and below what level (dBFS RMS) there is no voice.
const SHIFT_HOP: f32 = 0.005;
const SHIFT_GATE_DB: f32 = -55.0;
/// The grains' spacing where there is no voice (seconds).
const UNVOICED: f32 = 0.005;
/// How far (periods) a grain's mark may move to sit on the voice's pulse.
const EPOCH: f32 = 0.25;
/// The input and output lines (samples, powers of two).
const IN_LINE: usize = 1 << 13;
const OUT_LINE: usize = 1 << 13;

pub struct VoiceShift {
    pub(crate) params: VoiceShiftParams,
    rate: f32,
    detector: Detector,
    /// The newest input (mono), as a ring, and it in order for the detector.
    heard: Vec<f32>,
    frame: Vec<f32>,
    at: usize,
    filled: usize,
    since: usize,
    hop: usize,
    /// Whether there is a voice, and its period (samples).
    voiced: bool,
    period: f32,
    /// The longest period it follows (samples), and how late it sounds.
    longest: f32,
    latency: usize,
    /// The input (per channel) and where the grains are added up.
    line: [Vec<f32>; 2],
    sum: [Vec<f32>; 2],
    /// Samples since it started (where the input is written and the output read).
    now: u64,
    /// Where the next grain is centred (output) and the last grain's mark (input).
    next: f64,
    mark: f64,
    /// 0 when it moves nothing (the input, only later), 1 when it moves.
    shifted: Smooth,
    mix: Smooth,
    gain: Smooth,
}

impl VoiceShift {
    /// Allocates; off the audio thread.
    pub fn new(params: VoiceShiftParams, rate: f32) -> Self {
        let detector = Detector::with_range(rate, SHIFT_FMIN, SHIFT_FMAX);
        let len = detector.len;
        let longest = (rate / SHIFT_FMIN).ceil();
        let mut shift = Self {
            params,
            rate,
            detector,
            heard: vec![0.0; len],
            frame: vec![0.0; len],
            at: 0,
            filled: 0,
            since: 0,
            hop: ((SHIFT_HOP * rate) as usize).max(1),
            voiced: false,
            period: UNVOICED * rate,
            longest,
            latency: ((2.0 + EPOCH) * longest).ceil() as usize + 4,
            line: [vec![0.0; IN_LINE], vec![0.0; IN_LINE]],
            sum: [vec![0.0; OUT_LINE], vec![0.0; OUT_LINE]],
            now: 0,
            next: 0.0,
            mark: 0.0,
            shifted: Smooth::new(if Self::moves(&params) { 1.0 } else { 0.0 }, 10.0, rate),
            mix: Smooth::new(params.mix, 10.0, rate),
            gain: Smooth::new(db_to_gain(params.output_db), 10.0, rate),
        };
        shift.reset();
        shift
    }

    /// Forgets what it heard, without allocating.
    pub fn reset(&mut self) {
        self.heard.fill(0.0);
        self.at = 0;
        self.filled = 0;
        self.since = 0;
        self.voiced = false;
        self.period = UNVOICED * self.rate;
        self.line.iter_mut().for_each(|l| l.fill(0.0));
        self.sum.iter_mut().for_each(|l| l.fill(0.0));
        self.now = IN_LINE as u64;
        self.next = self.now as f64 + self.longest as f64;
        self.mark = self.next - self.latency as f64;
    }

    /// How late it sounds (samples), whatever it is set to: some two and a
    /// quarter of the longest period it follows (it needs a grain's input,
    /// a period on from its pulse, before it lays it out, a period ahead of
    /// where it sounds).
    pub fn latency(&self) -> usize {
        self.latency
    }

    fn moves(p: &VoiceShiftParams) -> bool {
        p.pitch != 0.0 || p.fine != 0.0 || p.formant != 0.0
    }

    /// Hears the newest input: whether there is a voice, and its period.
    fn listen(&mut self) {
        let len = self.heard.len();
        for i in 0..len {
            self.frame[i] = self.heard[(self.at + i) % len];
        }
        let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / len as f32).sqrt();
        let period = if self.filled >= len && 20.0 * rms.max(1e-9).log10() > SHIFT_GATE_DB { self.detector.period(&self.frame) } else { None };
        match period.filter(|p| *p <= self.longest && *p >= 2.0) {
            Some(p) => {
                self.voiced = true;
                self.period = p;
            }
            None => {
                self.voiced = false;
                self.period = UNVOICED * self.rate;
            }
        }
    }

    /// Channel `c` of the input at `at` (samples since it started), between samples.
    #[inline]
    fn sample(&self, c: usize, at: f64) -> f32 {
        let i = at.floor();
        let t = (at - i) as f32;
        let i = i as i64;
        let line = &self.line[c];
        let x = |k: i64| line[(k as usize) & (IN_LINE - 1)];
        let (y0, y1, y2, y3) = (x(i - 1), x(i), x(i + 1), x(i + 2));
        // Cubic Hermite.
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    /// Where the voice's pulse is (its loudest sample) within `radius` of
    /// `guess`: the grains centred on the pulses keep all of each.
    fn epoch(&self, guess: f64, radius: f32) -> f64 {
        let (from, to) = ((guess - radius as f64).ceil() as i64, (guess + radius as f64).floor() as i64);
        let mono = |k: i64| {
            let i = (k as usize) & (IN_LINE - 1);
            (self.line[0][i] + self.line[1][i]).abs()
        };
        let mut best = (guess, -1.0);
        for k in from..=to {
            let level = mono(k);
            if level > best.1 {
                best = (k as f64, level);
            }
        }
        best.0
    }

    /// Lays out the grain centred at `at` (output), moved `ratio` in pitch
    /// and `formant` in formants.
    fn grain(&mut self, at: f64, ratio: f32, formant: f32) {
        let period = self.period;
        // The mark on the voice for it: a whole number of periods along the
        // last one, so every grain is in phase with the one before.
        let target = at - self.latency as f64;
        if (target - self.mark).abs() > 2.0 * self.longest as f64 {
            self.mark = target;
        }
        while self.mark + period as f64 <= target {
            let guess = self.mark + period as f64;
            self.mark = if self.voiced { self.epoch(guess, EPOCH * period) } else { guess };
        }
        let half = period / formant.max(1.0);
        let spacing = if self.voiced { period / ratio } else { period };
        // Power kept as the grains thin out or crowd together.
        let level = (spacing / half).sqrt();
        let from = (at - half as f64).ceil() as i64;
        let to = (at + half as f64).floor() as i64;
        for j in from..=to {
            let k = (j as f64 - at) as f32;
            let w = 0.5 * (1.0 + (PI * k / half).cos()) * level;
            let read = self.mark + (k * formant) as f64;
            let o = (j as usize) & (OUT_LINE - 1);
            let (a, b) = (self.sample(0, read), self.sample(1, read));
            self.sum[0][o] += a * w;
            self.sum[1][o] += b * w;
        }
        self.next = at + spacing.max(1.0) as f64;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let p = self.params;
        let ratio = 2f32.powf((p.pitch.clamp(-24.0, 24.0) + p.fine.clamp(-100.0, 100.0) / 100.0) / 12.0);
        let formant = 2f32.powf(p.formant.clamp(-12.0, 12.0) / 12.0);
        let moves = if Self::moves(&p) { 1.0 } else { 0.0 };
        let (mix_target, gain_target) = (p.mix.clamp(0.0, 1.0), db_to_gain(p.output_db));
        let len = self.heard.len();
        for (left, right) in l.iter_mut().zip(r.iter_mut()) {
            let w = (self.now as usize) & (IN_LINE - 1);
            self.line[0][w] = *left;
            self.line[1][w] = *right;
            self.heard[self.at] = 0.5 * (*left + *right);
            self.at = (self.at + 1) % len;
            self.filled = (self.filled + 1).min(len);
            self.since += 1;
            if self.since >= self.hop {
                self.since = 0;
                self.listen();
            }
            // The grains it needs from here on, a longest period ahead.
            while self.next <= self.now as f64 + self.longest as f64 {
                self.grain(self.next, ratio, formant);
            }
            let o = (self.now as usize) & (OUT_LINE - 1);
            let (a, b) = (self.sum[0][o], self.sum[1][o]);
            self.sum[0][o] = 0.0;
            self.sum[1][o] = 0.0;
            let d = (self.now as usize - self.latency) & (IN_LINE - 1);
            let (dry_l, dry_r) = (self.line[0][d], self.line[1][d]);
            let shifted = self.shifted.next(moves);
            let (wet_l, wet_r) = (dry_l + shifted * (a - dry_l), dry_r + shifted * (b - dry_r));
            let mix = self.mix.next(mix_target);
            let gain = self.gain.next(gain_target);
            *left = (dry_l + mix * (wet_l - dry_l)) * gain;
            *right = (dry_r + mix * (wet_r - dry_r)) * gain;
            self.now += 1;
        }
    }
}

#[cfg(test)]
mod tests;
