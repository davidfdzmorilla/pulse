//! «Transición»: tape stop and start, repeats (stutter, beat repeat) and
//! cuts, in time with the song.
//!
//! **When.** «Active» comes from an automation lane, which the engine applies
//! once every [`SUB_BLOCK`] frames: a switch drawn on the beat is seen up to
//! 63 frames after it. The effect sounds [`LATENCY`] frames late (the mixer
//! makes up for it), so it learns of every switch before it has to play it:
//! one seen up to a sub-block after a 1/64-note line is put back on that
//! line, and the fades around it start (switching on) or end (switching
//! off) right there. A drop after a cut comes in whole, on its beat. All of
//! it happens on the same frames whatever the buffer.
//!
//! **How.** What it plays is read from its history (the input of the last
//! ~10 s) by position, with a 4-point Hermite interpolation:
//! - a tape is a read head whose speed falls from 1 to 0 (or rises from 0 to
//!   1) along a curve, pitch and time together; its level fades out as it
//!   stops, so it never ends on a held sample (DC);
//! - a repeat copies the piece of the grid it falls in (from the line before
//!   the switch) and plays it again each grid step, every repeat quieter
//!   (`decay_db`), higher or lower (`pitch`, by reading faster or slower),
//!   shorter (`gate`) and filtered further (`sweep`) than the last, with
//!   2 ms cross-fades between them. The first repeat is the sound itself;
//! - a cut is silence, or the sound through a filter.

use crate::effects::{ProcessContext, Svf, SvfCoefs};
use crate::engine::SUB_BLOCK;
use crate::model::{db_to_gain, FilterMode, TransitionMode, TransitionParams};
use std::f64::consts::FRAC_PI_2;

/// How late it sounds, frames: two sub-blocks, so a switch put back on its
/// line is still at least a sub-block ahead (the fades fit before it).
pub(crate) const LATENCY: usize = 2 * SUB_BLOCK as usize;
/// The lines switches are put back on, beats (a 1/64 note).
const SNAP_BEATS: f64 = 1.0 / 16.0;
/// What its history holds, seconds (a tape of 2 bars at 50 BPM).
const HISTORY_SECONDS: f32 = 10.0;
/// The longest piece a repeat holds, seconds (2 beats at 40 BPM).
const SLICE_SECONDS: f32 = 3.0;
/// Switching on and off, seconds (at most a sub-block of frames).
const EDGE_SECONDS: f32 = 0.001;
/// Between repeats, seconds.
const REPEAT_FADE_SECONDS: f32 = 0.002;
/// A tape starting, the cross-fade from its late sound to the live one, seconds.
const CATCH_UP_SECONDS: f32 = 0.03;
/// Under this speed a tape fades out (and from it, fades in).
const QUIET_SPEED: f64 = 0.1;
/// Switches waiting to be played.
const MAX_EDGES: usize = 8;

/// A switch: on or off at input frame `at`, and the song's beat there.
#[derive(Clone, Copy, Default)]
struct Edge {
    at: i64,
    on: bool,
    /// The beat at `at`, and frames a beat (None: the transport was stopped).
    beat: f64,
    frames_per_beat: f64,
    playing: bool,
}

/// One repeat of the piece: from `start` (input frame), read at `rate`.
#[derive(Clone, Copy)]
struct Repeat {
    index: u32,
    start: f64,
    /// Its grid step, frames.
    length: f64,
    rate: f64,
    gain: f32,
    /// Frames it sounds (under `length`: gated, or read faster).
    sounds: f64,
    /// It sounds the whole step and fades out over the next one's start.
    full: bool,
    cutoff: f32,
}

pub struct Transition {
    pub(crate) params: TransitionParams,
    rate: f32,
    history: [Vec<f32>; 2],
    mask: usize,
    /// Input frames written (the next one's index).
    written: i64,
    edges: [Edge; MAX_EDGES],
    edge_count: usize,
    /// The on/off last turned into a switch.
    last_on: bool,
    was_playing: bool,
    // While on (or fading out).
    engaged: bool,
    mode: TransitionMode,
    from: i64,
    /// Where it fades back to the input (i64::MAX: not yet known).
    until: i64,
    // A tape.
    tape_frames: f64,
    tape_curve: f64,
    // A repeat: the piece, from input frame `piece_from`.
    piece: [Vec<f32>; 2],
    piece_from: i64,
    piece_filled: usize,
    piece_wanted: usize,
    current: Repeat,
    previous: Option<Repeat>,
    frames_per_beat: f64,
    // The repeats' or the cut's filter.
    filter: [Svf; 2],
    cutoff: f32,
}

impl Transition {
    /// Allocates its history and piece; call off the audio thread.
    pub fn new(params: TransitionParams, rate: f32) -> Self {
        let size = ((HISTORY_SECONDS * rate) as usize).next_power_of_two();
        let piece = (SLICE_SECONDS * rate) as usize + 1024;
        let idle = Repeat { index: 0, start: 0.0, length: 1.0, rate: 1.0, gain: 1.0, sounds: 1.0, full: true, cutoff: 1000.0 };
        Self {
            params,
            rate,
            history: [vec![0.0; size], vec![0.0; size]],
            mask: size - 1,
            written: 0,
            edges: [Edge::default(); MAX_EDGES],
            edge_count: 0,
            last_on: false,
            was_playing: false,
            engaged: false,
            mode: params.mode,
            from: 0,
            until: i64::MAX,
            tape_frames: 1.0,
            tape_curve: 1.0,
            piece: [vec![0.0; piece], vec![0.0; piece]],
            piece_from: 0,
            piece_filled: 0,
            piece_wanted: 0,
            current: idle,
            previous: None,
            frames_per_beat: 1.0,
            filter: Default::default(),
            cutoff: 1000.0,
        }
    }

    pub fn latency(&self) -> usize {
        LATENCY
    }

    pub fn reset(&mut self) {
        self.history.iter_mut().for_each(|h| h.fill(0.0));
        self.written = 0;
        self.edge_count = 0;
        self.last_on = false;
        self.was_playing = false;
        self.engaged = false;
        self.previous = None;
        self.filter = Default::default();
    }

    /// Edge fade, frames (at least one; at most a sub-block, see [`LATENCY`]).
    fn edge_frames(&self) -> f64 {
        (EDGE_SECONDS * self.rate).round().clamp(1.0, SUB_BLOCK as f32) as f64
    }

    fn push_edge(&mut self, edge: Edge) -> bool {
        if self.edge_count == MAX_EDGES {
            return false;
        }
        // In order: never before the one waiting last.
        let at = match self.edge_count {
            0 => edge.at,
            n => edge.at.max(self.edges[n - 1].at + 1),
        };
        self.edges[self.edge_count] = Edge { at, ..edge };
        self.edge_count += 1;
        true
    }

    fn pop_edge(&mut self) {
        self.edges.copy_within(1..self.edge_count, 0);
        self.edge_count -= 1;
    }

    /// Turns «Active» changing (this block's first frame) into a switch.
    fn listen(&mut self, ctx: &ProcessContext) {
        let playing = ctx.beats_per_sample > 0.0;
        if playing != self.was_playing {
            if playing {
                // Starting: on from the first frame, if it is.
                self.last_on = false;
            } else {
                // Stopped: the lane no longer plays, so it lets go, and waits
                // to be switched again.
                self.edge_count = 0;
                if self.engaged {
                    let at = self.written - LATENCY as i64 + self.edge_frames() as i64;
                    self.until = self.until.min(at);
                }
                self.last_on = self.params.on();
            }
        }
        let on = self.params.on();
        if on != self.last_on {
            let frames_per_beat = if playing { 1.0 / ctx.beats_per_sample } else { 60.0 * self.rate as f64 / ctx.tempo.max(1.0) };
            let (mut at, mut beat) = (self.written, ctx.beat);
            // Seen just after a line: it was drawn on it (an automation step
            // is seen at the next sub-block; a whole sub-block late when the
            // transport's position, added up block by block, falls a hair
            // short of a line drawn on one).
            if playing && self.was_playing {
                let line = (ctx.beat / SNAP_BEATS + 1e-9).floor() * SNAP_BEATS;
                let back = (ctx.beat - line) * frames_per_beat;
                if back < SUB_BLOCK as f64 + 0.5 {
                    at -= back.round() as i64;
                    beat = line;
                }
            }
            if self.push_edge(Edge { at, on, beat, frames_per_beat, playing }) {
                self.last_on = on;
            }
        }
        self.was_playing = playing;
    }

    fn read(&self, channel: usize, at: f64) -> f32 {
        let i = at.floor();
        let t = (at - i) as f32;
        let i = i as i64;
        let h = &self.history[channel];
        let get = |k: i64| h[(k as usize) & self.mask];
        hermite(get(i - 1), get(i), get(i + 1), get(i + 2), t)
    }

    fn read_piece(&self, channel: usize, at: f64) -> f32 {
        let i = at.floor();
        let t = (at - i) as f32;
        let i = i as i64;
        let p = &self.piece[channel];
        let last = self.piece_filled as i64 - 1;
        let get = |k: i64| if (0..=last).contains(&k) { p[k as usize] } else { 0.0 };
        hermite(get(i - 1), get(i), get(i + 1), get(i + 2), t)
    }

    /// Switches on at input frame `at`.
    fn engage(&mut self, edge: Edge) {
        let p = self.params;
        self.engaged = true;
        self.mode = p.mode;
        self.from = edge.at;
        self.until = i64::MAX;
        self.frames_per_beat = edge.frames_per_beat;
        self.filter = Default::default();
        self.cutoff = p.filter_hz;
        let history = (self.mask + 1) as f64;
        match p.mode {
            TransitionMode::TapeStop | TransitionMode::TapeStart => {
                let most = history - (LATENCY + 4096) as f64;
                self.tape_frames = (p.length_beats.max(1.0 / 64.0) * edge.frames_per_beat).clamp(64.0, most);
                self.tape_curve = 3f64.powf(p.curve.clamp(-1.0, 1.0) as f64);
            }
            TransitionMode::Repeat => {
                let fade = (REPEAT_FADE_SECONDS * self.rate) as f64;
                let room = (self.piece[0].len() - 8) as f64 - fade;
                let grid = p.grid_beats.max(1.0 / 64.0);
                let length = (grid * edge.frames_per_beat).clamp(4.0 * fade, room);
                // From the grid line it falls after.
                let start = if edge.playing {
                    let line = (edge.beat / grid + 1e-9).floor() * grid;
                    edge.at as f64 - ((edge.beat - line) * edge.frames_per_beat).round()
                } else {
                    edge.at as f64
                };
                let start = start.max((edge.at - (self.mask as i64) / 2) as f64);
                self.piece_from = start as i64;
                self.piece_filled = 0;
                self.piece_wanted = (length + fade).ceil() as usize + 4;
                self.current = Repeat { index: 0, start, length, rate: 1.0, gain: 1.0, sounds: length, full: true, cutoff: p.filter_hz };
                self.previous = None;
                self.fill_piece();
            }
            TransitionMode::Cut => {}
        }
    }

    /// Copies what has come of the piece so far.
    fn fill_piece(&mut self) {
        let have = (self.written - self.piece_from).max(0) as usize;
        let until = have.min(self.piece_wanted);
        for k in self.piece_filled..until {
            let at = (self.piece_from + k as i64) as usize & self.mask;
            self.piece[0][k] = self.history[0][at];
            self.piece[1][k] = self.history[1][at];
        }
        self.piece_filled = self.piece_filled.max(until);
    }

    /// The next repeat after `last`, with the settings now.
    fn next_repeat(&self, last: &Repeat) -> Repeat {
        let p = self.params;
        let fade = (REPEAT_FADE_SECONDS * self.rate) as f64;
        let index = last.index + 1;
        let room = (self.piece_wanted - 4) as f64 - fade;
        let length = (p.grid_beats.max(1.0 / 64.0) * self.frames_per_beat).clamp(4.0 * fade, room);
        let step = 2f64.powf(p.pitch.clamp(-12.0, 12.0) as f64 / 12.0);
        let rate = (if index == 1 { step } else { last.rate * step }).clamp(1.0 / 16.0, 16.0);
        let gain = if index == 1 { db_to_gain(-p.decay_db.max(0.0)) } else { last.gain * db_to_gain(-p.decay_db.max(0.0)) };
        let cutoff = if index == 1 { p.filter_hz } else { last.cutoff * 2f32.powf(p.sweep) };
        let gate = p.gate.clamp(0.05, 1.0) as f64;
        // Read faster, the piece runs out before the step ends.
        let sounds = (gate * length).min(room / rate).max(2.0 * fade);
        let full = gate >= 1.0 && rate <= 1.0 && sounds >= length;
        Repeat { index, start: last.start + last.length, length, rate, gain, sounds, full, cutoff: cutoff.clamp(20.0, 20000.0) }
    }

    /// What `repeat` gives at input frame `now` (both channels), if anything.
    fn repeat_at(&self, repeat: &Repeat, now: f64, fade: f64) -> Option<[f32; 2]> {
        let phase = now - repeat.start;
        if phase < 0.0 {
            return None;
        }
        let fade_out = |left: f64| (FRAC_PI_2 * (left / fade).clamp(0.0, 1.0)).sin().powi(2);
        let level = if repeat.full {
            if phase >= repeat.length + fade {
                return None;
            }
            if phase >= repeat.length { fade_out(repeat.length + fade - phase) } else { 1.0 }
        } else {
            if phase >= repeat.sounds {
                return None;
            }
            fade_out(repeat.sounds - phase)
        };
        // The first is the sound itself: it doesn't fade in.
        let level = if repeat.index == 0 { level } else { level * fade_out(phase) };
        let at = phase * repeat.rate;
        let g = level as f32 * repeat.gain;
        Some([self.read_piece(0, at) * g, self.read_piece(1, at) * g])
    }

    fn filtered(&mut self, x: [f32; 2], cutoff: f32) -> [f32; 2] {
        let Some(mode) = self.params.filter else { return x };
        // Glides to the next cutoff in about 3 ms.
        let glide = 1.0 - (-1.0 / (0.003 * self.rate)).exp();
        self.cutoff *= (cutoff.max(20.0) / self.cutoff.max(20.0)).powf(glide);
        let k = 2.0 - 2.0 * self.params.resonance.clamp(0.0, 0.9);
        let c = SvfCoefs::new(self.cutoff, k, self.rate);
        let mut out = [0.0; 2];
        for ch in 0..2 {
            let (low, band, high) = self.filter[ch].tick(x[ch], &c);
            out[ch] = match mode {
                FilterMode::LowPass => low,
                FilterMode::HighPass => high,
                FilterMode::BandPass => band * k,
            };
        }
        out
    }

    /// The effect's sound at input frame `now` (while engaged), dry being the input there.
    fn wet(&mut self, now: i64, dry: [f32; 2]) -> [f32; 2] {
        let p = self.params;
        let x = (now - self.from) as f64;
        match self.mode {
            TransitionMode::TapeStop | TransitionMode::TapeStart => {
                let (d, c) = (self.tape_frames, self.tape_curve);
                let u = x / d;
                let stop = self.mode == TransitionMode::TapeStop;
                if u >= 1.0 {
                    if stop {
                        return [0.0; 2];
                    }
                    // Up to speed, but late: across to the live sound.
                    let late = d / (c + 1.0);
                    let y = (x - d) / (CATCH_UP_SECONDS * self.rate) as f64;
                    if y >= 1.0 {
                        return dry;
                    }
                    let (a, b) = ((FRAC_PI_2 * y).cos().powi(2) as f32, (FRAC_PI_2 * y).sin().powi(2) as f32);
                    let at = now as f64 - late;
                    return [self.read(0, at) * a + dry[0] * b, self.read(1, at) * a + dry[1] * b];
                }
                let rest = (1.0 - u).powf(c + 1.0);
                let (speed, travelled) = if stop { ((1.0 - u).powf(c), (1.0 - rest) / (c + 1.0)) } else { (1.0 - (1.0 - u).powf(c), u - (1.0 - rest) / (c + 1.0)) };
                let at = self.from as f64 + d * travelled;
                let level = smoothstep(speed / QUIET_SPEED) as f32;
                [self.read(0, at) * level, self.read(1, at) * level]
            }
            TransitionMode::Repeat => {
                self.fill_piece();
                let fade = (REPEAT_FADE_SECONDS * self.rate) as f64;
                let now = now as f64;
                while now >= self.current.start + self.current.length {
                    let next = self.next_repeat(&self.current);
                    self.previous = self.current.full.then_some(self.current);
                    self.current = next;
                }
                let mut plain = [0.0f32; 2];
                let mut repeated = [0.0f32; 2];
                for repeat in [self.previous, Some(self.current)].into_iter().flatten() {
                    if let Some(s) = self.repeat_at(&repeat, now, fade) {
                        let into = if repeat.index == 0 { &mut plain } else { &mut repeated };
                        into[0] += s[0];
                        into[1] += s[1];
                    }
                }
                let repeated = if self.current.index > 0 { self.filtered(repeated, self.current.cutoff) } else { repeated };
                let wet = [plain[0] + repeated[0], plain[1] + repeated[1]];
                let mix = p.mix.clamp(0.0, 1.0);
                [lerp(dry[0], wet[0], mix), lerp(dry[1], wet[1], mix)]
            }
            TransitionMode::Cut => {
                let kept = if p.filter.is_some() { self.filtered(dry, p.filter_hz) } else { [0.0; 2] };
                let mix = p.mix.clamp(0.0, 1.0);
                [lerp(dry[0], kept[0], mix), lerp(dry[1], kept[1], mix)]
            }
        }
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], ctx: &ProcessContext) {
        self.listen(ctx);
        let edge = self.edge_frames();
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let at = self.written as usize & self.mask;
            self.history[0][at] = *a;
            self.history[1][at] = *b;
            self.written += 1;
            let now = self.written - 1 - LATENCY as i64;
            let dry = [self.history[0][now as usize & self.mask], self.history[1][now as usize & self.mask]];
            // The switches due now; a switch off waits ahead (its fade ends on it).
            while self.edge_count > 0 {
                let e = self.edges[0];
                if e.on {
                    if e.at > now {
                        break;
                    }
                    self.engage(e);
                } else if self.engaged && self.until == i64::MAX {
                    self.until = e.at;
                } else if self.engaged {
                    break;
                }
                // (A switch off with nothing on is nothing.)
                self.pop_edge();
            }
            if self.engaged && now >= self.until {
                self.engaged = false;
                self.previous = None;
            }
            if !self.engaged {
                (*a, *b) = (dry[0], dry[1]);
                continue;
            }
            let wet = self.wet(now, dry);
            let rise = ((now - self.from + 1) as f64 / edge).clamp(0.0, 1.0);
            let fall = if self.until == i64::MAX { 1.0 } else { ((self.until - now) as f64 / edge).clamp(0.0, 1.0) };
            let w = (rise * fall) as f32;
            *a = lerp(dry[0], wet[0], w);
            *b = lerp(dry[1], wet[1], w);
        }
    }
}

/// From `a` (t = 0) to `b` (t = 1), each exactly at its end.
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    if t >= 1.0 { b } else { a + (b - a) * t }
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// 4-point, third-order Hermite between `y1` and `y2`.
fn hermite(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

#[cfg(test)]
mod tests;
