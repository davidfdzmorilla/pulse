//! Pulse Keys: a piano, two electric pianos and a tonewheel organ, all synthesized.
//!
//! Every voice is a bank of decaying sine partials (complex rotators, cheap and
//! exactly tuned), shaped per model:
//! - Piano: inharmonic, stretch-tuned partials from a hammer that brightens with
//!   velocity, two strings a note (beating and the fast-then-slow decay), dampers that
//!   kill the high partials first, no dampers on the top keys, a soundboard.
//! - Tines and reeds: the vibrating tine or reed through a non-linear pickup, so hard
//!   notes growl ("bark"); a short bell partial on the attack; tremolo.
//! - Organ: nine drawbars of folded-back tonewheels, single-triggered percussion, key
//!   click and a rotary speaker whose horn and drum speed up and slow down.
//!
//! Fixed-size and allocation-free while playing (the delay lines are made once).

use crate::instruments::{svf_g, Gate, Svf};
use crate::model::{KeysModel, KeysParams};
use crate::synth::midi_to_freq;
use std::f32::consts::{PI, TAU};

const VOICES: usize = 32;
/// Rotators a voice can hold (the piano's two strings × 40 partials).
const OSC: usize = 80;
const PARTIALS: usize = 40;
const LANES: usize = 8;
const CHUNK: usize = 32;
/// Footage of each drawbar, as a multiple of the note.
const DRAWBARS: [f32; 9] = [0.5, 1.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0];
/// The organ's lowest and highest tonewheels: footages beyond them fold back an octave.
const WHEEL_LOW: f32 = 32.7;
const WHEEL_HIGH: f32 = 5920.0;
/// The piano's undamped top keys (from F#6 up) ring on after they are let go.
const UNDAMPED: u8 = 90;
/// Seconds an organ note takes to stop once its key is up.
const ORGAN_RELEASE: f32 = 0.05;
/// ln(1000): turns "seconds to fall 60 dB" into a decay rate.
const SIXTY_DB: f32 = 6.9078;

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// 0..1 from a hash.
fn unit(x: u32) -> f32 {
    (hash(x) >> 8) as f32 / (1u32 << 24) as f32
}

/// The piano's inharmonicity coefficient: stiff, short treble strings stretch their
/// overtones the most; the wound bass strings a little.
pub(crate) fn inharmonicity(pitch: u8) -> f32 {
    let p = pitch as f32;
    if p >= 48.0 {
        1.2e-4 * 10f32.powf(1.9 * (p - 48.0) / 60.0)
    } else {
        1.2e-4 * (1.0 + 1.2 * (48.0 - p) / 27.0)
    }
}

/// Partial `n` (1 = the fundamental) of a string with inharmonicity `b`, as a multiple
/// of the fundamental.
pub(crate) fn partial(n: f32, b: f32) -> f32 {
    n * ((1.0 + b * n * n) / (1.0 + b)).sqrt()
}

/// Stretch tuning, in cents: pianos are tuned a little flat in the bass and sharp in
/// the treble so their stretched overtones agree.
fn stretch(pitch: u8) -> f32 {
    let x = (pitch as f32 - 64.0) / 44.0;
    x * x * x * 22.0 + x * 4.0
}

fn cents(c: f32) -> f32 {
    2f32.powf(c / 1200.0)
}

/// Drawbar position (0..8) to amplitude: each step is 3 dB, 0 is silent.
fn drawbar(level: f32) -> f32 {
    if level <= 0.01 {
        0.0
    } else {
        10f32.powf(-3.0 * (8.0 - level) / 20.0) * level.min(1.0)
    }
}

/// An organ footage on the tonewheels: below the lowest wheel or above the highest it
/// folds back an octave, as on the real organ.
fn fold(freq: f32) -> f32 {
    let mut f = freq;
    while f > WHEEL_HIGH {
        f *= 0.5;
    }
    while f < WHEEL_LOW {
        f *= 2.0;
    }
    f
}

#[derive(Clone, Copy)]
struct Voice {
    gate: Gate,
    active: bool,
    released: bool,
    model: KeysModel,
    /// Rotators in use, a multiple of [`LANES`] (the rest are silent).
    count: usize,
    re: [f32; OSC],
    im: [f32; OSC],
    cr: [f32; OSC],
    ci: [f32; OSC],
    /// Each rotator's frequency over the note's.
    ratio: [f32; OSC],
    /// Each rotator's decay, per second.
    rate: [f32; OSC],
    /// The note's frequency, unbent.
    freq: f32,
    /// The bend its rotators are tuned to.
    bend: f32,
    /// Loudness (sum of squared amplitudes), for stealing and ending.
    level: f32,
    /// Loudness from velocity.
    gain: f32,
    seed: u32,
    noise: f32,
    noise_decay: f32,
    noise_filter: Svf,
    noise_g: f32,
    /// Pickup: drive and offset of the tine or reed from its centre.
    drive: f32,
    offset: f32,
    tone: Svf,
    tone_g: f32,
    low_cut: Svf,
    /// The organ's attack ramp.
    onset: f32,
    onset_step: f32,
    pan: (f32, f32),
}

impl Voice {
    const IDLE: Voice = Voice {
        gate: Gate::OFF,
        active: false,
        released: false,
        model: KeysModel::Piano,
        count: 0,
        re: [0.0; OSC],
        im: [0.0; OSC],
        cr: [0.0; OSC],
        ci: [0.0; OSC],
        ratio: [0.0; OSC],
        rate: [0.0; OSC],
        freq: 0.0,
        bend: 1.0,
        level: 0.0,
        gain: 0.0,
        seed: 1,
        noise: 0.0,
        noise_decay: 0.0,
        noise_filter: Svf { ic1: 0.0, ic2: 0.0 },
        noise_g: 0.0,
        drive: 1.0,
        offset: 0.0,
        tone: Svf { ic1: 0.0, ic2: 0.0 },
        tone_g: 0.0,
        low_cut: Svf { ic1: 0.0, ic2: 0.0 },
        onset: 1.0,
        onset_step: 1.0,
        pan: (0.707, 0.707),
    };

    /// Adds a rotator starting at amplitude `amp` and phase `phase` (turns).
    fn push(&mut self, ratio: f32, amp: f32, rate: f32, phase: f32) {
        if self.count >= OSC {
            return;
        }
        let k = self.count;
        let (s, c) = (TAU * phase).sin_cos();
        self.re[k] = amp * c;
        self.im[k] = amp * s;
        self.ratio[k] = ratio;
        self.rate[k] = rate;
        self.count += 1;
    }

    /// Pads the rotators to whole lanes and clears the unused ones.
    fn seal(&mut self) {
        let used = self.count;
        self.count = used.div_ceil(LANES) * LANES;
        for k in used..OSC {
            self.re[k] = 0.0;
            self.im[k] = 0.0;
            self.cr[k] = 0.0;
            self.ci[k] = 0.0;
            self.ratio[k] = 0.0;
            self.rate[k] = 0.0;
        }
    }

    /// Tunes the rotators for `bend`; any that would pass Nyquist fall silent.
    fn tune(&mut self, bend: f32, sr: f32) {
        self.bend = bend;
        for k in 0..self.count {
            let w = TAU * self.freq * self.ratio[k] * bend / sr;
            let r = if w < PI * 0.95 && self.ratio[k] > 0.0 { (-self.rate[k] / sr).exp() } else { 0.0 };
            let (s, c) = w.sin_cos();
            self.cr[k] = r * c;
            self.ci[k] = r * s;
        }
    }

    fn white(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    /// Lets go: the dampers (or the organ's contacts) stop the note. High partials of
    /// pianos and tines die first; the piano's top keys have no dampers.
    fn release(&mut self, p: &KeysParams, sr: f32) {
        if self.released || !self.active {
            return;
        }
        self.released = true;
        if self.model == KeysModel::Piano && self.gate.pitch >= UNDAMPED {
            return;
        }
        let damp = SIXTY_DB / if self.model == KeysModel::Organ { ORGAN_RELEASE } else { p.release.max(0.01) };
        let slope = if self.model == KeysModel::Organ { 0.0 } else { 0.12 };
        for k in 0..self.count {
            self.rate[k] += damp * (1.0 + slope * (self.ratio[k] - 1.0).max(0.0));
        }
        if self.model == KeysModel::Organ {
            // The contacts click again as the key comes up.
            self.noise = self.noise.max(p.click * 0.06);
        }
        self.tune(self.bend, sr);
    }

    /// Fades out within a few milliseconds (the same key struck again).
    fn choke(&mut self, sr: f32) {
        for k in 0..self.count {
            self.rate[k] += SIXTY_DB / 0.04;
        }
        self.released = true;
        self.tune(self.bend, sr);
    }

    fn measure(&mut self) {
        let mut sum = 0.0;
        for k in 0..self.count {
            sum += self.re[k] * self.re[k] + self.im[k] * self.im[k];
        }
        self.level = sum;
    }
}

/// The organ's rotary speaker: a horn for the highs and a drum for the lows, turning at
/// a speed that takes a moment to change, heard by two microphones.
struct Rotary {
    split: Svf,
    horn: Vec<f32>,
    drum: Vec<f32>,
    write: usize,
    horn_angle: f32,
    drum_angle: f32,
    horn_hz: f32,
    drum_hz: f32,
}

impl Rotary {
    fn new(sr: f32) -> Self {
        let len = ((0.004 * sr) as usize + 8).next_power_of_two();
        Self { split: Svf::default(), horn: vec![0.0; len], drum: vec![0.0; len], write: 0, horn_angle: 0.0, drum_angle: 0.3, horn_hz: 0.8, drum_hz: 0.67 }
    }

    /// Moves the rotors towards `fast` (0 slow .. 1 fast) over `frames` frames.
    fn spin(&mut self, fast: f32, frames: usize, sr: f32) {
        let horn = 0.8 + (6.8 - 0.8) * fast;
        let drum = 0.67 + (5.9 - 0.67) * fast;
        let t = frames as f32 / sr;
        // The light horn gets there in about a second, the heavy drum in four.
        self.horn_hz += (horn - self.horn_hz) * (1.0 - (-t / 0.45).exp());
        self.drum_hz += (drum - self.drum_hz) * (1.0 - (-t / 1.8).exp());
    }

    fn read(buf: &[f32], write: usize, delay: f32) -> f32 {
        let mask = buf.len() - 1;
        let pos = write as f32 - delay;
        let i = pos.floor();
        let frac = pos - i;
        let a = buf[(i as isize).rem_euclid(buf.len() as isize) as usize & mask];
        let b = buf[((i as isize) + 1).rem_euclid(buf.len() as isize) as usize & mask];
        a + (b - a) * frac
    }

    fn tick(&mut self, x: f32, sr: f32) -> (f32, f32) {
        let (low, _, _) = self.split.tick(x, svf_g(800.0, sr), 1.414);
        let high = x - low;
        let mask = self.horn.len() - 1;
        self.horn[self.write] = high;
        self.drum[self.write] = low;
        self.horn_angle = (self.horn_angle + self.horn_hz / sr).fract();
        self.drum_angle = (self.drum_angle + self.drum_hz / sr).fract();
        let ms = sr / 1000.0;
        let mut out = [0.0; 2];
        for (mic, o) in out.iter_mut().enumerate() {
            let quarter = mic as f32 * 0.25;
            let (hs, hc) = (TAU * (self.horn_angle + quarter)).sin_cos();
            let (ds, dc) = (TAU * (self.drum_angle + quarter)).sin_cos();
            let h = Self::read(&self.horn, self.write, (0.45 + 0.35 * hs) * ms) * (0.62 + 0.38 * hc);
            let d = Self::read(&self.drum, self.write, (0.25 + 0.12 * ds) * ms) * (0.85 + 0.15 * dc);
            *o = h + d;
        }
        self.write = (self.write + 1) & mask;
        (out[0], out[1])
    }
}

/// A stereo pair of short delays: the piano's lid and room.
struct Reflections {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    taps: (usize, usize),
}

impl Reflections {
    fn new(sr: f32) -> Self {
        let len = ((0.016 * sr) as usize + 4).next_power_of_two();
        Self { left: vec![0.0; len], right: vec![0.0; len], write: 0, taps: ((0.0071 * sr) as usize, (0.0097 * sr) as usize) }
    }

    fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mask = self.left.len() - 1;
        self.left[self.write] = l;
        self.right[self.write] = r;
        let from_right = self.right[(self.write + self.left.len() - self.taps.0) & mask];
        let from_left = self.left[(self.write + self.left.len() - self.taps.1) & mask];
        self.write = (self.write + 1) & mask;
        (from_right, from_left)
    }
}

/// The soundboard's strongest resonances: (Hz, level).
const BOARD: [(f32, f32); 5] = [(98.0, 0.5), (196.0, 0.45), (410.0, 0.35), (860.0, 0.25), (1700.0, 0.15)];

pub struct Keys {
    params: KeysParams,
    voices: [Voice; VOICES],
    sample_rate: f32,
    counter: u64,
    /// Pitch bend and vibrato, as a frequency ratio.
    pitch: f32,
    board: [[Svf; 5]; 2],
    board_g: [f32; 5],
    reflections: Reflections,
    rotary: Rotary,
    tremolo: f32,
    tone: [Svf; 2],
    dc: [(f32, f32); 2],
    /// Frames into the current chunk.
    offset: usize,
}

impl Keys {
    pub fn new(params: KeysParams, sample_rate: f32) -> Self {
        Self {
            params,
            voices: [Voice::IDLE; VOICES],
            sample_rate,
            counter: 0,
            pitch: 1.0,
            board: [[Svf::default(); 5]; 2],
            board_g: BOARD.map(|(f, _)| svf_g(f, sample_rate)),
            reflections: Reflections::new(sample_rate),
            rotary: Rotary::new(sample_rate),
            tremolo: 0.0,
            tone: [Svf::default(); 2],
            dc: [(0.0, 0.0); 2],
            offset: 0,
        }
    }

    pub fn params(&self) -> KeysParams {
        self.params
    }

    pub fn set_params(&mut self, params: KeysParams) {
        self.params = params;
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: f32, gate: u32) {
        let (p, sr) = (self.params, self.sample_rate);
        self.counter += 1;
        // The same key again: the old note stops as the new one starts.
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.pitch == pitch) {
            v.choke(sr);
        }
        // Percussion only on a note played with no other key down (single trigger).
        let legato = self.voices.iter().any(|v| v.active && !v.released && v.model == KeysModel::Organ);
        let index = self.free_voice();
        let mut v = Voice::IDLE;
        v.gate = Gate::new(pitch, velocity.clamp(0.0, 1.0), gate, self.counter);
        v.active = true;
        v.model = p.model;
        v.seed = hash(pitch as u32 * 7919 + (velocity * 127.0) as u32 + 1) | 1;
        let vel = v.gate.velocity;
        v.gain = if p.model == KeysModel::Organ { 1.0 } else { (1.0 - p.dynamics) + p.dynamics * vel * vel };
        match p.model {
            KeysModel::Piano => start_piano(&mut v, &p, pitch, vel, sr),
            KeysModel::Tines | KeysModel::Reed => start_electric(&mut v, &p, pitch, vel, sr),
            KeysModel::Organ => start_organ(&mut v, &p, pitch, !legato, sr),
        }
        v.seal();
        v.tune(self.pitch, sr);
        v.measure();
        self.voices[index] = v;
    }

    /// An idle voice, else the quietest released one, else the quietest.
    fn free_voice(&self) -> usize {
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        let quietest = |released: bool| {
            self.voices
                .iter()
                .enumerate()
                .filter(|(_, v)| v.released == released)
                .min_by(|a, b| a.1.level.total_cmp(&b.1.level))
                .map(|(i, _)| i)
        };
        quietest(true).or_else(|| quietest(false)).unwrap_or(0)
    }

    pub fn note_off(&mut self, pitch: u8) {
        let (p, sr) = (self.params, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.active && v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.release(&p, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (p, sr) = (self.params, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.gate.held = false;
            v.release(&p, sr);
        }
    }

    /// Adds the instrument's output to `left`/`right`.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        // Chunks keep to the same frames whatever the buffer size, so the sound does too.
        let mut done = 0;
        while done < left.len() {
            let n = (left.len() - done).min(CHUNK - self.offset);
            self.render_chunk(&mut left[done..done + n], &mut right[done..done + n]);
            self.offset = (self.offset + n) % CHUNK;
            done += n;
        }
    }

    /// Renders the next frames of the current chunk.
    fn render_chunk(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let (p, sr, bend) = (self.params, self.sample_rate, self.pitch);
        let (starts, ends) = (self.offset == 0, self.offset + n == CHUNK);
        let mut out_l = [0.0f32; CHUNK];
        let mut out_r = [0.0f32; CHUNK];
        let bars = p.drawbars.map(|d| drawbar(d) / 3.2);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            if (v.bend - bend).abs() > 1e-6 {
                v.tune(bend, sr);
            }
            // Drawbars move while notes sound; the held organ notes follow them.
            if starts && v.model == KeysModel::Organ && !v.released {
                hold_drawbars(v, &bars);
            }
            let mut mono = [0.0f32; CHUNK];
            for slot in mono.iter_mut().take(n) {
                if v.gate.tick() {
                    v.release(&p, sr);
                }
                *slot = rotate(v);
            }
            shape(v, &mut mono[..n], sr);
            for i in 0..n {
                out_l[i] += mono[i] * v.pan.0;
                out_r[i] += mono[i] * v.pan.1;
            }
            if ends {
                v.measure();
                let organ_held = v.model == KeysModel::Organ && !v.released;
                if v.level < 1e-10 && v.noise < 1e-5 && !organ_held {
                    v.active = false;
                }
            }
        }
        if starts && p.model == KeysModel::Organ {
            self.rotary.spin(p.leslie_fast, CHUNK, sr);
        }
        self.finish(&mut out_l[..n], &mut out_r[..n]);
        for i in 0..n {
            left[i] += out_l[i];
            right[i] += out_r[i];
        }
    }

    /// The instrument's own sound after the voices: soundboard, tremolo, rotary
    /// speaker, drive, tone, width and level.
    fn finish(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (p, sr) = (self.params, self.sample_rate);
        let n = left.len();
        let tone_cut = match p.model {
            KeysModel::Organ => 1500.0 * 12f32.powf(p.tone),
            _ => 20000.0,
        };
        let tone_g = svf_g(tone_cut, sr);
        let drive = 1.0 + 9.0 * p.drive * p.drive;
        let trem_step = p.tremolo_rate / sr;
        let width = p.width * 2.0;
        // Similar loudness from every model at the same Volumen.
        let model_level = match p.model {
            KeysModel::Piano => 1.28,
            KeysModel::Tines => 1.5,
            KeysModel::Reed => 1.15,
            KeysModel::Organ => 0.8,
        };
        let level = p.gain * 2.0;
        for i in 0..n {
            let (mut l, mut r) = (left[i] * model_level * 0.5, right[i] * model_level * 0.5);
            if p.model == KeysModel::Piano && p.body > 0.0 {
                let mut board = [0.0; 2];
                for (ch, x) in [l, r].into_iter().enumerate() {
                    for (k, &(_, level)) in BOARD.iter().enumerate() {
                        let (_, band, _) = self.board[ch][k].tick(x, self.board_g[k], 0.35);
                        board[ch] += band * 0.35 * level;
                    }
                }
                let (er_l, er_r) = self.reflections.tick(l, r);
                l += p.body * (board[0] + 0.22 * er_l);
                r += p.body * (board[1] + 0.22 * er_r);
            }
            if p.drive > 0.0 {
                l = (drive * l).tanh() / drive.sqrt();
                r = (drive * r).tanh() / drive.sqrt();
            }
            if matches!(p.model, KeysModel::Tines | KeysModel::Reed) && p.tremolo > 0.0 {
                self.tremolo = (self.tremolo + trem_step).fract();
                let lfo = (TAU * self.tremolo).sin();
                if p.model == KeysModel::Tines {
                    // The suitcase's tremolo pans side to side.
                    let (gl, gr) = (1.0 + p.tremolo * lfo, 1.0 - p.tremolo * lfo);
                    l *= gl;
                    r *= gr;
                } else {
                    let g = 1.0 - p.tremolo * 0.5 * (1.0 + lfo);
                    l *= g;
                    r *= g;
                }
            }
            if p.model == KeysModel::Organ {
                let (lo, _, _) = self.tone[0].tick(l, tone_g, 1.0);
                let (ro, _, _) = self.tone[1].tick(r, tone_g, 1.0);
                let mono = 0.5 * (lo + ro);
                let (wl, wr) = self.rotary.tick(mono, sr);
                l = lo + (wl - lo) * p.leslie;
                r = ro + (wr - ro) * p.leslie;
            }
            if p.model != KeysModel::Piano {
                let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * width);
                l = mid + side;
                r = mid - side;
            }
            // Pickups and drive lean to one side; take the offset out.
            for (ch, x) in [&mut l, &mut r].into_iter().enumerate() {
                let (prev_in, prev_out) = self.dc[ch];
                let y = *x - prev_in + 0.9995 * prev_out;
                self.dc[ch] = (*x, y);
                *x = y;
            }
            left[i] = soft_clip(l * level);
            right[i] = soft_clip(r * level);
        }
    }
}

/// Leaves everything below 0.8 alone and rounds louder peaks off below 1.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.8 {
        x
    } else {
        (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh()).copysign(x)
    }
}

/// One sample of a voice's rotators: advances them all and sums their sines.
#[inline]
fn rotate(v: &mut Voice) -> f32 {
    let count = v.count;
    let mut acc = [0.0f32; LANES];
    for (((re, im), cr), ci) in v.re[..count]
        .chunks_exact_mut(LANES)
        .zip(v.im[..count].chunks_exact_mut(LANES))
        .zip(v.cr[..count].chunks_exact(LANES))
        .zip(v.ci[..count].chunks_exact(LANES))
    {
        for l in 0..LANES {
            let (a, b) = (re[l], im[l]);
            re[l] = a * cr[l] - b * ci[l];
            im[l] = a * ci[l] + b * cr[l];
            acc[l] += im[l];
        }
    }
    acc.iter().sum()
}

/// Keeps a held organ note's drawbar rotators at the drawbars' levels (which also
/// stops rounding from slowly changing them).
fn hold_drawbars(v: &mut Voice, bars: &[f32; 9]) {
    for (k, &target) in bars.iter().enumerate() {
        let mag = (v.re[k] * v.re[k] + v.im[k] * v.im[k]).sqrt();
        let to = mag + (target - mag) * 0.25;
        if mag > 1e-7 {
            let g = to / mag;
            v.re[k] *= g;
            v.im[k] *= g;
        } else if target > 0.0 {
            let phase = unit(v.seed.wrapping_add(k as u32)) * TAU;
            v.re[k] = to * phase.cos();
            v.im[k] = to * phase.sin();
        }
    }
}

/// A voice's own sound around its rotators: the hammer, the pickup, its tone.
fn shape(v: &mut Voice, mono: &mut [f32], sr: f32) {
    match v.model {
        KeysModel::Piano => {
            for x in mono.iter_mut() {
                let noise = v.white() * v.noise;
                v.noise *= v.noise_decay;
                let (_, band, _) = v.noise_filter.tick(noise, v.noise_g, 1.2);
                *x = *x * v.gain + band;
            }
        }
        KeysModel::Tines | KeysModel::Reed => {
            let (k, o) = (v.drive, v.offset);
            let rest = (k * o).tanh();
            let norm = 1.0 / (k * (1.0 - rest * rest)).max(1e-3);
            let reed = v.model == KeysModel::Reed;
            let low_g = svf_g(180.0, sr);
            for x in mono.iter_mut() {
                let noise = v.white() * v.noise;
                v.noise *= v.noise_decay;
                let (_, thump, _) = v.noise_filter.tick(noise, v.noise_g, 1.5);
                // The pickup: the tine or reed swings across a field it does not sit
                // in the middle of, so big swings come out lopsided and growl.
                let swing = *x * v.gain * 0.6;
                let mut y = ((k * (swing + o)).tanh() - rest) * norm;
                if reed {
                    let (_, _, high) = v.low_cut.tick(y, low_g, 1.0);
                    y = high;
                }
                let (lp, _, _) = v.tone.tick(y, v.tone_g, 1.2);
                *x = lp + thump;
            }
        }
        KeysModel::Organ => {
            for x in mono.iter_mut() {
                let noise = v.white() * v.noise;
                v.noise *= v.noise_decay;
                let (_, click, _) = v.noise_filter.tick(noise, v.noise_g, 0.8);
                v.onset = (v.onset + v.onset_step).min(1.0);
                *x = *x * v.gain * v.onset + click;
            }
        }
    }
}

/// Stereo position by key, like sitting at the instrument.
fn key_pan(pitch: u8, width: f32) -> (f32, f32) {
    let x = ((pitch as f32 - 64.0) / 40.0).clamp(-1.0, 1.0) * width;
    let angle = (x + 1.0) * PI / 4.0;
    (angle.cos(), angle.sin())
}

fn start_piano(v: &mut Voice, p: &KeysParams, pitch: u8, vel: f32, sr: f32) {
    let f0 = midi_to_freq(pitch) * cents(stretch(pitch));
    v.freq = f0;
    let b = inharmonicity(pitch);
    let h = v.seed;
    // Where the hammer hits the string: about an eighth of the way along.
    let strike = 1.0 / (7.2 + unit(h) * 0.8);
    let hard = (vel * (0.35 + 1.1 * p.tone)).clamp(0.02, 1.45);
    let corner = (250.0 + 6500.0 * hard.powf(1.6)).max(f0 * 1.5);
    let length = p.length.max(0.05);
    let tau = 4.0 * 2f32.powf(-(pitch as f32 - 60.0) / 24.0) * length;
    let spread = cents(p.strings * 3.0 * (0.55 + 0.45 * unit(h ^ 0x51ed)) * if h & 2 == 0 { 1.0 } else { -1.0 });
    let mut amps = [0.0f32; PARTIALS];
    let mut ratios = [0.0f32; PARTIALS];
    let mut used = 0;
    for n in 1..=PARTIALS {
        let ratio = partial(n as f32, b);
        let f = f0 * ratio;
        if f > sr * 0.45 {
            break;
        }
        let comb = (n as f32 * PI * strike).sin().abs();
        let hammer = 1.0 / (1.0 + (f / corner).powi(4)).sqrt();
        // The soundboard hardly radiates the bass strings' fundamentals.
        let radiation = f * f / (f * f + 80.0 * 80.0);
        amps[n - 1] = comb * (n as f32).powf(-0.7) * hammer * radiation;
        ratios[n - 1] = ratio;
        used = n;
    }
    let peak = amps.iter().cloned().fold(0.0, f32::max).max(1e-9);
    let energy = amps.iter().map(|a| a * a).sum::<f32>().sqrt().max(1e-9);
    for n in 0..used {
        if amps[n] < peak * 0.002 {
            continue;
        }
        let a = amps[n] / energy * 0.5;
        let f = f0 * ratios[n];
        let rate = (1.0 / tau + 0.5 * (f / 1000.0).powi(2)) / length;
        // Two strings: one dies fast (the prompt sound), the other, a little detuned,
        // sings on (the aftersound) and beats against it.
        v.push(ratios[n], a * 0.72, rate * 3.0, 0.0);
        v.push(ratios[n] * spread, a * 0.28, rate, 0.0);
    }
    v.noise = p.hammer * 0.22 * vel.powf(1.5) * v.gain;
    v.noise_decay = (-1.0 / (0.004 * sr)).exp();
    v.noise_g = svf_g((f0 * 6.0).clamp(600.0, 5000.0), sr);
    v.pan = key_pan(pitch, p.width);
}

fn start_electric(v: &mut Voice, p: &KeysParams, pitch: u8, vel: f32, sr: f32) {
    let f0 = midi_to_freq(pitch);
    v.freq = f0;
    let reed = p.model == KeysModel::Reed;
    let length = p.length.max(0.05);
    let octaves = (pitch as f32 - 60.0) / 12.0;
    let tau = if reed { 1.5 } else { 2.4 } * 2f32.powf(-octaves * 0.4) * length;
    let bell_tau = (if reed { 0.06 } else { 0.22 }) * 2f32.powf(-octaves * 0.5);
    let bell = p.bell * (0.3 + 0.7 * vel);
    let partials: &[(f32, f32, f32)] = if reed {
        &[(1.0, 1.0, 1.0), (2.0, 0.18, 2.0), (3.0, 0.08, 3.0), (4.0, 0.03, 4.0)]
    } else {
        &[(1.0, 1.0, 1.0), (2.0, 0.08, 2.0), (3.0, 0.025, 3.0)]
    };
    for &(ratio, amp, speed) in partials {
        v.push(ratio, amp, speed / tau, 0.0);
    }
    if reed {
        v.push(5.4, bell * 0.35, 1.0 / bell_tau, 0.0);
    } else {
        // The tine's bell: an inharmonic overtone that rings out quickly.
        v.push(7.11, bell * 0.5, 1.0 / bell_tau, 0.0);
        v.push(17.3, bell * 0.12, 2.5 / bell_tau, 0.0);
    }
    v.drive = if reed { 1.0 + 4.0 * p.bark } else { 0.8 + 3.0 * p.bark };
    v.offset = if reed { 0.3 + 0.3 * p.bark } else { 0.15 + 0.25 * p.bark };
    let base = if reed { 700.0 + 6000.0 * p.tone * p.tone } else { 500.0 + 9000.0 * p.tone * p.tone };
    v.tone_g = svf_g(2.0 * f0 + base + 3000.0 * vel * p.dynamics, sr);
    v.noise = p.hammer * 0.06 * vel * v.gain;
    v.noise_decay = (-1.0 / (0.003 * sr)).exp();
    v.noise_g = svf_g(if reed { 1800.0 } else { 1200.0 }, sr);
    v.pan = (0.707, 0.707);
}

fn start_organ(v: &mut Voice, p: &KeysParams, pitch: u8, percussion: bool, sr: f32) {
    let f0 = midi_to_freq(pitch);
    v.freq = f0;
    for (k, &footage) in DRAWBARS.iter().enumerate() {
        let ratio = fold(f0 * footage) / f0;
        // Tonewheels turn all the time: each footage comes in at its own phase.
        v.push(ratio, drawbar(p.drawbars[k]) / 3.2, 0.0, unit(v.seed.wrapping_add(k as u32)));
    }
    if percussion && p.percussion > 0.0 {
        let ratio = fold(f0 * if p.perc_third { 3.0 } else { 2.0 }) / f0;
        v.push(ratio, p.percussion * 0.45, SIXTY_DB / p.perc_decay.max(0.05), 0.0);
    }
    v.onset = 0.0;
    v.onset_step = 1.0 / ((0.004 - 0.0035 * p.click) * sr).max(1.0);
    v.noise = p.click * 0.12;
    v.noise_decay = (-1.0 / (0.0025 * sr)).exp();
    v.noise_g = svf_g(2500.0, sr);
    v.pan = (0.707, 0.707);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::synth::HELD;

    const SR: f32 = 48000.0;

    fn render(keys: &mut Keys, seconds: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (seconds * SR) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            keys.render(cl, cr);
        }
        (l, r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// Magnitude of the signal's component at `freq` (a single DFT bin).
    fn at(x: &[f32], freq: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, s) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * freq as f64 * i as f64 / SR as f64;
            re += *s as f64 * w.cos();
            im += *s as f64 * w.sin();
        }
        ((re * re + im * im).sqrt() / x.len() as f64) as f32
    }

    /// The strongest frequency within a semitone of `around`, to a cent.
    fn peak(x: &[f32], around: f32) -> f32 {
        (-100..=100).map(|c| around * cents(c as f32)).max_by(|a, b| at(x, *a).total_cmp(&at(x, *b))).unwrap()
    }

    fn centroid(x: &[f32]) -> f32 {
        let bins: Vec<f32> = (1..120).map(|k| k as f32 * 50.0).collect();
        let mags: Vec<f32> = bins.iter().map(|&f| at(x, f)).collect();
        bins.iter().zip(&mags).map(|(f, m)| f * m).sum::<f32>() / mags.iter().sum::<f32>()
    }

    fn keys(model: KeysModel) -> Keys {
        Keys::new(KeysParams::model_default(model), SR)
    }

    #[test]
    fn every_model_plays_in_tune() {
        for model in KeysModel::ALL {
            let mut k = keys(model);
            k.note_on(57, 0.8, HELD);
            let (l, _) = render(&mut k, 0.5);
            assert!(rms(&l) > 0.01, "{model:?} sounds");
            let found = peak(&l[4800..], 220.0);
            let off = 1200.0 * (found / 220.0).log2();
            assert!(off.abs() <= 6.0, "{model:?} plays A3 at {found} Hz ({off} cents)");
        }
    }

    #[test]
    fn released_notes_fall_silent_and_free_their_voice() {
        for model in KeysModel::ALL {
            let mut k = keys(model);
            k.note_on(60, 0.9, HELD);
            render(&mut k, 0.3);
            k.note_off(60);
            render(&mut k, 1.5);
            let (l, _) = render(&mut k, 0.1);
            assert!(rms(&l) < 1e-4, "{model:?} is silent after its release");
            assert!(k.voices.iter().all(|v| !v.active), "{model:?} freed the voice");
        }
    }

    #[test]
    fn a_held_piano_note_dies_away_but_an_organ_note_does_not() {
        let mut piano = keys(KeysModel::Piano);
        piano.note_on(72, 0.8, HELD);
        let (early, _) = render(&mut piano, 0.2);
        render(&mut piano, 3.0);
        let (late, _) = render(&mut piano, 0.2);
        assert!(rms(&late) < rms(&early) * 0.5, "the piano decays while held");
        assert!(rms(&late) > rms(&early) * 0.02, "and still sings: the slow string");

        let mut organ = keys(KeysModel::Organ);
        organ.set_params(KeysParams { percussion: 0.0, leslie: 0.0, ..organ.params() });
        organ.note_on(72, 0.8, HELD);
        render(&mut organ, 0.2);
        let (early, _) = render(&mut organ, 0.2);
        render(&mut organ, 3.0);
        let (late, _) = render(&mut organ, 0.2);
        assert!((rms(&late) / rms(&early) - 1.0).abs() < 0.05, "the organ holds its level");
    }

    #[test]
    fn harder_piano_notes_are_brighter() {
        let brightness = |vel| {
            let mut k = keys(KeysModel::Piano);
            k.note_on(48, vel, HELD);
            let (l, _) = render(&mut k, 0.3);
            centroid(&l[1000..])
        };
        let (soft, hard) = (brightness(0.25), brightness(1.0));
        assert!(hard > soft * 1.3, "soft {soft} Hz, hard {hard} Hz");
    }

    #[test]
    fn piano_overtones_are_stretched() {
        // The 12th partial of a treble note sits clearly above 12 × the fundamental.
        let b = inharmonicity(84);
        let ratio = partial(12.0, b);
        assert!(ratio > 12.2 && ratio < 14.0, "{ratio}");
        assert!(inharmonicity(100) > inharmonicity(60) && inharmonicity(30) > inharmonicity(48));
        let mut k = keys(KeysModel::Piano);
        k.note_on(40, 1.0, HELD);
        let (l, _) = render(&mut k, 0.5);
        let f0 = midi_to_freq(40) * cents(stretch(40));
        let tenth = peak(&l, f0 * 10.0);
        assert!(tenth > f0 * 10.0 * cents(2.0), "the 10th partial is sharp: {tenth} vs {}", f0 * 10.0);
    }

    #[test]
    fn the_top_keys_have_no_dampers() {
        // Level 0.4 s after letting go, against the same note held.
        let after = |pitch, release: bool| {
            let mut k = keys(KeysModel::Piano);
            k.note_on(pitch, 0.8, HELD);
            render(&mut k, 0.1);
            if release {
                k.note_off(pitch);
            }
            render(&mut k, 0.4);
            rms(&render(&mut k, 0.05).0)
        };
        assert!(after(96, true) > after(96, false) * 0.95, "an undamped key rings on");
        assert!(after(72, true) < after(72, false) * 0.01, "a damped key stops");
    }

    #[test]
    fn hard_tines_bark() {
        // Second harmonic (from the pickup) relative to the fundamental.
        let bark = |vel| {
            let mut k = keys(KeysModel::Tines);
            k.set_params(KeysParams { bark: 0.8, bell: 0.0, ..k.params() });
            k.note_on(48, vel, HELD);
            let (l, _) = render(&mut k, 0.3);
            let f0 = midi_to_freq(48);
            at(&l[2000..], f0 * 2.0) / at(&l[2000..], f0)
        };
        assert!(bark(1.0) > bark(0.2) * 2.0, "soft {} hard {}", bark(0.2), bark(1.0));
    }

    #[test]
    fn drawbars_choose_the_footages() {
        let level = |bars: [f32; 9], freq: f32| {
            let mut k = keys(KeysModel::Organ);
            k.set_params(KeysParams { drawbars: bars, percussion: 0.0, leslie: 0.0, click: 0.0, ..k.params() });
            k.note_on(57, 1.0, HELD);
            let (l, _) = render(&mut k, 0.3);
            at(&l[4800..], freq)
        };
        let eight = [0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let sixteen = [8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert!(level(eight, 220.0) > level(eight, 110.0) * 20.0);
        assert!(level(sixteen, 110.0) > level(sixteen, 220.0) * 20.0);
        assert_eq!(fold(9000.0), 4500.0);
        assert_eq!(fold(20.0), 40.0);
    }

    #[test]
    fn percussion_only_on_the_first_key_down() {
        let mut k = keys(KeysModel::Organ);
        k.set_params(KeysParams { percussion: 1.0, ..k.params() });
        k.note_on(60, 1.0, HELD);
        let first = k.voices.iter().find(|v| v.active && v.gate.pitch == 60).unwrap();
        assert!(first.rate[9] > 0.0 && first.re[9].hypot(first.im[9]) > 0.1, "drawbars + a decaying percussion rotator");
        k.note_on(64, 1.0, HELD);
        let second = k.voices.iter().find(|v| v.active && v.gate.pitch == 64).unwrap();
        assert_eq!(second.count, 16, "padded to lanes");
        assert_eq!(second.rate[9], 0.0, "no percussion rotator on a legato note");
    }

    #[test]
    fn the_fast_rotor_wobbles_more() {
        let wobble = |fast: f32| {
            let mut k = keys(KeysModel::Organ);
            k.set_params(KeysParams { leslie: 1.0, leslie_fast: fast, percussion: 0.0, drawbars: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 8.0], ..k.params() });
            k.rotary.horn_hz = if fast > 0.5 { 6.8 } else { 0.8 };
            k.note_on(72, 1.0, HELD);
            render(&mut k, 0.2);
            let (l, _) = render(&mut k, 1.0);
            // How often the level swings past its mean: the horn going round.
            let env: Vec<f32> = l.chunks(240).map(rms).collect();
            let mean = env.iter().sum::<f32>() / env.len() as f32;
            env.windows(2).filter(|w| (w[0] > mean) != (w[1] > mean)).count()
        };
        let (slow, fast) = (wobble(0.0), wobble(1.0));
        assert!(fast >= 10 && fast > slow * 3, "slow {slow} fast {fast}");
    }

    #[test]
    fn loud_chords_stay_in_range_and_finite() {
        for (name, preset) in KeysParams::presets() {
            let mut k = Keys::new(preset, SR);
            for pitch in [36, 48, 55, 60, 64, 67, 72, 76, 79, 84] {
                k.note_on(pitch, 1.0, HELD);
            }
            let (l, r) = render(&mut k, 1.0);
            let peak = l.iter().chain(&r).fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak.is_finite() && peak <= 1.0, "{name}: peak {peak}");
            assert!(peak > 0.05, "{name}: sounds ({peak})");
        }
    }

    #[test]
    fn it_renders_the_same_every_time() {
        let play = || {
            let mut k = Keys::new(KeysParams::presets()[5].1, SR);
            k.note_on(60, 0.7, HELD);
            k.note_on(64, 0.6, 9000);
            render(&mut k, 0.4).0
        };
        assert_eq!(play(), play());
    }

    #[test]
    fn pitch_bend_retunes_sounding_notes() {
        let mut k = keys(KeysModel::Tines);
        k.note_on(57, 0.8, HELD);
        render(&mut k, 0.1);
        k.set_pitch(cents(200.0));
        let (l, _) = render(&mut k, 0.4);
        let found = peak(&l, 246.94);
        assert!((1200.0 * (found / 246.94).log2()).abs() < 6.0, "{found}");
    }

    #[test]
    fn many_notes_steal_the_quietest() {
        let mut k = keys(KeysModel::Piano);
        for i in 0..(VOICES as u8 + 8) {
            k.note_on(30 + i, 0.5 + (i % 3) as f32 * 0.1, HELD);
            render(&mut k, 0.01);
        }
        assert_eq!(k.voices.iter().filter(|v| v.active).count(), VOICES);
        assert!(k.voices.iter().any(|v| v.gate.pitch == 30 + VOICES as u8 + 7), "the last note plays");
    }

    /// Writes a short phrase for every factory preset as 16-bit WAV files, to listen to:
    /// `PULSE_DEMOS=dir cargo test --release -p engine keys_demos -- --ignored`.
    #[test]
    #[ignore]
    fn keys_demos() {
        let Ok(dir) = std::env::var("PULSE_DEMOS") else { return };
        std::fs::create_dir_all(&dir).unwrap();
        // (beat, length in beats, pitch, velocity) at 90 BPM: a progression, then a line.
        let mut phrase: Vec<(f32, f32, u8, f32)> = Vec::new();
        for (bar, chord) in [[48, 55, 64, 67], [45, 52, 60, 64], [41, 53, 57, 64], [43, 50, 59, 62]].iter().enumerate() {
            for (i, &pitch) in chord.iter().enumerate() {
                phrase.push((bar as f32 * 2.0 + i as f32 * 0.02, 1.9, pitch, 0.55 + i as f32 * 0.08));
            }
        }
        for (i, &(pitch, vel)) in [(72, 0.5), (74, 0.6), (76, 0.75), (79, 0.9), (76, 0.6), (74, 0.5), (72, 0.7), (67, 0.4)].iter().enumerate() {
            phrase.push((8.0 + i as f32 * 0.5, 0.45, pitch, vel));
        }
        phrase.push((12.0, 3.0, 36, 1.0));
        phrase.push((12.0, 3.0, 60, 1.0));
        phrase.push((12.0, 3.0, 64, 1.0));
        let beat = 60.0 / 90.0;
        for (name, preset) in KeysParams::presets() {
            let mut k = Keys::new(preset, SR);
            let total = (17.0 * beat * SR) as usize;
            let (mut l, mut r) = (vec![0.0; total], vec![0.0; total]);
            let mut events: Vec<(usize, Option<(u8, f32)>, u8)> = Vec::new();
            for &(start, len, pitch, vel) in &phrase {
                events.push(((start * beat * SR) as usize, Some((pitch, vel)), pitch));
                events.push((((start + len) * beat * SR) as usize, None, pitch));
            }
            events.sort_by_key(|e| (e.0, e.1.is_some()));
            let mut at = 0;
            for (frame, on, pitch) in events {
                let frame = frame.min(total);
                if frame > at {
                    k.render(&mut l[at..frame], &mut r[at..frame]);
                    at = frame;
                }
                match on {
                    Some((p, v)) => k.note_on(p, v, HELD),
                    None => k.note_off(pitch),
                }
            }
            k.render(&mut l[at..], &mut r[at..]);
            let file: String = name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
            write_wav(&format!("{dir}/{file}.wav"), &l, &r);
        }
    }

    pub(crate) fn write_wav(path: &str, l: &[f32], r: &[f32]) {
        let mut data = Vec::with_capacity(l.len() * 4);
        for (a, b) in l.iter().zip(r) {
            for s in [a, b] {
                data.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&(SR as u32).to_le_bytes());
        out.extend_from_slice(&(SR as u32 * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        std::fs::write(path, out).unwrap();
    }
}
