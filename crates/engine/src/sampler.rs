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
    let native = if sample.sample_rate > 0.0 { sample.sample_rate } else { sample_rate };
    (semitones(transpose) * native / sample_rate) as f64
}

// ----- Sampler --------------------------------------------------------------------------

const SAMPLER_VOICES: usize = 16;

#[derive(Clone, Copy)]
struct SamplerVoice {
    gate: Gate,
    env: Adsr,
    position: f64,
    rate: f64,
    filters: [Svf; 2],
}

pub struct Sampler {
    params: SamplerParams,
    voices: [SamplerVoice; SAMPLER_VOICES],
    sample_rate: f32,
    counter: u64,
}

impl Sampler {
    pub fn new(params: SamplerParams, sample_rate: f32) -> Self {
        let voice = SamplerVoice { gate: Gate::OFF, env: Adsr::IDLE, position: 0.0, rate: 1.0, filters: [Svf::default(); 2] };
        Self { params, voices: [voice; SAMPLER_VOICES], sample_rate, counter: 0 }
    }

    pub fn params(&self) -> SamplerParams {
        self.params
    }

    pub fn set_params(&mut self, params: SamplerParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, samples: &[Arc<Sample>], pitch: u8, velocity: f32, gate: u32) {
        let Some(sample) = samples.first().filter(|s| s.frames() > 0) else { return };
        let p = self.params;
        self.counter += 1;
        let index = free_voice(self.voices.iter().enumerate().map(|(i, v)| (i, v.env.stage == Stage::Idle, v.gate.age)));
        let voice = &mut self.voices[index];
        // One-shots ignore note length: they play until the sample (or the envelope) ends.
        let gate = if p.one_shot { HELD } else { gate };
        voice.gate = Gate::new(pitch, velocity, gate, self.counter);
        voice.position = (p.start.clamp(0.0, 0.99) as f64) * sample.frames() as f64;
        voice.rate = playback_rate(sample, pitch as f32 - p.root + p.tune, self.sample_rate);
        voice.filters = [Svf::default(); 2];
        voice.env = Adsr::IDLE;
        voice.env.trigger();
    }

    pub fn note_off(&mut self, pitch: u8) {
        if self.params.one_shot {
            return;
        }
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.gate.held && v.gate.pitch == pitch) {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn all_notes_off(&mut self) {
        let (release, sr) = (self.params.release, self.sample_rate);
        for v in &mut self.voices {
            v.gate.held = false;
            v.env.release(release, sr);
        }
    }

    pub fn render(&mut self, samples: &[Arc<Sample>], left: &mut [f32], right: &mut [f32]) {
        let p = self.params;
        let sr = self.sample_rate;
        let Some(sample) = samples.first().filter(|s| s.frames() > 0) else {
            for v in &mut self.voices {
                v.env = Adsr::IDLE;
            }
            return;
        };
        let frames = sample.frames() as f64;
        let loop_start = p.start.clamp(0.0, 0.99) as f64 * frames;
        let g = svf_g(p.cutoff, sr);
        let k = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let filtered = p.cutoff < 19_000.0;
        for voice in self.voices.iter_mut().filter(|v| v.env.stage != Stage::Idle) {
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
                let (mut l, mut r) = sample.read(voice.position);
                voice.position += voice.rate;
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
    age: u64,
}

pub struct DrumRack {
    params: RackParams,
    voices: [PadVoice; RACK_VOICES],
    sample_rate: f32,
    counter: u64,
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
            age: 0,
        };
        Self { params, voices: [voice; RACK_VOICES], sample_rate, counter: 0 }
    }

    pub fn params(&self) -> RackParams {
        self.params
    }

    pub fn set_params(&mut self, params: RackParams) {
        self.params = params;
    }

    pub fn note_on(&mut self, samples: &[Arc<Sample>], pitch: u8, velocity: f32) {
        let Some(pad) = rack_pad(pitch) else { return };
        let Some(sample) = samples.get(pad).filter(|s| s.frames() > 0) else { return };
        let p = self.params;
        let group = p.choke[pad];
        if group != 0 {
            for v in self.voices.iter_mut().filter(|v| v.active && v.pad != pad && p.choke[v.pad] == group) {
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
            rate: playback_rate(sample, p.tune[pad], self.sample_rate),
            gain: (angle.cos() * amp, angle.sin() * amp),
            remaining,
            fade: 1.0,
            fading: false,
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
}
