//! The mix assistant: renders the mix and each track on its own (offline, in the
//! background), measures them (loudness, true peak, range, stereo, balance, low
//! end) and says what to fix for where the song is going (Spotify, Apple Music,
//! a club…), each with a button that does it. A reference song can be loaded to
//! compare the loudness and the tonal balance with.
//!
//! «Mezcla de partida» sets the tracks against each other the way a mix
//! usually starts: each takes a role (kick, bass, voice, pads…), from its
//! name, its instrument or its sound, and gets that role's usual level next
//! to the kick or the drums, its place in the stereo and, if it doesn't need
//! its lows, a low cut.

use crate::app::PulseApp;
use crate::export::{self, Range};
use crate::theme::{ACCENT, METER_RED, PLAY, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};
use engine::model::{CompressorParams, Device, DuckerParams, Effect, Eq3Params, EqEightParams, EqShape, InstrumentKind, KickDrive, LimiterParams, Param, SaturatorParams};
use stems::loudness;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const RATE: f32 = 48_000.0;
/// The most the assistant drives the master's limiter (dB): past it, a mix is
/// squashed rather than made louder.
const MAX_LIMITER_GAIN: f32 = 12.0;
/// From this loudness (LUFS) on, the master gets a clipper before its limiter
/// (see `PulseApp::loud_master`): how far it clips (dB), and the limiter's release.
const LOUD: f64 = -8.5;
const CLIP_DB: f32 = 3.0;
const LOUD_RELEASE_MS: f32 = 40.0;

/// Where the song is going, and the loudness and true peak it should have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Streaming,
    Apple,
    Club,
    /// Hard techno and rave: masters as loud as a club takes them.
    HardTechno,
    Broadcast,
    /// As loud as the reference song.
    Reference,
}

impl Target {
    const ALL: [Target; 6] = [Target::Streaming, Target::Apple, Target::Club, Target::HardTechno, Target::Broadcast, Target::Reference];

    fn label(self) -> &'static str {
        match self {
            Target::Streaming => "Spotify, YouTube, Tidal (−14 LUFS)",
            Target::Apple => "Apple Music (−16 LUFS)",
            Target::Club => "Club, DJ, SoundCloud (−9 LUFS)",
            Target::HardTechno => "Hard techno, rave (−7 LUFS)",
            Target::Broadcast => "Radio y TV, EBU R128 (−23 LUFS)",
            Target::Reference => "Como la referencia",
        }
    }

    /// Whether a platform turns down what goes over its loudness.
    fn turned_down(self) -> bool {
        matches!(self, Target::Streaming | Target::Apple | Target::Broadcast)
    }

    /// The loudness (LUFS) and the highest true peak (dBTP).
    fn levels(self, reference: Option<&Measures>) -> Option<(f64, f64)> {
        match self {
            Target::Streaming => Some((-14.0, -1.0)),
            Target::Apple => Some((-16.0, -1.0)),
            Target::Club => Some((-9.0, -1.0)),
            Target::HardTechno => Some((-7.0, -1.0)),
            Target::Broadcast => Some((-23.0, -1.0)),
            Target::Reference => reference.and_then(|r| r.lufs).map(|l| (l, -1.0)),
        }
    }
}

/// What a piece of audio measures.
#[derive(Clone, Debug, Default)]
pub struct Measures {
    pub lufs: Option<f64>,
    pub range: Option<f64>,
    pub short_max: Option<f64>,
    pub true_peak: f64,
    pub peak: f64,
    pub correlation: f64,
    pub bands: [f64; 10],
}

impl Measures {
    pub fn of(left: &[f32], right: &[f32], rate: f32) -> Self {
        Self {
            lufs: loudness::integrated(left, right, rate),
            range: loudness::range(left, right, rate),
            short_max: loudness::short_term_max(left, right, rate),
            true_peak: loudness::true_peak(left, right),
            peak: loudness::sample_peak(left, right),
            correlation: loudness::correlation(left, right),
            bands: loudness::bands(left, right, rate),
        }
    }
}

/// One track, played on its own.
#[derive(Clone, Debug)]
pub struct TrackMeasures {
    pub index: usize,
    /// Its id (what a sidechain names it by).
    pub id: u32,
    pub name: String,
    /// Drums (see [`is_drums`]); `kick` if it is a kick of its own (the Kick Designer).
    pub drums: bool,
    pub kick: bool,
    /// The tracks (ids) its sidechains listen to.
    pub listens: Vec<u32>,
    pub peak: f64,
    pub lufs: Option<f64>,
    pub correlation: f64,
    /// Its level below 120 Hz every 0.1 s (dB).
    low_end: Vec<f64>,
    /// What it is in the mix, where it is in the stereo, whether it has a low
    /// cut already, and its octave bands (see [`balance_tracks`]).
    pub role: Role,
    pub pan: f32,
    pub cut: bool,
    pub bands: [f64; 10],
    /// Its fader (linear gain).
    pub gain: f32,
    /// Its level in each of [`MID_HZ`] every 0.1 s (dB), and where its EQ
    /// Eight already makes room (Hz of its cuts).
    pub mids: Vec<[f32; MID_BANDS]>,
    pub carves: Vec<f32>,
    /// How loud its loud parts and its soft parts are (dBFS: its peak every
    /// 50 ms, the 90th and 20th percentiles of what sounds), and whether it
    /// has a compressor already (see [`compression`]).
    pub dynamics: Option<(f32, f32)>,
    pub compressed: bool,
}

// ----- Compression to start from ------------------------------------------------------

/// How a role is compressed to start with: its ratio, attack (ms), how much it
/// takes off the loud parts (dB), and from what range (loud to soft, dB) it
/// needs it.
fn squeeze(role: Role) -> Option<(f32, f32, f32, f32)> {
    match role {
        Role::Vocal => Some((3.0, 5.0, 5.0, 8.0)),
        Role::Backing => Some((3.0, 5.0, 4.0, 8.0)),
        Role::Lead => Some((2.5, 10.0, 3.0, 10.0)),
        Role::Bass => Some((4.0, 15.0, 4.0, 6.0)),
        // Slow to let the hit through: punch, not a flattened beat.
        Role::Drums => Some((4.0, 25.0, 3.0, 10.0)),
        _ => None,
    }
}

/// The loud and soft parts of `left`/`right` (see [`TrackMeasures::dynamics`]).
pub fn dynamics(left: &[f32], right: &[f32], rate: f32) -> Option<(f32, f32)> {
    let window = (0.05 * rate) as usize;
    let mut peaks: Vec<f32> = left.chunks(window).zip(right.chunks(window)).map(|(l, r)| 20.0 * l.iter().chain(r).fold(0f32, |m, x| m.max(x.abs())).max(1e-9).log10()).collect();
    let loudest = peaks.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    // What sounds: within 40 dB of its loudest.
    peaks.retain(|&p| p > loudest - 40.0);
    if peaks.len() < 20 {
        return None;
    }
    peaks.sort_by(f32::total_cmp);
    let at = |q: f32| peaks[((peaks.len() - 1) as f32 * q) as usize];
    Some((at(0.9), at(0.2)))
}

/// The compressor a track starts with, for its role and how much its level
/// moves, at `tempo`: the threshold where it takes the role's dB off the loud
/// parts (as its effects hear them: before the fader and the pan), the
/// release a sixteenth (a bass, an eighth), and makeup for about half of it.
pub fn compression(t: &TrackMeasures, tempo: f64) -> Option<CompressorParams> {
    let (ratio, attack_ms, reduce, needs) = squeeze(t.role)?;
    let (loud, soft) = t.dynamics?;
    // Ducking to another (a rumble to its kick, a bass to the drums) moves
    // its level on purpose: that is no reason to compress it.
    if t.compressed || loud - soft < needs || !t.listens.is_empty() {
        return None;
    }
    // Back to before the fader and the pan (constant power, unity at centre).
    let angle = (t.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let pan_db = 20.0 * (angle.cos().max(angle.sin()) * std::f32::consts::SQRT_2).log10();
    let before = loud - 20.0 * t.gain.max(1e-6).log10() - pan_db;
    let threshold = before - reduce * ratio / (ratio - 1.0);
    let note = if t.role == Role::Bass { 0.5 } else { 0.25 };
    let release = (note * 60.0 / tempo.max(1.0) * 1000.0).clamp(50.0, 250.0) as f32;
    let round = |x: f32| (x * 2.0).round() / 2.0;
    Some(CompressorParams { threshold_db: round(threshold), ratio, attack_ms, release_ms: release.round(), makeup_db: round(reduce * 0.5), sidechain: None })
}

// ----- Tracks covering each other in the mids ------------------------------------------

/// The bands (half an octave each) where tracks cover each other: from the
/// voice's body to its presence.
pub const MID_BANDS: usize = 9;
pub const MID_HZ: [f32; MID_BANDS] = [250.0, 354.0, 500.0, 707.0, 1000.0, 1414.0, 2000.0, 2828.0, 4000.0];
/// Under this a track isn't playing in a band (dB, a sine's level).
const MID_ON: f32 = -50.0;
/// Two tracks this close (dB) in a band at once cover each other.
const MID_CLOSE: f32 = 6.0;
/// How deep the room made is (dB) and how wide (Q).
const CARVE_DB: f32 = -4.0;
const CARVE_Q: f32 = 1.4;

/// The level of `left`/`right` in each of [`MID_HZ`] every 0.1 s (dB).
pub fn mids(left: &[f32], right: &[f32], rate: f32) -> Vec<[f32; MID_BANDS]> {
    use rustfft::num_complex::Complex;
    const SIZE: usize = 4096;
    let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(SIZE);
    let hop = (0.1 * rate) as usize;
    let window: Vec<f32> = (0..SIZE).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos()).collect();
    // The bins of each band: from a quarter octave under its centre to a quarter over.
    let edges: Vec<(usize, usize)> = MID_HZ.iter().map(|&hz| ((hz / 2f32.powf(0.25) * SIZE as f32 / rate) as usize, (hz * 2f32.powf(0.25) * SIZE as f32 / rate) as usize)).collect();
    let mut buffer = vec![Complex::new(0f32, 0f32); SIZE];
    let mut out = Vec::new();
    let mut at = 0;
    while at + SIZE <= left.len() {
        for (i, b) in buffer.iter_mut().enumerate() {
            *b = Complex::new(0.5 * (left[at + i] + right[at + i]) * window[i], 0.0);
        }
        fft.process(&mut buffer);
        // By the window's energy (Σw² = 3N/8 for Hann): a sine of amplitude
        // A adds up to A² over its bins.
        let scale = 32.0 / (3.0 * (SIZE * SIZE) as f32);
        out.push(std::array::from_fn(|k| {
            let (a, b) = edges[k];
            let power: f32 = buffer[a..=b].iter().map(|c| c.norm_sqr()).sum::<f32>() * scale;
            10.0 * power.max(1e-12).log10()
        }));
        at += hop;
    }
    out
}

/// Who has the right of way in the mids: the voice, then the lead, then the rest.
fn presence(role: Role) -> u8 {
    match role {
        Role::Vocal => 9,
        Role::Lead => 8,
        Role::Backing => 7,
        Role::Kick | Role::Drums => 6,
        Role::Bass => 5,
        Role::Chords => 4,
        Role::Other => 3,
        Role::Pad => 2,
        Role::Percussion => 1,
        Role::Fx => 0,
    }
}

/// Two tracks covering each other in the mids: the one that makes room (its
/// index in the tracks), the one it makes room for, where (a band of
/// [`MID_HZ`]) and how much of the time they cover each other there.
#[derive(Clone, Debug, PartialEq)]
pub struct MidClash {
    pub yields: usize,
    pub to: usize,
    pub band: usize,
    pub share: f64,
}

/// The pairs of tracks that cover each other in the mids, the worst first:
/// in a band, at least half the time both play there they are as loud. Of
/// the bands where they do, the one where the track with the right of way
/// is loudest (its body: room there is what lets it through). Drums against
/// drums aren't counted (they take turns), nor a kick or a bass (their place
/// is the low end).
pub fn mid_clashes(tracks: &[TrackMeasures]) -> Vec<MidClash> {
    let rhythm = |r: Role| matches!(r, Role::Kick | Role::Drums | Role::Percussion);
    let mut out = Vec::new();
    for i in 0..tracks.len() {
        for j in i + 1..tracks.len() {
            let (a, b) = (&tracks[i], &tracks[j]);
            if (rhythm(a.role) && rhythm(b.role)) || matches!(a.role, Role::Kick | Role::Bass) || matches!(b.role, Role::Kick | Role::Bass) {
                continue;
            }
            let frames = a.mids.len().min(b.mids.len());
            if frames == 0 {
                continue;
            }
            // The one with less right of way makes room; of two alike, the quieter.
            let a_first = (presence(a.role), a.lufs.unwrap_or(-99.0)) >= (presence(b.role), b.lufs.unwrap_or(-99.0));
            let (yields, to) = if a_first { (j, i) } else { (i, j) };
            let lead = &tracks[to];
            let mut worst: Option<(usize, f64, f32)> = None;
            for k in 0..MID_BANDS {
                let on = |m: &[[f32; MID_BANDS]]| m[..frames].iter().filter(|f| f[k] > MID_ON).count();
                let both = (0..frames).filter(|&f| a.mids[f][k] > MID_ON && b.mids[f][k] > MID_ON).count();
                let close = (0..frames).filter(|&f| a.mids[f][k] > MID_ON && b.mids[f][k] > MID_ON && (a.mids[f][k] - b.mids[f][k]).abs() < MID_CLOSE).count();
                // Both there a fair part of the song, and as loud most of that time.
                if on(&a.mids).min(on(&b.mids)) * 5 < frames || both * 4 < frames || close * 2 < both {
                    continue;
                }
                let share = close as f64 / frames as f64;
                let playing: Vec<f32> = lead.mids[..frames].iter().map(|f| f[k]).filter(|&l| l > MID_ON).collect();
                let level = playing.iter().sum::<f32>() / playing.len().max(1) as f32;
                if worst.is_none_or(|(_, _, l)| level > l) {
                    worst = Some((k, share, level));
                }
            }
            if let Some((band, share, _)) = worst {
                out.push(MidClash { yields, to, band, share });
            }
        }
    }
    out.sort_by(|a, b| b.share.total_cmp(&a.share));
    out
}

/// Whether `carves` (a track's cuts, Hz) make room at `hz` already: one
/// within a quarter octave.
fn carved_at(carves: &[f32], hz: f32) -> bool {
    carves.iter().any(|c| (c / hz).log2().abs() < 0.25)
}

/// Where a track's EQ Eights cut (Hz): bells at least 2 dB down.
fn carves(track: &crate::project::Track) -> Vec<f32> {
    track
        .devices
        .iter()
        .filter_map(|d| if let Effect::EqEight(eq) = &d.effect { Some(eq) } else { None })
        .flat_map(|eq| eq.bands.iter().filter(|b| b.on && b.shape == EqShape::Bell && b.gain_db <= -2.0).map(|b| b.freq))
        .collect()
}

fn hz_label(hz: f32) -> String {
    if hz >= 1000.0 { format!("{} kHz", num(hz as f64 / 1000.0)) } else { format!("{hz:.0} Hz") }
}

// ----- A starting balance -------------------------------------------------------------

/// What a track is in the mix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Kick,
    Drums,
    Percussion,
    Bass,
    Vocal,
    Backing,
    Lead,
    Chords,
    Pad,
    Fx,
    Other,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Kick => "bombo",
            Role::Drums => "batería",
            Role::Percussion => "percusión",
            Role::Bass => "bajo",
            Role::Vocal => "voz",
            Role::Backing => "coros",
            Role::Lead => "solista",
            Role::Chords => "acordes",
            Role::Pad => "pad",
            Role::Fx => "efectos",
            Role::Other => "otra",
        }
    }

    /// Its loudness (LU) next to the kick, where a mix usually starts: the
    /// voice as loud, the bass a little under, pads and percussion well under.
    fn level(self) -> Option<f64> {
        match self {
            Role::Kick | Role::Vocal => Some(0.0),
            Role::Drums | Role::Bass => Some(-2.0),
            Role::Lead => Some(-3.0),
            Role::Chords => Some(-5.0),
            Role::Backing => Some(-7.0),
            Role::Percussion | Role::Pad => Some(-8.0),
            Role::Fx => Some(-10.0),
            Role::Other => None,
        }
    }

    /// Where its low cut goes (Hz), if it doesn't need its lows.
    fn low_cut(self) -> Option<f32> {
        match self {
            Role::Vocal => Some(90.0),
            Role::Lead => Some(120.0),
            Role::Backing | Role::Chords | Role::Pad | Role::Fx => Some(150.0),
            Role::Percussion => Some(250.0),
            _ => None,
        }
    }

    /// In the middle: the low end (a club is mono down there) and the voice.
    fn centred(self) -> bool {
        matches!(self, Role::Kick | Role::Bass | Role::Vocal)
    }

    /// How far apart several of it go (pan, each way).
    fn spread(self) -> Option<f32> {
        match self {
            Role::Lead => Some(0.25),
            Role::Percussion => Some(0.3),
            Role::Chords => Some(0.35),
            Role::Pad | Role::Fx => Some(0.4),
            Role::Backing => Some(0.5),
            _ => None,
        }
    }
}

/// The role a track's name says (Spanish or English), or its instrument.
pub fn role_hint(name: &str, instrument: &InstrumentKind, drums: bool) -> Option<Role> {
    let name = name.to_lowercase();
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |keys: &[&str]| words.iter().any(|w| keys.iter().any(|k| w.starts_with(k)));
    // The most telling first: «Kick Rumble» is low end, «Voz FX» an effect.
    let by_name = [
        (Role::Fx, &["fx", "riser", "sweep", "impact", "efecto", "noise", "ruido"][..]),
        (Role::Backing, &["coro", "backing", "bvs", "dobl"]),
        (Role::Vocal, &["voz", "voces", "vocal", "vox", "voice", "cant"]),
        (Role::Bass, &["bass", "bajo", "sub", "808", "rumble"]),
        (Role::Kick, &["kick", "bombo", "bd"]),
        (Role::Percussion, &["hat", "hihat", "charles", "perc", "shaker", "tops", "ride", "crash", "conga", "bongo", "clap", "palma", "tambor"]),
        (Role::Drums, &["drum", "bater", "beat", "ritmo", "break", "snare", "caja"]),
        (Role::Lead, &["lead", "melod", "solo", "arp", "pluck", "hook"]),
        (Role::Chords, &["acorde", "chord", "keys", "piano", "rhodes", "guitar", "stab", "organ", "órgano"]),
        (Role::Pad, &["pad", "string", "cuerda", "atmos", "ambient", "drone"]),
    ];
    if let Some((role, _)) = by_name.iter().find(|(_, keys)| has(keys)) {
        return Some(*role);
    }
    match instrument {
        InstrumentKind::Kick(_) => Some(Role::Kick),
        InstrumentKind::Acid(_) => Some(Role::Bass),
        _ if drums => Some(Role::Drums),
        _ => None,
    }
}

/// The role a track's sound says, when its name and instrument don't: low
/// end mostly, a bass; highs mostly, percussion.
pub fn role_by_sound(bands: &[f64; 10]) -> Role {
    let power: Vec<f64> = bands.iter().map(|b| 10f64.powf(b / 10.0)).collect();
    let total = power.iter().sum::<f64>().max(1e-30);
    let share = |range: std::ops::RangeInclusive<usize>| power[range].iter().sum::<f64>() / total;
    if share(0..=2) > 0.6 {
        Role::Bass
    } else if share(7..=9) > 0.6 {
        Role::Percussion
    } else {
        Role::Other
    }
}

/// What the starting balance changes on a track.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub track: usize,
    pub name: String,
    pub role: Role,
    /// Its volume moved (dB), its pan set, a low cut at (Hz).
    pub gain_db: f32,
    pub pan: Option<f32>,
    pub low_cut: Option<f32>,
}

impl Change {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.gain_db != 0.0 {
            parts.push(format!("volumen {} dB", db(self.gain_db as f64)));
        }
        if let Some(pan) = self.pan {
            parts.push(match pan {
                p if p.abs() < 0.01 => "al centro".to_string(),
                p => format!("panorama {:.0} % {}", p.abs() * 100.0, if p < 0.0 { "izquierda" } else { "derecha" }),
            });
        }
        if let Some(hz) = self.low_cut {
            parts.push(format!("sin graves bajo {hz:.0} Hz"));
        }
        format!("«{}» ({}): {}", self.name, self.role.label(), parts.join(", "))
    }
}

/// A fader's top (linear gain: +6 dB).
const FADER_TOP: f32 = 2.0;
/// The least a volume is moved (dB): less isn't worth the change.
const BALANCE_STEP: f64 = 1.5;
/// How loud a track's lowest octaves may be (dB under its loudest) before
/// it is worth a low cut.
const LOWS_WORTH_CUTTING: f64 = -30.0;

/// The track the balance is set from: the loudest kick, else drums, voice or
/// lead.
fn anchor(tracks: &[TrackMeasures]) -> Option<&TrackMeasures> {
    [Role::Kick, Role::Drums, Role::Vocal, Role::Lead]
        .into_iter()
        .find_map(|role| tracks.iter().filter(|t| t.role == role && t.lufs.is_some()).max_by(|a, b| a.lufs.partial_cmp(&b.lufs).unwrap()))
}

/// A starting balance: what each track needs to sit where its role
/// usually does, next to the anchor (see [`anchor`]).
pub fn balance_tracks(tracks: &[TrackMeasures]) -> (Option<&TrackMeasures>, Vec<Change>) {
    let Some(anchor) = anchor(tracks) else { return (None, Vec::new()) };
    let reference = anchor.lufs.unwrap_or(0.0) - anchor.role.level().unwrap_or(0.0);
    let sounding: Vec<&TrackMeasures> = tracks.iter().filter(|t| t.lufs.is_some()).collect();
    let count = |role: Role| sounding.iter().filter(|t| t.role == role).count();
    let mut changes = Vec::new();
    for t in &sounding {
        let mut change = Change { track: t.index, name: t.name.clone(), role: t.role, gain_db: 0.0, pan: None, low_cut: None };
        // Its level: the role's, shared among the tracks of it.
        if let (Some(level), Some(lufs)) = (t.role.level(), t.lufs) {
            if t.index != anchor.index {
                let goal = reference + level - 10.0 * (count(t.role) as f64).log10();
                // Up no further than a dB under full scale, nor past the
                // fader's top.
                let room = 20.0 * (FADER_TOP / t.gain.max(1e-6)).log10() as f64;
                let gain = (goal - lufs).clamp(-24.0, 24.0).min(-1.0 - t.peak).min(room);
                if gain.abs() >= BALANCE_STEP {
                    change.gain_db = (gain * 2.0).round() as f32 / 2.0;
                }
            }
        }
        // In the stereo: the low end and the voice in the middle; several of
        // a kind, spread across (only those not placed by hand).
        if t.role.centred() && t.pan.abs() > 0.15 && count(t.role) == 1 {
            change.pan = Some(0.0);
        }
        if let Some(spread) = t.role.spread() {
            let alike: Vec<&&TrackMeasures> = sounding.iter().filter(|o| o.role == t.role && o.pan.abs() < 0.05).collect();
            if alike.len() >= 2 {
                let k = alike.iter().position(|o| o.index == t.index);
                if let Some(k) = k {
                    let pan = -spread + 2.0 * spread * k as f32 / (alike.len() - 1) as f32;
                    if pan.abs() > 0.01 {
                        change.pan = Some(pan);
                    }
                }
            }
        }
        // Lows it doesn't need.
        if let Some(hz) = t.role.low_cut().filter(|_| !t.cut) {
            let loudest = t.bands.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            if t.bands[0].max(t.bands[1]) - loudest > LOWS_WORTH_CUTTING {
                change.low_cut = Some(hz);
            }
        }
        if change.gain_db != 0.0 || change.pan.is_some() || change.low_cut.is_some() {
            changes.push(change);
        }
    }
    (Some(anchor), changes)
}

/// Whether a track's effects already cut its lows (an EQ Eight's low cut, or
/// an EQ Three with its lows well down).
fn cuts_lows(track: &crate::project::Track) -> bool {
    track.devices.iter().any(|d| match &d.effect {
        Effect::EqEight(eq) => eq.bands.iter().any(|b| b.on && matches!(b.shape, EqShape::LowCut12 | EqShape::LowCut48) && b.freq >= 50.0),
        Effect::Eq3(eq) => eq.low_db <= -6.0,
        _ => false,
    })
}

#[derive(Clone, Debug)]
pub struct Report {
    pub mix: Measures,
    pub tracks: Vec<TrackMeasures>,
    /// The song's tempo (for release times).
    pub tempo: f64,
}

/// A change the assistant can make.
#[derive(Clone, Debug, PartialEq)]
pub enum Fix {
    /// The master's limiter: its gain moved by `gain_db`, its ceiling set.
    MasterLimiter { gain_db: f32, ceiling_db: f32 },
    /// A track's volume moved (dB).
    TrackGain { track: usize, name: String, db: f32 },
    /// A ducker on a track that listens to the drums (by their track id), so it
    /// makes room each time they hit.
    Duck { track: usize, name: String, source: u32, source_name: String },
    /// Less low end on a track (an EQ Three, lows down).
    CutLows { track: usize, name: String },
    /// The master's EQ Three moved by these (dB).
    MasterEq { low: f32, mid: f32, high: f32 },
    /// A starting balance (see [`balance_tracks`]).
    Balance(Vec<Change>),
    /// Room made in a track at `hz` for another (an EQ Eight bell down).
    Carve { track: usize, name: String, hz: f32, for_name: String },
    /// A compressor to start from on a track (see [`compression`]).
    Compress { track: usize, name: String, params: CompressorParams },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Good,
    Warning,
    Note,
}

#[derive(Clone, Debug)]
pub struct Advice {
    pub level: Level,
    pub text: String,
    pub fix: Option<Fix>,
    /// What it would do, a line each.
    pub details: Vec<String>,
}

fn advice(level: Level, text: String, fix: Option<Fix>) -> Advice {
    Advice { level, text, fix, details: Vec::new() }
}

fn db(x: f64) -> String {
    format!("{x:+.1}").replace('.', ",")
}

fn num(x: f64) -> String {
    format!("{x:.1}").replace('.', ",")
}

/// A level below which a track is not playing in the low end (dB of power).
const LOW_END_ON: f64 = -30.0;

/// The pairs of tracks that fill the low end at the same time, most of the
/// time one of them does: their indices in `tracks`, and that share.
fn low_end_clashes(tracks: &[TrackMeasures]) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    for i in 0..tracks.len() {
        for j in i + 1..tracks.len() {
            let (a, b) = (&tracks[i].low_end, &tracks[j].low_end);
            let frames = a.len().min(b.len());
            if frames == 0 {
                continue;
            }
            let on = |x: &[f64]| x[..frames].iter().filter(|&&l| l > LOW_END_ON).count();
            let both = (0..frames).filter(|&k| a[k] > LOW_END_ON && b[k] > LOW_END_ON).count();
            let fewer = on(a).min(on(b));
            if fewer * 10 >= frames && both * 2 > fewer {
                out.push((i, j, both as f64 / frames as f64));
            }
        }
    }
    out
}

/// Whether the assistant takes a track for drums: a drum instrument, the Kick
/// Designer, or an audio track named like drums («Kick», «Tops», «Perc»…).
fn is_drums(track: &crate::project::Track) -> bool {
    track.is_drums() || matches!(track.instrument, engine::model::InstrumentKind::Kick(_)) || (track.is_audio() && crate::loop_key::is_drums(&track.name))
}

/// The tracks (ids) a track's sidechains listen to.
fn listens(track: &crate::project::Track) -> Vec<u32> {
    track.devices.iter().filter_map(|d| d.effect.sidechain()).collect()
}

/// What to do about the mix for `target`.
pub fn advise(report: &Report, target: Target, reference: Option<&Measures>) -> Vec<Advice> {
    let mut out = Vec::new();
    let mix = &report.mix;
    let Some(lufs) = mix.lufs else {
        out.push(advice(Level::Warning, "La mezcla está en silencio".into(), None));
        return out;
    };
    // Loudness and true peak.
    if let Some((goal, ceiling)) = target.levels(reference) {
        let off = goal - lufs;
        let ceiling_db = (ceiling - 0.8) as f32;
        if off.abs() < 1.0 {
            out.push(advice(Level::Good, format!("Sonoridad {} LUFS: justo para {}", num(lufs), target.label()), None));
        } else if off > 0.0 {
            out.push(advice(
                Level::Warning,
                format!("Sonoridad {} LUFS: {} dB por debajo del objetivo ({} LUFS). Al lado de otras canciones sonará más floja", num(lufs), num(off), num(goal)),
                Some(Fix::MasterLimiter { gain_db: off as f32, ceiling_db }),
            ));
        } else {
            out.push(advice(
                Level::Note,
                format!(
                    "Sonoridad {} LUFS: {} dB por encima del objetivo ({} LUFS). {}",
                    num(lufs),
                    num(-off),
                    num(goal),
                    if target.turned_down() { "La plataforma la bajará, y lo que hayas apretado de más solo le quita dinámica" } else { "Más apretada no suena más fuerte en el club: solo pierde pegada" }
                ),
                Some(Fix::MasterLimiter { gain_db: off as f32, ceiling_db }),
            ));
        }
        if mix.true_peak > ceiling && off.abs() < 1.0 {
            out.push(advice(
                Level::Warning,
                format!("Pico real {} dBTP, por encima de {} dBTP: al pasarla a MP3 o AAC puede distorsionar", db(mix.true_peak), num(ceiling)),
                Some(Fix::MasterLimiter { gain_db: 0.0, ceiling_db }),
            ));
        } else if mix.true_peak <= ceiling {
            out.push(advice(Level::Good, format!("Pico real {} dBTP: sin riesgo de distorsión", db(mix.true_peak)), None));
        }
    }
    // The tracks against each other.
    let (anchor, changes) = balance_tracks(&report.tracks);
    if let Some(anchor) = anchor {
        if changes.is_empty() {
            out.push(advice(Level::Good, format!("Las pistas están equilibradas para empezar a mezclar, junto a «{}»", anchor.name), None));
        } else {
            let mut a = advice(
                Level::Note,
                format!("Mezcla de partida: {} {} fuera del sitio habitual de su papel junto a «{}»", changes.len(), if changes.len() == 1 { "pista" } else { "pistas" }, anchor.name),
                Some(Fix::Balance(changes.clone())),
            );
            a.details = changes.iter().map(Change::describe).collect();
            out.push(a);
        }
    }
    // Tracks over 0 dB (the master adds them up past full scale).
    for t in report.tracks.iter().filter(|t| t.peak > 0.0) {
        let lower = -(t.peak + 1.0) as f32;
        out.push(advice(
            Level::Warning,
            format!("«{}» llega a {} dBFS: por encima de 0 dB satura en cuanto pasa por algo que no lo aguante", t.name, db(t.peak)),
            Some(Fix::TrackGain { track: t.index, name: t.name.clone(), db: lower }),
        ));
    }
    // Two tracks in the low end at once.
    for (i, j, share) in low_end_clashes(&report.tracks) {
        let (a, b) = (&report.tracks[i], &report.tracks[j]);
        // One already makes room for the other (a bass or a rumble ducking to the kick).
        if let Some((ducks, to)) = [(a, b), (b, a)].into_iter().find(|(x, y)| x.listens.contains(&y.id)) {
            out.push(advice(Level::Good, format!("«{}» se aparta cuando suena «{}»: sus graves no se tapan", ducks.name, to.name), None));
            continue;
        }
        let text = format!("«{}» y «{}» llenan los graves a la vez el {:.0} % del tiempo: se tapan y la mezcla se embarra", a.name, b.name, share * 100.0);
        // A kick against the rest: the rest ducks. Two drum tracks: a kick of
        // its own keeps the low end. Otherwise the one with less gives it up.
        // Drums that duck to another track themselves (a rumble to its kick)
        // pass the ducking on to that one: the bass makes room for the kick.
        let duck = |to: &TrackMeasures, drums: &TrackMeasures| {
            let leader = drums.listens.iter().find_map(|id| report.tracks.iter().find(|t| t.id == *id)).unwrap_or(drums);
            Some(Fix::Duck { track: to.index, name: to.name.clone(), source: leader.id, source_name: leader.name.clone() })
        };
        let fix = match (a.drums, b.drums) {
            (true, false) => duck(b, a),
            (false, true) => duck(a, b),
            (true, true) if a.kick != b.kick => {
                let other = if a.kick { b } else { a };
                Some(Fix::CutLows { track: other.index, name: other.name.clone() })
            }
            _ => {
                let mean = |t: &TrackMeasures| t.low_end.iter().sum::<f64>() / t.low_end.len().max(1) as f64;
                let lesser = if mean(a) < mean(b) { a } else { b };
                Some(Fix::CutLows { track: lesser.index, name: lesser.name.clone() })
            }
        };
        // Ducking already to the one it would duck to (the bass to the kick,
        // against the kick's rumble): fine. The same fix from another clash
        // once is enough.
        if let Some(Fix::Duck { track, source, source_name, name }) = &fix {
            if report.tracks.iter().any(|t| t.index == *track && t.listens.contains(source)) {
                let text = format!("«{name}» se aparta cuando suena «{source_name}»: sus graves no se tapan");
                if !out.iter().any(|a| a.text == text) {
                    out.push(advice(Level::Good, text, None));
                }
                continue;
            }
        }
        if fix.is_some() && out.iter().any(|a| a.fix == fix) {
            continue;
        }
        out.push(advice(Level::Warning, text, fix));
    }
    // Tracks whose level moves too much to sit in the mix.
    for t in &report.tracks {
        let Some((_, _, reduce, needs)) = squeeze(t.role) else { continue };
        let Some((loud, soft)) = t.dynamics else { continue };
        if loud - soft < needs || !t.listens.is_empty() {
            continue;
        }
        if t.compressed {
            out.push(advice(Level::Good, format!("«{}» ya tiene compresor", t.name), None));
        } else if let Some(params) = compression(t, report.tempo) {
            out.push(advice(
                Level::Note,
                format!(
                    "«{}» ({}) va de {} a {} dBFS: entre sus partes fuertes y suaves hay {} dB, y en la mezcla unas se pierden o saltan. Un compresor le quitaría unos {} dB a lo fuerte",
                    t.name,
                    t.role.label(),
                    db(soft as f64),
                    db(loud as f64),
                    num((loud - soft) as f64),
                    num(reduce as f64)
                ),
                Some(Fix::Compress { track: t.index, name: t.name.clone(), params }),
            ));
        }
    }
    // Two tracks covering each other in the mids.
    for clash in mid_clashes(&report.tracks).into_iter().take(6) {
        let (yields, to) = (&report.tracks[clash.yields], &report.tracks[clash.to]);
        let hz = MID_HZ[clash.band];
        if carved_at(&yields.carves, hz) {
            out.push(advice(Level::Good, format!("«{}» deja hueco a «{}» en {}", yields.name, to.name, hz_label(hz)), None));
            continue;
        }
        out.push(advice(
            Level::Warning,
            format!("«{}» y «{}» se tapan en {} el {:.0} % del tiempo: cuesta oír «{}» con claridad", to.name, yields.name, hz_label(hz), clash.share * 100.0, to.name),
            Some(Fix::Carve { track: yields.index, name: yields.name.clone(), hz, for_name: to.name.clone() }),
        ));
    }
    // Stereo.
    if mix.correlation < 0.0 {
        out.push(advice(Level::Warning, format!("Correlación estéreo {:.2}: hay mucho en contrafase y en mono (un móvil, un club) se perderá", mix.correlation), None));
    }
    for t in report.tracks.iter().filter(|t| t.correlation < -0.2) {
        out.push(advice(Level::Warning, format!("«{}» tiene los canales en contrafase (correlación {:.2}): en mono casi desaparece", t.name, t.correlation), None));
    }
    // Dynamics.
    match mix.range {
        Some(r) if r < 3.0 => out.push(advice(Level::Note, format!("Rango de sonoridad {} LU: muy apretada, apenas cambia de volumen entre partes", num(r)), None)),
        Some(r) if r > 15.0 => out.push(advice(Level::Note, format!("Rango de sonoridad {} LU: las partes suaves y fuertes están muy lejos (bien para cine o clásica; en una canción, cuesta oír las suaves)", num(r)), None)),
        _ => {}
    }
    // The tonal balance against the reference.
    if let Some(reference) = reference {
        let [low, mid, high] = balance(&mix.bands, &reference.bands);
        let mut fix = [0f32; 3];
        for (k, (name, d)) in [("Graves", low), ("Medios", mid), ("Agudos", high)].into_iter().enumerate() {
            if d.abs() >= 2.0 {
                out.push(advice(Level::Warning, format!("{name}: {} dB {} que la referencia", num(d.abs()), if d > 0.0 { "más" } else { "menos" }), None));
                fix[k] = (-d).clamp(-6.0, 6.0) as f32;
            }
        }
        if fix != [0.0; 3] {
            out.push(advice(Level::Note, "Un EQ Three en el master acerca el balance al de la referencia".into(), Some(Fix::MasterEq { low: fix[0], mid: fix[1], high: fix[2] })));
        } else {
            out.push(advice(Level::Good, "El balance de graves, medios y agudos se parece al de la referencia".into(), None));
        }
    }
    out
}

/// How the mix's bands differ from the reference's (both levelled to the same
/// average), over lows (63–250 Hz), mids (500 Hz–2 kHz) and highs (4–16 kHz).
pub fn balance(mix: &[f64; 10], reference: &[f64; 10]) -> [f64; 3] {
    let level = |b: &[f64; 10]| b[1..=8].iter().sum::<f64>() / 8.0;
    let (m, r) = (level(mix), level(reference));
    let d: Vec<f64> = (0..10).map(|i| (mix[i] - m) - (reference[i] - r)).collect();
    let mean = |range: std::ops::RangeInclusive<usize>| range.clone().map(|i| d[i]).sum::<f64>() / range.count() as f64;
    [mean(1..=3), mean(4..=6), mean(7..=9)]
}

// ----- Running it ----------------------------------------------------------------

type Outcome = Result<Report, String>;

pub struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

pub struct ReferenceJob {
    name: String,
    result: Arc<Mutex<Option<Measures>>>,
}

#[derive(Clone, Copy)]
struct Chase {
    goal: f64,
    max_peak: f64,
    steps: u8,
    last: (f32, Option<f64>),
}

/// The window's state.
pub struct Assistant {
    pub open: bool,
    range: Range,
    pub target: Target,
    job: Option<Job>,
    pub report: Option<Report>,
    /// Changed since it was analysed.
    stale: bool,
    pub reference: Option<(String, Measures)>,
    reference_job: Option<ReferenceJob>,
    /// Bringing the mix to a loudness and under a true peak: the goal (LUFS),
    /// the highest true peak (dBTP) and the steps taken. A limiter pushed harder
    /// gives back less than its gain, and its peaks between samples go over its
    /// ceiling, so it takes a few, each measured. The last step's gain and the
    /// loudness before it tell how much the next needs.
    chase: Option<Chase>,
    /// What the last «Aplicar» could not do, shown until the next.
    note: Option<String>,
}

impl Default for Assistant {
    fn default() -> Self {
        Self { open: false, range: Range::Song, target: Target::Streaming, job: None, report: None, stale: false, reference: None, reference_job: None, chase: None, note: None }
    }
}

/// One render of the analysis, measured.
enum Measured {
    Mix(Measures),
    Track(TrackMeasures),
}

impl PulseApp {
    /// Renders and measures the mix and each track (in the background).
    pub fn analyze_mix(&mut self) {
        if export::beat_range(&self.project, self.mix.range).is_none() {
            self.status = Some(match self.mix.range {
                Range::Song | Range::Sections => "El Arrangement está vacío: coloca clips en él para analizar la canción (o analiza el loop)".into(),
                Range::Loop => "El loop está vacío".into(),
            });
            return;
        }
        self.capture_plugins();
        let settings = export::Settings { range: self.mix.range, stems: true, sample_rate: RATE as u32, ..Default::default() };
        let live = std::mem::take(&mut self.samples.plugins);
        let mix = export::plan(&self.project, &mut self.samples, &export::Settings { stems: false, ..settings }, std::path::Path::new("mezcla.wav"));
        let tracks = export::plan(&self.project, &mut self.samples, &settings, std::path::Path::new("mezcla.wav"));
        self.samples.plugins = live;
        let names: Vec<(String, bool, bool, u32, Vec<u32>)> = self
            .project
            .tracks
            .iter()
            .map(|t| (t.name.clone(), is_drums(t), matches!(t.instrument, engine::model::InstrumentKind::Kick(_)), t.id, listens(t)))
            .collect();
        // For the balance: the role its name or instrument says, its pan, its low cut.
        let compressed = |t: &crate::project::Track| t.devices.iter().any(|d| d.enabled && matches!(d.effect, Effect::Compressor(c) if c.sidechain.is_none()));
        let mixing: Vec<(Option<Role>, f32, bool, f32, Vec<f32>, bool)> = self.project.tracks.iter().map(|t| (role_hint(&t.name, &t.instrument, is_drums(t)), t.mixer.pan, cuts_lows(t), t.mixer.gain, carves(t), compressed(t))).collect();
        let tempo = self.project.tempo;
        let (progress, cancel, result) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (p, c, r) = (progress.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            // The mix and every track at once (see `export::render_each`).
            let renders: Vec<_> = mix.into_iter().chain(tracks).collect();
            let measured = export::render_each(renders, RATE, &p, &c, |render, left, right| {
                Ok(match render.track {
                    None => Measured::Mix(Measures::of(&left, &right, RATE)),
                    Some(index) => {
                        let (name, drums, kick, id, listens) = names.get(index).cloned().unwrap_or_default();
                        let (hint, pan, cut, gain, carves, compressed) = mixing.get(index).cloned().unwrap_or((None, 0.0, false, 1.0, Vec::new(), false));
                        let bands = loudness::bands(&left, &right, RATE);
                        Measured::Track(TrackMeasures {
                            index,
                            id,
                            name,
                            drums,
                            kick,
                            listens,
                            peak: loudness::sample_peak(&left, &right),
                            lufs: loudness::integrated(&left, &right, RATE),
                            correlation: loudness::correlation(&left, &right),
                            low_end: loudness::low_end(&left, &right, RATE),
                            role: hint.unwrap_or_else(|| role_by_sound(&bands)),
                            pan,
                            cut,
                            bands,
                            gain,
                            mids: mids(&left, &right, RATE),
                            carves,
                            dynamics: dynamics(&left, &right, RATE),
                            compressed,
                        })
                    }
                })
            });
            let outcome = measured.map_err(|_| "Análisis cancelado".to_string()).and_then(|all| {
                let mut all = all.into_iter();
                let Some(Measured::Mix(mix)) = all.next() else { return Err("Nada que analizar".to_string()) };
                let tracks = all.filter_map(|t| if let Measured::Track(t) = t { Some(t) } else { None }).collect();
                Ok(Report { mix, tracks, tempo })
            });
            if let Ok(mut slot) = r.lock() {
                *slot = Some(outcome);
            }
        });
        self.mix.job = Some(Job { progress, cancel, result });
        self.status = Some("Analizando la mezcla…".into());
    }

    /// Measures a song to compare the mix with (in the background).
    pub fn choose_reference(&mut self) {
        let dialog = rfd::FileDialog::new().add_filter("Audio", &crate::app::AUDIO_EXTENSIONS);
        let Some(path) = crate::files::pick_file(dialog) else { return };
        self.load_reference(&path);
    }

    pub fn load_reference(&mut self, path: &std::path::Path) {
        let name = path.file_stem().map_or("Referencia".into(), |s| s.to_string_lossy().into_owned());
        let sample = self.sample(&path.to_string_lossy());
        if sample.frames() == 0 {
            self.status = Some(format!("No se pudo leer «{name}»"));
            return;
        }
        let result = Arc::new(Mutex::new(None));
        let shared = result.clone();
        std::thread::spawn(move || {
            let measures = Measures::of(&sample.left, &sample.right, sample.sample_rate);
            if let Ok(mut slot) = shared.lock() {
                *slot = Some(measures);
            }
        });
        self.mix.reference_job = Some(ReferenceJob { name, result });
    }

    /// Every frame: picks up finished analyses.
    pub fn follow_mix(&mut self, ctx: &egui::Context) {
        if self.mix.job.is_none() && self.mix.reference_job.is_none() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if let Some(job) = &self.mix.reference_job {
            if let Some(measures) = job.result.lock().ok().and_then(|mut r| r.take()) {
                let name = job.name.clone();
                self.mix.reference = Some((name, measures));
                self.mix.reference_job = None;
                if self.mix.target != Target::Reference && self.mix.report.is_some() {
                    self.status = Some("Referencia lista: los consejos ya la comparan".into());
                }
            }
        }
        let Some(job) = &self.mix.job else { return };
        let Some(outcome) = job.result.lock().ok().and_then(|mut r| r.take()) else { return };
        self.mix.job = None;
        match outcome {
            Ok(report) => {
                // On the way to a loudness: another step, if it is not there yet.
                if let (Some(chase), Some(lufs)) = (self.mix.chase, report.mix.lufs) {
                    let over = report.mix.true_peak - chase.max_peak;
                    // Pushing harder no longer makes it louder: the limiter is
                    // flattening it. The last step goes back, and that is as far as it goes.
                    if let (gain, Some(before)) = chase.last {
                        let gained = (lufs - before) * (gain as f64).signum();
                        if gain > 0.0 && gained < 0.3 && over <= 0.0 && chase.goal > lufs + 0.5 {
                            self.mix.chase = None;
                            self.mix.report = Some(report);
                            self.apply_fix(&Fix::MasterLimiter { gain_db: -gain, ceiling_db: self.master_ceiling().unwrap_or(-1.0) });
                            self.mix.note = Some(format!(
                                "Con el limitador no llega a {} LUFS sin aplastarla: se queda en unos {}. Para más volumen, comprime o satura las pistas (sobre todo la batería y el bajo)",
                                num(chase.goal),
                                num(before)
                            ));
                            return;
                        }
                    }
                    if ((chase.goal - lufs).abs() > 0.5 || over > 0.0) && chase.steps < 6 {
                        let ceiling = self.master_ceiling().unwrap_or(-1.0) - if over > 0.0 { (over + 0.1) as f32 } else { 0.0 };
                        // How much loudness the last dB of gain gave.
                        let slope = match chase.last {
                            (gain, Some(before)) if gain.abs() > 0.1 => ((lufs - before) / gain as f64).clamp(0.15, 1.0),
                            _ => 1.0,
                        };
                        let gain = ((chase.goal - lufs) / slope).clamp(-12.0, 12.0) as f32;
                        self.mix.report = Some(report);
                        let was = self.master_limiter_gain();
                        self.apply_fix(&Fix::MasterLimiter { gain_db: gain, ceiling_db: ceiling.max(-12.0) });
                        // What the limiter took (it stops at its most): going back
                        // undoes that, not what was asked.
                        let applied = self.master_limiter_gain() - was;
                        if gain > 0.0 && applied < 0.05 && chase.goal > lufs + 0.5 {
                            // The limiter is at its most: that is as far as it goes.
                            self.mix.chase = None;
                            let note = format!(
                                "Con el limitador no llega a {} LUFS sin aplastarla: se queda en unos {}. Para más volumen, comprime o satura las pistas (sobre todo la batería y el bajo)",
                                num(chase.goal),
                                num(lufs)
                            );
                            self.status = Some(note.clone());
                            self.mix.note = Some(note);
                            return;
                        }
                        self.mix.chase = Some(Chase { steps: chase.steps + 1, last: (applied, Some(lufs)), ..chase });
                        return;
                    }
                    self.mix.chase = None;
                }
                self.status = Some(match report.mix.lufs {
                    Some(l) => format!("Mezcla analizada: {} LUFS, pico real {} dBTP", num(l), db(report.mix.true_peak)),
                    None => "Mezcla analizada: está en silencio".into(),
                });
                self.mix.report = Some(report);
                self.mix.stale = false;
                if let Some(note) = &self.mix.note {
                    self.status = Some(note.clone());
                }
            }
            Err(e) => self.status = Some(e),
        }
    }

    /// Brings the mix to `goal` LUFS, its true peak at most `max_peak` dBTP,
    /// with the master's limiter, step by step.
    pub fn reach_loudness(&mut self, goal: f64, max_peak: f64, gain_db: f32, ceiling_db: f32) {
        if goal >= LOUD {
            self.loud_master(ceiling_db);
        }
        let before = self.mix.report.as_ref().and_then(|r| r.mix.lufs);
        let was = self.master_limiter_gain();
        self.apply_fix(&Fix::MasterLimiter { gain_db, ceiling_db });
        let applied = self.master_limiter_gain() - was;
        self.mix.chase = Some(Chase { goal, max_peak, steps: 1, last: (applied, before) });
    }

    /// A master as loud as hard techno's: a clipper takes the peaks (the
    /// kick's) before the limiter, which then lets go quicker. A limiter alone
    /// pumps with every kick, louder no more, and lets true peaks through.
    fn loud_master(&mut self, ceiling_db: f32) {
        let devices = &mut self.project.master.devices;
        let limiter = match devices.iter().rposition(|d| matches!(d.effect, Effect::Limiter(_))) {
            Some(i) => i,
            None => {
                devices.push(Device::new(Effect::Limiter(LimiterParams { ceiling_db, ..Default::default() })));
                devices.len() - 1
            }
        };
        if let Effect::Limiter(l) = &mut devices[limiter].effect {
            l.release_ms = l.release_ms.min(LOUD_RELEASE_MS);
        }
        let clipped = limiter > 0 && matches!(devices[limiter - 1].effect, Effect::Saturator(SaturatorParams { kind: KickDrive::Hard, .. }));
        if !clipped {
            let clipper = SaturatorParams { drive_db: CLIP_DB, output_db: -CLIP_DB, mix: 1.0, kind: KickDrive::Hard, oversample: true };
            devices.insert(limiter, Device::new(Effect::Saturator(clipper)));
        }
    }

    /// The gain of the master's last limiter (0 without one).
    fn master_limiter_gain(&self) -> f32 {
        self.project.master.devices.iter().rev().find_map(|d| if let Effect::Limiter(l) = &d.effect { Some(l.gain_db) } else { None }).unwrap_or(0.0)
    }

    /// The ceiling of the master's last limiter.
    fn master_ceiling(&self) -> Option<f32> {
        self.project.master.devices.iter().rev().find_map(|d| if let Effect::Limiter(l) = &d.effect { Some(l.ceiling_db) } else { None })
    }

    /// Makes the change, and analyses again to show what it did.
    pub fn apply_fix(&mut self, fix: &Fix) {
        let track_ok = |app: &PulseApp, t: usize, name: &str| app.project.tracks.get(t).is_some_and(|tr| tr.name == name);
        match fix {
            Fix::MasterLimiter { gain_db, ceiling_db } => {
                let devices = &mut self.project.master.devices;
                match devices.iter_mut().rev().find_map(|d| if let Effect::Limiter(l) = &mut d.effect { Some(l) } else { None }) {
                    Some(l) => {
                        l.gain_db = (l.gain_db + gain_db).clamp(-24.0, MAX_LIMITER_GAIN);
                        l.ceiling_db = *ceiling_db;
                    }
                    None => devices.push(Device::new(Effect::Limiter(LimiterParams { gain_db: gain_db.clamp(-24.0, MAX_LIMITER_GAIN), ceiling_db: *ceiling_db, ..Default::default() }))),
                }
                self.status = Some("Limitador del master ajustado".into());
            }
            Fix::TrackGain { track, name, db } if track_ok(self, *track, name) => {
                let mixer = &mut self.project.tracks[*track].mixer;
                mixer.gain *= 10f32.powf(db / 20.0);
                self.status = Some(format!("«{name}» {:+.1} dB", db));
            }
            Fix::Duck { track, name, source, source_name } if track_ok(self, *track, name) => {
                // Listening to the drums; a quarter of a beat to come back.
                let sidechain = self.project.tracks.iter().any(|t| t.id == *source).then_some(*source);
                self.project.tracks[*track].devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 0.6, release: 0.25, rate_beats: 1.0, sidechain, ..Default::default() })));
                self.status = Some(format!("Ducker en «{name}» con sidechain de «{source_name}»: se aparta cada vez que suena"));
            }
            Fix::CutLows { track, name } if track_ok(self, *track, name) => {
                self.project.tracks[*track].devices.push(Device::new(Effect::Eq3(Eq3Params { low_db: -9.0, low_freq: 150.0, ..Default::default() })));
                self.status = Some(format!("Graves de «{name}» recortados (EQ Three)"));
            }
            Fix::MasterEq { low, mid, high } => {
                let devices = &mut self.project.master.devices;
                // Before the limiter, if there is one.
                let at = devices.iter().position(|d| matches!(d.effect, Effect::Limiter(_))).unwrap_or(devices.len());
                match devices.iter_mut().find_map(|d| if let Effect::Eq3(e) = &mut d.effect { Some(e) } else { None }) {
                    Some(e) => {
                        e.low_db = (e.low_db + low).clamp(-15.0, 15.0);
                        e.mid_db = (e.mid_db + mid).clamp(-15.0, 15.0);
                        e.high_db = (e.high_db + high).clamp(-15.0, 15.0);
                    }
                    None => {
                        devices.insert(at, Device::new(Effect::Eq3(Eq3Params { low_db: *low, mid_db: *mid, high_db: *high, ..Default::default() })));
                        // What pointed at the devices after it moves one along.
                        let shift = |param: Param| match param {
                            Param::Device { device, param } if device >= at => Param::Device { device: device + 1, param },
                            other => other,
                        };
                        for lane in &mut self.project.master.automation {
                            lane.param = shift(lane.param);
                        }
                        self.retarget(|t, param| Some((t, if t == engine::Target::Master { shift(param) } else { param })));
                    }
                }
                self.status = Some("EQ Three del master ajustado a la referencia".into());
            }
            Fix::Balance(changes) if changes.iter().all(|c| track_ok(self, c.track, &c.name)) => {
                for c in changes {
                    let track = &mut self.project.tracks[c.track];
                    track.mixer.gain *= 10f32.powf(c.gain_db / 20.0);
                    if let Some(pan) = c.pan {
                        track.mixer.pan = pan;
                    }
                    if let Some(hz) = c.low_cut {
                        let mut eq = EqEightParams::default();
                        eq.bands[0].on = true;
                        eq.bands[0].shape = EqShape::LowCut12;
                        eq.bands[0].freq = hz;
                        track.devices.push(Device::new(Effect::EqEight(eq)));
                    }
                }
                self.status = Some(format!("Mezcla de partida: {} pistas en su sitio (⌘Z lo deshace)", changes.len()));
            }
            Fix::Carve { track, name, hz, for_name } if track_ok(self, *track, name) => {
                let devices = &mut self.project.tracks[*track].devices;
                // In its EQ Eight, in a bell left flat (the nearest); else a new one.
                let free = devices.iter_mut().filter_map(|d| if let Effect::EqEight(eq) = &mut d.effect { Some(eq) } else { None }).find_map(|eq| {
                    let k = (0..eq.bands.len()).filter(|&k| eq.bands[k].shape == EqShape::Bell && eq.bands[k].gain_db == 0.0).min_by(|&x, &y| (eq.bands[x].freq / hz).log2().abs().total_cmp(&(eq.bands[y].freq / hz).log2().abs()))?;
                    Some(&mut eq.bands[k])
                });
                let carve = |band: &mut engine::model::EqBand| {
                    band.on = true;
                    band.freq = *hz;
                    band.gain_db = CARVE_DB;
                    band.q = CARVE_Q;
                };
                match free {
                    Some(band) => carve(band),
                    None => {
                        let mut eq = EqEightParams::default();
                        carve(&mut eq.bands[3]);
                        devices.push(Device::new(Effect::EqEight(eq)));
                    }
                }
                self.status = Some(format!("«{name}» deja hueco a «{for_name}» en {} (EQ Eight, {CARVE_DB} dB)", hz_label(*hz)));
            }
            Fix::Compress { track, name, params } if track_ok(self, *track, name) => {
                self.project.tracks[*track].devices.push(Device::new(Effect::Compressor(*params)));
                self.status = Some(format!("Compresor en «{name}»: umbral {} dB, {}:1", num(params.threshold_db as f64), num(params.ratio as f64)));
            }
            _ => {
                self.status = Some("Esa pista ya no está donde estaba: vuelve a analizar".into());
                return;
            }
        }
        self.sync_all();
        self.mix.stale = true;
        self.analyze_mix();
    }
}

// ----- Window -------------------------------------------------------------------------

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.mix.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Asistente de mezcla y master")
        .id(egui::Id::new("mix assistant"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([640.0, 640.0])
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| contents(app, ui));
    app.mix.open &= open;
}

fn contents(app: &mut PulseApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.mix.range, Range::Song, "La canción");
        ui.selectable_value(&mut app.mix.range, Range::Loop, "El loop");
        match &app.mix.job {
            Some(job) => {
                let progress = f32::from_bits(job.progress.load(Ordering::Relaxed));
                ui.add(egui::ProgressBar::new(progress).desired_width(180.0).text("Analizando…"));
                if ui.button("Cancelar").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
            None => {
                let label = if app.mix.report.is_some() { "Volver a analizar" } else { "Analizar la mezcla" };
                if ui.button(label).on_hover_text("La toca entera sin sonido (más rápido que en tiempo real), y cada pista sola, y las mide").clicked() {
                    app.mix.note = None;
                    app.analyze_mix();
                }
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Destino").color(TEXT_DIM));
        let has_reference = app.mix.reference.is_some();
        egui::ComboBox::from_id_salt("mix target").selected_text(app.mix.target.label()).width(250.0).show_ui(ui, |ui| {
            for target in Target::ALL {
                if target != Target::Reference || has_reference {
                    ui.selectable_value(&mut app.mix.target, target, target.label());
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Referencia").color(TEXT_DIM));
        match (&app.mix.reference, &app.mix.reference_job) {
            (_, Some(job)) => {
                ui.label(format!("Midiendo «{}»…", job.name));
            }
            (Some((name, m)), None) => {
                ui.label(RichText::new(format!("«{name}»: {} LUFS", m.lufs.map_or("—".into(), num))).color(TEXT));
                if ui.small_button("Quitar").clicked() {
                    app.mix.reference = None;
                    if app.mix.target == Target::Reference {
                        app.mix.target = Target::Streaming;
                    }
                }
            }
            (None, None) => {}
        }
        if ui.button("Elegir canción…").on_hover_text("Una canción que suene como quieres que suene la tuya: compara la sonoridad y el balance de graves, medios y agudos").clicked() {
            app.choose_reference();
        }
    });
    ui.separator();
    let Some(report) = app.mix.report.clone() else {
        ui.label(RichText::new("Analiza la mezcla para ver su sonoridad, sus picos y qué mejorar para el destino elegido.").color(TEXT_DIM));
        return;
    };
    if app.mix.stale && app.mix.job.is_some() {
        ui.label(RichText::new("Has cambiado la mezcla: se está volviendo a analizar").color(ACCENT).size(10.5));
    }
    let m = &report.mix;
    egui::Grid::new("mix numbers").num_columns(5).spacing(vec2(18.0, 2.0)).show(ui, |ui| {
        for title in ["Sonoridad", "Pico real", "Rango", "Lo más fuerte", "Estéreo"] {
            ui.label(RichText::new(title).color(TEXT_DIM).size(10.5));
        }
        ui.end_row();
        let big = |text: String| RichText::new(text).size(20.0).strong();
        ui.label(big(m.lufs.map_or("—".into(), |l| format!("{} LUFS", num(l)))));
        ui.label(big(format!("{} dBTP", db(m.true_peak)))).on_hover_text(format!("Entre muestras; el pico de las muestras es {} dBFS", db(m.peak)));
        ui.label(big(m.range.map_or("—".into(), |r| format!("{} LU", num(r)))));
        ui.label(big(m.short_max.map_or("—".into(), |l| format!("{} LUFS", num(l)))));
        ui.label(big(format!("{:.2}", m.correlation)));
        ui.end_row();
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Consejos").strong());
    let reference = app.mix.reference.as_ref().map(|(_, m)| m.clone());
    let list = advise(&report, app.mix.target, reference.as_ref());
    let mut chosen = None;
    let busy = app.mix.job.is_some();
    for a in &list {
        ui.horizontal_wrapped(|ui| {
            let (mark, color) = match a.level {
                Level::Good => ("●", PLAY),
                Level::Warning => ("▲", METER_RED),
                Level::Note => ("•", ACCENT),
            };
            ui.label(RichText::new(mark).color(color));
            ui.label(&a.text);
            if let Some(fix) = &a.fix {
                if ui.add_enabled(!busy, egui::Button::new("Aplicar").small()).on_hover_text(fix_hint(fix)).clicked() {
                    chosen = Some(fix.clone());
                }
            }
        });
        for line in &a.details {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(RichText::new(line).color(TEXT_DIM).size(10.5));
            });
        }
    }
    if let Some(note) = &app.mix.note {
        ui.label(RichText::new(note).color(ACCENT));
    }
    if let Some(fix) = chosen {
        app.mix.note = None;
        match (&fix, app.mix.target.levels(reference.as_ref())) {
            (Fix::MasterLimiter { gain_db, ceiling_db }, Some((goal, peak))) => app.reach_loudness(goal, peak, *gain_db, *ceiling_db),
            _ => app.apply_fix(&fix),
        }
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Balance por octavas").strong());
    balance_chart(ui, &m.bands, reference.as_ref().map(|r| &r.bands));
    ui.add_space(6.0);
    egui::CollapsingHeader::new("Pistas").default_open(false).show(ui, |ui| {
        egui::Grid::new("mix tracks").striped(true).spacing(vec2(14.0, 2.0)).show(ui, |ui| {
            for title in ["Pista", "Pico", "Sonoridad", "Estéreo"] {
                ui.label(RichText::new(title).color(TEXT_DIM).size(10.5));
            }
            ui.end_row();
            for t in &report.tracks {
                ui.label(&t.name);
                ui.label(RichText::new(format!("{} dBFS", db(t.peak))).color(if t.peak > 0.0 { METER_RED } else { TEXT }));
                ui.label(t.lufs.map_or("silencio".into(), |l| format!("{} LUFS", num(l))));
                ui.label(format!("{:.2}", t.correlation));
                ui.end_row();
            }
        });
    });
}

fn fix_hint(fix: &Fix) -> String {
    match fix {
        Fix::MasterLimiter { gain_db, ceiling_db } => format!("Limiter en el master: ganancia {gain_db:+.1} dB, techo {ceiling_db:.1} dB"),
        Fix::TrackGain { name, db, .. } => format!("Volumen de «{name}» {db:+.1} dB"),
        Fix::Duck { name, source_name, .. } => format!("Añade un Ducker a «{name}» que escucha a «{source_name}» (sidechain): baja cuando suena, para dejarle sitio"),
        Fix::CutLows { name, .. } => format!("Añade un EQ Three a «{name}» con los graves (bajo 150 Hz) a −9 dB"),
        Fix::MasterEq { low, mid, high } => format!("EQ Three en el master: graves {low:+.1}, medios {mid:+.1}, agudos {high:+.1} dB"),
        Fix::Carve { name, hz, for_name, .. } => format!("Baja {} dB «{name}» en {} (un EQ Eight, campana estrecha) para que «{for_name}» se oiga ahí", -CARVE_DB, hz_label(*hz)),
        Fix::Compress { name, params, .. } => format!(
            "Añade un Compressor a «{name}»: umbral {:.1} dB, ratio {}:1, ataque {:.0} ms, release {:.0} ms (una semicorchea al tempo; el bajo, una corchea) y {:+.1} dB de ganancia para que siga igual de fuerte",
            params.threshold_db, params.ratio, params.attack_ms, params.release_ms, params.makeup_db
        ),
        Fix::Balance(_) => "Mueve volúmenes y panoramas y pone un EQ Eight con corte de graves donde dice: un punto de partida para seguir mezclando a oído. ⌘Z lo deshace todo".into(),
    }
}

/// The mix's octave bands as bars, and the reference's as a line, both
/// levelled to the same average.
fn balance_chart(ui: &mut egui::Ui, mix: &[f64; 10], reference: Option<&[f64; 10]>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(560.0), 120.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, crate::theme::SLOT);
    let level = |b: &[f64; 10]| b[1..=8].iter().sum::<f64>() / 8.0;
    // ±18 dB around the average.
    let y_of = |d: f64| rect.center().y - (d.clamp(-18.0, 18.0) / 18.0) as f32 * (rect.height() / 2.0 - 14.0);
    let w = rect.width() / 10.0;
    painter.line_segment([pos2(rect.left(), y_of(0.0)), pos2(rect.right(), y_of(0.0))], Stroke::new(1.0, Color32::from_gray(70)));
    let mean = level(mix);
    for (i, &b) in mix.iter().enumerate() {
        let x = rect.left() + w * (i as f32 + 0.5);
        let y = y_of(b - mean);
        let bar = Rect::from_two_pos(pos2(x - w * 0.3, y_of(0.0)), pos2(x + w * 0.3, y));
        painter.rect_filled(bar, 1.0, ACCENT.gamma_multiply(0.8));
        let hz = stems::loudness::BAND_HZ[i];
        let label = if hz >= 1000.0 { format!("{}k", hz / 1000.0) } else { format!("{hz}") };
        painter.text(pos2(x, rect.bottom() - 2.0), Align2::CENTER_BOTTOM, label, crate::theme::small(), TEXT_DIM);
    }
    if let Some(reference) = reference {
        let r_mean = level(reference);
        let points: Vec<egui::Pos2> = reference.iter().enumerate().map(|(i, &b)| pos2(rect.left() + w * (i as f32 + 0.5), y_of(b - r_mean))).collect();
        painter.add(egui::Shape::line(points, Stroke::new(2.0, Color32::WHITE)));
        painter.text(rect.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, "Barras: tu mezcla · línea: la referencia", crate::theme::small(), TEXT_DIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(index: usize, name: &str, drums: bool, peak: f64, low: Vec<f64>) -> TrackMeasures {
        TrackMeasures { index, id: index as u32 + 10, name: name.into(), drums, kick: false, listens: Vec::new(), peak, lufs: Some(-20.0), correlation: 0.9, low_end: low, role: Role::Other, pan: 0.0, cut: false, bands: [0.0; 10], gain: 0.5, mids: Vec::new(), carves: Vec::new(), dynamics: None, compressed: false }
    }

    fn report(lufs: f64, true_peak: f64, tracks: Vec<TrackMeasures>) -> Report {
        Report { mix: Measures { lufs: Some(lufs), range: Some(6.0), short_max: Some(lufs + 3.0), true_peak, peak: true_peak, correlation: 0.8, bands: [0.0; 10] }, tracks, tempo: 120.0 }
    }

    /// A hard techno track (the Hard techno machine's beat and an acid bass at
    /// 150) brought to the genre's -7 LUFS with the master's limiter, its
    /// true peak under -1 dBTP; told when louder than that.
    #[test]
    fn a_hard_techno_track_reaches_minus_seven() {
        use crate::drum_gen::{self, Style};
        use crate::project::{Project, Track};
        use engine::model::{ArrangementClip, Clip, InstrumentKind, MidiClip, Note};
        let mut project = Project::empty();
        project.tempo = 150.0;
        let kit = drum_gen::Kit::from_rows(&engine::model::DRUM_VOICES.iter().map(|&(p, n)| (p, n.to_string())).collect::<Vec<_>>());
        let settings = drum_gen::Settings { style: Style::HardTechno, level: drum_gen::Level::Medium, bars: 8, sounds: Style::HardTechno.sounds(), ..Default::default() };
        let (notes, groove) = drum_gen::generate(&kit, &settings, 32.0, &[], 0);
        let mut drums = Track::new("Drums", [0; 3], InstrumentKind::DrumMachine(Style::HardTechno.sound()));
        drums.arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes, groove, ..MidiClip::new("Beat", 32.0, [0; 3]) }) });
        let mut bass = Track::new("Acid", [0; 3], InstrumentKind::Acid(Default::default()));
        let line: Vec<Note> = (0..128).filter(|s| s % 4 != 0).map(|s| Note { start: s as f64 * 0.25, length: 0.2, pitch: 29 + 12 * (s % 3 == 0) as u8, velocity: 0.9, chance: 1.0 }).collect();
        bass.arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(MidiClip { notes: line, ..MidiClip::new("Acid", 32.0, [0; 3]) }) });
        // Saturated, as the genre's drums are: their peaks give way to loudness.
        drums.devices.push(engine::model::Device::new(Effect::Saturator(engine::model::SaturatorParams { drive_db: 9.0, output_db: -6.0, kind: engine::model::KickDrive::Hard, ..Default::default() })));
        project.tracks = vec![drums, bass];
        let mut app = PulseApp::headless(project);
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 120);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let before = app.mix.report.as_ref().unwrap().mix.lufs.unwrap();
        app.mix.target = Target::HardTechno;
        let list = advise(app.mix.report.as_ref().unwrap(), Target::HardTechno, None);
        let Some(Fix::MasterLimiter { gain_db, ceiling_db }) = list.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::MasterLimiter { .. }))) else { panic!("a limiter fix: {list:?}") };
        app.reach_loudness(-7.0, -1.0, gain_db, ceiling_db);
        wait(&mut app);
        let mix = app.mix.report.as_ref().unwrap().mix.clone();
        let limiter = app.project.master.devices.iter().find_map(|d| if let Effect::Limiter(l) = &d.effect { Some(l.gain_db) } else { None });
        println!("hard techno: {before:.1} → {:.1} LUFS (objetivo -7), pico real {:.1} dBTP, limitador {limiter:?} dB · {:?}", mix.lufs.unwrap(), mix.true_peak, app.status);
        assert!((mix.lufs.unwrap() - -7.0).abs() <= 1.0, "{:?}", mix.lufs);
        assert!(mix.true_peak <= -1.0);
        // A clipper right before the limiter, which lets go quicker.
        let kinds: Vec<&str> = app.project.master.devices.iter().map(|d| d.effect.label()).collect();
        assert_eq!(kinds, ["Saturator", "Limiter"]);
        assert!(matches!(app.project.master.devices[1].effect, Effect::Limiter(l) if l.release_ms == LOUD_RELEASE_MS));
        assert!(advise(app.mix.report.as_ref().unwrap(), Target::HardTechno, None).iter().any(|a| a.level == Level::Good && a.text.contains("Hard techno, rave")));
        // Over it: no platform turns a club track down, so it isn't said.
        let loud = Report { mix: Measures { lufs: Some(-5.0), true_peak: -1.2, ..mix }, tracks: Vec::new(), tempo: 150.0 };
        let told = advise(&loud, Target::HardTechno, None);
        assert!(told.iter().any(|a| a.text.contains("pierde pegada")) && !told.iter().any(|a| a.text.contains("plataforma")), "{told:?}");
    }

    #[test]
    fn quiet_mixes_get_a_limiter_and_loud_ones_are_told() {
        let quiet = advise(&report(-20.0, -6.0, Vec::new()), Target::Streaming, None);
        assert!(quiet.iter().any(|a| a.fix == Some(Fix::MasterLimiter { gain_db: 6.0, ceiling_db: -1.8 })), "{quiet:?}");
        let right = advise(&report(-14.3, -1.5, Vec::new()), Target::Streaming, None);
        assert!(right.iter().all(|a| a.level == Level::Good), "{right:?}");
        // On target but peaking over -1 dBTP: the ceiling comes down.
        let peaky = advise(&report(-14.0, 0.4, Vec::new()), Target::Streaming, None);
        assert!(peaky.iter().any(|a| a.fix == Some(Fix::MasterLimiter { gain_db: 0.0, ceiling_db: -1.8 })));
        let club = advise(&report(-14.0, -1.5, Vec::new()), Target::Club, None);
        assert!(club.iter().any(|a| matches!(a.fix, Some(Fix::MasterLimiter { gain_db, .. }) if gain_db == 5.0)));
    }

    #[test]
    fn tracks_over_zero_and_clashing_low_ends_are_found() {
        let on = vec![-10.0; 100];
        let half: Vec<f64> = (0..100).map(|i| if i % 2 == 0 { -10.0 } else { -60.0 }).collect();
        let off = vec![-60.0; 100];
        let tracks = vec![track(0, "Kick", true, -3.0, half), track(1, "Bass", false, 2.0, on.clone()), track(2, "Hats", true, -6.0, off), track(3, "Pad", false, -8.0, on)];
        let list = advise(&report(-14.0, -1.5, tracks), Target::Streaming, None);
        assert!(list.iter().any(|a| a.fix == Some(Fix::TrackGain { track: 1, name: "Bass".into(), db: -3.0 })), "{list:?}");
        // Kick and bass: the bass ducks. Bass and pad: the pad gives up its lows.
        assert!(list.iter().any(|a| a.fix == Some(Fix::Duck { track: 1, name: "Bass".into(), source: 10, source_name: "Kick".into() })));
        assert!(list.iter().any(|a| matches!(&a.fix, Some(Fix::CutLows { .. }))));
        assert!(!list.iter().any(|a| a.text.contains("Hats")));
    }

    #[test]
    fn the_balance_is_compared_with_the_reference() {
        let flat = [0.0; 10];
        let mut boomy = flat;
        for b in &mut boomy[1..=3] {
            *b += 6.0;
        }
        let [low, mid, high] = balance(&boomy, &flat);
        assert!(low > 3.0 && mid < 0.0 && high < 0.0, "{low} {mid} {high}");
        let reference = Measures { lufs: Some(-9.0), bands: flat, ..Default::default() };
        let mut r = report(-14.0, -1.5, Vec::new());
        r.mix.bands = boomy;
        let list = advise(&r, Target::Reference, Some(&reference));
        assert!(list.iter().any(|a| a.text.starts_with("Graves")));
        assert!(list.iter().any(|a| matches!(a.fix, Some(Fix::MasterEq { low, .. }) if low < -3.0)));
        // As loud as the reference: 5 dB up.
        assert!(list.iter().any(|a| matches!(a.fix, Some(Fix::MasterLimiter { gain_db, .. }) if gain_db == 5.0)));
    }

    /// The demo's loop, analysed for real: a loudness, a peak, one entry per
    /// track; and a limiter brings it to the target.
    #[test]
    fn the_demo_loop_is_analysed_and_brought_to_the_target() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        // Its clips on the arrangement, a bar long loop.
        for t in 0..app.project.tracks.len() {
            if let Some(clip) = app.project.tracks[t].clips[0].clone() {
                app.project.tracks[t].arrangement = vec![engine::model::ArrangementClip { start: 0.0, length: 8.0, clip }];
            }
        }
        app.project.loop_start = 0.0;
        app.project.loop_end = 8.0;
        app.mix.range = Range::Loop;
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 60);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let report = app.mix.report.clone().expect("a report");
        assert_eq!(report.tracks.len(), app.project.tracks.len());
        let before = report.mix.lufs.unwrap();
        assert!(before > -40.0 && before < 0.0, "{before}");
        // Bring it to the club's -9 LUFS.
        app.mix.target = Target::Club;
        let list = advise(&report, Target::Club, None);
        let Some(Fix::MasterLimiter { gain_db, ceiling_db }) = list.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::MasterLimiter { .. }))) else { panic!("a limiter fix") };
        // -7 is more than this loop gives without squashing it: it stops short
        // and says so, without the limiter driven to the end.
        app.reach_loudness(-7.0, -1.0, gain_db + 2.0, ceiling_db);
        wait(&mut app);
        let after = app.mix.report.as_ref().unwrap().mix.lufs.unwrap();
        println!("demo: {before:.1} → {after:.1} LUFS (objetivo -7), pico real {:.1} dBTP", app.mix.report.as_ref().unwrap().mix.true_peak);
        assert!(after > before + 1.5, "{before} → {after}");
        assert!(app.mix.report.as_ref().unwrap().mix.true_peak <= -1.0, "under -1 dBTP");
        let limiter = |app: &PulseApp| app.project.master.devices.iter().find_map(|d| if let Effect::Limiter(l) = &d.effect { Some(*l) } else { None }).unwrap();
        assert!(limiter(&app).gain_db <= MAX_LIMITER_GAIN);
        assert!(app.status.as_deref().is_some_and(|s| s.contains("sin aplastarla")), "{:?}", app.status);
        // A loudness it can reach, it reaches.
        app.reach_loudness(-11.0, -1.0, (-11.0 - after) as f32, limiter(&app).ceiling_db);
        wait(&mut app);
        let report = app.mix.report.clone().unwrap();
        println!("demo: → {:.1} LUFS (objetivo -11), pico real {:.1} dBTP", report.mix.lufs.unwrap(), report.mix.true_peak);
        assert!((report.mix.lufs.unwrap() - -11.0).abs() <= 0.5);
        assert!(report.mix.true_peak <= -1.0);
        // The window draws it: the numbers, the advice and the tracks.
        app.open_panel(crate::app::Panel::Mix);
        let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0))), ..Default::default() };
        let _ = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let output = ctx.run_ui(input(), |ui| window(&mut app, ui.ctx()));
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => texts.push(t.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        for expected in ["Sonoridad", "Pico real", "Consejos", "Balance por octavas", "1k"] {
            assert!(texts.iter().any(|t| t == expected), "{expected}: {texts:?}");
        }
        assert!(texts.iter().any(|t| t.ends_with(" LUFS")));
        assert!(app.project.master.devices.iter().any(|d| matches!(d.effect, Effect::Limiter(_))));
    }

    /// Two drum tracks in the low end at once: the kick of its own keeps it, the
    /// other drums give it up. A track already ducking to the other is fine.
    #[test]
    fn the_kick_keeps_the_low_end_over_other_drums() {
        let on = vec![-10.0; 100];
        let kick = TrackMeasures { kick: true, ..track(0, "Kick", true, -3.0, on.clone()) };
        let list = advise(&report(-9.0, -1.5, vec![track(1, "Drums", true, -6.0, on.clone()), kick.clone()]), Target::Club, None);
        assert!(list.iter().any(|a| a.fix == Some(Fix::CutLows { track: 1, name: "Drums".into() })), "{list:?}");
        let rumble = TrackMeasures { listens: vec![kick.id], ..track(2, "Kick Rumble", true, -10.0, on.clone()) };
        let list = advise(&report(-9.0, -1.5, vec![kick.clone(), rumble]), Target::Club, None);
        assert!(list.iter().all(|a| a.fix.is_none()), "{list:?}");
        assert!(list.iter().any(|a| a.text == "«Kick Rumble» se aparta cuando suena «Kick»: sus graves no se tapan"));
        // A bass against the kick and against its rumble: it ducks to the kick, once.
        let rumble = TrackMeasures { listens: vec![kick.id], ..track(2, "Kick Rumble", true, -10.0, on.clone()) };
        let bass = track(3, "Bass", false, -6.0, on);
        let list = advise(&report(-9.0, -1.5, vec![kick.clone(), rumble, bass]), Target::Club, None);
        let ducks: Vec<&Fix> = list.iter().filter_map(|a| a.fix.as_ref()).filter(|f| matches!(f, Fix::Duck { .. })).collect();
        assert_eq!(ducks, [&Fix::Duck { track: 3, name: "Bass".into(), source: kick.id, source_name: "Kick".into() }], "{list:?}");
        // Once it does, against the rumble too it is fine.
        let rumble = TrackMeasures { listens: vec![kick.id], ..track(2, "Kick Rumble", true, -10.0, vec![-10.0; 100]) };
        let bass = TrackMeasures { listens: vec![kick.id], ..track(3, "Bass", false, -6.0, vec![-10.0; 100]) };
        let list = advise(&report(-9.0, -1.5, vec![kick.clone(), rumble, bass]), Target::Club, None);
        assert!(list.iter().all(|a| a.fix.is_none()), "{list:?}");
    }

    /// A hard techno kick on its own track (the Kick Designer) and a bass: the
    /// assistant takes the kick for drums and has the bass duck to it; once it
    /// does, the clash is gone. A rumble ducking to its kick is no clash either.
    #[test]
    fn a_kick_designer_track_is_drums_and_the_bass_ducks_to_it() {
        use crate::project::{Project, Track};
        use engine::model::{ArrangementClip, Clip, InstrumentKind, KickParams, MidiClip, Note};
        let mut project = Project::empty();
        project.tempo = 150.0;
        let mut kick = Track::new("Kick", [200, 0, 0], InstrumentKind::Kick(KickParams::presets().into_iter().find(|(n, _)| *n == "Hard · Kick").unwrap().1));
        let mut beat = MidiClip::new("4/4", 32.0, [0; 3]);
        beat.notes = (0..32).map(|b| Note { start: b as f64, length: 0.25, pitch: 36, velocity: 1.0, chance: 1.0 }).collect();
        kick.arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(beat) });
        let mut bass = Track::new("Bass", [0, 200, 0], InstrumentKind::Acid(engine::model::AcidParams { cutoff: 300.0, ..Default::default() }));
        let mut line = MidiClip::new("Bass", 32.0, [0; 3]);
        line.notes = (0..128).filter(|s| s % 4 != 0).map(|s| Note { start: s as f64 * 0.25, length: 0.2, pitch: 29, velocity: 0.9, chance: 1.0 }).collect();
        bass.arrangement.push(ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Midi(line) });
        project.tracks = vec![kick, bass];
        let mut app = PulseApp::headless(project);
        app.make_rumble(0);
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 120);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let report = app.mix.report.clone().unwrap();
        let kick = report.tracks.iter().find(|t| t.name == "Kick").unwrap();
        assert!(kick.drums && kick.kick);
        let advice = advise(&report, Target::Club, None);
        for a in &advice {
            println!("{} → {:?}", a.text, a.fix);
        }
        let duck = advice.iter().find_map(|a| match &a.fix {
            Some(Fix::Duck { name, source_name, .. }) if name == "Bass" && source_name == "Kick" => a.fix.clone(),
            _ => None,
        });
        assert!(duck.is_some(), "the bass ducks to the kick");
        assert!(!advice.iter().any(|a| matches!(&a.fix, Some(Fix::CutLows { name, .. }) if name.contains("Kick"))), "no kick (or its rumble) loses its lows");
        assert!(!advice.iter().any(|a| a.fix.is_some() && a.text.contains("Rumble")), "the rumble ducks to its kick already");
        // Applied, the bass listens to the kick: it says so instead of the clash.
        app.apply_fix(&duck.unwrap());
        assert!(listens(&app.project.tracks[2]).contains(&app.project.tracks[0].id));
        app.mix.report = None;
        app.analyze_mix();
        wait(&mut app);
        let advice = advise(&app.mix.report.clone().unwrap(), Target::Club, None);
        assert!(!advice.iter().any(|a| matches!(a.fix, Some(Fix::Duck { .. }))), "{advice:?}");
    }

    #[test]
    fn a_tracks_role_comes_from_its_name_or_instrument() {
        use engine::model::{AcidParams, KickParams};
        let synth = InstrumentKind::Synth(Default::default());
        for (name, role) in [
            ("Kick", Role::Kick),
            ("Bombo 909", Role::Kick),
            ("Kick Rumble", Role::Bass),
            ("Bajo", Role::Bass),
            ("Sub 808", Role::Bass),
            ("Batería", Role::Drums),
            ("Hi-Hats", Role::Percussion),
            ("Charles abierto", Role::Percussion),
            ("Voz principal", Role::Vocal),
            ("Coros", Role::Backing),
            ("Voz FX", Role::Fx),
            ("Melodía", Role::Lead),
            ("Acordes", Role::Chords),
            ("Piano", Role::Chords),
            ("Pad oscuro", Role::Pad),
        ] {
            assert_eq!(role_hint(name, &synth, false), Some(role), "{name}");
        }
        assert_eq!(role_hint("Pista 3", &synth, false), None);
        assert_eq!(role_hint("Pista 3", &InstrumentKind::Kick(KickParams::default()), false), Some(Role::Kick));
        assert_eq!(role_hint("Pista 3", &InstrumentKind::Acid(AcidParams::default()), false), Some(Role::Bass));
        assert_eq!(role_hint("Pista 3", &synth, true), Some(Role::Drums));
        // By its sound: lows, a bass; highs, percussion.
        assert_eq!(role_by_sound(&[-10.0, -5.0, -8.0, -25.0, -35.0, -40.0, -45.0, -50.0, -55.0, -60.0]), Role::Bass);
        assert_eq!(role_by_sound(&[-60.0, -55.0, -50.0, -45.0, -40.0, -30.0, -20.0, -8.0, -5.0, -9.0]), Role::Percussion);
        assert_eq!(role_by_sound(&[-30.0; 10]), Role::Other);
    }

    /// Each track where its role usually starts, next to the kick: the bass
    /// up and to the middle, the pad down without its lows, two chord tracks
    /// sharing their level and spread, the voice where it was.
    #[test]
    fn the_tracks_start_where_their_role_does() {
        let t = |index: usize, name: &str, role: Role, lufs: f64, peak: f64, pan: f32, lows: bool| TrackMeasures {
            role,
            pan,
            lufs: Some(lufs),
            bands: if lows { [-20.0; 10] } else { [-70.0, -70.0, -40.0, -30.0, -20.0, -15.0, -12.0, -10.0, -10.0, -12.0] },
            ..track(index, name, false, peak, Vec::new())
        };
        let tracks = vec![
            t(0, "Kick", Role::Kick, -12.0, -3.0, 0.0, true),
            t(1, "Bajo", Role::Bass, -20.0, -10.0, 0.4, true),
            t(2, "Pad", Role::Pad, -10.0, -4.0, 0.0, true),
            t(3, "Acordes A", Role::Chords, -17.0, -8.0, 0.0, false),
            t(4, "Acordes B", Role::Chords, -17.0, -8.0, 0.0, false),
            t(5, "Voz", Role::Vocal, -12.5, -6.0, 0.0, true),
            t(6, "Hats", Role::Percussion, -20.0, -12.0, 0.0, false),
            t(7, "Ruido raro", Role::Other, -30.0, -20.0, 0.7, true),
            // Too quiet to raise it all the way: up to a dB under full scale.
            t(8, "Lead", Role::Lead, -30.0, -4.0, 0.0, false),
        ];
        let (anchor, changes) = balance_tracks(&tracks);
        assert_eq!(anchor.unwrap().name, "Kick");
        for c in &changes {
            println!("{}", c.describe());
        }
        let of = |name: &str| changes.iter().find(|c| c.name == name).cloned();
        assert_eq!(of("Bajo"), Some(Change { track: 1, name: "Bajo".into(), role: Role::Bass, gain_db: 6.0, pan: Some(0.0), low_cut: None }));
        assert_eq!(of("Pad"), Some(Change { track: 2, name: "Pad".into(), role: Role::Pad, gain_db: -10.0, pan: None, low_cut: Some(150.0) }));
        // Two of them: 3 dB under the chords' level each, one each side.
        assert_eq!(of("Acordes A").map(|c| (c.gain_db, c.pan)), Some((-3.0, Some(-0.35))));
        assert_eq!(of("Acordes B").map(|c| (c.gain_db, c.pan)), Some((-3.0, Some(0.35))));
        // Within a dB and a half: as it is, but its rumble goes.
        assert_eq!(of("Voz").map(|c| (c.gain_db, c.pan, c.low_cut)), Some((0.0, None, Some(90.0))));
        assert_eq!(of("Lead").map(|c| c.gain_db), Some(3.0));
        assert!(of("Kick").is_none() && of("Hats").is_none() && of("Ruido raro").is_none());
        let list = advise(&report(-14.0, -1.5, tracks.clone()), Target::Streaming, None);
        let balance = list.iter().find(|a| matches!(a.fix, Some(Fix::Balance(_)))).expect("the balance advice");
        assert!(balance.text.contains("6 pistas") && balance.text.contains("«Kick»"), "{}", balance.text);
        assert_eq!(balance.details.len(), 6);
        // Nothing to set it from: no advice about it.
        let none: Vec<TrackMeasures> = tracks.into_iter().filter(|t| matches!(t.role, Role::Pad | Role::Other)).collect();
        assert!(balance_tracks(&none).0.is_none());
    }

    /// A song from «De tarareo a canción» (melody, chords, bass, drums),
    /// analysed for real, balanced, and analysed again: then it is balanced.
    #[test]
    fn a_song_is_balanced_in_one_step() {
        use crate::project::{Project, Track};
        use engine::model::{ArrangementClip, Clip, MidiClip, Note};
        let mut project = Project::empty();
        let mut voice = Track::new("Tarareo", [10, 20, 30], InstrumentKind::Synth(Default::default()));
        voice.id = 1;
        let tune = [[60, 64, 67, 64], [65, 69, 72, 69], [67, 71, 74, 71], [72, 67, 64, 60]];
        let notes: Vec<Note> = tune.iter().enumerate().flat_map(|(b, ns)| ns.iter().enumerate().map(move |(k, &p)| Note { start: b as f64 * 4.0 + k as f64, length: 1.0, pitch: p, velocity: 0.8, chance: 1.0 })).collect();
        voice.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip { notes, ..MidiClip::new("Tarareo", 16.0, [10, 20, 30]) }) }];
        project.tracks = vec![voice];
        let mut app = PulseApp::headless(project);
        app.hum_to_song(crate::app::ClipRef::Arrangement { track: 0, index: 0 }, crate::drum_gen::Style::PopRock);
        // The hummed tune itself out of the way: the song is the four tracks.
        app.project.tracks[0].mixer.active = false;
        // Out of balance: the pad-like chords far too loud, the bass far too quiet.
        app.project.tracks[2].mixer.gain *= 4.0;
        app.project.tracks[3].mixer.gain *= 0.1;
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 120);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let report = app.mix.report.clone().unwrap();
        let roles: Vec<(String, Role)> = report.tracks.iter().map(|t| (t.name.clone(), t.role)).collect();
        println!("{roles:?}");
        for (name, role) in [("Melodía", Role::Lead), ("Acordes", Role::Chords), ("Bajo", Role::Bass), ("Batería", Role::Drums)] {
            assert!(roles.contains(&(name.to_string(), role)), "{name}: {roles:?}");
        }
        let advice = advise(&report, Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Balance(_)))).expect("a balance to apply");
        let Fix::Balance(changes) = &fix else { unreachable!() };
        for c in changes {
            println!("{}", c.describe());
        }
        assert!(changes.iter().any(|c| c.name == "Acordes" && c.gain_db < -6.0));
        assert!(changes.iter().any(|c| c.name == "Bajo" && c.gain_db > 6.0));
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let after = app.mix.report.clone().unwrap();
        let (_, left) = balance_tracks(&after.tracks);
        assert!(left.is_empty(), "balanced once applied: {left:?}");
        assert!(advise(&after, Target::Streaming, None).iter().any(|a| a.level == Level::Good && a.text.starts_with("Las pistas están equilibradas")));
        // Lows cut where the plan said (only where there were lows to cut),
        // never on the bass.
        for t in 1..app.project.tracks.len() {
            let planned = changes.iter().any(|c| c.track == t && c.low_cut.is_some());
            assert_eq!(cuts_lows(&app.project.tracks[t]), planned, "{}", app.project.tracks[t].name);
        }
        assert!(!cuts_lows(&app.project.tracks[3]), "not the bass");
    }

    /// A 1 kHz tone at half scale is −6 dB in its band and nothing in the
    /// far ones.
    #[test]
    fn the_mids_are_measured_band_by_band() {
        let tone: Vec<f32> = (0..48_000).map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / RATE).sin()).collect();
        let frames = mids(&tone, &tone, RATE);
        assert_eq!(frames.len(), 10);
        let f = frames[3];
        println!("{f:?}");
        assert!((f[4] + 6.0).abs() < 0.5, "{}", f[4]);
        assert!(f[0] < MID_ON && f[8] < MID_ON);
    }

    fn with_mids(index: usize, name: &str, role: Role, lufs: f64, mids: Vec<[f32; MID_BANDS]>) -> TrackMeasures {
        TrackMeasures { role, lufs: Some(lufs), mids, ..track(index, name, false, -6.0, Vec::new()) }
    }

    /// The voice and the chords as loud at 1 kHz: the chords make room. A
    /// pad well under, a hi-hat far up and a bass: no clash with them.
    #[test]
    fn tracks_covering_each_other_in_the_mids_are_found() {
        let band = |k: usize, level: f32| -> [f32; MID_BANDS] { std::array::from_fn(|b| if b == k { level } else { -80.0 }) };
        let voice = with_mids(0, "Voz", Role::Vocal, -14.0, vec![band(4, -20.0); 100]);
        let chords = with_mids(1, "Acordes", Role::Chords, -16.0, vec![band(4, -23.0); 100]);
        let pad = with_mids(2, "Pad", Role::Pad, -24.0, vec![band(4, -40.0); 100]);
        let hats = with_mids(3, "Hats", Role::Percussion, -20.0, vec![band(8, -20.0); 100]);
        let bass = with_mids(4, "Bajo", Role::Bass, -14.0, vec![band(0, -20.0); 100]);
        let tracks = vec![voice, chords, pad, hats, bass];
        let clashes = mid_clashes(&tracks);
        assert_eq!(clashes, [MidClash { yields: 1, to: 0, band: 4, share: 1.0 }]);
        let list = advise(&report(-14.0, -1.5, tracks.clone()), Target::Streaming, None);
        let carve = list.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Carve { .. })));
        assert_eq!(carve, Some(Fix::Carve { track: 1, name: "Acordes".into(), hz: 1000.0, for_name: "Voz".into() }));
        // Two of a kind: the quieter makes room. Once it does, all is well.
        let mut alike = tracks.clone();
        alike[0].role = Role::Chords;
        assert_eq!(mid_clashes(&alike)[0].yields, 1);
        let mut carved = tracks;
        carved[1].carves = vec![1050.0];
        let list = advise(&report(-14.0, -1.5, carved), Target::Streaming, None);
        assert!(list.iter().all(|a| !matches!(a.fix, Some(Fix::Carve { .. }))));
        assert!(list.iter().any(|a| a.level == Level::Good && a.text == "«Acordes» deja hueco a «Voz» en 1,0 kHz"), "{list:?}");
    }

    /// For real: a melody and chords in the same octave, analysed. The chords
    /// make room for the melody; applied, they do, in an EQ Eight.
    #[test]
    fn chords_make_room_for_the_melody() {
        use crate::project::{Project, Track};
        use engine::model::{ArrangementClip, Clip, MidiClip, Note};
        let note = |start: f64, length: f64, pitch: u8| Note { start, length, pitch, velocity: 0.8, chance: 1.0 };
        let mut project = Project::empty();
        let mut lead = Track::new("Melodía", [0; 3], InstrumentKind::Synth(Default::default()));
        let tune: Vec<Note> = (0..16).map(|k| note(k as f64, 1.0, [72, 76, 79, 76][k % 4])).collect();
        lead.arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: tune, ..MidiClip::new("Melodía", 16.0, [0; 3]) }) });
        let mut chords = Track::new("Acordes", [0; 3], InstrumentKind::Synth(Default::default()));
        let voiced: Vec<Note> = (0..4).flat_map(|bar| [72, 76, 79].map(|p| note(bar as f64 * 4.0, 4.0, p))).collect();
        chords.arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(MidiClip { notes: voiced, ..MidiClip::new("Acordes", 16.0, [0; 3]) }) });
        project.tracks = vec![lead, chords];
        let mut app = PulseApp::headless(project);
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 120);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let report = app.mix.report.clone().unwrap();
        let clashes = mid_clashes(&report.tracks);
        println!("{clashes:?}");
        let advice = advise(&report, Target::Streaming, None);
        for a in &advice {
            println!("{} → {:?}", a.text, a.fix);
        }
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Carve { .. }))).expect("room to make");
        let Fix::Carve { name, for_name, hz, .. } = &fix else { unreachable!() };
        assert_eq!((name.as_str(), for_name.as_str()), ("Acordes", "Melodía"));
        assert!((500.0..=2000.0).contains(hz), "{hz}");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        assert_eq!(carves(&app.project.tracks[1]), vec![*hz]);
        let after = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        assert!(!after.iter().any(|a| matches!(&a.fix, Some(Fix::Carve { hz: h, .. }) if h == hz)), "{after:?}");
        assert!(after.iter().any(|a| a.level == Level::Good && a.text.starts_with("«Acordes» deja hueco a «Melodía»")));
    }

    /// A voice going from −22 to −6 dBFS gets a compressor that takes 5 dB
    /// off its loud parts: the threshold from its level before the fader
    /// (−6 dB here) and the pan, a sixteenth's release at 120.
    #[test]
    fn a_voice_that_moves_too_much_gets_a_compressor() {
        let voice = TrackMeasures { role: Role::Vocal, dynamics: Some((-6.0, -22.0)), gain: 0.5, pan: 0.0, ..track(0, "Voz", false, -3.0, Vec::new()) };
        let params = compression(&voice, 120.0).unwrap();
        // Before the fader: −6 + 6 dB = 0; 5 dB off at 3:1 is 7.5 dB over the threshold.
        let fader = 20.0 * 0.5f32.log10();
        assert!((params.threshold_db - (-6.0 - fader - 7.5)).abs() <= 0.25, "{params:?}");
        assert_eq!((params.ratio, params.attack_ms, params.release_ms, params.makeup_db), (3.0, 5.0, 125.0, 2.5));
        // A bass releases in an eighth; panned hard, a channel has 3 dB more.
        let bass = TrackMeasures { role: Role::Bass, pan: 1.0, gain: 1.0, ..voice.clone() };
        let b = compression(&bass, 120.0).unwrap();
        assert_eq!(b.release_ms, 250.0);
        assert!((b.threshold_db - (-6.0 - 3.0 - 4.0 * 4.0 / 3.0)).abs() <= 0.25, "{b:?}");
        let list = advise(&report(-14.0, -1.5, vec![voice.clone()]), Target::Streaming, None);
        assert!(list.iter().any(|a| a.fix == Some(Fix::Compress { track: 0, name: "Voz".into(), params })), "{list:?}");
        // Already compressed, steady enough, or a role not compressed: nothing to do.
        let done = TrackMeasures { compressed: true, ..voice.clone() };
        assert!(compression(&done, 120.0).is_none());
        assert!(advise(&report(-14.0, -1.5, vec![done]), Target::Streaming, None).iter().any(|a| a.level == Level::Good && a.text == "«Voz» ya tiene compresor"));
        assert!(compression(&TrackMeasures { dynamics: Some((-6.0, -11.0)), ..voice.clone() }, 120.0).is_none());
        assert!(compression(&TrackMeasures { role: Role::Pad, ..voice.clone() }, 120.0).is_none());
        // Ducking to the kick, it moves on purpose.
        let ducking = TrackMeasures { listens: vec![99], ..bass };
        assert!(compression(&ducking, 120.0).is_none());
        assert!(advise(&report(-14.0, -1.5, vec![ducking]), Target::Streaming, None).iter().all(|a| !a.text.contains("compres")));
    }

    /// For real: a voice singing loud and soft phrases. Compressed, its loud
    /// parts come down about what was asked, and then it says so in green.
    #[test]
    fn a_voices_loud_parts_come_down() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let audio: Vec<f32> = (0..(8.0 * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let level = if (t as usize) % 2 == 0 { 0.5 } else { 0.06 };
                let x = std::f32::consts::TAU * 220.0 * t;
                level * (x.sin() + 0.5 * (2.0 * x).sin() + 0.25 * (3.0 * x).sin()) / 1.75
            })
            .collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("voz-dinamica-{}.wav", std::process::id()));
        crate::export::write(&path, &audio, &audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut voice = Track::new_audio("Voz", [0; 3]);
        voice.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: "Voz".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) });
        project.tracks = vec![voice];
        let mut app = PulseApp::headless(project);
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 60);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let before = app.mix.report.clone().unwrap().tracks[0].clone();
        let (loud, soft) = before.dynamics.unwrap();
        println!("antes: {loud:.1} / {soft:.1}");
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Compress { .. }))).expect("a compressor");
        let Fix::Compress { params, .. } = &fix else { unreachable!() };
        println!("{params:?}");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let after = app.mix.report.clone().unwrap().tracks[0].clone();
        let (loud_after, soft_after) = after.dynamics.unwrap();
        println!("después: {loud_after:.1} / {soft_after:.1}");
        // 5 dB off the loud parts, give or take the knee and the release.
        let taken = loud - (loud_after - params.makeup_db);
        assert!((3.5..=6.5).contains(&taken), "{taken} dB");
        // The soft parts untouched (only the makeup).
        assert!((soft_after - params.makeup_db - soft).abs() < 1.0, "{soft} → {soft_after}");
        assert!(advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None).iter().any(|a| a.level == Level::Good && a.text == "«Voz» ya tiene compresor"));
    }
}
