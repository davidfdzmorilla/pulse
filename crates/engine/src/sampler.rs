//! Sample-playing instruments: a chromatic sampler and a sixteen-pad drum rack.
//!
//! Samples are owned by the track and passed in on every call, so swapping them
//! never allocates or frees memory on the audio thread. Voices only keep indices
//! and positions, and fall silent if their sample disappears.

use crate::instruments::{Adsr, Gate, Stage, Svf, free_voice, semitones, svf_g};
use crate::model::{RACK_FULL_LENGTH, RackParams, SamplerParams, rack_pad};
use crate::sample::Sample;
use crate::synth::HELD;
use std::sync::Arc;

/// Rate at which a sample recorded at `sample_rate` plays back at the engine rate, transposed.
fn playback_rate(sample: &Sample, transpose: f32, sample_rate: f32) -> f64 {
    native_rate(sample, sample_rate) * semitones(transpose) as f64
}

/// Rate at which a sample plays at its own pitch.
fn native_rate(sample: &Sample, sample_rate: f32) -> f64 {
    let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sample_rate };
    (native / sample_rate) as f64
}

/// How long a voice cut by a mono note takes to fade (no click).
const CUT_SECONDS: f32 = 0.005;
/// A note this close to the end of the last one only touches it: no slide.
const TOUCH_SECONDS: f32 = 0.002;

// ----- Sampler --------------------------------------------------------------------------

const SAMPLER_VOICES: usize = 16;

#[derive(Clone, Copy)]
struct SamplerVoice {
    /// The track's sample it plays (its zone's).
    slot: usize,
    gate: Gate,
    env: Adsr,
    position: f64,
    rate: f64,
    filters: [Svf; 2],
    /// The note its sample plays at its own pitch (its zone's root, tuned).
    root: f32,
    /// Its pitch now and where it slides to, in (fractional) MIDI notes.
    note: f32,
    target: f32,
    /// Its sample's rate at its own pitch.
    native: f64,
}

impl SamplerVoice {
    /// Sounding and not let go: what a new mono note cuts or slides from.
    fn sounding(&self) -> bool {
        !matches!(self.env.stage, Stage::Idle | Stage::Release)
    }
}

pub struct Sampler {
    params: SamplerParams,
    voices: [SamplerVoice; SAMPLER_VOICES],
    sample_rate: f32,
    counter: u64,
    /// Pitch bend and vibrato, as a playback-rate ratio.
    pitch: f32,
    /// What the controllers do to the cutoff, as a ratio (see [`Self::set_brightness`]).
    bright: f32,
    /// Mono: the keys held (not clip notes, which end on their own), oldest
    /// first, to go back to when the last is let go.
    held: [u8; 16],
    held_count: usize,
    /// Mono: the last note played, where a Mono glide starts.
    last_note: Option<f32>,
}

impl Sampler {
    pub fn new(params: SamplerParams, sample_rate: f32) -> Self {
        let voice = SamplerVoice {
            slot: 0,
            gate: Gate::OFF,
            env: Adsr::IDLE,
            position: 0.0,
            rate: 1.0,
            filters: [Svf::default(); 2],
            root: 60.0,
            note: 60.0,
            target: 60.0,
            native: 1.0,
        };
        Self { params, voices: [voice; SAMPLER_VOICES], sample_rate, counter: 0, pitch: 1.0, bright: 1.0, held: [0; 16], held_count: 0, last_note: None }
    }

    /// Bends every note by `ratio` (a frequency multiplier: pitch bend and vibrato).
    pub fn set_pitch(&mut self, ratio: f32) {
        self.pitch = ratio;
    }

    /// Opens (above 1) or closes the filter by `ratio` (brightness, CC 74, and
    /// pressure). With the filter open, closing it still darkens the sample.
    pub fn set_brightness(&mut self, ratio: f32) {
        self.bright = ratio;
    }

    pub fn params(&self) -> SamplerParams {
        self.params
    }

    pub fn set_params(&mut self, params: SamplerParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, samples: &[Arc<Sample>], pitch: u8, velocity: f32, gate: u32) {
        let p = self.params;
        // One-shots ignore note length: they play until the sample (or the envelope) ends.
        let gate = if p.one_shot { HELD } else { gate };
        let mut glide_from = None;
        if p.mono {
            // Only keys get a note-off; clip notes end on their own.
            if gate == HELD && !p.one_shot && self.held_count < self.held.len() {
                self.held[self.held_count] = pitch;
                self.held_count += 1;
            }
            // Legato: a note over one still sounding slides there, the sample
            // and its envelope carrying on.
            let margin = (self.sample_rate * TOUCH_SECONDS) as u32;
            let overlaps = self.voices.iter().any(|v| v.sounding() && v.gate.open_for(margin));
            if p.legato && overlaps {
                self.slide_to(pitch, gate);
                return;
            }
            // Mono glides from the last note always; Legato only into a tie.
            if p.glide > 0.0 && !p.legato {
                glide_from = self.last_note;
            }
            // The last note (and any tail) cut, quickly.
            for v in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
                v.gate.close();
                v.env.cut(CUT_SECONDS, self.sample_rate);
            }
            self.last_note = Some(pitch as f32);
        }
        if p.zone_count == 0 {
            self.start_voice(samples, 0, p.root, pitch, velocity, gate, glide_from);
            return;
        }
        // Every zone with this note and velocity sounds (layers too).
        for (slot, zone) in p.zones().iter().enumerate() {
            if zone.plays(pitch, velocity) {
                self.start_voice(samples, slot, zone.root as f32, pitch, velocity, gate, glide_from);
            }
        }
    }

    /// The sounding mono voices (the layers of one note) glide to `pitch`,
    /// holding until `gate` ends; the sample and envelope carry on.
    fn slide_to(&mut self, pitch: u8, gate: u32) {
        self.counter += 1;
        let glide = self.params.glide > 0.0;
        for v in self.voices.iter_mut().filter(|v| v.sounding()) {
            v.gate = Gate::new(pitch, v.gate.velocity, gate, self.counter);
            v.target = pitch as f32;
            if !glide {
                v.note = v.target;
                v.rate = v.native * semitones(v.note - v.root) as f64;
            }
        }
        self.last_note = Some(pitch as f32);
    }

    #[allow(clippy::too_many_arguments)]
    fn start_voice(&mut self, samples: &[Arc<Sample>], slot: usize, root: f32, pitch: u8, velocity: f32, gate: u32, glide_from: Option<f32>) {
        let Some(sample) = samples.get(slot).filter(|s| s.frames() > 0) else { return };
        let p = self.params;
        self.counter += 1;
        let index = free_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.gate.age)));
        let voice = &mut self.voices[index];
        voice.slot = slot;
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.position = (p.start.clamp(0.0, 0.99) as f64) * sample.frames() as f64;
        voice.root = root - p.tune;
        voice.target = pitch as f32;
        voice.note = glide_from.unwrap_or(voice.target);
        voice.native = native_rate(sample, self.sample_rate);
        voice.rate = voice.native * semitones(voice.note - voice.root) as f64;
        voice.filters = [Svf::default(); 2];
        voice.env = Adsr::IDLE;
        voice.env.trigger();
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.params.one_shot {
            return;
        }
        let (release, sr) = (self.params.release, self.sample_rate);
        if self.params.mono {
            if let Some(at) = self.held[..self.held_count].iter().position(|&n| n == pitch) {
                self.held.copy_within(at + 1..self.held_count, at);
                self.held_count -= 1;
            }
            // The key sounding let go with another still down: back to that
            // one, sliding, without a new attack.
            let sounding = self.voices.iter().any(|v| v.sounding() && v.gate.held && v.gate.pitch == pitch);
            if sounding && self.held_count > 0 {
                let previous = self.held[self.held_count - 1];
                self.slide_to(previous, HELD);
                return;
            }
        }
        for v in self.voices.iter_mut().filter(|v| v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (release, sr) = (self.params.release, self.sample_rate);
        self.held_count = 0;
        for v in &mut self.voices {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        // At rest the preset's own filter, untouched.
        let cutoff = if self.bright == 1.0 { p.cutoff } else { (p.cutoff.min(19_000.0) * self.bright).clamp(20.0, sr * 0.45) };
        let g = svf_g(cutoff, sr);
        let k = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let filtered = cutoff < 19_000.0;
        let pitch = self.pitch as f64;
        // A glide's time constant is a third of its time: the pitch is 95% of
        // the way there when the glide time is up (as in the synths).
        let glide_k = if p.glide > 0.0 { 1.0 - (-3.0 / (p.glide * 0.001 * sr)).exp() } else { 1.0 };
        for voice in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
            // Its sample gone (or swapped for an empty one): silent.
            let Some(sample) = samples.get(voice.slot).filter(|s| s.frames() > 0) else {
                voice.env = Adsr::IDLE;
                continue;
            };
            let frames = sample.frames() as f64;
            let loop_start = p.start.clamp(0.0, 0.99) as f64 * frames;
            for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
                if voice.gate.tick() {
                    voice.env.release(p.release, sr);
                }
                if voice.position >= frames {
                    if p.looping && !p.one_shot && voice.env.stage != Stage::Release && frames - loop_start >= 1.0 {
                        voice.position = loop_start + (voice.position - frames) % (frames - loop_start);
                    } else {
                        voice.env = Adsr::IDLE;
                        break;
                    }
                }
                let level = voice.env.tick(p.attack, p.decay, p.sustain, sr);
                if voice.env.stage == Stage::Idle {
                    break;
                }
                if voice.note != voice.target {
                    // Sliding: the read rate follows the pitch smoothly and
                    // the position never jumps (no click).
                    voice.note += (voice.target - voice.note) * glide_k;
                    if (voice.target - voice.note).abs() < 1e-4 {
                        voice.note = voice.target;
                    }
                    voice.rate = voice.native * semitones(voice.note - voice.root) as f64;
                }
                let (mut l, mut r) = sample.read(voice.position);
                voice.position += voice.rate * pitch;
                if filtered {
                    l = voice.filters[0].tick(l, g, k).0;
                    r = voice.filters[1].tick(r, g, k).0;
                }
                let amp = level * voice.gate.velocity * p.gain;
                *out_l += l * amp;
                *out_r += r * amp;
            }
        }
    }
}

// ----- Drum Rack ------------------------------------------------------------------------

const RACK_VOICES: usize = 32;
/// Fade applied when a pad is choked or reaches its length, to avoid clicks.
const FADE_SECONDS: f32 = 0.005;

#[derive(Clone, Copy)]
struct PadVoice {
    pad: usize,
    active: bool,
    position: f64,
    rate: f64,
    gain: (f32, f32),
    /// Frames left before the fade-out starts.
    remaining: u32,
    fade: f32,
    fading: bool,
    /// Played by a note outside the pads (see [`RackParams::chromatic`]).
    chromatic: bool,
    age: u64,
}

pub struct DrumRack {
    params: RackParams,
    voices: [PadVoice; RACK_VOICES],
    sample_rate: f32,
    counter: u64,
    /// Pitch bend now, -1..1 (see [`RackParams::bend`]).
    bend: f32,
}

impl DrumRack {
    pub fn new(params: RackParams, sample_rate: f32) -> Self {
        let voice = PadVoice {
            pad: 0,
            active: false,
            position: 0.0,
            rate: 1.0,
            gain: (0.0, 0.0),
            remaining: 0,
            fade: 1.0,
            fading: false,
            chromatic: false,
            age: 0,
        };
        Self { params, voices: [voice; RACK_VOICES], sample_rate, counter: 0, bend: 0.0 }
    }

    /// Pitch bend, -1..1: the hits that start from now on take it, each pad
    /// as far as its bend range says.
    pub fn set_bend(&mut self, bend: f32) {
        self.bend = bend.clamp(-1.0, 1.0);
    }

    pub fn params(&self) -> RackParams {
        self.params
    }

    pub fn set_params(&mut self, params: RackParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, samples: &[Arc<Sample>], pitch: u8, velocity: f32) {
        let p = self.params;
        let Some((pad, transpose)) = p.pad_for(pitch) else { return };
        let Some(sample) = samples.get(pad).filter(|s| s.frames() > 0) else { return };
        let chromatic = rack_pad(pitch).is_none();
        let group = p.choke[pad];
        if group != 0 {
            for v in self.voices.iter_mut().filter(|v| v.active && v.pad != pad && p.choke[v.pad] == group) {
                v.fading = true;
            }
        }
        // A chromatic note cuts the one before, as a mono 808 does.
        if chromatic {
            for v in self.voices.iter_mut().filter(|v| v.active && v.chromatic) {
                v.fading = true;
            }
        }
        self.counter += 1;
        let index = free_voice(self.voices.iter().enumerate().map(|(i, v)| (i, !v.active, v.age)));
        // Constant-power pan; centre is unity on both sides.
        let angle = (p.pan[pad].clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let amp = velocity * p.level[pad] * std::f32::consts::SQRT_2;
        let length = p.length[pad];
        let remaining = if length >= RACK_FULL_LENGTH * 0.999 { u32::MAX } else { (length * self.sample_rate) as u32 };
        self.voices[index] = PadVoice {
            pad,
            active: true,
            position: 0.0,
            rate: playback_rate(sample, p.tune[pad] + transpose + self.bend * p.bend[pad], self.sample_rate),
            gain: (angle.cos() * amp, angle.sin() * amp),
            remaining,
            fade: 1.0,
            fading: false,
            chromatic,
            age: self.counter,
        };
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
        let fade_step = 1.0 / (FADE_SECONDS * self.sample_rate);
        for voice in self.voices.iter_mut().filter(|v| v.active) {
            let Some(sample) = samples.get(voice.pad).filter(|s| s.frames() > 0) else {
                voice.active = false;
                continue;
            };
            let frames = sample.frames() as f64;
            for (out_l, out_r) in left.iter_mut().zip(right.iter_mut()) {
                if voice.position >= frames {
                    voice.active = false;
                    break;
                }
                if voice.remaining == 0 {
                    voice.fading = true;
                } else if voice.remaining != u32::MAX {
                    voice.remaining -= 1;
                }
                if voice.fading {
                    voice.fade -= fade_step;
                    if voice.fade <= 0.0 {
                        voice.active = false;
                        break;
                    }
                }
                let (l, r) = sample.read(voice.position);
                voice.position += voice.rate;
                *out_l += l * voice.gain.0 * voice.fade;
                *out_r += r * voice.gain.1 * voice.fade;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RACK_FIRST_NOTE, RACK_PADS};

    const SR: f32 = 48_000.0;

    /// A one-second ramp from 0 to 1 at `rate`, so positions can be read back from values.
    fn ramp(rate: f32) -> Arc<Sample> {
        let frames = rate as usize;
        let data: Vec<f32> = (0..frames).map(|i| i as f32 / frames as f32).collect();
        Arc::new(Sample::from_channels(data.clone(), data, rate))
    }

    fn constant(frames: usize, value: f32) -> Arc<Sample> {
        Arc::new(Sample::from_channels(vec![value; frames], vec![value; frames], SR))
    }

    fn render(block: usize, mut f: impl FnMut(&mut [f32], &mut [f32])) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        f(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn sampler_plays_an_octave_up_at_twice_the_speed() {
        let samples = [ramp(SR)];
        let params = SamplerParams { gain: 1.0, ..Default::default() };
        let mut sampler = Sampler::new(params, SR);
        sampler.note_on(&samples, 72, 1.0, HELD);
        let (l, _) = render(1000, |l, r| sampler.render(&samples, l, r));
        // After 1000 output frames an octave up has read ~2000 source frames.
        let expected = 2000.0 / SR;
        assert!((l[999] - expected).abs() < 0.002, "{} vs {expected}", l[999]);
    }

    #[test]
    fn sampler_resamples_files_at_other_rates() {
        let samples = [ramp(24_000.0)];
        let mut sampler = Sampler::new(SamplerParams { gain: 1.0, ..Default::default() }, SR);
        sampler.note_on(&samples, 60, 1.0, HELD);
        let (l, _) = render(4800, |l, r| sampler.render(&samples, l, r));
        // 0.1 s at 48 kHz is 2400 frames of a 24 kHz file: a tenth of the ramp.
        assert!((l[4799] - 0.1).abs() < 0.002, "{}", l[4799]);
    }

    #[test]
    fn sampler_stops_at_the_end_unless_looping() {
        let samples = [constant(100, 0.5)];
        let mut sampler = Sampler::new(SamplerParams { gain: 1.0, ..Default::default() }, SR);
        sampler.note_on(&samples, 60, 1.0, HELD);
        let (l, _) = render(300, |l, r| sampler.render(&samples, l, r));
        assert!(l[50] > 0.4);
        assert_eq!(l[200], 0.0, "the sample is only 100 frames long");

        let mut looping = Sampler::new(SamplerParams { gain: 1.0, looping: true, ..Default::default() }, SR);
        looping.note_on(&samples, 60, 1.0, HELD);
        let (l, _) = render(300, |l, r| looping.render(&samples, l, r));
        assert!(l[250] > 0.4, "a held looping note keeps sounding");
    }

    #[test]
    fn one_shots_ignore_note_off() {
        let samples = [constant(48_000, 0.5)];
        let mut sampler = Sampler::new(SamplerParams { gain: 1.0, one_shot: true, ..Default::default() }, SR);
        sampler.note_on(&samples, 60, 1.0, 10);
        sampler.note_off(60);
        let (l, _) = render(20_000, |l, r| sampler.render(&samples, l, r));
        assert!(l[19_999] > 0.4);
    }

    /// Zones: each note plays its zone's sample from that zone's root, and
    /// velocity picks the layer.
    #[test]
    fn zones_pick_the_sample_and_layer_for_each_note() {
        use crate::model::Zone;
        // Three files told apart by their level: low notes, high soft, high loud.
        let samples = [constant(48_000, 0.1), constant(48_000, 0.2), constant(48_000, 0.4)];
        let mut params = SamplerParams { gain: 1.0, attack: 0.0, ..Default::default() };
        params.zones[0] = Zone { lo: 0, hi: 59, root: 48, vel_lo: 1, vel_hi: 127 };
        params.zones[1] = Zone { lo: 60, hi: 127, root: 72, vel_lo: 1, vel_hi: 63 };
        params.zones[2] = Zone { lo: 60, hi: 127, root: 72, vel_lo: 64, vel_hi: 127 };
        params.zone_count = 3;
        let level = |pitch: u8, velocity: f32| {
            let mut sampler = Sampler::new(params, SR);
            sampler.note_on(&samples, pitch, velocity, HELD);
            let (l, _) = render(2000, |l, r| sampler.render(&samples, l, r));
            l[1999] / velocity
        };
        assert!((level(50, 1.0) - 0.1).abs() < 0.01, "{}", level(50, 1.0));
        assert!((level(70, 0.3) - 0.2).abs() < 0.01, "{}", level(70, 0.3));
        assert!((level(70, 0.9) - 0.4).abs() < 0.01, "{}", level(70, 0.9));

        // Each zone plays at its own root: a ramp at 72 in the upper zone reads at 1x.
        let ramps = [ramp(SR), ramp(SR), ramp(SR)];
        let mut sampler = Sampler::new(params, SR);
        sampler.note_on(&ramps, 72, 1.0, HELD);
        let (l, _) = render(1000, |l, r| sampler.render(&ramps, l, r));
        assert!((l[999] - 1000.0 / SR).abs() < 0.002, "{}", l[999]);
        // And 48 is the lower zone's root: 1x too, not two octaves down.
        let mut sampler = Sampler::new(params, SR);
        sampler.note_on(&ramps, 48, 1.0, HELD);
        let (l, _) = render(1000, |l, r| sampler.render(&ramps, l, r));
        assert!((l[999] - 1000.0 / SR).abs() < 0.002, "{}", l[999]);
    }

    #[test]
    fn sampler_is_silent_without_a_sample() {
        let mut sampler = Sampler::new(SamplerParams::default(), SR);
        let samples = [Arc::new(Sample::default())];
        sampler.note_on(&samples, 60, 1.0, HELD);
        let (l, r) = render(256, |l, r| sampler.render(&samples, l, r));
        assert!(l.iter().chain(&r).all(|&s| s == 0.0));
        sampler.render(&[], &mut [0.0; 4], &mut [0.0; 4]);
    }

    #[test]
    fn rack_pads_play_their_own_sample_and_pan() {
        let mut samples: Vec<Arc<Sample>> = (0..RACK_PADS).map(|_| Arc::new(Sample::default())).collect();
        samples[2] = constant(1000, 0.5);
        let mut params = RackParams::default();
        params.level[2] = 1.0;
        params.pan[2] = -1.0;
        let mut rack = DrumRack::new(params, SR);
        rack.note_on(&samples, RACK_FIRST_NOTE + 1, 1.0);
        let (l, r) = render(100, |l, r| rack.render(&samples, l, r));
        assert!(l.iter().chain(&r).all(|&s| s == 0.0), "pad 2 has no sample");

        rack.note_on(&samples, RACK_FIRST_NOTE + 2, 1.0);
        let (l, r) = render(100, |l, r| rack.render(&samples, l, r));
        assert!(l[50] > 0.6 && r[50].abs() < 1e-6, "hard left: {} {}", l[50], r[50]);
    }

    #[test]
    fn choke_groups_cut_each_other() {
        let samples: Vec<Arc<Sample>> = (0..RACK_PADS).map(|_| constant(48_000, 0.5)).collect();
        let mut params = RackParams::default();
        params.choke[0] = 1;
        params.choke[1] = 1;
        params.pan[1] = 1.0;
        let mut rack = DrumRack::new(params, SR);
        rack.note_on(&samples, RACK_FIRST_NOTE, 1.0);
        render(100, |l, r| rack.render(&samples, l, r));
        rack.note_on(&samples, RACK_FIRST_NOTE + 1, 1.0);
        let (l, r) = render(1000, |l, r| rack.render(&samples, l, r));
        assert!(l[999].abs() < 1e-6, "pad 1 (left) was choked: {}", l[999]);
        assert!(r[999] > 0.4, "pad 2 (right) keeps playing");
    }

    #[test]
    fn pad_length_fades_the_hit_out() {
        let samples: Vec<Arc<Sample>> = (0..RACK_PADS).map(|_| constant(48_000, 0.5)).collect();
        let mut params = RackParams::default();
        params.length[0] = 0.02;
        let mut rack = DrumRack::new(params, SR);
        rack.note_on(&samples, RACK_FIRST_NOTE, 1.0);
        let (l, _) = render(4800, |l, r| rack.render(&samples, l, r));
        assert!(l[500] > 0.3);
        assert_eq!(l[4000], 0.0, "faded out after 20 ms plus the fade");
    }

    /// A steady sine `seconds` long at `hz`, the sample of these tests' 808.
    fn sine(hz: f32, seconds: f32) -> Arc<Sample> {
        let frames = (seconds * SR) as usize;
        let data: Vec<f32> = (0..frames).map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / SR).sin()).collect();
        Arc::new(Sample::from_channels(data.clone(), data, SR))
    }

    /// The pitch of `x` (Hz) from its rising zero crossings, interpolated.
    fn frequency(x: &[f32]) -> f32 {
        let crossings: Vec<f32> = (1..x.len()).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).map(|i| (i - 1) as f32 + x[i - 1] / (x[i - 1] - x[i])).collect();
        let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
        SR * (crossings.len() - 1) as f32 / (last - first)
    }

    fn cents(hz: f32, note: f32) -> f32 {
        1200.0 * (hz / (440.0 * 2f32.powf((note - 69.0) / 12.0))).log2()
    }

    fn ms(m: f32) -> usize {
        (m * SR / 1000.0) as usize
    }

    /// Plays timed notes (start ms, length ms, pitch) as a clip does, in
    /// blocks of `block`; the left channel.
    fn play_notes(params: SamplerParams, samples: &[Arc<Sample>], notes: &[(f32, f32, u8)], total_ms: f32, block: usize) -> Vec<f32> {
        let mut sampler = Sampler::new(params, SR);
        let total = ms(total_ms);
        let (mut l, mut r) = (vec![0.0; total], vec![0.0; total]);
        let mut at = 0;
        while at < total {
            for &(start, length, pitch) in notes.iter().filter(|n| ms(n.0) == at) {
                let _ = start;
                sampler.note_on(samples, pitch, 1.0, ms(length) as u32);
            }
            // Up to the next note or the end of the block.
            let next = notes.iter().map(|n| ms(n.0)).filter(|&s| s > at).min().unwrap_or(total).min(at + block).min(total);
            sampler.render(samples, &mut l[at..next], &mut r[at..next]);
            at = next;
        }
        l
    }

    /// An 808 line in Legato with clip notes: a note that overlaps the one
    /// sounding slides from its pitch, the sample and envelope carrying on;
    /// after a rest, or just touching the last, a note starts on its pitch.
    #[test]
    fn legato_slides_only_into_overlapping_clip_notes() {
        // A3 (220 Hz) recorded at its own pitch: root 57.
        let samples = [sine(220.0, 4.0)];
        let p = SamplerParams { gain: 1.0, root: 57.0, mono: true, legato: true, glide: 80.0, release: 0.02, ..Default::default() };
        // 45 alone; 57 tied over it at 400 ms; a rest; 50 alone; 52 touching it.
        let notes = [(0.0, 500.0, 45u8), (400.0, 500.0, 57), (1200.0, 300.0, 50), (1500.0, 300.0, 52)];
        let l = play_notes(p, &samples, &notes, 2000.0, 512);
        let pitch = |from: f32, to: f32| frequency(&l[ms(from)..ms(to)]);
        assert!(cents(pitch(15.0, 60.0), 45.0).abs() < 3.0, "a loose note starts on its pitch: {}", cents(pitch(15.0, 60.0), 45.0));
        // 15 ms into the tie it is still on its way up.
        let early = 12.0 * (pitch(405.0, 425.0) / 110.0).log2() + 45.0;
        assert!(early > 45.5 && early < 54.0, "a tied note slides up from the last: {early}");
        assert!(cents(pitch(600.0, 880.0), 57.0).abs() < 3.0, "the slide arrives: {}", cents(pitch(600.0, 880.0), 57.0));
        // No new attack: the level doesn't dip or restart round the tie.
        let level = |from: f32, to: f32| l[ms(from)..ms(to)].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(level(395.0, 400.0) > 0.45 && level(400.0, 410.0) > 0.45, "{} {}", level(395.0, 400.0), level(400.0, 410.0));
        // The tie lasts as long as the new note: 400 + 500 ms, then its release.
        assert!(level(880.0, 895.0) > 0.4 && level(950.0, 1100.0) < 1e-3, "{} {}", level(880.0, 895.0), level(950.0, 1100.0));
        assert!(cents(pitch(1215.0, 1260.0), 50.0).abs() < 3.0, "after a rest, on its pitch: {}", cents(pitch(1215.0, 1260.0), 50.0));
        assert!(cents(pitch(1515.0, 1560.0), 52.0).abs() < 3.0, "touching notes don't slide: {}", cents(pitch(1515.0, 1560.0), 52.0));
        // The same in any block size.
        assert_eq!(l, play_notes(p, &samples, &notes, 2000.0, 64));
    }

    /// Mono cuts the last note (quickly, without a click) and glides from it
    /// always; Poly lets notes ring together.
    #[test]
    fn mono_cuts_the_last_note_and_glides_from_it() {
        let samples = [sine(220.0, 4.0)];
        let mono = SamplerParams { gain: 1.0, root: 57.0, mono: true, glide: 60.0, ..Default::default() };
        let notes = [(0.0, 1000.0, 57u8), (300.0, 600.0, 45)];
        let l = play_notes(mono, &samples, &notes, 1000.0, 512);
        let early = 12.0 * (frequency(&l[ms(305.0)..ms(320.0)]) / 110.0).log2() + 45.0;
        assert!(early > 46.0, "mono glides down from the last note: {early}");
        assert!(cents(frequency(&l[ms(500.0)..ms(800.0)]), 45.0).abs() < 3.0, "and arrives");
        // One note sounding: the level stays a single sine's.
        let peak = l[ms(310.0)..].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.51, "the first note was cut: {peak}");
        let mut poly = Sampler::new(SamplerParams { gain: 1.0, root: 57.0, ..Default::default() }, SR);
        poly.note_on(&samples, 57, 1.0, HELD);
        poly.note_on(&samples, 45, 1.0, HELD);
        assert_eq!(poly.voices.iter().filter(|v| v.sounding()).count(), 2);
        // Without a glide, mono starts each note on its pitch.
        let l = play_notes(SamplerParams { glide: 0.0, ..mono }, &samples, &notes, 1000.0, 512);
        assert!(cents(frequency(&l[ms(305.0)..ms(340.0)]), 45.0).abs() < 3.0);
    }

    /// The largest step between samples while sliding an octave up (Legato)
    /// or cutting to a new note (Mono) is no more than the steepest a sine at
    /// the highest pitch has: no click.
    #[test]
    fn slides_and_cuts_dont_click() {
        let samples = [sine(110.0, 4.0)];
        // A sine of amplitude a at f Hz moves at most 2π·f·a/SR a sample.
        let steepest = |hz: f32| std::f32::consts::TAU * hz * 0.5 / SR;
        let jump = |x: &[f32]| x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0f32, f32::max);
        for glide in [0.0, 30.0, 120.0] {
            let p = SamplerParams { gain: 1.0, root: 45.0, mono: true, legato: true, glide, ..Default::default() };
            let l = play_notes(p, &samples, &[(0.0, 600.0, 45u8), (300.0, 600.0, 57)], 800.0, 512);
            assert!(cents(frequency(&l[ms(650.0)..ms(800.0)]), 57.0).abs() < 3.0, "glide {glide}: it went there");
            let most = jump(&l[ms(250.0)..ms(800.0)]);
            println!("legato, glide {glide} ms: largest step {most:.5} (a 220 Hz sine's: {:.5})", steepest(220.0));
            assert!(most <= steepest(220.0) * 1.02, "glide {glide}: {most}");
        }
        let p = SamplerParams { gain: 1.0, root: 45.0, mono: true, ..Default::default() };
        let l = play_notes(p, &samples, &[(0.0, 600.0, 45u8), (303.0, 600.0, 57)], 800.0, 512);
        let most = jump(&l[ms(250.0)..ms(800.0)]);
        println!("mono cut: largest step {most:.5}");
        // The fading note and the new one together, no more.
        assert!(most <= (steepest(110.0) + steepest(220.0)) * 1.02, "{most}");
    }

    /// Keys (held notes) in Legato: letting go of the last one goes back to
    /// the one still held, sliding, without a new attack.
    #[test]
    fn letting_go_of_a_key_slides_back_to_the_one_held() {
        let samples = [sine(220.0, 4.0)];
        let mut sampler = Sampler::new(SamplerParams { gain: 1.0, root: 57.0, mono: true, legato: true, glide: 40.0, ..Default::default() }, SR);
        sampler.note_on(&samples, 45, 1.0, HELD);
        render(ms(100.0), |l, r| sampler.render(&samples, l, r));
        sampler.note_on(&samples, 52, 1.0, HELD);
        render(ms(300.0), |l, r| sampler.render(&samples, l, r));
        assert_eq!(sampler.voices.iter().filter(|v| v.sounding()).count(), 1);
        sampler.note_off(52);
        let (l, _) = render(ms(400.0), |l, r| sampler.render(&samples, l, r));
        assert!(cents(frequency(&l[ms(200.0)..]), 45.0).abs() < 3.0, "back on the held key");
        sampler.note_off(45);
        let (l, _) = render(ms(500.0), |l, r| sampler.render(&samples, l, r));
        assert!(l[ms(400.0)..].iter().all(|s| s.abs() < 1e-4), "and let go");
        assert_eq!(sampler.held_count, 0);
    }

    /// Pads follow pitch bend as far as their range: each hit takes the bend
    /// it starts with. A pad with no range ignores it.
    #[test]
    fn pads_take_the_bend_each_hit_starts_with() {
        let samples: Vec<Arc<Sample>> = (0..RACK_PADS).map(|_| sine(220.0, 1.0)).collect();
        let mut params = RackParams::default();
        params.bend[0] = 12.0;
        let hit = |params: RackParams, pitch: u8, bend: f32| {
            let mut rack = DrumRack::new(params, SR);
            rack.set_bend(bend);
            rack.note_on(&samples, pitch, 1.0);
            // The bend moving after the hit doesn't move it.
            rack.set_bend(0.0);
            let (l, _) = render(ms(100.0), |l, r| rack.render(&samples, l, r));
            frequency(&l)
        };
        let up = hit(params, RACK_FIRST_NOTE, 1.0);
        let half = hit(params, RACK_FIRST_NOTE, -0.25);
        let other = hit(params, RACK_FIRST_NOTE + 1, 1.0);
        println!("pad bent fully: {up:.2} Hz, a quarter down: {half:.2} Hz, a pad without range: {other:.2} Hz");
        assert!(cents(up, 69.0).abs() < 2.0, "an octave up: {up}");
        assert!(cents(half, 54.0).abs() < 2.0, "3 semitones down: {half}");
        assert!(cents(other, 57.0).abs() < 2.0, "unbent: {other}");
    }

    /// A chromatic pad: the notes outside the pads play it, C3 at its pitch,
    /// each cutting the last; the pads' own notes still play the pads.
    #[test]
    fn a_chromatic_pad_plays_the_notes_outside_the_pads() {
        let samples: Vec<Arc<Sample>> = (0..RACK_PADS).map(|_| sine(220.0, 1.0)).collect();
        let params = RackParams { chromatic: 3, ..Default::default() };
        assert_eq!(params.pad_for(60), Some((2, 0.0)));
        assert_eq!(params.pad_for(RACK_FIRST_NOTE + 5), Some((5, 0.0)), "pads stay pads");
        assert_eq!(RackParams::default().pad_for(60), None);
        let mut rack = DrumRack::new(params, SR);
        rack.note_on(&samples, 67, 1.0);
        let (l, _) = render(ms(50.0), |l, r| rack.render(&samples, l, r));
        assert!(cents(frequency(&l), 64.0).abs() < 2.0, "a fifth up: {}", frequency(&l));
        rack.note_on(&samples, 55, 1.0);
        let (l, _) = render(ms(100.0), |l, r| rack.render(&samples, l, r));
        assert!(cents(frequency(&l[ms(20.0)..]), 52.0).abs() < 2.0, "a fifth down: {}", frequency(&l[ms(20.0)..]));
        assert_eq!(rack.voices.iter().filter(|v| v.active).count(), 1, "each note cuts the last");
    }
}
