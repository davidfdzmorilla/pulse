//! How each sound of «Sonidos de Pulse» is made (see `factory_sounds`): Pulse's
//! own instruments (the drum machine's three models, the kick designer, the
//! FM, Acid, Super Saw, Keys and Wavetable synths) played offline, layered and
//! run through Pulse's own effects. Nothing here is a recording: every sound
//! comes out the same from the same code.

use crate::project::{Project, Track};
use crate::sync::{self, SampleCache};
use engine::model::{
    ArrangementClip, BitcrusherParams, Clip, CompressorParams, ConvolutionParams, DelayParams, DrumModel, DrumParams, Effect, EqBand, EqEightParams, EqShape, FmParams, InstrumentKind, KickDrive, KickParams,
    MidiClip, Note, ReverbSpace, SaturatorParams, TransientParams, DRUM_SOUNDS, DRUM_VOICES,
};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

pub const RATE: f32 = 48_000.0;

pub type Stereo = (Vec<f32>, Vec<f32>);

/// What a sound is, for how loud it is made and what is checked of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Kick,
    /// Snares and claps.
    Snare,
    /// Rims, snaps and clicks: short and high.
    Rim,
    /// Closed and open hats.
    Hat,
    /// Crashes and rides.
    Cymbal,
    Perc,
    /// 808s and subs, tuned.
    Sub,
    /// Bass one-shots, tuned.
    Bass,
    /// Chord stabs, their root tuned.
    Stab,
    Fx,
}

/// One sound of the library.
pub struct Recipe {
    pub folder: &'static str,
    pub name: &'static str,
    pub role: Role,
    /// The MIDI note its name says it is in (the root, for a chord): what
    /// the tests measure its tuning against.
    #[cfg_attr(not(test), allow(dead_code))]
    pub note: Option<u8>,
    /// Exactly this many seconds long (a riser that ends on the drop), if set.
    pub length: Option<f32>,
    pub make: fn() -> Stereo,
}

pub const KICKS: &str = "Bombos";
pub const SNARES: &str = "Cajas y palmas";
pub const HATS: &str = "Charles y platos";
pub const PERC: &str = "Percusión";
pub const SUBS: &str = "808 y subs";
pub const BASSES: &str = "Bajos";
pub const STABS: &str = "Stabs y acordes";
pub const FX: &str = "FX";

/// The drum machine's sounds, by their place in `DRUM_VOICES`.
const KICK: usize = 0;
const RIM: usize = 1;
const SNARE: usize = 2;
const CLAP: usize = 3;
const CH: usize = 4;
const OH: usize = 5;
const LOW_TOM: usize = 6;
const BELL: usize = 7;
const MID_TOM: usize = 8;
const HIGH_TOM: usize = 9;
const CRASH: usize = 10;
const RIDE: usize = 11;
const SHAKER: usize = 12;
const TAMB: usize = 13;
const CONGA: usize = 14;
const CLAVE: usize = 15;

use DrumModel::{Classic, Tr808, Tr909};

// ---------------------------------------------------------------- sources

/// One sound of the drum machine, alone and at full level.
fn dm(model: DrumModel, voice: usize, tune: f32, decay: f32, drive: f32) -> Stereo {
    let mut p = DrumParams { model, level: [0.0; DRUM_SOUNDS], ..DrumParams::default() };
    p.level[voice] = 1.0;
    p.tune[voice] = tune;
    p.decay[voice] = decay;
    p.drive[voice] = drive;
    engine::render_drum_hit(p, DRUM_VOICES[voice].0, RATE)
}

/// A hit of the kick designer.
fn kick(p: KickParams) -> Stereo {
    let body = engine::render_kick(p, None, RATE, 120.0);
    (body.clone(), body)
}

/// Notes (start and length in beats, at 120 BPM) played by an instrument
/// through the engine, as a track of a project would play them, with its tail.
pub fn play(kind: InstrumentKind, notes: &[(f64, u8, f32, f64)], beats: f64) -> Stereo {
    let mut project = Project::empty();
    project.tempo = 120.0;
    let mut track = Track::new("Sonido", [0, 0, 0], kind);
    let mut clip = MidiClip::new("Sonido", beats, [0, 0, 0]);
    for &(start, pitch, velocity, length) in notes {
        clip.notes.push(Note { start, length, pitch, velocity, chance: 1.0 });
    }
    track.arrangement.push(ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) });
    project.tracks = vec![track];
    render_project(&project, beats)
}

/// A project rendered from its start for `beats`, with its tail.
pub fn render_project(project: &Project, beats: f64) -> Stereo {
    let commands = sync::project_commands(project, &mut SampleCache::default(), RATE, &HashSet::new());
    crate::export::render(commands, (0.0, beats), crate::export::Song::of(project), RATE, &AtomicBool::new(false), &|_| {}).unwrap_or_default()
}

/// A factory preset of an instrument, by the instrument's name and its own.
fn preset(instrument: &str, name: &str) -> InstrumentKind {
    InstrumentKind::catalog()
        .into_iter()
        .filter(|k| k.label() == instrument)
        .flat_map(|k| k.factory_presets())
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no preset «{name}» in {instrument}"))
        .1
}

/// A chord (or a note) struck at once and held `beats`.
fn chord(kind: InstrumentKind, notes: &[u8], velocity: f32, beats: f64) -> Stereo {
    let notes: Vec<(f64, u8, f32, f64)> = notes.iter().map(|&n| (0.0, n, velocity, beats)).collect();
    play(kind, &notes, beats.max(0.25))
}

// ---------------------------------------------------------------- shaping

/// Through effects (with room for their tails).
fn fx(sound: Stereo, effects: &[Effect]) -> Stereo {
    engine::render_effects(effects, &sound.0, &sound.1, RATE, 120.0, 2.5)
}

/// Layers: (sound, gain in dB, delay in ms, pan -1..1).
fn layer(parts: Vec<(Stereo, f32, f32, f32)>) -> Stereo {
    let length = parts.iter().map(|(s, _, delay, _)| s.0.len() + (delay * 0.001 * RATE) as usize).max().unwrap_or(0);
    let (mut l, mut r) = (vec![0.0; length], vec![0.0; length]);
    for ((left, right), db, delay, pan) in parts {
        let gain = 10f32.powf(db / 20.0);
        // Constant power, unity in the centre.
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
        let (gl, gr) = (angle.cos() * std::f32::consts::SQRT_2 * gain, angle.sin() * std::f32::consts::SQRT_2 * gain);
        let at = (delay * 0.001 * RATE) as usize;
        for (i, (a, b)) in left.iter().zip(&right).enumerate() {
            l[at + i] += a * gl;
            r[at + i] += b * gr;
        }
    }
    (l, r)
}

/// Played backwards.
fn reverse((mut l, mut r): Stereo) -> Stereo {
    // Its silent end first would be a gap: from where it sounds.
    let peak = l.iter().chain(&r).fold(0f32, |m, s| m.max(s.abs()));
    let last = l.iter().zip(&r).rposition(|(a, b)| a.abs().max(b.abs()) > peak * 1e-3).map_or(0, |i| i + 1);
    l.truncate(last);
    r.truncate(last);
    l.reverse();
    r.reverse();
    (l, r)
}

/// Open for `open_ms`, then shut over `close_ms` (a gated reverb).
fn gate((mut l, mut r): Stereo, open_ms: f32, close_ms: f32) -> Stereo {
    let open = (open_ms * 0.001 * RATE) as usize;
    let close = ((close_ms * 0.001 * RATE) as usize).max(1);
    let end = (open + close).min(l.len());
    for i in open..end {
        let x = (i - open) as f32 / close as f32;
        let g = 0.5 + 0.5 * (x * std::f32::consts::PI).cos();
        l[i] *= g;
        r[i] *= g;
    }
    l.truncate(end);
    r.truncate(end);
    (l, r)
}

// ---------------------------------------------------------------- effects

fn eq(bands: &[(EqShape, f32, f32, f32)]) -> Effect {
    let mut p = EqEightParams::default();
    for band in p.bands.iter_mut() {
        band.on = false;
    }
    for (i, &(shape, freq, gain_db, q)) in bands.iter().enumerate() {
        p.bands[i] = EqBand { on: true, shape, freq, gain_db, q };
    }
    Effect::EqEight(p)
}

fn low_cut(hz: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::LowCut12, hz, 0.0, 0.71)
}

fn steep_low_cut(hz: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::LowCut48, hz, 0.0, 0.71)
}

fn high_cut(hz: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::HighCut12, hz, 0.0, 0.71)
}

fn bell(hz: f32, db: f32, q: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::Bell, hz, db, q)
}

fn low_shelf(hz: f32, db: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::LowShelf, hz, db, 0.71)
}

fn high_shelf(hz: f32, db: f32) -> (EqShape, f32, f32, f32) {
    (EqShape::HighShelf, hz, db, 0.71)
}

fn drive(kind: KickDrive, drive_db: f32) -> Effect {
    Effect::Saturator(SaturatorParams { drive_db, output_db: -drive_db * 0.5, mix: 1.0, kind, oversample: true })
}

fn crush(bits: f32, downsample: f32, mix: f32) -> Effect {
    Effect::Bitcrusher(BitcrusherParams { bits, downsample, mix })
}

/// A space of the convolution reverb: decay scale, mix and high cut.
fn space(space: ReverbSpace, decay: f32, mix: f32, high_cut_hz: f32) -> Effect {
    Effect::Convolution(ConvolutionParams { space, decay, size: 1.0, predelay_ms: 0.0, low_cut_hz: 180.0, high_cut_hz, width: 1.0, mix })
}

fn transient(attack: f32, sustain: f32) -> Effect {
    Effect::Transient(TransientParams { attack, sustain, output_db: 0.0 })
}

fn squeeze(threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32) -> Effect {
    Effect::Compressor(CompressorParams { threshold_db, ratio, attack_ms, release_ms, makeup_db: 0.0, sidechain: None })
}

fn echo(time_beats: f64, feedback: f32, mix: f32, filter_hz: f32) -> Effect {
    Effect::Delay(DelayParams { time_beats, feedback, mix, ping_pong: true, filter_hz })
}

// ---------------------------------------------------------------- kits' tones

/// Lo-fi: a dull, crushed, saturated hit.
fn dusty(sound: Stereo, top: f32) -> Stereo {
    fx(sound, &[drive(KickDrive::Tube, 8.0), crush(10.0, 3.0, 0.7), eq(&[low_cut(40.0), high_cut(top), bell(250.0, 2.0, 0.8)]), space(ReverbSpace::Room, 0.6, 0.12, 5000.0)])
}

/// Boom bap: knocking, warm, with a touch of room.
fn knock(sound: Stereo, top: f32) -> Stereo {
    fx(sound, &[drive(KickDrive::Tube, 6.0), squeeze(-18.0, 4.0, 8.0, 90.0), eq(&[low_cut(35.0), high_cut(top)]), space(ReverbSpace::Room, 0.7, 0.1, 9000.0)])
}

/// Techno: driven and metallic.
fn metal(sound: Stereo, db: f32) -> Stereo {
    fx(sound, &[drive(KickDrive::Fold, db), eq(&[low_cut(300.0), bell(6000.0, 3.0, 1.2)])])
}

fn fm(ratio: f32, index: f32, mod_decay: f32, feedback: f32, decay: f32) -> InstrumentKind {
    InstrumentKind::Fm(FmParams { ratio, index, mod_decay, mod_sustain: 0.0, feedback, attack: 0.001, decay, sustain: 0.0, release: 0.05, gain: 0.5 })
}

// ---------------------------------------------------------------- the library

fn r(folder: &'static str, name: &'static str, role: Role, make: fn() -> Stereo) -> Recipe {
    Recipe { folder, name, role, note: None, length: None, make }
}

fn tuned(folder: &'static str, name: &'static str, role: Role, note: u8, make: fn() -> Stereo) -> Recipe {
    Recipe { folder, name, role, note: Some(note), length: None, make }
}

/// MIDI notes of C (Do) used for tuned sounds: C1 for 808s and basses, C3 for chords.
pub const C1: u8 = 36;
pub const C3: u8 = 60;

pub fn recipes() -> Vec<Recipe> {
    use Role::*;
    vec![
        // ---------------- Bombos
        r(KICKS, "Bombo house", Kick, || {
            let p = KickParams { tune: 33.0, pitch_amount: 36.0, pitch_time: 45.0, hold: 40.0, decay: 230.0, click: 0.5, click_tone: 4000.0, drive: 0.25, phase: 90.0, punch: 0.3, click_type: 2, ..KickParams::default() };
            fx(kick(p), &[eq(&[low_cut(28.0), bell(380.0, -3.0, 1.0)])])
        }),
        r(KICKS, "Bombo house profundo", Kick, || {
            let p = KickParams { tune: 31.0, pitch_amount: 30.0, pitch_time: 55.0, pitch_curve: 0.5, hold: 60.0, decay: 340.0, decay_curve: 0.3, click: 0.2, click_tone: 1800.0, drive: 0.1, sub_level: 0.3, eq_high: -3.0, ..KickParams::default() };
            kick(p)
        }),
        r(KICKS, "Bombo techno saturado", Kick, || {
            let p = KickParams { tune: 29.0, pitch_amount: 44.0, pitch_time: 50.0, hold: 60.0, decay: 360.0, decay_curve: 0.5, harmonics: 0.2, click: 0.55, click_tone: 5000.0, click_type: 5, drive: 0.6, drive_mode: KickDrive::Hard, phase: 90.0, punch: 0.4, ..KickParams::default() };
            fx(kick(p), &[drive(KickDrive::Hard, 6.0), eq(&[low_cut(30.0), bell(90.0, 2.0, 1.0), bell(420.0, -4.0, 1.2), high_shelf(5000.0, 2.0)])])
        }),
        r(KICKS, "Bombo techno con rumble", Kick, || {
            let p = KickParams { tune: 29.0, pitch_amount: 40.0, pitch_time: 60.0, hold: 70.0, decay: 300.0, harmonics: 0.15, click: 0.45, click_type: 1, drive: 0.5, phase: 90.0, punch: 0.3, ..KickParams::default() };
            let dry = kick(p);
            // The rumble: the kick through a dark room, driven and low-passed.
            let rumble = fx(dry.clone(), &[Effect::Convolution(ConvolutionParams { space: ReverbSpace::Hall, decay: 0.9, size: 0.8, predelay_ms: 20.0, low_cut_hz: 30.0, high_cut_hz: 400.0, width: 0.0, mix: 1.0 }), drive(KickDrive::Tube, 14.0), eq(&[low_cut(35.0), high_cut(220.0)])]);
            layer(vec![(dry, 0.0, 0.0, 0.0), (rumble, -6.0, 0.0, 0.0)])
        }),
        r(KICKS, "Bombo trap", Kick, || {
            let p = KickParams { tune: 31.0, pitch_amount: 40.0, pitch_time: 30.0, pitch_curve: 0.7, hold: 30.0, decay: 200.0, click: 0.35, click_tone: 3500.0, click_type: 1, drive: 0.3, punch: 0.5, phase: 90.0, ..KickParams::default() };
            fx(kick(p), &[eq(&[low_cut(30.0)])])
        }),
        r(KICKS, "Bombo drum and bass", Kick, || {
            let p = KickParams { tune: 38.0, pitch_amount: 36.0, pitch_time: 25.0, pitch_curve: 0.7, hold: 20.0, decay: 180.0, decay_curve: 0.5, harmonics: 0.1, click: 0.7, click_tone: 4500.0, drive: 0.45, click_type: 8, phase: 90.0, punch: 0.5, ..KickParams::default() };
            fx(kick(p), &[transient(0.4, -0.2), eq(&[low_cut(40.0), bell(3500.0, 2.0, 1.0)])])
        }),
        r(KICKS, "Bombo lo-fi", Kick, || {
            let p = KickParams { tune: 30.0, pitch_amount: 26.0, pitch_time: 50.0, pitch_curve: 0.4, hold: 40.0, decay: 260.0, harmonics: 0.35, click: 0.35, click_tone: 900.0, click_length: 12.0, drive: 0.4, click_type: 11, drive_mode: KickDrive::Crush, eq_high: -6.0, ..KickParams::default() };
            dusty(kick(p), 3500.0)
        }),
        r(KICKS, "Bombo garage", Kick, || fx(dm(Tr909, KICK, 1.0, 0.45, 0.1), &[transient(0.3, -0.4), eq(&[low_cut(30.0), bell(60.0, 2.0, 1.0)])])),
        r(KICKS, "Bombo afro", Kick, || {
            let p = KickParams { tune: 31.0, pitch_amount: 24.0, pitch_time: 60.0, hold: 50.0, decay: 300.0, decay_curve: 0.3, click: 0.25, click_type: 6, drive: 0.15, drive_mode: KickDrive::Tube, sub_level: 0.3, ..KickParams::default() };
            fx(kick(p), &[eq(&[low_cut(28.0), high_cut(9000.0)])])
        }),
        r(KICKS, "Bombo ochentero", Kick, || fx(dm(Tr909, KICK, -1.0, 0.7, 0.15), &[eq(&[low_cut(30.0), bell(100.0, 2.0, 0.9)]), space(ReverbSpace::Ambience, 1.0, 0.18, 8000.0)])),
        r(KICKS, "Bombo minimal", Kick, || {
            let p = KickParams { tune: 34.0, pitch_amount: 50.0, pitch_time: 15.0, pitch_curve: 0.85, hold: 15.0, decay: 140.0, decay_curve: 0.7, click: 0.8, click_tone: 6500.0, click_length: 3.0, drive: 0.3, click_type: 2, punch: 0.3, ..KickParams::default() };
            kick(p)
        }),
        r(KICKS, "Bombo boom bap", Kick, || {
            let p = KickParams { tune: 30.0, pitch_amount: 30.0, pitch_time: 45.0, hold: 50.0, decay: 280.0, harmonics: 0.3, click: 0.3, click_type: 8, click_tone: 2500.0, drive: 0.35, drive_mode: KickDrive::Tube, ..KickParams::default() };
            fx(kick(p), &[drive(KickDrive::Tube, 6.0), eq(&[low_cut(35.0), high_cut(7000.0), bell(120.0, 2.0, 1.0)]), space(ReverbSpace::Room, 0.7, 0.08, 6000.0)])
        }),
        r(KICKS, "Bombo electro", Kick, || fx(dm(Tr808, KICK, 0.0, 0.8, 0.1), &[eq(&[low_cut(25.0)])])),
        // ---------------- Cajas y palmas
        r(SNARES, "Caja house", Snare, || fx(dm(Tr909, SNARE, 0.0, 1.0, 0.0), &[eq(&[low_cut(120.0)]), space(ReverbSpace::Studio, 0.8, 0.15, 11000.0)])),
        r(SNARES, "Palmada house", Snare, || fx(dm(Tr909, CLAP, 0.0, 1.2, 0.0), &[eq(&[low_cut(250.0)]), space(ReverbSpace::Room, 1.0, 0.25, 12000.0)])),
        r(SNARES, "Aro house", Rim, || fx(dm(Tr909, RIM, 0.0, 1.0, 0.0), &[space(ReverbSpace::Room, 0.8, 0.15, 12000.0)])),
        r(SNARES, "Caja techno", Snare, || fx(dm(Tr909, SNARE, -1.0, 0.8, 0.3), &[drive(KickDrive::Hard, 6.0), eq(&[low_cut(150.0), bell(3000.0, 2.0, 1.0)]), space(ReverbSpace::Plate, 0.5, 0.15, 10000.0)])),
        r(SNARES, "Palmada techno", Snare, || fx(dm(Tr909, CLAP, -1.0, 1.0, 0.1), &[eq(&[low_cut(300.0)]), space(ReverbSpace::Plate, 0.8, 0.35, 9000.0)])),
        r(SNARES, "Aro techno", Rim, || metal(dm(Tr909, RIM, 2.0, 0.8, 0.4), 9.0)),
        r(SNARES, "Caja trap", Snare, || {
            let body = dm(Tr808, SNARE, 3.0, 0.7, 0.0);
            let snap = dm(Tr909, CLAP, 2.0, 0.5, 0.0);
            fx(layer(vec![(body, 0.0, 0.0, 0.0), (snap, -4.0, 0.0, 0.0)]), &[transient(0.6, -0.3), eq(&[low_cut(150.0), bell(220.0, 2.0, 1.0), high_shelf(7000.0, 3.0)])])
        }),
        r(SNARES, "Palmada trap", Snare, || {
            let a = dm(Tr808, CLAP, 1.0, 0.7, 0.0);
            let b = dm(Tr909, CLAP, 2.0, 0.5, 0.0);
            fx(layer(vec![(a.clone(), 0.0, 0.0, -0.3), (b, -2.0, 6.0, 0.3), (a, -6.0, 13.0, 0.0)]), &[transient(0.5, 0.0), eq(&[low_cut(400.0), high_shelf(6000.0, 2.0)]), space(ReverbSpace::Room, 0.6, 0.12, 12000.0)])
        }),
        r(SNARES, "Chasquido", Rim, || fx(dm(Tr909, CLAP, 7.0, 0.3, 0.0), &[eq(&[low_cut(1200.0), bell(3000.0, 3.0, 1.5)]), space(ReverbSpace::Room, 0.6, 0.15, 12000.0)])),
        r(SNARES, "Caja drum and bass", Snare, || {
            let body = dm(Tr909, SNARE, 4.0, 0.7, 0.2);
            let crack = dm(Tr909, RIM, 3.0, 0.6, 0.2);
            fx(layer(vec![(body, 0.0, 0.0, 0.0), (crack, -8.0, 0.0, 0.0)]), &[transient(0.7, -0.3), eq(&[low_cut(140.0), bell(220.0, 3.0, 1.0), bell(5000.0, 2.0, 0.8)]), space(ReverbSpace::Plate, 0.5, 0.12, 12000.0)])
        }),
        r(SNARES, "Caja suave drum and bass", Snare, || fx(dm(Tr909, SNARE, 4.0, 0.4, 0.0), &[eq(&[low_cut(200.0), high_cut(6000.0)])])),
        r(SNARES, "Caja lo-fi", Snare, || dusty(dm(Tr808, SNARE, -3.0, 0.8, 0.0), 5000.0)),
        r(SNARES, "Aro lo-fi", Rim, || dusty(dm(Tr808, RIM, -2.0, 1.0, 0.0), 5000.0)),
        r(SNARES, "Caja garage", Snare, || fx(dm(Tr909, SNARE, 2.0, 0.7, 0.0), &[transient(0.1, -0.2), eq(&[low_cut(160.0), high_shelf(8000.0, 2.0)]), space(ReverbSpace::Studio, 0.6, 0.1, 12000.0)])),
        r(SNARES, "Aro garage", Rim, || fx(dm(Tr909, RIM, 2.0, 1.0, 0.0), &[space(ReverbSpace::Studio, 0.7, 0.15, 12000.0)])),
        r(SNARES, "Palmada afro", Snare, || fx(dm(Tr808, CLAP, 0.0, 1.0, 0.0), &[eq(&[low_cut(250.0)]), space(ReverbSpace::Room, 1.2, 0.3, 9000.0)])),
        r(SNARES, "Caja ochentera", Snare, || {
            // A gated reverb: snare and clap in a big plate, shut after a quarter second.
            let hit = layer(vec![(dm(Tr909, SNARE, -1.0, 1.6, 0.05), 0.0, 0.0, 0.0), (dm(Tr909, CLAP, -1.0, 1.0, 0.0), -5.0, 0.0, 0.0)]);
            let wet = fx(hit, &[eq(&[low_cut(120.0), bell(200.0, 3.0, 1.0)]), Effect::Convolution(ConvolutionParams { space: ReverbSpace::Plate, decay: 1.4, size: 1.2, predelay_ms: 0.0, low_cut_hz: 200.0, high_cut_hz: 9000.0, width: 1.0, mix: 0.55 })]);
            gate(wet, 260.0, 40.0)
        }),
        r(SNARES, "Palmada ochentera", Snare, || {
            let wet = fx(dm(Tr909, CLAP, 0.0, 1.2, 0.0), &[eq(&[low_cut(250.0)]), Effect::Convolution(ConvolutionParams { space: ReverbSpace::Hall, decay: 1.2, size: 1.0, predelay_ms: 0.0, low_cut_hz: 250.0, high_cut_hz: 9000.0, width: 1.0, mix: 0.5 })]);
            gate(wet, 220.0, 40.0)
        }),
        r(SNARES, "Clic minimal", Rim, || fx(dm(Tr808, RIM, 5.0, 1.0, 0.0), &[eq(&[low_cut(500.0)]), echo(0.75, 0.25, 0.15, 4000.0)])),
        r(SNARES, "Caja boom bap", Snare, || knock(dm(Classic, SNARE, -2.0, 1.4, 0.05), 8000.0)),
        r(SNARES, "Aro boom bap", Rim, || knock(dm(Classic, RIM, -1.0, 1.0, 0.0), 8000.0)),
        // ---------------- Charles y platos
        r(HATS, "Charles cerrado house", Hat, || dm(Tr909, CH, 0.0, 0.8, 0.0)),
        r(HATS, "Charles abierto house", Hat, || dm(Tr909, OH, 0.0, 0.8, 0.0)),
        r(HATS, "Charles cerrado techno", Hat, || metal(dm(Tr909, CH, 2.0, 0.5, 0.4), 6.0)),
        r(HATS, "Charles abierto techno", Hat, || metal(dm(Tr909, OH, 2.0, 0.6, 0.3), 6.0)),
        r(HATS, "Charles cerrado trap", Hat, || fx(dm(Tr808, CH, 2.0, 0.5, 0.1), &[eq(&[low_cut(500.0), high_shelf(8000.0, 3.0)])])),
        r(HATS, "Charles abierto trap", Hat, || fx(dm(Tr808, OH, 1.0, 0.6, 0.06), &[eq(&[low_cut(500.0), high_shelf(8000.0, 2.0)])])),
        r(HATS, "Charles cerrado drum and bass", Hat, || fx(dm(Tr909, CH, 3.0, 0.4, 0.0), &[eq(&[low_cut(600.0)])])),
        r(HATS, "Charles abierto drum and bass", Hat, || fx(dm(Tr909, OH, 2.0, 0.5, 0.0), &[eq(&[low_cut(600.0)])])),
        r(HATS, "Charles cerrado lo-fi", Hat, || dusty(dm(Tr808, CH, -4.0, 0.8, 0.06), 7000.0)),
        r(HATS, "Charles abierto lo-fi", Hat, || dusty(dm(Tr808, OH, -4.0, 0.7, 0.04), 7000.0)),
        r(HATS, "Charles cerrado garage", Hat, || fx(dm(Tr909, CH, 1.0, 0.6, 0.0), &[eq(&[low_cut(400.0)]), space(ReverbSpace::Ambience, 1.0, 0.1, 14000.0)])),
        r(HATS, "Charles abierto garage", Hat, || fx(dm(Tr909, OH, 1.0, 0.55, 0.0), &[eq(&[low_cut(400.0)])])),
        r(HATS, "Charles cerrado minimal", Hat, || fx(dm(Tr808, CH, 3.0, 0.45, 0.0), &[eq(&[low_cut(1000.0)])])),
        r(HATS, "Charles cerrado boom bap", Hat, || knock(dm(Classic, CH, -1.0, 1.0, 0.1), 9000.0)),
        r(HATS, "Charles abierto boom bap", Hat, || knock(dm(Classic, OH, -2.0, 0.8, 0.0), 9000.0)),
        r(HATS, "Charles cerrado ochentero", Hat, || fx(dm(Tr909, CH, 0.0, 0.7, 0.0), &[eq(&[low_cut(500.0)]), space(ReverbSpace::Plate, 0.6, 0.12, 12000.0)])),
        r(HATS, "Crash brillante", Cymbal, || dm(Tr909, CRASH, 0.0, 1.2, 0.0)),
        r(HATS, "Crash oscuro", Cymbal, || fx(dm(Tr808, CRASH, -3.0, 1.4, 0.0), &[eq(&[low_cut(300.0), high_cut(8000.0)])])),
        r(HATS, "Crash lo-fi", Cymbal, || dusty(dm(Tr808, CRASH, -3.0, 1.0, 0.0), 6000.0)),
        r(HATS, "Ride", Cymbal, || dm(Tr909, RIDE, 0.0, 1.0, 0.0)),
        r(HATS, "Ride seco", Cymbal, || fx(dm(Tr808, RIDE, 2.0, 0.6, 0.0), &[eq(&[low_cut(800.0)])])),
        // ---------------- Percusión
        r(PERC, "Conga grave", Perc, || dm(Tr808, CONGA, -5.0, 1.2, 0.0)),
        r(PERC, "Conga aguda", Perc, || dm(Tr808, CONGA, 2.0, 1.0, 0.0)),
        r(PERC, "Bongó", Perc, || fx(dm(Tr808, CONGA, 7.0, 0.6, 0.0), &[space(ReverbSpace::Room, 0.6, 0.12, 10000.0)])),
        r(PERC, "Cencerro", Perc, || dm(Tr808, BELL, 0.0, 1.0, 0.0)),
        r(PERC, "Cencerro saturado", Perc, || metal(dm(Tr808, BELL, 3.0, 0.8, 0.4), 6.0)),
        r(PERC, "Clave", Perc, || dm(Tr808, CLAVE, 0.0, 1.0, 0.0)),
        r(PERC, "Bloque de madera", Perc, || fx(dm(Tr808, CLAVE, -7.0, 1.5, 0.0), &[space(ReverbSpace::Room, 0.6, 0.12, 10000.0)])),
        r(PERC, "Shaker", Perc, || dm(Tr909, SHAKER, 0.0, 1.0, 0.0)),
        r(PERC, "Shaker corto", Perc, || dm(Tr808, SHAKER, 2.0, 0.5, 0.0)),
        r(PERC, "Pandereta", Perc, || dm(Tr909, TAMB, 0.0, 1.0, 0.0)),
        r(PERC, "Tom grave", Perc, || dm(Tr909, LOW_TOM, 0.0, 1.0, 0.0)),
        r(PERC, "Tom medio", Perc, || dm(Tr909, MID_TOM, 0.0, 1.0, 0.0)),
        r(PERC, "Tom agudo", Perc, || dm(Tr909, HIGH_TOM, 0.0, 1.0, 0.0)),
        r(PERC, "Tom electrónico", Perc, || fx(dm(Tr808, LOW_TOM, 0.0, 1.5, 0.1), &[eq(&[low_cut(40.0)])])),
        r(PERC, "Tom ochentero", Perc, || {
            let wet = fx(dm(Tr909, LOW_TOM, -3.0, 1.6, 0.05), &[Effect::Convolution(ConvolutionParams { space: ReverbSpace::Plate, decay: 1.2, size: 1.1, predelay_ms: 0.0, low_cut_hz: 150.0, high_cut_hz: 8000.0, width: 1.0, mix: 0.45 })]);
            gate(wet, 380.0, 60.0)
        }),
        r(PERC, "Tom lo-fi", Perc, || dusty(dm(Tr808, MID_TOM, -2.0, 1.0, 0.0), 5000.0)),
        r(PERC, "Percusión metálica", Perc, || fx(play(fm(3.17, 6.0, 0.12, 0.2, 0.35), &[(0.0, 76, 1.0, 0.25)], 0.5), &[eq(&[low_cut(300.0)]), space(ReverbSpace::Room, 0.6, 0.15, 12000.0)])),
        r(PERC, "Blip", Perc, || fx(play(fm(2.0, 2.5, 0.04, 0.0, 0.09), &[(0.0, 84, 1.0, 0.1)], 0.25), &[echo(0.75, 0.3, 0.2, 5000.0)])),
        r(PERC, "Gota", Perc, || {
            // A sine whose pitch jumps up as it starts: the kick designer, high up.
            let p = KickParams { tune: 79.0, pitch_amount: -12.0, pitch_time: 25.0, pitch_curve: 0.3, hold: 10.0, decay: 90.0, click: 0.0, drive: 0.0, ..KickParams::default() };
            fx(kick(p), &[space(ReverbSpace::Room, 0.6, 0.15, 12000.0)])
        }),
        // ---------------- 808 y subs
        tuned(SUBS, "808 largo (Do)", Sub, C1, || kick(KickParams { tune: C1 as f32, pitch_amount: 12.0, pitch_time: 40.0, pitch_curve: 0.7, hold: 300.0, decay: 1500.0, decay_curve: 0.3, harmonics: 0.05, click: 0.15, click_type: 6, drive: 0.15, sub_level: 0.4, ..KickParams::default() })),
        tuned(SUBS, "808 saturado (Do)", Sub, C1, || {
            let p = KickParams { tune: C1 as f32, pitch_amount: 12.0, pitch_time: 35.0, pitch_curve: 0.7, hold: 250.0, decay: 1200.0, decay_curve: 0.3, harmonics: 0.2, click: 0.3, click_type: 1, drive: 0.6, drive_mode: KickDrive::Tube, sub_level: 0.5, ..KickParams::default() };
            fx(kick(p), &[drive(KickDrive::Soft, 8.0), eq(&[low_cut(25.0)])])
        }),
        tuned(SUBS, "808 corto (Do)", Sub, C1, || kick(KickParams { tune: C1 as f32, pitch_amount: 14.0, pitch_time: 30.0, pitch_curve: 0.7, hold: 120.0, decay: 420.0, decay_curve: 0.4, harmonics: 0.1, click: 0.35, click_type: 2, drive: 0.3, sub_level: 0.4, punch: 0.3, ..KickParams::default() })),
        tuned(SUBS, "808 con golpe (Do)", Sub, C1, || kick(KickParams { tune: C1 as f32, pitch_amount: 24.0, pitch_time: 30.0, pitch_curve: 0.8, hold: 200.0, decay: 1100.0, decay_curve: 0.35, harmonics: 0.1, click: 0.7, click_tone: 3500.0, click_type: 1, drive: 0.35, phase: 90.0, punch: 0.5, sub_level: 0.4, ..KickParams::default() })),
        tuned(SUBS, "Sub limpio (Do)", Sub, C1, || kick(KickParams { tune: C1 as f32, pitch_amount: 0.0, attack: 3.0, hold: 500.0, decay: 700.0, decay_curve: 0.3, click: 0.0, drive: 0.0, ..KickParams::default() })),
        tuned(SUBS, "Sub cálido (Do)", Sub, C1, || kick(KickParams { tune: C1 as f32, pitch_amount: 0.0, attack: 3.0, hold: 400.0, decay: 800.0, decay_curve: 0.3, harmonics: 0.35, click: 0.0, drive: 0.25, drive_mode: KickDrive::Tube, sub_level: 0.5, ..KickParams::default() })),
        // ---------------- Bajos
        tuned(BASSES, "Bajo acid (Do)", Bass, C1, || chord(preset("Acid Bass", "Acid · Clásico"), &[C1 + 12], 1.0, 0.5)),
        tuned(BASSES, "Bajo reese (Do)", Bass, C1, || fx(chord(preset("Super Saw", "Bajo · Reese"), &[C1], 0.9, 1.5), &[drive(KickDrive::Soft, 6.0), eq(&[low_cut(30.0)])])),
        tuned(BASSES, "Bajo FM (Do)", Bass, C1, || chord(preset("FM Synth", "Bajo · FM de los 80"), &[C1 + 12], 0.9, 0.5)),
        tuned(BASSES, "Bajo rebotado (Do)", Bass, C1, || chord(preset("Super Saw", "Bajo · Rebotado"), &[C1 + 12], 0.9, 0.5)),
        tuned(BASSES, "Bajo de órgano (Do)", Bass, C1, || chord(preset("Pulse Keys", "Órgano · House"), &[C1 + 12], 0.9, 0.5)),
        tuned(BASSES, "Bajo techno (Do)", Bass, C1, || chord(preset("Acid Bass", "Bajo · Techno rodante"), &[C1 + 12], 0.9, 0.5)),
        tuned(BASSES, "Bajo sierra (Do)", Bass, C1, || chord(preset("Pulse Synth", "Bajo · Sierra con mordida"), &[C1 + 12], 0.9, 0.5)),
        tuned(BASSES, "Bajo slap (Do)", Bass, C1, || chord(preset("FM Synth", "Bajo · Slap"), &[C1 + 12], 1.0, 0.5)),
        // ---------------- Stabs y acordes
        tuned(STABS, "Stab de house (Do menor 7)", Stab, C3, || fx(chord(preset("Super Saw", "Acordes · Stab de house"), &[60, 63, 67, 70], 0.9, 0.25), &[space(ReverbSpace::Room, 0.8, 0.15, 10000.0)])),
        tuned(STABS, "Stab de órgano (Do menor 7)", Stab, C3, || chord(preset("Pulse Keys", "Órgano · House"), &[60, 63, 67, 70], 0.9, 0.35)),
        tuned(STABS, "Acorde dub (Do menor 7)", Stab, C3, || fx(chord(preset("Super Saw", "Acordes · Progressive"), &[60, 63, 67, 70], 0.8, 0.25), &[eq(&[low_cut(200.0), high_cut(2500.0)]), echo(0.75, 0.55, 0.35, 1800.0), space(ReverbSpace::Hall, 1.0, 0.3, 6000.0)])),
        tuned(STABS, "Stab rave (Do menor)", Stab, C3, || fx(chord(preset("Super Saw", "Acordes · Rave de los 90"), &[60, 63, 67, 72], 1.0, 0.35), &[drive(KickDrive::Soft, 6.0), space(ReverbSpace::Hall, 0.7, 0.15, 10000.0)])),
        tuned(STABS, "Stab de quintas (Do)", Stab, C3, || fx(chord(preset("Super Saw", "Lead · Rave desafinado"), &[60, 67, 72], 1.0, 0.2), &[drive(KickDrive::Hard, 6.0), eq(&[low_cut(150.0)]), space(ReverbSpace::Plate, 0.6, 0.15, 10000.0)])),
        tuned(STABS, "Acorde de piano eléctrico (Do menor 9)", Stab, C3, || chord(preset("Pulse Keys", "Púas · Neo-soul"), &[60, 63, 67, 70, 74], 0.8, 1.0)),
        tuned(STABS, "Acorde lo-fi (Do mayor 7)", Stab, C3, || dusty(chord(preset("Pulse Keys", "Piano · Lo-fi"), &[60, 64, 67, 71], 0.7, 1.0), 4500.0)),
        tuned(STABS, "Stab de metales (Do)", Stab, C3, || chord(preset("FM Synth", "Metales · Stab FM"), &[60, 64, 67], 0.9, 0.3)),
        tuned(STABS, "Acorde ochentero (Do menor)", Stab, C3, || fx(chord(preset("Super Saw", "Pluck · Largo"), &[60, 63, 67, 72], 0.9, 0.5), &[Effect::Chorus(Default::default()), space(ReverbSpace::Hall, 1.0, 0.25, 9000.0)])),
        // ---------------- FX
        Recipe { folder: FX, name: "Subida 4 compases (120 BPM)", role: Fx, note: None, length: Some(8.0), make: || riser(16.0) },
        Recipe { folder: FX, name: "Subida 8 compases (120 BPM)", role: Fx, note: None, length: Some(16.0), make: || riser(32.0) },
        r(FX, "Subida de ruido", Fx, || swell(play(preset("Polychrome Synth", "FX · Subida de ruido"), &[(0.0, 60, 0.9, 8.0)], 8.0), 4.0, 15.0)),
        r(FX, "Bajada", Fx, || play(preset("Wavetable Synth", "FX · Caída de ruido"), &[(0.0, 60, 0.9, 8.0)], 8.0)),
        r(FX, "Impacto", Fx, || {
            let boom = kick(KickParams { tune: 24.0, pitch_amount: 24.0, pitch_time: 200.0, pitch_curve: 0.5, hold: 100.0, decay: 1500.0, click: 0.6, click_type: 0, drive: 0.4, ..KickParams::default() });
            let body = play(preset("Wavetable Synth", "FX · Impacto"), &[(0.0, 36, 1.0, 2.0)], 2.0);
            layer(vec![(body, 0.0, 0.0, 0.0), (fx(boom, &[space(ReverbSpace::Hall, 1.2, 0.3, 4000.0)]), -3.0, 0.0, 0.0)])
        }),
        r(FX, "Crash al revés", Fx, || reverse(fx(dm(Tr909, CRASH, -2.0, 2.0, 0.0), &[space(ReverbSpace::Hall, 1.0, 0.35, 12000.0)]))),
        r(FX, "Barrido de ruido", Fx, || noise_sweep()),
        r(FX, "Caída de sub", Fx, || kick(KickParams { tune: 24.0, pitch_amount: 24.0, pitch_time: 1500.0, pitch_curve: 0.3, hold: 300.0, decay: 1500.0, decay_curve: 0.4, click: 0.0, drive: 0.2, sub_level: 0.3, ..KickParams::default() })),
        r(FX, "Láser", Fx, || play(preset("Wavetable Synth", "FX · Láser"), &[(0.0, 72, 1.0, 0.5)], 1.0)),
        r(FX, "Zap", Fx, || kick(KickParams { tune: 33.0, pitch_amount: 72.0, pitch_time: 120.0, pitch_curve: 0.9, hold: 30.0, decay: 200.0, decay_curve: 0.5, click: 0.2, click_tone: 9000.0, drive: 0.2, click_type: 10, ..KickParams::default() })),
        r(FX, "Viento", Fx, || play(preset("Wavetable Synth", "FX · Viento"), &[(0.0, 60, 0.9, 6.0)], 6.0)),
        r(FX, "Ruido de vinilo", Fx, vinyl),
    ]
}

/// A riser of `beats` at 120 BPM: noise and a saw stack opening and rising,
/// cut on the drop.
fn riser(beats: f64) -> Stereo {
    let InstrumentKind::Wavetable(mut p) = preset("Wavetable Synth", "FX · Riser 4 bars") else { unreachable!() };
    p.lfo[0].sync_beats = beats as f32;
    let seconds = beats as f32 * 0.5;
    swell(play(InstrumentKind::Wavetable(p), &[(0.0, 60, 1.0, beats)], beats), seconds, 20.0)
}

/// Grows from `db` under to full over `seconds` (faster towards the end, as
/// a build-up does), then stays.
fn swell((mut l, mut r): Stereo, seconds: f32, db: f32) -> Stereo {
    let frames = (seconds * RATE) as usize;
    for i in 0..frames.min(l.len()) {
        let t = i as f32 / frames as f32;
        let g = 10f32.powf(-db * (1.0 - t * t) / 20.0);
        l[i] *= g;
        r[i] *= g;
    }
    (l, r)
}

/// White noise through a band-pass that sweeps up over two seconds.
fn noise_sweep() -> Stereo {
    let InstrumentKind::Wavetable(mut p) = preset("Wavetable Synth", "FX · Viento") else { unreachable!() };
    let up = engine::model::Shape::from_points(&[(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)]);
    p.random_rate = 0.0;
    p.matrix[1] = engine::model::ModSlot::EMPTY;
    p.filter.cutoff = 200.0;
    p.lfo[0] = engine::model::WaveLfo { shape: up, mode: engine::model::LfoMode::Envelope, sync_beats: 4.0, ..p.lfo[0] };
    p.matrix[0] = engine::model::ModSlot { source: engine::model::ModSource::Lfo1, dest: 32, amount: 0.7, bipolar: false };
    p.env[0] = engine::model::WaveEnv { attack: 1.6, hold: 0.0, decay: 1.0, sustain: 1.0, release: 0.4 };
    play(InstrumentKind::Wavetable(p), &[(0.0, 60, 0.9, 4.0)], 4.0)
}

/// Four seconds of record surface: a soft hiss and crackles, the same every time.
fn vinyl() -> Stereo {
    let frames = (RATE * 4.0) as usize;
    let mut seed = 0x2545_f491u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    let (mut hiss_l, mut hiss_r, mut pop_l, mut pop_r) = (0f32, 0f32, 0f32, 0f32);
    let lowpass = (-std::f32::consts::TAU * 2500.0 / RATE).exp();
    let fall = (-1.0 / (0.0004 * RATE)).exp();
    for i in 0..frames {
        hiss_l = hiss_l * lowpass + (random() * 2.0 - 1.0) * (1.0 - lowpass);
        hiss_r = hiss_r * lowpass + (random() * 2.0 - 1.0) * (1.0 - lowpass);
        // About twelve crackles a second, now and then a louder one.
        if random() < 12.0 / RATE {
            let size = if random() < 0.15 { 0.3 } else { 0.08 + random() * 0.1 };
            let sign = if random() < 0.5 { -1.0 } else { 1.0 };
            let side = random();
            pop_l += sign * size * (1.0 - side * 0.6);
            pop_r += sign * size * (0.4 + side * 0.6);
        }
        pop_l *= fall;
        pop_r *= fall;
        l[i] = hiss_l * 0.25 + pop_l;
        r[i] = hiss_r * 0.25 + pop_r;
    }
    fx((l, r), &[eq(&[steep_low_cut(300.0), high_cut(9000.0), low_shelf(800.0, -3.0)])])
}
