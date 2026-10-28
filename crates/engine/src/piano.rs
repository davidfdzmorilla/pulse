//! Pulse Keys' acoustic piano: digital waveguide strings struck by a hammer through the
//! soundboard (commuted synthesis).
//!
//! Each key has two or three strings, each a delay line closed by:
//! - a dispersion filter (allpasses) that makes the stiff string's overtones run sharp;
//! - a loss filter, so high partials die sooner than low ones;
//! - a fractional delay that tunes it exactly.
//!
//! The strings are tuned a few cents apart and lose energy at different rates, so they
//! beat and the note falls fast, then sings on (the piano's double decay). What strikes
//! them is the soundboard's own ringing response, shaped by the hammer (softer and
//! duller for gentle notes) and by where the hammer hits the string. That is the
//! wooden knock and the body a sum of sines can't have.

use std::f64::consts::PI;

/// Strings a key can have.
pub(crate) const STRINGS: usize = 3;
/// Most allpasses in a string's dispersion filter.
const DISPERSION: usize = 4;
/// Seconds of soundboard response that strike a string.
const BOARD_SECONDS: f32 = 0.35;

/// The soundboard's response to a tap, made once: a few dozen wooden modes that ring
/// briefly, over a short burst of noise.
pub(crate) struct Soundboard {
    pub(crate) response: Vec<f32>,
}

impl Soundboard {
    pub(crate) fn new(sample_rate: f32) -> Self {
        let n = (BOARD_SECONDS * sample_rate) as usize;
        let mut response = vec![0.0f32; n];
        let mut seed = 0x9E37_79B9u32;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        // Modes spread evenly in log-frequency from 70 Hz to 5 kHz; higher ones die faster.
        let modes = 72;
        for k in 0..modes {
            let f = 70.0 * (5000.0f32 / 70.0).powf((k as f32 + random()) / modes as f32);
            if f > sample_rate * 0.45 {
                continue;
            }
            let decay = 0.09 * (200.0 / f).powf(0.45) * (0.7 + 0.6 * random());
            let amp = (0.6 + 0.8 * random()) * (f / 200.0).powf(-0.35) * if random() < 0.5 { -1.0 } else { 1.0 };
            let (w, r) = (std::f32::consts::TAU * f / sample_rate, (-1.0 / (decay * sample_rate)).exp());
            let phase = random() * std::f32::consts::TAU;
            let mut env = 1.0;
            for (i, s) in response.iter_mut().enumerate() {
                *s += amp * env * (w * i as f32 + phase).sin();
                env *= r;
                if env < 1e-4 {
                    break;
                }
            }
        }
        // The knock: a short, bright burst.
        for (i, s) in response.iter_mut().enumerate().take((0.01 * sample_rate) as usize) {
            *s += (random() * 2.0 - 1.0) * 2.5 * (-(i as f32) / (0.0015 * sample_rate)).exp();
        }
        let energy = response.iter().map(|s| s * s).sum::<f32>().sqrt().max(1e-9);
        for s in &mut response {
            *s /= energy;
        }
        Soundboard { response }
    }
}

/// Phase delay (samples) of a first-order allpass with coefficient `a` at `w` (radians).
fn allpass_delay(a: f64, w: f64) -> f64 {
    // H = (a + z⁻¹) / (1 + a z⁻¹)
    let (c, s) = (w.cos(), w.sin());
    let num = (a + c, -s);
    let den = (1.0 + a * c, -a * s);
    let phase = num.1.atan2(num.0) - den.1.atan2(den.0);
    -phase / w
}

/// Phase delay of the loss filter g(1 + a) / (1 + a z⁻¹).
fn loss_delay(a: f64, w: f64) -> f64 {
    let den = (1.0 + a * w.cos(), -a * w.sin());
    den.1.atan2(den.0) / w
}

/// The loss filter's gain at `w`.
fn loss_gain(g: f64, a: f64, w: f64) -> f64 {
    g * (1.0 + a) / (1.0 + 2.0 * a * w.cos() + a * a).sqrt()
}

/// One string: its delay line lives in the voice's slice of [`crate::keys::Keys`]' lines.
#[derive(Clone, Copy, Default)]
pub(crate) struct PianoString {
    /// Whole samples of delay, and the fractional delay's coefficient.
    pub(crate) length: usize,
    thiran: f32,
    thiran_state: (f32, f32),
    /// The dispersion allpasses: how many, their coefficient, their states.
    pub(crate) stages: usize,
    pub(crate) dispersion: f32,
    dispersion_state: [(f32, f32); DISPERSION],
    pub(crate) loss_g: f32,
    loss_a: f32,
    loss_state: f32,
    write: usize,
    /// Frequency and decays it is designed for (to redesign it when bent or damped).
    freq: f64,
    t60: (f64, f64),
}

impl PianoString {
    /// Designs a string at `freq` whose fundamental falls 60 dB in `t60.0` seconds and
    /// whose partials near 5 kHz in `t60.1`, with overtones stretched as `stretch(n)`
    /// says (the n-th over the fundamental). `max` is the longest delay it may have.
    fn design(freq: f64, t60: (f64, f64), stretch: impl Fn(f64) -> f64, sample_rate: f64, max: usize) -> PianoString {
        let period = (sample_rate / freq).min(max as f64 - 4.0);
        let w0 = 2.0 * PI * freq / sample_rate;
        // Loss: the decay at the fundamental, and at a high partial.
        let gain_for = |t: f64| 10f64.powf(-3.0 * period / (sample_rate * t.max(0.01)));
        let wh = (2.0 * PI * 5000.0 / sample_rate).min(PI * 0.8).max(w0 * 1.5);
        let (low, high) = (gain_for(t60.0), gain_for(t60.1).min(gain_for(t60.0) * 0.9999));
        // The filter's shape sets how much faster the highs die than the fundamental
        // (falling as a goes from 0 to −1); g then sets the fundamental's decay.
        let ratio = |a: f64| loss_gain(1.0, a, wh) / loss_gain(1.0, a, w0);
        let (mut lo, mut hi) = (-0.999, 0.0);
        for _ in 0..50 {
            let mid = 0.5 * (lo + hi);
            if ratio(mid) > high / low { hi = mid } else { lo = mid }
        }
        // Never so steep that the fundamental can't keep its decay (high notes, where the
        // highs are close above it).
        let (mut floor_lo, mut floor_hi) = (-0.999, 0.0);
        for _ in 0..50 {
            let mid = 0.5 * (floor_lo + floor_hi);
            if loss_gain(1.0, mid, w0) < low / 0.99999 { floor_lo = mid } else { floor_hi = mid }
        }
        let loss_a = (0.5 * (lo + hi)).max(floor_hi);
        let g = (low / loss_gain(1.0, loss_a, w0)).min(0.99999);
        // Dispersion: as many allpasses as the period has room for, set so a high partial
        // lands where the stiff string puts it.
        let stages = if period > 120.0 { 4 } else if period > 50.0 { 2 } else if period > 16.0 { 1 } else { 0 };
        let base = |a: f64| period - stages as f64 * allpass_delay(a, w0) - loss_delay(loss_a, w0);
        let n_ref = (1..=8).rev().find(|&n| stretch(n as f64) * w0 < PI * 0.7).unwrap_or(1) as f64;
        let wn = stretch(n_ref) * w0;
        // How far partial n_ref's round trip is from 2πn: shrinks as a goes negative.
        let error = |a: f64| {
            let rest = base(a);
            let length = (rest - 0.5).floor();
            let d = rest - length;
            let eta = (1.0 - d) / (1.0 + d);
            let total = length + stages as f64 * allpass_delay(a, wn) + loss_delay(loss_a, wn) + allpass_delay(eta, wn);
            wn * total - 2.0 * PI * n_ref
        };
        // Allpasses with a < 0 add delay at low frequencies: keep the delay line long enough.
        let floor_a = if stages == 0 {
            0.0
        } else {
            let q = ((period - 6.0) / stages as f64).max(1.0001);
            ((1.0 - q) / (1.0 + q)).max(-0.9)
        };
        let dispersion = if stages == 0 || n_ref < 2.0 || error(0.0) <= 0.0 {
            0.0
        } else if error(floor_a) > 0.0 {
            floor_a
        } else {
            let (mut lo, mut hi) = (floor_a, 0.0);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if error(mid) > 0.0 { hi = mid } else { lo = mid }
            }
            0.5 * (lo + hi)
        };
        // Tuning: what the filters don't delay, the line and the fractional delay do.
        let rest = base(dispersion);
        let mut length = (rest - 0.5).floor().max(2.0);
        let mut d = rest - length;
        // The fractional delay's phase delay drifts from d at high notes: correct it.
        for _ in 0..3 {
            let eta = (1.0 - d) / (1.0 + d);
            d += (rest - length) - allpass_delay(eta, w0);
            if d > 1.5 {
                length += 1.0;
                d -= 1.0;
            } else if d < 0.5 && length > 2.0 {
                length -= 1.0;
                d += 1.0;
            }
        }
        PianoString {
            length: length as usize,
            thiran: ((1.0 - d) / (1.0 + d)) as f32,
            stages,
            dispersion: dispersion as f32,
            loss_g: g as f32,
            loss_a: loss_a as f32,
            freq,
            t60,
            ..Default::default()
        }
    }

    /// The same string with other decays (the damper), keeping its sound going.
    fn redesign(&mut self, t60: (f64, f64), stretch: impl Fn(f64) -> f64, sample_rate: f64, max: usize) {
        let fresh = PianoString::design(self.freq, t60, stretch, sample_rate, max);
        self.loss_g = fresh.loss_g;
        self.loss_a = fresh.loss_a;
        self.t60 = t60;
    }

    #[inline]
    fn tick(&mut self, line: &mut [f32], input: f32) -> f32 {
        let mask = line.len() - 1;
        let mut y = line[(self.write + line.len() - self.length) & mask];
        let a = self.dispersion;
        for (x1, y1) in self.dispersion_state.iter_mut().take(self.stages) {
            let out = a * y + *x1 - a * *y1;
            *x1 = y;
            *y1 = out;
            y = out;
        }
        let lossy = self.loss_g * (1.0 + self.loss_a) * y - self.loss_a * self.loss_state;
        self.loss_state = lossy;
        let (x1, y1) = self.thiran_state;
        let tuned = self.thiran * lossy + x1 - self.thiran * y1;
        self.thiran_state = (lossy, tuned);
        let v = tuned + input;
        line[self.write] = v;
        self.write = (self.write + 1) & mask;
        v
    }
}

/// What a key sounds with: its strings, the strike reaching them, the hammer's tone.
#[derive(Clone, Copy, Default)]
pub(crate) struct PianoVoice {
    pub(crate) strings: [PianoString; STRINGS],
    pub(crate) count: usize,
    /// How far through the soundboard's response the strike is.
    strike: usize,
    /// The hammer's push on the strings: a half-sine this many samples long, and how far in.
    contact: usize,
    pushed: usize,
    roundness: f32,
    push: f32,
    /// Loudness of the strike (velocity), and how loud the soundboard's knock is.
    force: f32,
    knock: f32,
    /// The hammer's felt: two one-pole lowpasses.
    felt: (f32, f32),
    felt_g: f32,
    /// Where the hammer hits: the strike, again, this many samples later (cancels the
    /// partials with a node there).
    position: usize,
    comb_write: usize,
    /// Running loudness of the output (for stealing and ending).
    pub(crate) energy: f32,
    stretch_b: f32,
    /// Floats of each string's line, and of the comb's (see [`line_size`]).
    string_size: usize,
    comb_size: usize,
}

/// Per-voice room in the lines: each string's delay line, then the strike's comb.
pub(crate) fn line_size(sample_rate: f32) -> (usize, usize) {
    let string = ((sample_rate / 26.0) as usize + 16).next_power_of_two();
    let comb = ((sample_rate / 26.0 / 6.0) as usize + 8).next_power_of_two();
    (string, comb)
}

/// What a string of a key is like.
pub(crate) struct Strike {
    pub(crate) pitch: u8,
    pub(crate) freq: f64,
    pub(crate) velocity: f32,
    pub(crate) gain: f32,
    /// 0..1: the hammer's hardness.
    pub(crate) tone: f32,
    pub(crate) knock: f32,
    /// 0..1: how far apart the strings are tuned.
    pub(crate) detune: f32,
    /// Seconds the slow string takes to fall 60 dB at the fundamental.
    pub(crate) t60: f64,
    pub(crate) inharmonicity: f32,
    pub(crate) seed: u32,
}

impl PianoVoice {
    /// Strikes a key: designs its strings and clears its lines.
    pub(crate) fn strike(s: &Strike, sample_rate: f32, lines: &mut [f32]) -> PianoVoice {
        lines.fill(0.0);
        let sr = sample_rate as f64;
        let (string_len, comb_len) = line_size(sample_rate);
        let b = s.inharmonicity as f64;
        let stretch = move |n: f64| crate::keys::partial(n as f32, b as f32) as f64;
        let unit = |k: u32| crate::keys::unit_noise(s.seed.wrapping_add(k));
        // The bass has two strings (or one with two ways of swinging), the rest three.
        let count = if s.pitch < 46 { 2 } else { 3 };
        // Cents apart: a fresh tuning at the default, a bar piano at the top.
        let d = s.detune as f64;
        let spread = (d + 6.0 * d * d) * (0.6 + 0.4 * unit(1) as f64);
        let cents = |c: f64| 2f64.powf(c / 1200.0);
        // One string, closest to the bridge's pull, gives its energy up fast; the others sing on.
        let plan = [(0.0, 0.22), (spread, 1.0), (-spread * 0.8, 0.85)];
        let high = |t: f64| 1.0 / (1.0 / t + 0.5 / 6.9 * 25.0);
        let mut strings = [PianoString::default(); STRINGS];
        for (k, &(detune, decay)) in plan.iter().enumerate().take(count) {
            let t = s.t60 * decay;
            strings[k] = PianoString::design(s.freq * cents(detune), (t, high(t)), stretch, sr, string_len);
        }
        // The hammer: harder and brighter with velocity and Tono; never duller than the note.
        let hard = (s.velocity * (0.35 + 1.1 * s.tone)).clamp(0.02, 1.45);
        let corner = (300.0 + 6500.0 * hard.powf(1.6)).max(s.freq as f32 * 2.0).min(sample_rate * 0.45);
        // Felt touches the string for a few milliseconds: less when struck hard, and less
        // up the keyboard (smaller, harder hammers). Shorter is brighter.
        let ms = (3.0 - 2.2 * hard.min(1.2)) * 2f32.powf(-(s.pitch as f32 - 60.0) / 30.0);
        let contact = (ms.clamp(0.25, 6.0) * 0.001 * sample_rate).max(2.0) as usize;
        let position = ((sr / s.freq) / (7.2 + unit(2) as f64 * 0.8)).round().max(1.0) as usize;
        PianoVoice {
            strings,
            count,
            force: s.gain,
            // Soft notes hardly knock.
            knock: s.knock * hard.min(1.0).powf(1.5),
            contact,
            // Soft felt pushes roundly (fewer highs); hard felt, sharply.
            roundness: 1.0 + 1.5 * (1.0 - hard.min(1.0)),
            push: 4.0 / contact as f32 * (contact as f32 / (0.0015 * sample_rate)).powf(0.6),
            felt_g: 1.0 - (-std::f32::consts::TAU * corner / sample_rate).exp(),
            position,
            stretch_b: s.inharmonicity,
            energy: 1.0,
            string_size: string_len,
            comb_size: comb_len,
            ..Default::default()
        }
    }

    /// Redesigns the strings for a bend (pitch bend, vibrato).
    pub(crate) fn bend(&mut self, ratio: f32, sample_rate: f32) {
        let (string_len, _) = line_size(sample_rate);
        let b = self.stretch_b;
        for string in self.strings.iter_mut().take(self.count) {
            let base = string.freq;
            let mut bent = PianoString::design(base * ratio as f64, string.t60, |n| crate::keys::partial(n as f32, b) as f64, sample_rate as f64, string_len);
            bent.freq = base;
            bent.write = string.write;
            bent.thiran_state = string.thiran_state;
            bent.dispersion_state = string.dispersion_state;
            bent.loss_state = string.loss_state;
            *string = bent;
        }
    }

    /// The dampers: every string falls 60 dB in `seconds` (the highs twice as fast).
    pub(crate) fn damp(&mut self, seconds: f32, sample_rate: f32) {
        let (string_len, _) = line_size(sample_rate);
        let b = self.stretch_b;
        let t = seconds.max(0.01) as f64;
        for string in self.strings.iter_mut().take(self.count) {
            let t60 = (string.t60.0.min(t), string.t60.1.min(t * 0.5));
            string.redesign(t60, |n| crate::keys::partial(n as f32, b) as f64, sample_rate as f64, string_len);
        }
    }

    /// True while the strike still reaches the strings.
    pub(crate) fn striking(&self, board: &Soundboard) -> bool {
        self.strike < board.response.len() || self.pushed < self.contact
    }

    /// One sample of the key.
    #[inline]
    pub(crate) fn tick(&mut self, board: &Soundboard, lines: &mut [f32]) -> f32 {
        let (string_size, comb_size) = (self.string_size, self.comb_size);
        // The hammer's push, less its echo from where the hammer hits the string.
        let mut x = 0.0;
        if self.pushed < self.contact {
            let phase = (self.pushed as f32 + 0.5) / self.contact as f32;
            // Short, hard blows aren't louder for being shorter: about as much energy either way.
            x = self.force * (std::f32::consts::PI * phase).sin().powf(self.roundness) * self.push;
            self.pushed += 1;
        }
        // The soundboard's knock, through the felt, heard as it is.
        let mut knock = 0.0;
        if self.strike < board.response.len() {
            let raw = board.response[self.strike] * self.force;
            self.strike += 1;
            self.felt.0 += self.felt_g * (raw - self.felt.0);
            self.felt.1 += self.felt_g * (self.felt.0 - self.felt.1);
            knock = self.felt.1;
        }
        let (strings_part, rest) = lines.split_at_mut(STRINGS * string_size);
        let comb = &mut rest[..comb_size];
        let mask = comb.len() - 1;
        let echo = comb[(self.comb_write + comb.len() - self.position.min(mask)) & mask];
        comb[self.comb_write] = x;
        self.comb_write = (self.comb_write + 1) & mask;
        let input = x - 0.8 * echo;
        // The hammer never meets the strings of a key quite evenly: they beat gently.
        const SHARE: [[f32; STRINGS]; 2] = [[0.6, 0.4, 0.0], [0.45, 0.33, 0.22]];
        let share = &SHARE[self.count.saturating_sub(2).min(1)];
        let mut out = 0.0;
        for (k, string) in self.strings.iter_mut().enumerate().take(self.count) {
            out += string.tick(&mut strings_part[k * string_size..(k + 1) * string_size], input * share[k]);
        }
        let y = out * 0.5 + knock * self.knock;
        self.energy += 0.0005 * (y * y - self.energy);
        y
    }
}

/// Floats a voice needs in the lines.
pub(crate) fn voice_size(sample_rate: f32) -> usize {
    let (string, comb) = line_size(sample_rate);
    STRINGS * string + comb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_string_is_designed_in_tune_with_stretched_overtones() {
        let sr = 48000.0;
        for (freq, b) in [(55.0, 2e-4), (261.6, 3e-4), (2093.0, 6e-3)] {
            let s = PianoString::design(freq, (10.0, 1.0), |n| crate::keys::partial(n as f32, b) as f64, sr, 4096);
            let w0 = 2.0 * PI * freq / sr;
            let eta = s.thiran as f64;
            let total = s.length as f64 + s.stages as f64 * allpass_delay(s.dispersion as f64, w0) + loss_delay(s.loss_a as f64, w0) + allpass_delay(eta, w0);
            assert!((total - sr / freq).abs() < 0.02, "{freq} Hz: loop {total} for {}", sr / freq);
            assert!(s.loss_g < 1.0 && s.loss_a <= 0.0);
            // The fundamental keeps the decay asked for.
            let per_period = loss_gain(s.loss_g as f64, s.loss_a as f64, w0);
            let t60 = 3.0 * (sr / freq) / (sr * -per_period.log10());
            assert!((t60 / 10.0 - 1.0).abs() < 0.05, "{freq} Hz: T60 {t60}");
        }
    }

    #[test]
    fn the_lines_have_room_for_the_lowest_string_and_the_comb() {
        for sr in [44100.0, 48000.0, 96000.0, 192000.0] {
            let (string, comb) = line_size(sr);
            assert_eq!(voice_size(sr), STRINGS * string + comb);
            assert!(string as f32 > sr / 27.5 && string.is_power_of_two() && comb.is_power_of_two());
            assert!(comb as f32 > sr / 27.5 / 7.0, "the strike point of the lowest string fits");
        }
    }
}
