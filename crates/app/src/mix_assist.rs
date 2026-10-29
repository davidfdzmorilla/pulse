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
//!
//! «Espacio de partida» gives the dry tracks their room: each role sends
//! what it usually does to a reverb and a delay (made as returns if the
//! project has none), the voice both, pads and chords reverb, and the kick
//! and the bass nothing, so the low end stays clean.
//!
//! «Eses» finds a voice whose esses jump out (as loud as its vowels, or
//! nearly) and puts a De-esser on it, set where they are (see [`esses`]).
//!
//! «Ecos al final de frase» opens the voice's send to the delay on the last
//! word of the phrases a silence follows, so its echo fills it (a delay
//! throw), and leaves it where it was while it sings (see [`throws`]).
//!
//! «Secciones que crecen» measures each part of the song (between its
//! markers) and, where a chorus or a drop is no bigger than the verses and
//! build-ups, brings those down with the master's volume (see [`growth`]).
//!
//! «Contrafase» puts right a track that vanishes in mono (a channel the
//! wrong way, or too wide) and one whose lows are in stereo, with a Utility
//! (see [`stereo_fix`]).
//!
//! «Nivelar la voz» rides the voice's fader phrase by phrase (volume
//! automation), so it sits as far over the rest in every one (see [`ride`]).
//!
//! «Aplicar todo» does it all, in the order a mix is made (see [`stage`]),
//! measuring again after each change: the next one depends on what the last
//! did. While it changes tracks it measures only the tracks it changed (and
//! one whose fader alone moved, without rendering it: see [`louder_measures`]),
//! and the mix when what comes next hears it.
//!
//! The loudness comes from the master's limiter, a true-peak one: the mix is
//! rendered once up to it and gains are tried on that (see [`loudest_gain`]),
//! as far as the goal, but its peaks no more than [`MAX_LIMITER_GAIN`] into it
//! and the song's parts kept: a drop over its break and its intro (see
//! [`squashed`]). Short of the goal, it says what else to do.
//!
//! «Antes / Después» plays the mix as it was before the assistant's first
//! change and as it is, at the same loudness (the louder one's master fader
//! down, only in what the engine plays): louder sounds better, and that is
//! not what is being compared. The project and its undo are left alone.

use crate::app::PulseApp;
use crate::export::{self, Range};
use crate::theme::{ACCENT, METER_RED, PLAY, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};
use engine::model::{UtilityParams, CompressorParams, DeEsserParams, DelayParams, Device, DuckerParams, Effect, Eq3Params, EqEightParams, EqShape, InstrumentKind, KickDrive, LimiterParams, Param, ReverbParams, SaturatorParams};
use engine::MAX_RETURNS;
use stems::loudness;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const RATE: f32 = 48_000.0;
/// The most the assistant drives the mix's peaks into the master's limiter
/// (dB over its ceiling): past it, a mix is squashed rather than made louder
/// (a mastering engineer has a limiter take 3 to 6 dB off the peaks; what is
/// missing then comes from the mix). A mix with headroom under the ceiling
/// gets that much more gain, which takes nothing off it.
const MAX_LIMITER_GAIN: f32 = 6.0;
/// From this loudness (LUFS) on (a club's), the master gets a clipper before
/// its limiter (see `PulseApp::loud_master`): how far it clips (dB), and the
/// limiter's release. The kick's peaks stand 15 dB or more over the rest of a
/// techno mix: a limiter alone gets there squashing everything with them.
const LOUD: f64 = -9.0;
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
    /// A group (its tracks send on their own), and what it sends to each return.
    pub group: bool,
    pub sends: [f32; MAX_RETURNS],
    /// Its level every [`HOP`] s (dB of power, see [`levels`]), and whether its
    /// volume is automated already.
    pub levels: Vec<f32>,
    pub automated: bool,
    /// A voice's esses (see [`esses`]), and whether it has a De-esser already.
    pub esses: Option<Esses>,
    pub deessed: bool,
    /// Whether one of its sends is automated already.
    pub thrown: bool,
    /// Its middle and side (see [`stereo`]), and its last Utility, if it has one.
    pub stereo: Stereo,
    pub utility: Option<UtilityParams>,
}

// ----- Out of phase, and lows in stereo -------------------------------------------------

/// A track's power in its middle (L+R) and side (L−R), all of it and under
/// [`STEREO_LOW`] Hz.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stereo {
    pub mid: f64,
    pub side: f64,
    pub low_mid: f64,
    pub low_side: f64,
}

/// Where the lows end: under this, a club (and a phone) hears them in mono.
const STEREO_LOW: f64 = 120.0;

pub fn stereo(left: &[f32], right: &[f32], rate: f32) -> Stereo {
    let mid: Vec<f64> = left.iter().zip(right).map(|(a, b)| 0.5 * (*a as f64 + *b as f64)).collect();
    let side: Vec<f64> = left.iter().zip(right).map(|(a, b)| 0.5 * (*a as f64 - *b as f64)).collect();
    let power = |x: &[f64]| x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    let low = |x: &[f64]| {
        let q = std::f64::consts::FRAC_1_SQRT_2;
        filtered(&filtered(x, rate, STEREO_LOW, q, 1), rate, STEREO_LOW, q, 1)
    };
    Stereo { mid: power(&mid), side: power(&side), low_mid: power(&low(&mid)), low_side: power(&low(&side)) }
}

/// The correlation a track narrowed is left at: a little more of it in the
/// middle than in the side, so in mono it loses little.
const NARROW_TO: f64 = 0.2;

/// What a track's Utility needs so it doesn't vanish in mono: its right
/// channel flipped (it is the wrong way: correlation under -0.5), or its
/// width brought to where it is [`NARROW_TO`] (it is too wide: under -0.2);
/// and its lows in mono under [`STEREO_LOW`] Hz, if a tenth of it is lows
/// and their side is within 10 dB of their middle. None if it needs nothing.
pub fn stereo_fix(t: &TrackMeasures) -> Option<UtilityParams> {
    let mut fix = t.utility.unwrap_or_default();
    let before = fix;
    let st = t.stereo;
    if t.correlation < -0.5 {
        fix.flip_right = !fix.flip_right;
    } else if t.correlation < -0.2 && st.side > 0.0 {
        // With middle and side apart, correlation is (M − w²S) / (M + w²S).
        let w = (st.mid * (1.0 - NARROW_TO) / (st.side * (1.0 + NARROW_TO))).sqrt();
        fix.width = ((fix.width as f64 * w).clamp(0.0, 1.0) * 20.0).round() as f32 / 20.0;
    }
    let lows = st.low_mid + st.low_side;
    if fix.bass_mono_hz == 0.0 && lows > 0.1 * (st.mid + st.side) && st.low_side > 0.1 * st.low_mid {
        fix.bass_mono_hz = STEREO_LOW as f32;
    }
    (fix != before).then_some(fix)
}

// ----- Sections that grow ------------------------------------------------------------

/// A part of the song: its name, where it is (beats), and how loud the mix is
/// in it (LUFS).
#[derive(Clone, Debug, PartialEq)]
pub struct SectionLevel {
    pub name: String,
    pub start: f64,
    pub end: f64,
    pub lufs: Option<f64>,
}

/// What a part is for growing: `Some(true)` the peak (a chorus, a drop),
/// `Some(false)` what leads to it (a verse, a build-up), `None` the rest
/// (an intro, a break, a bridge, an outro: quieter on purpose, or anything).
pub fn section_kind(name: &str) -> Option<bool> {
    let name = name.to_lowercase();
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |keys: &[&str]| words.iter().any(|w| keys.iter().any(|k| w.starts_with(k)));
    // What leads first: «Pre-estribillo», «Subida al drop».
    if has(&["estrofa", "verse", "subida", "build", "pre", "groove"]) {
        Some(false)
    } else if has(&["estribillo", "coro", "chorus", "drop", "hook", "climax", "clímax"]) {
        Some(true)
    } else {
        None
    }
}

/// How much louder (LU) a peak should be than what leads to it, at least.
const GROW_LU: f64 = 1.5;
/// The most a part is brought down (dB).
const GROW_MAX: f64 = 6.0;

/// The parts to bring down so the quietest peak is [`GROW_LU`] over each of
/// them (those it is less than 1 LU over): their indices in `sections` and by
/// how much (dB, rounded to half).
pub fn growth(sections: &[SectionLevel]) -> Vec<(usize, f32)> {
    let peaks = sections.iter().filter(|s| section_kind(&s.name) == Some(true)).filter_map(|s| s.lufs);
    let Some(peak) = peaks.reduce(f64::min) else { return Vec::new() };
    sections
        .iter()
        .enumerate()
        .filter(|(_, s)| section_kind(&s.name) == Some(false))
        .filter_map(|(i, s)| {
            let over = s.lufs? - (peak - GROW_LU);
            // A LU over what leads to it is enough: the master's limiter may
            // take a little of the 1.5 (see `squashed`).
            (over > 0.5).then(|| (i, ((over.min(GROW_MAX) * 2.0).round() / 2.0).max(0.5) as f32))
        })
        .collect()
}

/// The seconds from beat `a` to beat `b`, at the project's tempo (and its
/// tempo automation, if it has any).
fn seconds_between(project: &crate::project::Project, a: f64, b: f64) -> f64 {
    let lane = project.master.automation.iter().find(|l| l.param == Param::Tempo && !l.points.is_empty());
    let Some(lane) = lane else { return (b - a) * 60.0 / project.tempo.max(1.0) };
    const STEP: f64 = 1.0 / 32.0;
    let mut beat = a;
    let mut seconds = 0.0;
    while beat < b {
        let step = STEP.min(b - beat);
        seconds += step * 60.0 / lane.value_at(beat).unwrap_or(project.tempo as f32).max(1.0) as f64;
        beat += step;
    }
    seconds
}

// ----- Delay throws -----------------------------------------------------------------------

/// What the send to the delay opens to on a throw, and for how long before
/// the phrase ends it opens (s): about its last word.
const THROW: f32 = 0.4;
const LAST_WORD: f64 = 0.35;
const THROW_GAP: f64 = 0.75;

/// The ends (s) of the voice's phrases (see [`phrases`]) a silence follows
/// long enough to hear their echo: as long as the delay's time (`echo`, s)
/// and [`THROW_GAP`] at least (not a breath). The last one too.
pub fn throws(levels: &[f32], echo: f64) -> Vec<f64> {
    let found = phrases(levels, &vec![0.0; levels.len()]);
    let gap = echo.max(THROW_GAP);
    found
        .iter()
        .enumerate()
        .filter(|(i, p)| found.get(i + 1).is_none_or(|next| next.start - p.end >= gap))
        .map(|(_, p)| p.end)
        .collect()
}

/// The send's points for throws at `ends` (s): where it is (`base`) while
/// it sings, [`THROW`] on each last word, and back right after it (the
/// delay's feedback goes on repeating it).
fn throw_points(ends: &[f64], base: f32) -> Vec<(f64, f32)> {
    let open = THROW.max(base);
    let mut out = Vec::new();
    for &end in ends {
        let start = (end - LAST_WORD).max(0.0);
        out.extend([(start - 0.05, base), (start, open), (end + 0.05, open), (end + 0.15, base)]);
    }
    out.retain(|p| p.0 >= 0.0);
    out
}

// ----- Esses ------------------------------------------------------------------------------

/// A voice's esses: how loud they are next to its vowels (dB: their 90th
/// percentile against the vowels' median, in 10 ms frames), where they are
/// (the De-esser's split, Hz), and the top part's peaks over that split, as
/// the De-esser hears them (dBFS, 90th percentiles) in the esses and in the
/// vowels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Esses {
    pub over_vowels: f32,
    pub freq: f32,
    pub esses_top: f32,
    pub vowels_top: f32,
}

/// A second-order filter (RBJ), run on `x`.
fn filtered(x: &[f64], rate: f32, hz: f64, q: f64, kind: u8) -> Vec<f64> {
    let w = std::f64::consts::TAU * hz / rate as f64;
    let (c, alpha) = (w.cos(), w.sin() / (2.0 * q));
    let (b, a) = match kind {
        // High-pass, low-pass, band-pass (0 dB at its peak).
        0 => ([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
        1 => ([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
        _ => ([alpha, 0.0, -alpha], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
    };
    let (mut s1, mut s2) = (0.0, 0.0);
    x.iter()
        .map(|&v| {
            let y = (b[0] * v + s1) / a[0];
            s1 = (b[1] * v - a[1] * y) + s2;
            s2 = b[2] * v - a[2] * y;
            y
        })
        .collect()
}

/// Finds a voice's esses: 10 ms frames where it sounds (within 40 dB of its
/// loudest) and most of its power is over 4.5 kHz. None without a few of them
/// and enough vowels to set them against.
pub fn esses(left: &[f32], right: &[f32], rate: f32) -> Option<Esses> {
    let x: Vec<f64> = left.iter().zip(right).map(|(a, b)| 0.5 * (*a as f64 + *b as f64)).collect();
    let frame = (0.01 * rate) as usize;
    let power = |y: &[f64]| -> Vec<f64> { y.chunks(frame).map(|c| c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).collect() };
    let db = |p: f64| (10.0 * p.max(1e-14).log10()) as f32;
    let total = power(&x);
    let high = power(&filtered(&x, rate, 4500.0, std::f64::consts::FRAC_1_SQRT_2, 0));
    let loudest = total.iter().copied().fold(0.0, f64::max);
    let sounds = |k: usize| db(total[k]) > (db(loudest) - 40.0).max(-60.0);
    let sibilant: Vec<usize> = (0..total.len()).filter(|&k| sounds(k) && high[k] > 0.5 * total[k]).collect();
    let vowels: Vec<usize> = (0..total.len()).filter(|&k| sounds(k) && high[k] < 0.2 * total[k]).collect();
    if sibilant.len() < 5 || vowels.len() < 20 {
        return None;
    }
    let percentile = |mut v: Vec<f32>, q: f32| {
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q) as usize]
    };
    let over_vowels = percentile(sibilant.iter().map(|&k| db(total[k])).collect(), 0.9) - percentile(vowels.iter().map(|&k| db(total[k])).collect(), 0.5);
    // Where they are: the loudest of three bands, and the split under it.
    let bands = [(5000.0, 4500.0), (7000.0, 6000.0), (9500.0, 7500.0)];
    let energy = |hz: f64| {
        let p = power(&filtered(&x, rate, hz, 1.4, 2));
        sibilant.iter().map(|&k| p[k]).sum::<f64>()
    };
    let freq = bands.iter().map(|&(hz, split)| (energy(hz), split)).max_by(|a, b| a.0.total_cmp(&b.0)).map_or(6000.0, |b| b.1);
    // The top part as the De-esser takes it (the sound less a low-pass), its
    // peak falling in 60 ms, at its highest in each frame.
    let low = filtered(&x, rate, freq as f64, std::f64::consts::FRAC_1_SQRT_2, 1);
    let fall = (-1.0 / (0.06 * rate as f64)).exp();
    let mut level = 0.0f64;
    let peaks: Vec<f32> = x
        .iter()
        .zip(&low)
        .map(|(v, l)| {
            level = (v - l).abs().max(level * fall);
            level
        })
        .collect::<Vec<f64>>()
        .chunks(frame)
        .map(|c| (20.0 * c.iter().copied().fold(0.0, f64::max).max(1e-9).log10()) as f32)
        .collect();
    Some(Esses {
        over_vowels,
        freq,
        esses_top: percentile(sibilant.iter().map(|&k| peaks[k]).collect(), 0.9),
        vowels_top: percentile(vowels.iter().map(|&k| peaks[k]).collect(), 0.9),
    })
}

/// Esses this close to the vowels (dB) or closer jump out of the voice.
const ESSES_LOUD: f32 = -8.0;
/// How much the De-esser takes off them (dB), and its range.
const DEESS_DB: f32 = 6.0;
const DEESS_RANGE: f32 = 8.0;

/// The De-esser a voice starts with: split where its esses are, its
/// threshold where it takes [`DEESS_DB`] off them (as its effects hear them:
/// before the fader and the pan), and never as low as its vowels' top, so
/// they stay bright; and how much it takes off them (dB). None if its esses
/// don't jump out, or if it can't take them down without the vowels.
pub fn deessing(t: &TrackMeasures) -> Option<(DeEsserParams, f32)> {
    let e = t.esses?;
    if t.deessed || e.over_vowels < ESSES_LOUD {
        return None;
    }
    let angle = (t.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    let pan_db = 20.0 * (angle.cos().max(angle.sin()) * std::f32::consts::SQRT_2).log10();
    let before = |x: f32| x - 20.0 * t.gain.max(1e-6).log10() - pan_db;
    let threshold = ((before(e.esses_top) - DEESS_DB).max(before(e.vowels_top) + 1.0) * 2.0).round() / 2.0;
    let takes = (before(e.esses_top) - threshold).min(DEESS_RANGE);
    if takes < 3.0 {
        return None;
    }
    Some((DeEsserParams { freq_hz: e.freq, threshold_db: threshold, range_db: DEESS_RANGE }, takes))
}

// ----- Riding the voice -----------------------------------------------------------------

/// How often the tracks' level is measured (s), and over how long.
pub const HOP: f64 = 0.05;
const WINDOW: f64 = 0.2;
/// Silence: under this (dB of power).
const QUIET: f32 = -60.0;

/// A track's level every [`HOP`] s, over [`WINDOW`] s (dB of power, both channels).
pub fn levels(left: &[f32], right: &[f32], rate: f32) -> Vec<f32> {
    let (hop, window) = ((HOP * rate as f64) as usize, (WINDOW * rate as f64) as usize);
    let n = left.len().min(right.len());
    (0..n / hop)
        .map(|k| {
            let (a, b) = (k * hop, (k * hop + window).min(n));
            let power = (a..b).map(|i| (left[i] * left[i] + right[i] * right[i]) * 0.5).sum::<f32>() / (b - a).max(1) as f32;
            10.0 * power.max(1e-12).log10()
        })
        .collect()
}

/// A phrase of the voice: from and to (s), and how far over the rest it is (dB).
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    pub start: f64,
    pub end: f64,
    pub over: f32,
}

/// The voice's phrases: where it sounds (within 35 dB of its loudest), gaps
/// under 0.35 s bridged, at least half a second long; each with how far over
/// `base` (the rest of the mix, as [`levels`]) it is. Over nothing (a cappella)
/// it is not measured.
pub fn phrases(voice: &[f32], base: &[f32]) -> Vec<Phrase> {
    let top = voice.iter().copied().fold(QUIET, f32::max);
    let on: Vec<bool> = voice.iter().map(|&v| v > (top - 35.0).max(QUIET)).collect();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (k, &sounds) in on.iter().enumerate() {
        if !sounds {
            continue;
        }
        match spans.last_mut() {
            Some((_, end)) if k - *end <= (0.35 / HOP) as usize => *end = k,
            _ => spans.push((k, k)),
        }
    }
    let power = |x: &[f32]| 10.0 * (x.iter().map(|d| 10f32.powf(d / 10.0)).sum::<f32>() / x.len().max(1) as f32).max(1e-12).log10();
    spans
        .into_iter()
        .filter(|(a, b)| (b - a) as f64 * HOP + WINDOW >= 0.5)
        .filter_map(|(a, b)| {
            let rest = power(base.get(a..=b.min(base.len().saturating_sub(1)))?);
            // A frame reaches [`WINDOW`] ahead: the first that hears the
            // phrase starts that much (less a hop) before it.
            (rest > QUIET).then(|| Phrase { start: a as f64 * HOP + WINDOW - HOP, end: (b + 1) as f64 * HOP, over: power(&voice[a..=b]) - rest })
        })
        .collect()
}

/// The most a phrase is moved (dB), and the least worth moving.
const RIDE_MAX: f32 = 6.0;
const RIDE_MIN: f32 = 0.5;

/// How much each phrase moves (dB) to sit as far over the rest as most do
/// (the median, by length).
pub fn ride(phrases: &[Phrase]) -> Vec<f32> {
    let mut sorted: Vec<(f32, f64)> = phrases.iter().map(|p| (p.over, p.end - p.start)).collect();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = sorted.iter().map(|s| s.1).sum();
    let mut run = 0.0;
    let Some(median) = sorted.iter().find(|s| {
        run += s.1;
        run >= total / 2.0
    }) else {
        return Vec::new();
    };
    phrases
        .iter()
        .map(|p| {
            let g = (median.0 - p.over).clamp(-RIDE_MAX, RIDE_MAX);
            if g.abs() < RIDE_MIN { 0.0 } else { (g * 2.0).round() / 2.0 }
        })
        .collect()
}

/// The beat `seconds` after `from`, at the project's tempo (and its tempo
/// automation, if it has any).
fn beat_after(project: &crate::project::Project, from: f64, seconds: f64) -> f64 {
    let lane = project.master.automation.iter().find(|l| l.param == Param::Tempo && !l.points.is_empty());
    let Some(lane) = lane else { return from + seconds * project.tempo / 60.0 };
    let (mut beat, mut left) = (from, seconds);
    const STEP: f64 = 1.0 / 32.0;
    loop {
        let bpm = lane.value_at(beat).unwrap_or(project.tempo as f32).max(1.0) as f64;
        let step = STEP * 60.0 / bpm;
        if left <= step {
            return beat + left * bpm / 60.0;
        }
        left -= step;
        beat += STEP;
    }
}

/// The volume points that ride the phrases: each phrase's gain (dB, on
/// `fader`) from the middle of the gap before it to the middle of the gap
/// after it, moving in 0.1 s there, where the voice is silent.
fn ride_points(phrases: &[Phrase], gains: &[f32], fader: f32) -> Vec<(f64, f32)> {
    let value = |g: f32| (fader * 10f32.powf(g / 20.0)).min(FADER_TOP);
    let mut out = Vec::new();
    for (i, (p, &g)) in phrases.iter().zip(gains).enumerate() {
        let left = if i == 0 { (p.start - 0.05).max(0.0) } else { (phrases[i - 1].end + p.start) / 2.0 + 0.05 };
        let right = match phrases.get(i + 1) {
            Some(next) => (p.end + next.start) / 2.0 - 0.05,
            None => p.end + 0.05,
        };
        out.push((left, value(g)));
        out.push((right.max(left), value(g)));
    }
    out
}

// ----- Space to start from ------------------------------------------------------------

/// The project's returns as the assistant sees them: the one with a reverb,
/// the one with a delay (another), and how many there are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Space {
    pub reverb: Option<usize>,
    pub delay: Option<usize>,
    pub returns: usize,
}

impl Space {
    pub fn of(returns: &[crate::project::ReturnTrack]) -> Self {
        let has = |r: &crate::project::ReturnTrack, delay: bool| r.devices.iter().any(|d| d.enabled && if delay { matches!(d.effect, Effect::Delay(_)) } else { matches!(d.effect, Effect::Reverb(_)) });
        let reverb = returns.iter().position(|r| has(r, false));
        let delay = returns.iter().enumerate().position(|(i, r)| Some(i) != reverb && has(r, true));
        Self { reverb, delay, returns: returns.len() }
    }
}

/// What a role sends to a reverb and to a delay to start with (0 to 1, as
/// the sends): the voice both (a short slap of delay keeps it close), the
/// backing vocals and pads more reverb, so they sit behind; the kick and the
/// bass nothing, so the low end stays tight.
fn room(role: Role) -> Option<(f32, f32)> {
    match role {
        Role::Vocal => Some((0.22, 0.14)),
        Role::Backing => Some((0.32, 0.08)),
        Role::Lead => Some((0.2, 0.16)),
        Role::Chords => Some((0.18, 0.0)),
        Role::Pad => Some((0.28, 0.0)),
        Role::Percussion => Some((0.12, 0.0)),
        Role::Drums => Some((0.08, 0.0)),
        Role::Fx => Some((0.3, 0.12)),
        Role::Kick | Role::Bass => Some((0.0, 0.0)),
        Role::Other => None,
    }
}

/// From this send on, reverb on the low end muddies it.
const MUDDY_SEND: f32 = 0.15;

/// What the space to start from changes on a track: its sends to the reverb
/// and to the delay (`None`: left as they are).
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceChange {
    pub track: usize,
    pub name: String,
    pub role: Role,
    pub reverb: Option<f32>,
    pub delay: Option<f32>,
}

impl SpaceChange {
    fn describe(&self) -> String {
        let pct = |x: f32| format!("{:.0} %", x * 100.0);
        if self.reverb.unwrap_or(0.0) == 0.0 && self.delay.unwrap_or(0.0) == 0.0 {
            return format!("«{}» ({}): sin reverb ni delay, para que sus graves no se embarren", self.name, self.role.label());
        }
        let mut parts = Vec::new();
        if let Some(r) = self.reverb.filter(|&r| r > 0.0) {
            parts.push(format!("reverb {}", pct(r)));
        }
        if let Some(d) = self.delay.filter(|&d| d > 0.0) {
            parts.push(format!("delay {}", pct(d)));
        }
        format!("«{}» ({}): {}", self.name, self.role.label(), parts.join(", "))
    }

    /// Gives it some room (not takes it away).
    fn wet(&self) -> bool {
        self.reverb.unwrap_or(0.0) > 0.0 || self.delay.unwrap_or(0.0) > 0.0
    }
}

/// The sends that give the mix its space: the tracks that send nothing to
/// any return get their role's; the kick and the bass, if they send a lot of
/// reverb, none. Tracks that send already are left as they are. A reverb or a
/// delay the project lacks is made, if there is room for another return.
pub fn space_changes(tracks: &[TrackMeasures], space: Space) -> Vec<SpaceChange> {
    let free = MAX_RETURNS.saturating_sub(space.returns);
    let mut out = Vec::new();
    for t in tracks.iter().filter(|t| t.lufs.is_some() && !t.group) {
        let Some((reverb, delay)) = room(t.role) else { continue };
        let sent = |at: Option<usize>| at.map_or(0.0, |i| t.sends[i]);
        if reverb == 0.0 && delay == 0.0 {
            let muddy = |at: Option<usize>| (sent(at) > MUDDY_SEND).then_some(0.0);
            let change = SpaceChange { track: t.index, name: t.name.clone(), role: t.role, reverb: muddy(space.reverb), delay: None };
            if change.reverb.is_some() {
                out.push(change);
            }
        } else if t.sends.iter().all(|&s| s == 0.0) {
            out.push(SpaceChange { track: t.index, name: t.name.clone(), role: t.role, reverb: (reverb > 0.0).then_some(reverb), delay: (delay > 0.0).then_some(delay) });
        }
    }
    // Returns to make: as many as are missing and wanted, while there is room.
    let wants = |f: fn(&SpaceChange) -> Option<f32>| out.iter().any(|c| f(c).unwrap_or(0.0) > 0.0);
    let new_reverb = space.reverb.is_none() && wants(|c| c.reverb) && free > 0;
    let new_delay = space.delay.is_none() && wants(|c| c.delay) && free > usize::from(new_reverb);
    for c in &mut out {
        if space.reverb.is_none() && !new_reverb {
            c.reverb = None;
        }
        if space.delay.is_none() && !new_delay {
            c.delay = None;
        }
    }
    out.retain(|c| c.reverb.is_some() || c.delay.is_some());
    out
}

/// A reverb return to start from: all wet, its lows and its top cut so it
/// doesn't muddy the low end or hiss.
fn reverb_return(index: usize) -> crate::project::ReturnTrack {
    let mut eq = EqEightParams::default();
    eq.bands[0].on = true;
    eq.bands[0].shape = EqShape::LowCut12;
    eq.bands[0].freq = 250.0;
    eq.bands[7].on = true;
    eq.bands[7].shape = EqShape::HighCut12;
    eq.bands[7].freq = 9000.0;
    crate::project::ReturnTrack {
        name: format!("{} Reverb", crate::project::return_letter(index)),
        devices: vec![Device::new(Effect::Reverb(ReverbParams { size: 0.7, damping: 0.55, width: 1.0, mix: 1.0 })), Device::new(Effect::EqEight(eq))],
        ..crate::returns::new_return(index)
    }
}

/// A delay return to start from: a dotted eighth, at the tempo, darker each
/// repeat, without lows.
fn delay_return(index: usize) -> crate::project::ReturnTrack {
    let mut eq = EqEightParams::default();
    eq.bands[0].on = true;
    eq.bands[0].shape = EqShape::LowCut12;
    eq.bands[0].freq = 300.0;
    crate::project::ReturnTrack {
        name: format!("{} Delay", crate::project::return_letter(index)),
        devices: vec![Device::new(Effect::Delay(DelayParams { time_beats: 0.75, feedback: 0.3, mix: 1.0, ping_pong: true, filter_hz: 4000.0 })), Device::new(Effect::EqEight(eq))],
        ..crate::returns::new_return(index)
    }
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
/// within a band of it ([`MID_HZ`] are half an octave apart, and a carve,
/// about an octave wide, still takes 2 dB off the next band). Carved for one
/// track, the band where it clashes with another next to it is made room in.
fn carved_at(carves: &[f32], hz: f32) -> bool {
    carves.iter().any(|c| (c / hz).log2().abs() < 0.55)
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

fn freq_label(hz: f32) -> String {
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
        (Role::Fx, &["fx", "riser", "sweep", "impact", "efecto", "noise", "ruido", "transici"][..]),
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

/// How far under the kick (LU) drums sit that have no kick of their own (the
/// kick taken out to a track of its own: hats, claps, percussion). Next to
/// it as loud as a whole kit, they'd stand over everything else, and an intro
/// of drums alone would sound nearly as big as the drop.
const TOPS_LEVEL: f64 = -6.0;

/// Whether `t` is drums without a kick of their own: little of them under
/// 125 Hz (a fifth of their power at most), with a kick on another track.
fn tops(t: &TrackMeasures, tracks: &[TrackMeasures]) -> bool {
    let power: Vec<f64> = t.bands.iter().map(|b| 10f64.powf(b / 10.0)).collect();
    let total = power.iter().sum::<f64>().max(1e-30);
    t.role == Role::Drums && power[..3].iter().sum::<f64>() / total < 0.2 && tracks.iter().any(|o| o.index != t.index && o.role == Role::Kick && o.lufs.is_some())
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
        let level = if tops(t, tracks) { Some(TOPS_LEVEL) } else { t.role.level() };
        if let (Some(level), Some(lufs)) = (level, t.lufs) {
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
    /// Its reverb and delay returns.
    pub space: Space,
    /// Where it was rendered from (beat).
    pub start: f64,
    /// The delay return's time (beats; a delay to make, a dotted eighth).
    pub echo: f64,
    /// The song's parts (empty without markers, or analysing the loop), and
    /// whether the master's volume is automated already.
    pub sections: Vec<SectionLevel>,
    pub master_automated: bool,
    /// Whether the assistant left the master's limiter short of the loudness
    /// on purpose (see [`Limited`]), and it is as it left it.
    pub limited: Option<Limited>,
    /// The pairs of tracks (ids: the one that made room, the one it made it
    /// for) the assistant carved room in the mids for, still carved: once is
    /// enough, even where the clash moved to another band.
    pub carved: Vec<(u32, u32)>,
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
    /// Sends to a reverb and a delay (see [`space_changes`]); the returns
    /// they go to, as they were when measured (`None`: made).
    Space { reverb: Option<usize>, delay: Option<usize>, changes: Vec<SpaceChange> },
    /// A De-esser on a voice (see [`deessing`]).
    DeEss { track: usize, name: String, params: DeEsserParams },
    /// A track's Utility (its last one, or a new one) set as [`stereo_fix`] says.
    Stereo { track: usize, name: String, params: UtilityParams },
    /// The master's volume brought down (dB) in the parts at `cuts` (beats),
    /// so the peaks grow over them (see [`growth`]).
    Grow { cuts: Vec<(String, f64, f64, f32)> },
    /// The voice's send to the delay (return `delay`; `None`: made) opened on
    /// the last word of the phrases ending at `ends` (s from `from`, a beat).
    Throws { track: usize, name: String, delay: Option<usize>, from: f64, ends: Vec<f64>, base: f32 },
    /// The voice ridden phrase by phrase (see [`ride`]): its phrases (s from
    /// `from`, a beat) and how much each moves (dB).
    Ride { track: usize, name: String, from: f64, phrases: Vec<Phrase>, gains: Vec<f32> },
}

/// Where a fix goes in a mix made from the start («Aplicar todo»): nothing
/// over 0 dB, the levels and pans, the low end, the mids, the dynamics, the
/// space, and last the master (its tone, then its loudness).
fn stage(fix: &Fix) -> u8 {
    match fix {
        Fix::TrackGain { .. } | Fix::Stereo { .. } => 0,
        Fix::Balance(_) => 1,
        Fix::Duck { .. } | Fix::CutLows { .. } => 2,
        Fix::Carve { .. } => 3,
        Fix::Compress { .. } => 4,
        Fix::DeEss { .. } => 5,
        Fix::Ride { .. } => 6,
        Fix::Space { .. } => 7,
        Fix::Throws { .. } => 8,
        Fix::Grow { .. } => 9,
        Fix::MasterEq { .. } => 10,
        Fix::MasterLimiter { .. } => 11,
    }
}

/// The next fix «Aplicar todo» makes, after `done`: the earliest stage's,
/// none twice, and nothing after the master's loudness. Room in the mids, a
/// track for another once (where they clash the most). The balance twice: at
/// its stage, and again before the master, since cutting, ducking and
/// compressing move the levels (the compressor's makeup more than any).
pub fn next_fix(advice: &[Advice], done: &[Fix]) -> Option<Fix> {
    if done.iter().any(|f| matches!(f, Fix::MasterLimiter { .. })) {
        return None;
    }
    let balances = done.iter().filter(|f| matches!(f, Fix::Balance(_))).count();
    let carved = |track: usize, to: &str| done.iter().any(|f| matches!(f, Fix::Carve { track: t, for_name, .. } if *t == track && for_name == to));
    let order = |f: &Fix| match f {
        Fix::Balance(_) if balances > 0 => 2 * stage(&Fix::Space { reverb: None, delay: None, changes: Vec::new() }) + 1,
        _ => 2 * stage(f),
    };
    advice
        .iter()
        .filter_map(|a| a.fix.as_ref())
        .filter(|f| !done.contains(f))
        .filter(|f| match f {
            Fix::Balance(_) => balances < 2,
            Fix::Carve { track, for_name, .. } => !carved(*track, for_name),
            _ => true,
        })
        .min_by_key(|f| order(f))
        .cloned()
}

/// What a fix did, in a few words (for the summary of «Aplicar todo»).
fn fix_label(fix: &Fix) -> String {
    match fix {
        Fix::MasterLimiter { .. } => "sonoridad con el limitador del master".into(),
        Fix::TrackGain { name, .. } => format!("volumen de «{name}»"),
        Fix::Duck { name, .. } => format!("ducker en «{name}»"),
        Fix::CutLows { name, .. } => format!("graves de «{name}»"),
        Fix::MasterEq { .. } => "EQ del master".into(),
        Fix::Balance(_) => "volúmenes y panoramas".into(),
        Fix::Carve { name, for_name, .. } => format!("hueco en «{name}» para «{for_name}»"),
        Fix::Compress { name, .. } => format!("compresor en «{name}»"),
        Fix::Space { .. } => "reverb y delay".into(),
        Fix::Ride { name, .. } => format!("«{name}» nivelada"),
        Fix::DeEss { name, .. } => format!("de-esser en «{name}»"),
        Fix::Throws { name, .. } => format!("ecos de «{name}»"),
        Fix::Grow { .. } => "secciones que crecen".into(),
        Fix::Stereo { name, .. } => format!("estéreo de «{name}»"),
    }
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

/// The rest of the mix against track `index`: the other tracks' levels added
/// up (their power), groups left out (their tracks are counted).
fn base_levels(tracks: &[TrackMeasures], index: usize) -> Vec<f32> {
    let others: Vec<&TrackMeasures> = tracks.iter().filter(|t| t.index != index && !t.group && t.lufs.is_some()).collect();
    let n = others.iter().map(|t| t.levels.len()).max().unwrap_or(0);
    (0..n).map(|k| 10.0 * others.iter().map(|t| t.levels.get(k).map_or(0.0, |d| 10f32.powf(d / 10.0))).sum::<f32>().max(1e-12).log10()).collect()
}

/// «1:05», from seconds.
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
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
        let ceiling_db = (ceiling - PEAK_MARGIN) as f32;
        // The limiter as far as it goes for this target (the assistant left it
        // there, or it is at its most): what else to do, not more of it.
        let held = report.limited.as_ref().filter(|l| l.target == target).map(|l| l.held.clone());
        let is_held = held.is_some() && off > 0.0;
        if off.abs() < 1.0 {
            out.push(advice(Level::Good, format!("Sonoridad {} LUFS: justo para {}", num(lufs), target.label()), None));
        } else if let (true, Some(held)) = (off > 0.0, held) {
            out.push(advice(Level::Note, format!("Sonoridad {} LUFS: {} dB por debajo del objetivo ({} LUFS). {}. {LOUDER_WITHOUT_SQUASHING}", num(lufs), num(off), num(goal), held.says()), None));
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
        if mix.true_peak > ceiling && (off.abs() < 1.0 || is_held) {
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
        // Its lows cut already: what is left of them is its own.
        if let Some(Fix::CutLows { track, name }) = &fix {
            if report.tracks.iter().any(|t| t.index == *track && t.cut) {
                let text = format!("«{name}» ya tiene los graves recortados");
                if !out.iter().any(|a| a.text == text) {
                    out.push(advice(Level::Good, text, None));
                }
                continue;
            }
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
    // Esses that jump out of a voice.
    for t in report.tracks.iter().filter(|t| matches!(t.role, Role::Vocal | Role::Backing) && !t.group) {
        let Some(e) = t.esses else { continue };
        if e.over_vowels < ESSES_LOUD {
            continue;
        }
        if t.deessed {
            out.push(advice(Level::Good, format!("«{}» ya tiene De-esser", t.name), None));
        } else if let Some((params, takes)) = deessing(t) {
            out.push(advice(
                Level::Note,
                format!(
                    "Las eses de «{}» suenan a {} dB de sus vocales: saltan y pinchan, más aún tras comprimirla. Un De-esser en {} las bajaría unos {} dB sin apagar el resto",
                    t.name,
                    db(e.over_vowels as f64),
                    freq_label(e.freq),
                    format!("{takes:.0}")
                ),
                Some(Fix::DeEss { track: t.index, name: t.name.clone(), params }),
            ));
        }
    }
    // A voice that comes and goes against the rest.
    for t in report.tracks.iter().filter(|t| t.role == Role::Vocal && !t.group && t.lufs.is_some()) {
        let base = base_levels(&report.tracks, t.index);
        let found = phrases(&t.levels, &base);
        let gains = ride(&found);
        let moved = gains.iter().filter(|g| g.abs() >= 2.0).count();
        if found.len() < 2 || moved == 0 {
            continue;
        }
        if t.automated {
            out.push(advice(Level::Good, format!("«{}» ya tiene automatización de volumen", t.name), None));
            continue;
        }
        let (low, high) = found.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.over), b.max(p.over)));
        let mut a = advice(
            Level::Note,
            format!(
                "«{}» va de {} a {} dB sobre la base según la frase: unas se pierden y otras tapan la música. Nivelarla mueve {} de sus {} frases con automatización de volumen",
                t.name,
                db(low as f64),
                db(high as f64),
                gains.iter().filter(|g| **g != 0.0).count(),
                found.len()
            ),
            Some(Fix::Ride { track: t.index, name: t.name.clone(), from: report.start, phrases: found.clone(), gains: gains.clone() }),
        );
        a.details = found.iter().zip(&gains).filter(|(_, g)| g.abs() >= 2.0).take(6).map(|(p, g)| format!("{}: {} dB", clock(p.start), db(*g as f64))).collect();
        out.push(a);
    }
    // Room: reverb and delay.
    let changes = space_changes(&report.tracks, report.space);
    let fix = Fix::Space { reverb: report.space.reverb, delay: report.space.delay, changes: changes.clone() };
    let dry: Vec<&SpaceChange> = changes.iter().filter(|c| c.wet()).collect();
    if !dry.is_empty() {
        let names: Vec<String> = dry.iter().take(3).map(|c| format!("«{}»", c.name)).collect();
        let more = if dry.len() > 3 { format!(" y {} más", dry.len() - 3) } else { String::new() };
        let made = match (report.space.reverb.is_none() && changes.iter().any(|c| c.reverb.unwrap_or(0.0) > 0.0), report.space.delay.is_none() && changes.iter().any(|c| c.delay.unwrap_or(0.0) > 0.0)) {
            (true, true) => ". Crea un retorno de reverb y otro de delay",
            (true, false) => ". Crea un retorno de reverb",
            (false, true) => ". Crea un retorno de delay",
            (false, false) => "",
        };
        let mut a = advice(
            Level::Note,
            format!("Espacio de partida: {}{more} {}, sin reverb ni delay, y la mezcla queda plana, todo delante{made}", names.join(", "), if dry.len() == 1 { "suena seca" } else { "suenan secas" }),
            Some(fix),
        );
        a.details = changes.iter().map(SpaceChange::describe).collect();
        out.push(a);
    } else if let Some(c) = changes.first() {
        let mut a = advice(Level::Warning, format!("«{}» ({}) manda mucho a la reverb: en los graves embarra la mezcla", c.name, c.role.label()), Some(fix));
        a.details = changes.iter().map(SpaceChange::describe).collect();
        out.push(a);
    } else if report.tracks.iter().any(|t| t.lufs.is_some() && !t.group && room(t.role).is_some_and(|(r, d)| r + d > 0.0) && t.sends.iter().any(|&s| s > 0.0)) {
        out.push(advice(Level::Good, "Las pistas tienen su espacio: mandan a reverb o delay".into(), None));
    }
    // Echoes on the voice's last words.
    let echo_s = report.echo * 60.0 / report.tempo.max(1.0);
    for t in report.tracks.iter().filter(|t| t.role == Role::Vocal && !t.group && t.lufs.is_some()) {
        if t.thrown {
            out.push(advice(Level::Good, format!("«{}» ya tiene sus envíos automatizados", t.name), None));
            continue;
        }
        let ends = throws(&t.levels, echo_s);
        if ends.len() < 2 {
            continue;
        }
        // Without a delay, and no room for one: nothing to throw to.
        if report.space.delay.is_none() && report.space.returns >= MAX_RETURNS {
            continue;
        }
        let base = report.space.delay.map_or(0.0, |i| t.sends[i]);
        let mut a = advice(
            Level::Note,
            format!(
                "Ecos al final de frase: «{}» deja {} silencios tras sus frases. Abrir el envío al delay en su última palabra hace que el eco los rellene, sin ensuciarla mientras canta{}",
                t.name,
                ends.len(),
                if report.space.delay.is_none() { ". Crea un retorno de delay" } else { "" }
            ),
            Some(Fix::Throws { track: t.index, name: t.name.clone(), delay: report.space.delay, from: report.start, ends: ends.clone(), base }),
        );
        a.details = vec![ends.iter().take(8).map(|&e| clock(e)).collect::<Vec<_>>().join(" · ") + if ends.len() > 8 { " …" } else { "" }];
        out.push(a);
    }
    // A chorus or a drop that doesn't grow.
    let cuts = growth(&report.sections);
    let peak = report.sections.iter().filter(|s| section_kind(&s.name) == Some(true)).filter_map(|s| s.lufs.map(|l| (l, &s.name))).min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((peak, peak_name)) = peak {
        if cuts.is_empty() {
            if report.sections.iter().any(|s| section_kind(&s.name) == Some(false)) {
                out.push(advice(Level::Good, format!("«{peak_name}» suena por encima de lo que lleva a él: la canción crece"), None));
            }
        } else if report.master_automated {
            // (Its volume moves already: that is left to whoever wrote it.)
        } else {
            let (lead, lead_lufs) = cuts.iter().map(|&(i, _)| (&report.sections[i].name, report.sections[i].lufs.unwrap_or(peak))).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            let mut a = advice(
                Level::Note,
                format!(
                    "«{peak_name}» suena a {} LU de «{lead}»: no crece, y la canción no llega a ningún sitio. Bajar el master en lo que lleva a él lo deja {} LU por encima",
                    db(peak - lead_lufs),
                    num(GROW_LU)
                ),
                Some(Fix::Grow { cuts: cuts.iter().map(|&(i, d)| { let s = &report.sections[i]; (s.name.clone(), s.start, s.end, d) }).collect() }),
            );
            a.details = cuts.iter().map(|&(i, d)| format!("«{}»: {} dB", report.sections[i].name, db(-d as f64))).collect();
            out.push(a);
        }
    }
    // Two tracks covering each other in the mids.
    for clash in mid_clashes(&report.tracks).into_iter().take(6) {
        let (yields, to) = (&report.tracks[clash.yields], &report.tracks[clash.to]);
        let hz = MID_HZ[clash.band];
        let made_room = !yields.carves.is_empty() && report.carved.contains(&(yields.id, to.id));
        if carved_at(&yields.carves, hz) || made_room {
            out.push(advice(Level::Good, format!("«{}» deja hueco a «{}» en {}", yields.name, to.name, freq_label(hz)), None));
            continue;
        }
        out.push(advice(
            Level::Warning,
            format!("«{}» y «{}» se tapan en {} el {:.0} % del tiempo: cuesta oír «{}» con claridad", to.name, yields.name, freq_label(hz), clash.share * 100.0, to.name),
            Some(Fix::Carve { track: yields.index, name: yields.name.clone(), hz, for_name: to.name.clone() }),
        ));
    }
    // Stereo.
    if mix.correlation < 0.0 {
        out.push(advice(Level::Warning, format!("Correlación estéreo {:.2}: hay mucho en contrafase y en mono (un móvil, un club) se perderá", mix.correlation), None));
    }
    for t in report.tracks.iter().filter(|t| t.lufs.is_some() && !t.group) {
        let Some(fix) = stereo_fix(t) else { continue };
        let was = t.utility.unwrap_or_default();
        let mut parts = Vec::new();
        if fix.flip_right != was.flip_right {
            parts.push(format!("tiene un canal al revés (correlación {:.2}): en mono casi desaparece. Invertir el derecho lo pone en su sitio", t.correlation));
        } else if fix.width != was.width {
            parts.push(format!("es tan abierta (correlación {:.2}) que en mono pierde mucho. Estrecharla al {:.0} % la deja ancha y segura", t.correlation, fix.width * 100.0));
        }
        if fix.bass_mono_hz != was.bass_mono_hz {
            parts.push(format!("{} los graves abiertos en estéreo: en un club o un móvil (en mono) se pierden o retumban distinto. Ponerlos en mono por debajo de {} Hz los asienta", if parts.is_empty() { "tiene" } else { "y tiene" }, STEREO_LOW));
        }
        let level = if fix.flip_right != was.flip_right || fix.width != was.width { Level::Warning } else { Level::Note };
        out.push(advice(level, format!("«{}» {}", t.name, parts.join("; ")), Some(Fix::Stereo { track: t.index, name: t.name.clone(), params: fix })));
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

// ----- The master's loudness ----------------------------------------------------------

/// How far under the highest true peak allowed the limiter's ceiling goes
/// (dB): what the master's fader and the encoders may add.
const PEAK_MARGIN: f64 = 0.3;
/// The master's fader, read every this many samples (see [`master_gains`]).
const FADER_BLOCK: usize = 64;

/// Why the limiter stopped short of the loudness asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum Held {
    /// It drives the peaks [`MAX_LIMITER_GAIN`] over its ceiling already.
    Gain,
    /// More would take from `peak` (a drop, a chorus) what it grows over `other`.
    Section { peak: String, other: String },
    /// More would flatten it (its loudness range).
    Range,
}

impl Held {
    fn says(&self) -> String {
        match self {
            Held::Gain => format!("El limitador ya le quita {} dB a los picos, lo razonable: más la aplastaría", num(MAX_LIMITER_GAIN as f64)),
            Held::Section { peak, other } => format!("Más limitador le quitaría a «{peak}» lo que crece sobre «{other}»"),
            Held::Range => "Más limitador le quitaría la dinámica: todo sonaría igual de fuerte".into(),
        }
    }
}

/// What to do for more loudness than the limiter gives without squashing.
const LOUDER_WITHOUT_SQUASHING: &str = "Para más volumen sin aplastarla: sube la mezcla antes del master (los faders, sin que el master pase de 0 dBFS), comprime el bus (un Compressor suave antes del limitador: 2:1, que quite 2 o 3 dB) o satura la batería y el bajo";

/// The limiter held back, for a target: at what gain, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Limited {
    pub target: Target,
    pub gain_db: f32,
    pub held: Held,
}

/// The mix through a limiter (or none) at one gain, measured: its loudness,
/// true peak (dBTP), range, and each part's loudness (LUFS).
#[derive(Clone, Debug, PartialEq)]
pub struct Trial {
    pub gain_db: f32,
    pub lufs: Option<f64>,
    pub true_peak: f64,
    pub range: Option<f64>,
    pub sections: Vec<Option<f64>>,
}

fn lufs_of(power: f64) -> f64 {
    -0.691 + 10.0 * power.max(1e-20).log10()
}

/// Gated loudness (BS.1770) of 400 ms block powers.
fn gated(blocks: impl Iterator<Item = f64>) -> Option<f64> {
    let loud: Vec<f64> = blocks.filter(|&p| lufs_of(p) > -70.0).collect();
    if loud.is_empty() {
        return None;
    }
    let relative = lufs_of(loud.iter().sum::<f64>() / loud.len() as f64) - 10.0;
    let kept: Vec<f64> = loud.into_iter().filter(|&p| lufs_of(p) > relative).collect();
    Some(lufs_of(kept.iter().sum::<f64>() / kept.len().max(1) as f64))
}

/// `left`/`right` (the master before its limiter) through `limiter` (`None`:
/// as it is), then the master's fader (`fader`, one value every
/// [`FADER_BLOCK`] samples), measured as the analysis measures the mix, in one
/// pass: loudness, range and each part (`sections`, samples) from its 100 ms
/// K-weighted blocks, and the true peak.
pub fn trial(left: &[f32], right: &[f32], fader: &[f32], limiter: Option<LimiterParams>, sections: &[(usize, usize)]) -> Trial {
    let mut fx = limiter.map(|p| engine::OfflineLimiter::new(p, RATE));
    let mut meter = engine::dsp::LoudnessMeter::new(RATE);
    let mut peak = engine::dsp::TruePeakMeter::default();
    // The meter's blocks: 100 ms.
    const BLOCK: usize = RATE as usize / 10;
    let mut blocks: Vec<f64> = Vec::with_capacity(left.len() / BLOCK + 1);
    const CHUNK: usize = 4096;
    let (mut l, mut r) = (vec![0f32; CHUNK], vec![0f32; CHUNK]);
    let n = left.len().min(right.len());
    let mut at = 0;
    while at < n {
        let m = CHUNK.min(n - at);
        l[..m].copy_from_slice(&left[at..at + m]);
        r[..m].copy_from_slice(&right[at..at + m]);
        if let Some(fx) = &mut fx {
            fx.process(&mut l[..m], &mut r[..m], RATE);
        }
        for i in 0..m {
            let g = fader.get((at + i) / FADER_BLOCK).or(fader.last()).copied().unwrap_or(1.0);
            let (a, b) = (l[i] * g, r[i] * g);
            peak.push(a, b);
            if let Some(p) = meter.push_block(a, b) {
                blocks.push(p);
            }
        }
        at += m;
    }
    // 400 ms blocks every 100 ms, and 3 s ones, as `stems::loudness` makes them.
    let mean = |a: usize, k: usize| blocks[a..a + k].iter().sum::<f64>() / k as f64;
    let momentary = |from: usize, to: usize| (from..to.saturating_sub(3)).map(move |a| (a, mean(a, 4)));
    let lufs = gated(momentary(0, blocks.len()).map(|(_, p)| p));
    let range = {
        let short: Vec<f64> = (0..blocks.len().saturating_sub(29)).map(|a| lufs_of(mean(a, 30))).filter(|&l| l > -70.0).collect();
        (!short.is_empty()).then(|| {
            let mean = short.iter().map(|&l| 10f64.powf((l + 0.691) / 10.0)).sum::<f64>() / short.len() as f64;
            let gate = lufs_of(mean) - 20.0;
            let mut kept: Vec<f64> = short.into_iter().filter(|&l| l > gate).collect();
            kept.sort_by(f64::total_cmp);
            let at = |p: f64| kept[((p * (kept.len() - 1) as f64).round() as usize).min(kept.len() - 1)];
            at(0.95) - at(0.10)
        })
    };
    let sections = sections.iter().map(|&(a, b)| gated(momentary(a.div_ceil(BLOCK), (b / BLOCK).min(blocks.len())).map(|(_, p)| p))).collect();
    Trial { gain_db: limiter.map_or(0.0, |p| p.gain_db), lufs, true_peak: 20.0 * (peak.peak() as f64).max(1e-10).log10(), range, sections }
}

/// Whether `t` (the mix limited) has lost the shape of `base` (the mix
/// before the limiter): the song's peak (its quietest chorus or drop, else its
/// loudest part) no longer over the rest by about what it was. A part may lose
/// a third of its distance to the peak (half a dB to 2.5), and one that leads
/// to it keeps what makes the peak grow (1.15 LU, or what it had if less).
/// Without parts, the range
/// may lose a third (a dB at least).
pub fn squashed(base: &Trial, t: &Trial, names: &[String]) -> Option<Held> {
    let kinds: Vec<Option<bool>> = names.iter().map(|n| section_kind(n)).collect();
    let measured: Vec<usize> = (0..names.len()).filter(|&i| base.sections.get(i).copied().flatten().is_some() && t.sections.get(i).copied().flatten().is_some()).collect();
    if measured.len() >= 2 {
        let level = |trial: &Trial, i: usize| trial.sections[i].unwrap();
        let peaks: Vec<usize> = measured.iter().copied().filter(|&i| kinds[i] == Some(true)).collect();
        let peaks = if peaks.is_empty() { vec![*measured.iter().max_by(|&&a, &&b| level(base, a).total_cmp(&level(base, b))).unwrap()] } else { peaks };
        let peak_of = |trial: &Trial| peaks.iter().map(|&i| (level(trial, i), i)).min_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        let ((p0, quietest), (p1, _)) = (peak_of(base), peak_of(t));
        for &i in measured.iter().filter(|i| !peaks.contains(i)) {
            let (before, after) = (p0 - level(base, i), p1 - level(t, i));
            let allowed = (before / 3.0).clamp(0.5, 2.5);
            // What leads to the peak stays far enough under it that it grows
            // (see `growth`, which asks under a LU), with a little to spare.
            let grows = kinds[i] == Some(false) && after < (GROW_LU - 0.35).min(before - 0.05);
            if after < before - allowed || grows {
                return Some(Held::Section { peak: names[quietest].clone(), other: names[i].clone() });
            }
        }
        return None;
    }
    match (base.range, t.range) {
        (Some(r0), Some(r1)) if r0 - r1 > (r0 / 3.0).max(1.0) => Some(Held::Range),
        _ => None,
    }
}

/// The limiter's gain for `goal` LUFS, its true peak at most `max_peak`: the
/// most, up to [`MAX_LIMITER_GAIN`] over the ceiling for its peaks (and the
/// headroom it has under the ceiling), that doesn't go over the goal nor squash
/// the song (see [`squashed`]), tried on `left`/`right` (the master before its
/// limiter) a few gains at a time, at once. With why it stopped, if it stopped
/// short (more than half a dB under).
pub fn loudest_gain(left: &[f32], right: &[f32], fader: &[f32], limiter: LimiterParams, sections: &[(String, usize, usize)], goal: f64, max_peak: f64, cancel: &AtomicBool) -> Option<(Trial, Option<Held>)> {
    let names: Vec<String> = sections.iter().map(|s| s.0.clone()).collect();
    let spans: Vec<(usize, usize)> = sections.iter().map(|s| (s.1, s.2)).collect();
    let base = trial(left, right, fader, None, &spans);
    let spans = &spans;
    let try_all = |gains: &[f32]| -> Vec<Trial> {
        std::thread::scope(|scope| {
            let jobs: Vec<_> = gains.iter().map(|&g| scope.spawn(move || trial(left, right, fader, Some(LimiterParams { gain_db: g, ..limiter }), spans))).collect();
            jobs.into_iter().map(|j| j.join().expect("a trial")).collect()
        })
    };
    // Fine where it's fine: not over the goal, its true peak under, its shape kept.
    let fine = |t: &Trial| t.lufs.is_some_and(|l| l <= goal + 0.2) && t.true_peak <= max_peak + 0.05 && squashed(&base, t, &names).is_none();
    let need = (goal - base.lufs?) as f32;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(4, 8);
    // From under what it needs (a limiter gives less than its gain), and at
    // least 6 dB under its most: the most may squash it.
    let most = MAX_LIMITER_GAIN + headroom(left, right, limiter.ceiling_db);
    let low = (need - 1.0).min(most - 6.0).clamp(-24.0, most);
    let spread = |a: f32, b: f32, n: usize| -> Vec<f32> { if b - a < 0.01 { vec![a] } else { (0..n).map(|k| a + (b - a) * k as f32 / (n - 1) as f32).collect() } };
    let mut tried = try_all(&spread(low, most, threads));
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    // The last fine one, and between it and the next, closer.
    let best = |tried: &[Trial]| tried.iter().rposition(|t| fine(t));
    if let Some(k) = best(&tried) {
        if let Some(next) = tried.get(k + 1).map(|t| t.gain_db) {
            let from = tried[k].gain_db;
            let finer: Vec<f32> = spread(from, next, threads + 2)[1..threads + 1].to_vec();
            tried.extend(try_all(&finer));
            tried.sort_by(|a, b| a.gain_db.total_cmp(&b.gain_db));
        }
    }
    let chosen = match best(&tried) {
        Some(k) => k,
        // Not even the least: the least, and why.
        None => 0,
    };
    let pick = tried[chosen].clone();
    let short = pick.lufs.is_none_or(|l| l < goal - 0.5);
    let held = short.then(|| match tried.get(chosen + 1).and_then(|t| squashed(&base, t, &names)).or_else(|| squashed(&base, &pick, &names)) {
        Some(why) => why,
        None => Held::Gain,
    });
    Some((pick, held))
}

/// How far (dB) the true peak of `left`/`right` stays under `ceiling` (0 if over).
fn headroom(left: &[f32], right: &[f32], ceiling: f32) -> f32 {
    let mut meter = engine::dsp::TruePeakMeter::default();
    for (&a, &b) in left.iter().zip(right) {
        meter.push(a, b);
    }
    (((ceiling - 20.0 * meter.peak().max(1e-9).log10()) * 10.0).round() / 10.0).max(0.0)
}

/// The master's fader at every [`FADER_BLOCK`] samples of a render from
/// `start` (beat), `frames` long: its volume automation, or its fader; at the
/// tempo (and its automation).
fn master_gains(master: &crate::project::MasterTrack, tempo: f64, start: f64, frames: usize) -> Vec<f32> {
    let volume = master.automation.iter().find(|l| l.param == Param::Volume && !l.points.is_empty());
    let tempo_lane = master.automation.iter().find(|l| l.param == Param::Tempo && !l.points.is_empty());
    let mut beat = start;
    (0..frames.div_ceil(FADER_BLOCK))
        .map(|_| {
            let g = volume.and_then(|l| l.value_at(beat)).unwrap_or(master.mixer.gain);
            let bpm = tempo_lane.and_then(|l| l.value_at(beat)).map_or(tempo, |b| b as f64).max(1.0);
            beat += FADER_BLOCK as f64 / RATE as f64 * bpm / 60.0;
            g
        })
        .collect()
}

// ----- Running it ----------------------------------------------------------------

/// What a job brings back: an analysis (and what its tracks were rendered
/// from, see [`stem_key`]), or the limiter's gain for a loudness.
enum Done {
    Report(Report, Vec<crate::project::Project>),
    /// With the mix as it comes out of the limiter (and its parts), worked
    /// out from the render before it when nothing follows the limiter.
    Loud { trial: Trial, held: Option<Held>, ceiling_db: f32, mix: Option<(Measures, Vec<SectionLevel>)> },
}

type Outcome = Result<Done, String>;

pub struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

pub struct ReferenceJob {
    name: String,
    result: Arc<Mutex<Option<Measures>>>,
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
    /// The report's mix (its loudness, its parts) is from before the last
    /// change: «Aplicar todo» measures only the tracks while it changes them.
    mix_stale: bool,
    /// What each track of the report was rendered from (see [`stem_key`]),
    /// and for which range: a track whose key is the same isn't rendered again.
    keys: Option<(Range, Vec<crate::project::Project>)>,
    pub reference: Option<(String, Measures)>,
    reference_job: Option<ReferenceJob>,
    /// The limiter stopped short of the loudness: at what gain, and why.
    limited: Option<Limited>,
    /// The tracks (ids) it carved room in for others (see [`Report::carved`]).
    carved: Vec<(u32, u32)>,
    /// What the last «Aplicar» could not do, shown until the next.
    note: Option<String>,
    /// «Aplicar todo» under way: what it has applied so far.
    auto: Option<Vec<Fix>>,
    /// The mix before the assistant's first change, and its loudness then.
    pub(crate) before: Option<(crate::project::Project, Option<f64>)>,
    /// Comparing: whether «Antes» plays, the project when it started, and
    /// what the engine plays (see [`PulseApp::listen`]).
    pub(crate) listening: Option<Listening>,
}

#[cfg(test)]
impl Assistant {
    /// Analysing, or applying everything.
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.auto.is_some()
    }
}

pub(crate) struct Listening {
    pub before: bool,
    at: crate::project::Project,
    engine: crate::project::Project,
}

/// How much (dB) the master of the mix before and of the mix now come down
/// so both sound as loud: the louder one, by the difference. Without both
/// loudnesses, nothing.
pub fn matched(before: Option<f64>, now: Option<f64>) -> (f32, f32) {
    match (before, now) {
        (Some(b), Some(n)) => ((n - b).min(0.0) as f32, (b - n).min(0.0) as f32),
        _ => (0.0, 0.0),
    }
}

/// `project` with its master `db` louder (for the engine only).
fn levelled(project: &crate::project::Project, db: f32) -> crate::project::Project {
    let mut p = project.clone();
    let factor = 10f32.powf(db / 20.0);
    p.master.mixer.gain *= factor;
    // Its volume automated (sections that grow) holds it where it was: that too.
    for lane in p.master.automation.iter_mut().filter(|l| l.param == Param::Volume) {
        for point in &mut lane.points {
            point.value *= factor;
        }
    }
    p
}

impl Default for Assistant {
    fn default() -> Self {
        Self { open: false, range: Range::Song, target: Target::Streaming, job: None, report: None, stale: false, mix_stale: false, keys: None, reference: None, reference_job: None, limited: None, carved: Vec::new(), note: None, auto: None, before: None, listening: None }
    }
}

/// One render of the analysis, measured.
enum Measured {
    Mix(Measures, Vec<SectionLevel>),
    Track(TrackMeasures),
}

/// A track's measures as they would be `db` louder: everything in dB moves
/// by that much, its powers by as many tenths of a bel, its shape not at all.
fn louder_measures(m: &TrackMeasures, db: f64) -> TrackMeasures {
    let mut m = m.clone();
    let (d, f) = (db as f32, 10f64.powf(db / 10.0));
    m.peak += db;
    m.lufs = m.lufs.map(|l| l + db);
    m.low_end.iter_mut().for_each(|l| *l += db);
    m.bands.iter_mut().for_each(|b| *b += db);
    m.mids.iter_mut().flat_map(|f| f.iter_mut()).for_each(|b| *b += d);
    m.dynamics = m.dynamics.map(|(a, b)| (a + d, b + d));
    m.levels.iter_mut().for_each(|l| *l += d);
    m.esses = m.esses.map(|e| Esses { esses_top: e.esses_top + d, vowels_top: e.vowels_top + d, ..e });
    let st = m.stereo;
    m.stereo = Stereo { mid: st.mid * f, side: st.side * f, low_mid: st.low_mid * f, low_side: st.low_side * f };
    m
}

/// The project its tracks are rendered from for the analysis: each dry,
/// without what it sends to the returns (their tails would fill its silences
/// and pass for its soft parts), and before the master (its chain, its fader
/// and their automation: a limiter alone on each track would lift the quiet
/// ones, and it isn't how they sit against each other).
fn dry_project(project: &crate::project::Project) -> crate::project::Project {
    let mut dry = project.clone();
    for t in &mut dry.tracks {
        t.sends = [0.0; MAX_RETURNS];
        t.automation.retain(|l| !matches!(l.param, Param::Send(_)));
    }
    dry.master.devices.clear();
    dry.master.mixer.gain = 1.0;
    dry.master.mixer.pan = 0.0;
    dry.master.automation.retain(|l| l.param == Param::Tempo);
    dry
}

/// What track `t`'s render of the analysis depends on: the dry project (see
/// [`dry_project`]) as its stem plays it (see [`export::only`]), less its
/// returns (no track sends to them in it). Two alike sound alike.
fn stem_key(dry: &crate::project::Project, t: usize) -> crate::project::Project {
    let mut key = export::only(dry, t);
    key.returns.clear();
    key
}

impl PulseApp {
    /// Renders and measures the mix and each track (in the background).
    pub fn analyze_mix(&mut self) {
        self.analyze(true);
    }

    /// Renders and measures the tracks that changed since the last analysis
    /// (all, the first time) and, with `mix`, the mix; what isn't rendered
    /// again is kept from the last.
    fn analyze(&mut self, mix: bool) {
        if export::beat_range(&self.project, self.mix.range).is_none() {
            self.status = Some(match self.mix.range {
                Range::Song | Range::Sections => "El Arrangement está vacío: coloca clips en él para analizar la canción (o analiza el loop)".into(),
                Range::Loop => "El loop está vacío".into(),
            });
            self.mix.auto = None;
            return;
        }
        self.capture_plugins();
        let range = self.mix.range;
        let settings = export::Settings { range, stems: true, stems_master: false, sample_rate: RATE as u32, ..Default::default() };
        let live = std::mem::take(&mut self.samples.plugins);
        let old = self.mix.report.as_ref().filter(|_| self.mix.keys.as_ref().is_some_and(|k| k.0 == range));
        let mix = mix || old.is_none();
        let mut renders = if mix { export::plan(&self.project, &mut self.samples, &export::Settings { stems: false, ..settings }, std::path::Path::new("mezcla.wav")) } else { Vec::new() };
        let dry = dry_project(&self.project);
        let keys: Vec<crate::project::Project> = (0..dry.tracks.len()).map(|t| stem_key(&dry, t)).collect();
        // Tracks rendered as they are: kept; with only their fader moved
        // (and no volume automation), kept that much louder or softer.
        let same = |t: usize| -> Option<TrackMeasures> {
            let (_, old_keys) = self.mix.keys.as_ref()?;
            let measured = old?.tracks.iter().find(|m| m.index == t)?;
            if old_keys.len() != keys.len() {
                return None;
            }
            if old_keys[t] == keys[t] {
                return Some(measured.clone());
            }
            let (was, now) = (&old_keys[t].tracks[t], &keys[t].tracks[t]);
            let mut moved = old_keys[t].clone();
            moved.tracks[t].mixer.gain = now.mixer.gain;
            let automated = now.automation.iter().any(|l| l.param == Param::Volume && !l.points.is_empty());
            (moved == keys[t] && !automated && was.mixer.gain > 0.0 && now.mixer.gain > 0.0).then(|| louder_measures(measured, 20.0 * (now.mixer.gain / was.mixer.gain).log10() as f64))
        };
        let mut kept: Vec<TrackMeasures> = Vec::new();
        for render in export::plan(&dry, &mut self.samples, &settings, std::path::Path::new("mezcla.wav")) {
            match render.track.and_then(|t| same(t)) {
                Some(m) => kept.push(m),
                None => renders.push(render),
            }
        }
        let old_mix = old.map(|r| (r.mix.clone(), r.sections.clone()));
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
        let space = Space::of(&self.project.returns);
        // The parts, in seconds of the render (from the song's start).
        let parts: Vec<(String, f64, f64, f64, f64)> = match range {
            Range::Loop => Vec::new(),
            _ => export::sections(&self.project).into_iter().map(|(name, (a, b))| (name, a, b, seconds_between(&self.project, 0.0, a), seconds_between(&self.project, 0.0, b))).collect(),
        };
        let master_automated = self.project.master.automation.iter().any(|l| l.param == Param::Volume && !l.points.is_empty());
        let routing: Vec<(bool, [f32; MAX_RETURNS], bool, bool)> = self
            .project
            .tracks
            .iter()
            .map(|t| (t.is_group(), t.sends, t.automation.iter().any(|l| l.param == Param::Volume && !l.points.is_empty()), t.devices.iter().any(|d| d.enabled && matches!(d.effect, Effect::DeEsser(_)))))
            .collect();
        let utilities: Vec<Option<UtilityParams>> = self.project.tracks.iter().map(|t| t.devices.iter().rev().find_map(|d| if let (true, Effect::Utility(u)) = (d.enabled, &d.effect) { Some(*u) } else { None })).collect();
        let thrown: Vec<bool> = self.project.tracks.iter().map(|t| t.automation.iter().any(|l| matches!(l.param, Param::Send(_)) && !l.points.is_empty())).collect();
        let echo = space.delay.and_then(|i| self.project.returns[i].devices.iter().find_map(|d| if let Effect::Delay(p) = &d.effect { Some(p.time_beats) } else { None })).unwrap_or(0.75);
        let start = export::beat_range(&self.project, range).map_or(0.0, |r| r.0);
        let limiter_gain = self.project.master.devices.iter().rev().find_map(|d| if let Effect::Limiter(l) = &d.effect { Some(l.gain_db) } else { None });
        let limited = self.mix.limited.clone().filter(|l| limiter_gain.is_some_and(|g| (g - l.gain_db).abs() < 0.01));
        let carved = self.mix.carved.clone();
        // What a kept track was measured with that the project says, as it is now.
        let meta = move |m: &mut TrackMeasures| {
            let index = m.index;
            let (name, drums, kick, id, listens) = names.get(index).cloned().unwrap_or_default();
            let (hint, pan, cut, gain, carves, compressed) = mixing.get(index).cloned().unwrap_or((None, 0.0, false, 1.0, Vec::new(), false));
            let (group, sends, automated, deessed) = routing.get(index).copied().unwrap_or((false, [0.0; MAX_RETURNS], false, false));
            (m.name, m.drums, m.kick, m.id, m.listens) = (name, drums, kick, id, listens);
            (m.role, m.pan, m.cut, m.gain, m.carves, m.compressed) = (hint.unwrap_or_else(|| role_by_sound(&m.bands)), pan, cut, gain, carves, compressed);
            (m.group, m.sends, m.automated, m.deessed) = (group, sends, automated, deessed);
            m.thrown = thrown.get(index).copied().unwrap_or(false);
            m.utility = utilities.get(index).copied().flatten();
        };
        let voices: Vec<bool> = self.project.tracks.iter().map(|t| matches!(role_hint(&t.name, &t.instrument, is_drums(t)), Some(Role::Vocal | Role::Backing))).collect();
        let (progress, cancel, result) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (p, c, r) = (progress.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            // The mix and every track at once (see `export::render_each`).
            let measured = export::render_each(renders, RATE, &p, &c, |render, left, right| {
                Ok(match render.track {
                    None => {
                        let slice = |x: &[f32], a: f64, b: f64| x[((a * RATE as f64) as usize).min(x.len())..((b * RATE as f64) as usize).min(x.len())].to_vec();
                        let sections = parts.iter().map(|(name, start, end, a, b)| SectionLevel { name: name.clone(), start: *start, end: *end, lufs: loudness::integrated(&slice(&left, *a, *b), &slice(&right, *a, *b), RATE) }).collect();
                        Measured::Mix(Measures::of(&left, &right, RATE), sections)
                    }
                    Some(index) => {
                        let voice = voices.get(index).copied().unwrap_or(false);
                        Measured::Track(TrackMeasures {
                            index,
                            id: 0,
                            name: String::new(),
                            drums: false,
                            kick: false,
                            listens: Vec::new(),
                            peak: loudness::sample_peak(&left, &right),
                            lufs: loudness::integrated(&left, &right, RATE),
                            correlation: loudness::correlation(&left, &right),
                            low_end: loudness::low_end(&left, &right, RATE),
                            role: Role::Other,
                            pan: 0.0,
                            cut: false,
                            bands: loudness::bands(&left, &right, RATE),
                            gain: 1.0,
                            mids: mids(&left, &right, RATE),
                            carves: Vec::new(),
                            dynamics: dynamics(&left, &right, RATE),
                            compressed: false,
                            group: false,
                            sends: [0.0; MAX_RETURNS],
                            levels: levels(&left, &right, RATE),
                            automated: false,
                            esses: if voice { esses(&left, &right, RATE) } else { None },
                            deessed: false,
                            thrown: false,
                            stereo: stereo(&left, &right, RATE),
                            utility: None,
                        })
                    }
                })
            });
            let outcome = measured.map_err(|_| "Análisis cancelado".to_string()).and_then(|all| {
                let (mut mix, mut sections) = (None, Vec::new());
                let mut tracks = kept;
                for m in all {
                    match m {
                        Measured::Mix(m, s) => (mix, sections) = (Some(m), s),
                        Measured::Track(t) => tracks.push(t),
                    }
                }
                let mix = match (mix, old_mix) {
                    (Some(m), _) => m,
                    (None, Some((m, s))) => {
                        sections = s;
                        m
                    }
                    (None, None) => return Err("Nada que analizar".to_string()),
                };
                tracks.sort_by_key(|t| t.index);
                for t in &mut tracks {
                    meta(t);
                }
                Ok(Done::Report(Report { mix, tracks, tempo, space, start, echo, sections, master_automated, limited, carved }, keys))
            });
            if let Ok(mut slot) = r.lock() {
                *slot = Some(outcome);
            }
        });
        self.mix.mix_stale = !mix;
        self.mix.job = Some(Job { progress, cancel, result });
        self.status = Some(if mix { "Analizando la mezcla…" } else { "Midiendo las pistas…" }.into());
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
        // Changed while comparing (an edit, an undo): what it is plays again.
        if self.mix.listening.as_ref().is_some_and(|l| l.at != self.project) {
            self.stop_listening();
            self.status = Some("Has cambiado la mezcla: vuelve a sonar como está".into());
        }
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
            Ok(Done::Loud { trial, held, ceiling_db, mix }) => self.set_loudness(trial, held, ceiling_db, mix),
            Ok(Done::Report(report, keys)) => {
                self.status = Some(match (self.mix.mix_stale, report.mix.lufs) {
                    (true, _) => "Pistas medidas".into(),
                    (false, Some(l)) => format!("Mezcla analizada: {} LUFS, pico real {} dBTP", num(l), db(report.mix.true_peak)),
                    (false, None) => "Mezcla analizada: está en silencio".into(),
                });
                self.mix.keys = Some((self.mix.range, keys));
                self.mix.report = Some(report);
                self.mix.stale = false;
                if self.mix.auto.is_some() {
                    self.next_auto();
                    return;
                }
                if let Some(note) = &self.mix.note {
                    self.status = Some(note.clone());
                }
            }
            Err(e) => {
                self.mix.auto = None;
                self.status = Some(e);
            }
        }
    }

    /// «Aplicar todo»: makes the advice's fixes one by one, from the analysis
    /// there is, each measured before the next.
    pub fn apply_all(&mut self) {
        self.keep_before();
        self.mix.note = None;
        self.mix.auto = Some(Vec::new());
        self.next_auto();
    }

    /// Keeps the mix as it is, before the assistant's first change (for
    /// «Antes / Después»), and stops comparing.
    fn keep_before(&mut self) {
        self.stop_listening();
        if self.mix.before.is_none() {
            let lufs = self.mix.report.as_ref().and_then(|r| r.mix.lufs);
            self.mix.before = Some((self.project.clone(), lufs));
        }
    }

    /// What the engine plays for «Antes» (`before`) or «Después»: that mix,
    /// at the loudness of the softer one.
    pub fn heard(&self, before: bool) -> Option<crate::project::Project> {
        let (old, old_lufs) = self.mix.before.as_ref()?;
        let now = self.mix.report.as_ref().filter(|_| !self.mix.stale && !self.mix.mix_stale && self.mix.job.is_none()).and_then(|r| r.mix.lufs);
        let (down_before, down_now) = matched(*old_lufs, now);
        Some(if before { levelled(old, down_before) } else { levelled(&self.project, down_now) })
    }

    /// Plays the mix before the assistant's changes, or the one now, as loud
    /// as each other. Only the engine changes, with only what differs.
    pub fn listen(&mut self, before: bool) {
        let Some(target) = self.heard(before) else { return };
        let playing = self.mix.listening.take().map_or_else(|| self.project.clone(), |l| l.engine);
        self.capture_plugins();
        let rate = self.audio.sample_rate();
        for command in crate::sync::changed_commands(&playing, &target, &mut self.samples, rate, &self.overridden) {
            self.audio.send(command);
        }
        self.mix.listening = Some(Listening { before, at: self.project.clone(), engine: target });
    }

    /// Back to the mix as it is.
    pub fn stop_listening(&mut self) {
        if let Some(l) = self.mix.listening.take() {
            self.sync_from(&l.engine);
        }
    }

    /// The next step of «Aplicar todo», or its end.
    fn next_auto(&mut self) {
        let (Some(report), Some(done)) = (self.mix.report.clone(), self.mix.auto.clone()) else { return };
        let reference = self.mix.reference.as_ref().map(|(_, m)| m.clone());
        let advice = advise(&report, self.mix.target, reference.as_ref());
        // A change undone by hand meanwhile, or a mix that keeps asking: a cap.
        let next = if done.len() >= 40 { None } else { next_fix(&advice, &done) };
        // What comes next hears the mix (its parts, its loudness), or it is
        // the end: the mix measured again first, as it is now.
        // Right after the parts that grow (the master's fader, after its
        // limiter), the loudness measures the mix itself (see `reach_loudness`).
        let loudness = matches!(next, Some(Fix::MasterLimiter { .. })) && matches!(done.last(), Some(Fix::Grow { .. })) && self.mix.target.levels(reference.as_ref()).is_some();
        if self.mix.mix_stale && !loudness && next.as_ref().is_none_or(|f| stage(f) >= stage(&Fix::Grow { cuts: Vec::new() })) {
            self.analyze(true);
            if self.mix.auto.is_some() {
                self.status = Some(format!("Mezcla completa, paso {}: midiendo la mezcla…", done.len() + 1));
            }
            return;
        }
        let Some(fix) = next else {
            self.mix.auto = None;
            let mut labels: Vec<String> = Vec::new();
            let mut balanced = false;
            for fix in &done {
                let label = match fix {
                    Fix::Balance(_) if balanced => "volúmenes reajustados".into(),
                    _ => fix_label(fix),
                };
                balanced |= matches!(fix, Fix::Balance(_));
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
            let summary = match labels.is_empty() {
                true => "Mezcla completa: no había nada que cambiar".to_string(),
                false => format!("Mezcla completa en {} pasos: {}. Cada paso se deshace con ⌘Z", done.len(), labels.join(", ")),
            };
            // What the limiter could not reach stays said.
            self.mix.note = Some(match self.mix.note.take() {
                Some(note) => format!("{summary}. {note}"),
                None => summary,
            });
            self.status = self.mix.note.clone();
            return;
        };
        let mut done = done;
        done.push(fix.clone());
        let step = done.len();
        // Room in the mids: every pair advised at once (each its own band of
        // its own track), measured once after. One by one, each would cost a
        // render of the whole song.
        if let Fix::Carve { .. } = &fix {
            let mut carves = vec![fix.clone()];
            while let Some(more) = next_fix(&advice, &done).filter(|f| matches!(f, Fix::Carve { .. })) {
                let Fix::Carve { track, hz, .. } = &more else { unreachable!() };
                let twice = carves.iter().any(|c| matches!(c, Fix::Carve { track: t, hz: h, .. } if t == track && (h / hz).log2().abs() < 0.25));
                done.push(more.clone());
                if !twice {
                    carves.push(more);
                }
            }
            self.mix.auto = Some(done);
            if carves.iter().all(|c| self.make(c)) {
                self.sync_all();
                self.mix.stale = true;
                self.analyze(false);
            }
            if self.mix.auto.is_some() {
                let names: Vec<String> = carves.iter().map(fix_label).collect();
                self.status = Some(format!("Mezcla completa, paso {step}: {}…", names.join(", ")));
            }
            return;
        }
        self.mix.auto = Some(done);
        match (&fix, self.mix.target.levels(reference.as_ref())) {
            (Fix::MasterLimiter { .. }, Some((goal, peak))) => self.reach_loudness(goal, peak),
            _ => self.apply_fix(&fix),
        }
        if self.mix.auto.is_some() {
            self.status = Some(format!("Mezcla completa, paso {step}: {}…", fix_label(&fix)));
        }
    }

    /// Brings the mix to `goal` LUFS, its true peak at most `max_peak` dBTP,
    /// with the master's limiter (made if there is none): renders the master
    /// up to it once, and tries gains on that (see [`loudest_gain`]) in the
    /// background. Its peaks no more than [`MAX_LIMITER_GAIN`] over the
    /// ceiling, nor so much that the
    /// song loses its shape: then it says what else to do.
    pub fn reach_loudness(&mut self, goal: f64, max_peak: f64) {
        self.keep_before();
        let ceiling_db = (max_peak - PEAK_MARGIN) as f32;
        if goal >= LOUD {
            self.loud_master(ceiling_db);
        }
        let devices = &mut self.project.master.devices;
        let at = match devices.iter().rposition(|d| matches!(d.effect, Effect::Limiter(_))) {
            Some(i) => i,
            None => {
                devices.push(Device::new(Effect::Limiter(LimiterParams { ceiling_db, ..Default::default() })));
                devices.len() - 1
            }
        };
        let limiter = match &devices[at].effect {
            Effect::Limiter(l) => LimiterParams { ceiling_db, ..*l },
            _ => return,
        };
        // The master up to its limiter, with no fader: those are tried on it.
        let mut before = self.project.clone();
        before.master.devices.truncate(at);
        before.master.automation.retain(|l| !matches!(l.param, Param::Volume | Param::Device { .. }) || matches!(l.param, Param::Device { device, .. } if device < at));
        before.master.mixer.gain = 1.0;
        let range = self.mix.range;
        self.capture_plugins();
        let live = std::mem::take(&mut self.samples.plugins);
        let renders = export::plan(&before, &mut self.samples, &export::Settings { range, stems: false, sample_rate: RATE as u32, ..Default::default() }, std::path::Path::new("mezcla.wav"));
        self.samples.plugins = live;
        let master = self.project.master.clone();
        let tempo = self.project.tempo;
        let start = export::beat_range(&self.project, range).map_or(0.0, |r| r.0);
        let parts: Vec<(String, f64, f64, usize, usize)> = match range {
            Range::Loop => Vec::new(),
            _ => export::sections(&self.project).into_iter().map(|(name, (a, b))| (name, a, b, (seconds_between(&self.project, 0.0, a) * RATE as f64) as usize, (seconds_between(&self.project, 0.0, b) * RATE as f64) as usize)).collect(),
        };
        let sections: Vec<(String, usize, usize)> = parts.iter().map(|p| (p.0.clone(), p.3, p.4)).collect();
        // Nothing after the limiter, the master in the middle: what comes out
        // is the limiter on that render, then the fader. Else it is rendered.
        let alone = at + 1 == self.project.master.devices.len() && self.project.master.mixer.pan.abs() < 1e-6;
        let (progress, cancel, result) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (p, c, r) = (progress.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            let rendered = export::render_each(renders, RATE, &p, &c, |_, left, right| Ok((left, right)));
            let outcome = rendered.map_err(|_| "Cancelado".to_string()).and_then(|mut all| {
                let (left, right) = all.pop().ok_or("Nada que medir")?;
                let fader = master_gains(&master, tempo, start, left.len());
                let (trial, held) = loudest_gain(&left, &right, &fader, limiter, &sections, goal, max_peak, &c).ok_or("La mezcla está en silencio")?;
                let mix = alone.then(|| {
                    let (mut l, mut r) = (left, right);
                    engine::OfflineLimiter::new(LimiterParams { gain_db: (trial.gain_db * 10.0).round() / 10.0, ..limiter }, RATE).process(&mut l, &mut r, RATE);
                    for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                        let g = fader.get(i / FADER_BLOCK).copied().unwrap_or(1.0);
                        (*a, *b) = (*a * g, *b * g);
                    }
                    let slice = |x: &[f32], a: usize, b: usize| x[a.min(x.len())..b.min(x.len())].to_vec();
                    let sections = parts.iter().map(|(name, start, end, a, b)| SectionLevel { name: name.clone(), start: *start, end: *end, lufs: loudness::integrated(&slice(&l, *a, *b), &slice(&r, *a, *b), RATE) }).collect();
                    (Measures::of(&l, &r, RATE), sections)
                });
                Ok(Done::Loud { trial, held, ceiling_db, mix })
            });
            if let Ok(mut slot) = r.lock() {
                *slot = Some(outcome);
            }
        });
        self.mix.job = Some(Job { progress, cancel, result });
        self.status = Some("Buscando la ganancia del limitador…".into());
    }

    /// The limiter's gain found (see [`PulseApp::reach_loudness`]): set, said
    /// if short of the goal, and the mix analysed again.
    fn set_loudness(&mut self, trial: Trial, held: Option<Held>, ceiling_db: f32, mix: Option<(Measures, Vec<SectionLevel>)>) {
        let devices = &mut self.project.master.devices;
        if let Some(l) = devices.iter_mut().rev().find_map(|d| if let Effect::Limiter(l) = &mut d.effect { Some(l) } else { None }) {
            l.gain_db = (trial.gain_db * 10.0).round() / 10.0;
            l.ceiling_db = ceiling_db;
        }
        let gain = self.master_limiter_gain();
        let reference = self.mix.reference.as_ref().map(|(_, m)| m.clone());
        let goal = self.mix.target.levels(reference.as_ref()).map(|l| l.0);
        self.mix.limited = held.clone().map(|held| Limited { target: self.mix.target, gain_db: gain, held });
        if let (Some(held), Some(goal), Some(lufs)) = (held, goal, trial.lufs) {
            self.mix.note = Some(format!("Con el limitador a {} dB se queda en unos {} LUFS, {} dB por debajo de {}. {}. {LOUDER_WITHOUT_SQUASHING}", db(gain as f64), num(lufs), num(goal - lufs), num(goal), held.says()));
        }
        self.status = Some(format!("Limitador del master: {} dB, techo {} dB", db(gain as f64), num(ceiling_db as f64)));
        self.sync_all();
        // The mix as it comes out, measured already: the analysis has it.
        if let (Some((measures, sections)), Some(report)) = (mix, self.mix.report.as_mut()) {
            (report.mix, report.sections) = (measures, sections);
            report.limited = self.mix.limited.clone();
            self.mix.mix_stale = false;
            self.mix.stale = false;
            if self.mix.auto.is_some() {
                self.next_auto();
            } else if let Some(note) = &self.mix.note {
                self.status = Some(note.clone());
            }
            return;
        }
        self.mix.stale = true;
        self.analyze(true);
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

    /// Makes the change, and analyses again to show what it did.
    pub fn apply_fix(&mut self, fix: &Fix) {
        if !self.make(fix) {
            return;
        }
        self.sync_all();
        self.mix.stale = true;
        // While applying everything, a track's change is measured on the
        // tracks alone: the mix, when something hears it (see `next_auto`).
        let master = matches!(fix, Fix::MasterEq { .. } | Fix::MasterLimiter { .. });
        self.analyze(self.mix.auto.is_none() || master);
    }

    /// Makes the change (not analysed); false if it couldn't.
    fn make(&mut self, fix: &Fix) -> bool {
        self.keep_before();
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
                louder(&mut self.project.tracks[*track], *db);
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
                    louder(track, c.gain_db);
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
                if let Some(to) = self.project.tracks.iter().find(|t| &t.name == for_name).map(|t| t.id) {
                    self.mix.carved.push((self.project.tracks[*track].id, to));
                }
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
                self.status = Some(format!("«{name}» deja hueco a «{for_name}» en {} (EQ Eight, {CARVE_DB} dB)", freq_label(*hz)));
            }
            Fix::Compress { track, name, params } if track_ok(self, *track, name) => {
                self.project.tracks[*track].devices.push(Device::new(Effect::Compressor(*params)));
                self.status = Some(format!("Compresor en «{name}»: umbral {} dB, {}:1", num(params.threshold_db as f64), num(params.ratio as f64)));
            }
            Fix::Space { reverb, delay, changes } if changes.iter().all(|c| track_ok(self, c.track, &c.name)) && Space::of(&self.project.returns) == Space { reverb: *reverb, delay: *delay, returns: self.project.returns.len() } => {
                let wants = |f: fn(&SpaceChange) -> Option<f32>| changes.iter().any(|c| f(c).unwrap_or(0.0) > 0.0);
                let mut made = Vec::new();
                let mut make = |app: &mut PulseApp, at: Option<usize>, wanted: bool, new: fn(usize) -> crate::project::ReturnTrack| match at {
                    Some(i) => Some(i),
                    None if wanted && app.project.returns.len() < MAX_RETURNS => {
                        let i = app.project.returns.len();
                        app.project.returns.push(new(i));
                        made.push(app.project.returns[i].name.clone());
                        Some(i)
                    }
                    None => None,
                };
                let reverb = make(self, *reverb, wants(|c| c.reverb), reverb_return);
                let delay = make(self, *delay, wants(|c| c.delay), delay_return);
                for c in changes {
                    let sends = &mut self.project.tracks[c.track].sends;
                    for (at, amount) in [(reverb, c.reverb), (delay, c.delay)] {
                        if let (Some(i), Some(x)) = (at, amount) {
                            sends[i] = x;
                        }
                    }
                }
                self.status = Some(match made.is_empty() {
                    true => format!("Espacio de partida: {} pistas con su reverb y su delay (⌘Z lo deshace)", changes.len()),
                    false => format!("Espacio de partida: {} pistas, y nuevo{} {} (⌘Z lo deshace)", changes.len(), if made.len() > 1 { "s" } else { "" }, made.iter().map(|n| format!("«{n}»")).collect::<Vec<_>>().join(" y ")),
                });
            }
            Fix::Throws { track, name, delay, from, ends, base } if track_ok(self, *track, name) && Space::of(&self.project.returns).delay == *delay => {
                let delay = match delay {
                    Some(i) => *i,
                    None if self.project.returns.len() < MAX_RETURNS => {
                        let i = self.project.returns.len();
                        self.project.returns.push(delay_return(i));
                        i
                    }
                    None => {
                        self.status = Some("No queda sitio para un retorno de delay".into());
                        self.mix.auto = None;
                        return false;
                    }
                };
                let points: Vec<engine::model::AutomationPoint> =
                    throw_points(ends, *base).into_iter().map(|(at, value)| engine::model::AutomationPoint { beat: beat_after(&self.project, *from, at), value, curve: 0.0 }).collect();
                let automation = &mut self.project.tracks[*track].automation;
                automation.retain(|l| l.param != Param::Send(delay));
                let mut lane = engine::model::AutomationLane::new(Param::Send(delay));
                lane.points = points;
                lane.sort();
                automation.push(lane);
                self.status = Some(format!("Ecos de «{name}» en {} finales de frase (⌘Z lo deshace)", ends.len()));
            }
            Fix::Stereo { track, name, params } if track_ok(self, *track, name) => {
                let devices = &mut self.project.tracks[*track].devices;
                match devices.iter_mut().rev().find_map(|d| if let (true, Effect::Utility(u)) = (d.enabled, &mut d.effect) { Some(u) } else { None }) {
                    Some(u) => *u = *params,
                    None => devices.push(Device::new(Effect::Utility(*params))),
                }
                self.status = Some(format!("Utility de «{name}» ajustado"));
            }
            Fix::Grow { cuts } if !self.project.master.automation.iter().any(|l| l.param == Param::Volume && !l.points.is_empty()) => {
                let base = self.project.master.mixer.gain;
                let mut lane = engine::model::AutomationLane::new(Param::Volume);
                // Each part held at its level, moving in the last 1/32 beat
                // before the next; two parts brought down one after the other
                // go from one to the other.
                let edge = 1.0 / 32.0;
                let mut points = Vec::new();
                for (i, (_, start, end, d)) in cuts.iter().enumerate() {
                    let value = base * 10f32.powf(-d / 20.0);
                    let joined_before = i > 0 && (cuts[i - 1].2 - start).abs() < 1e-9;
                    let joined_after = cuts.get(i + 1).is_some_and(|next| (next.1 - end).abs() < 1e-9);
                    if !joined_before {
                        points.push((start - edge, base));
                    }
                    points.extend([(*start, value), (end - edge, value)]);
                    if !joined_after {
                        points.push((*end, base));
                    }
                }
                lane.points = points.into_iter().filter(|p| p.0 >= 0.0).map(|(beat, value)| engine::model::AutomationPoint { beat, value, curve: 0.0 }).collect();
                lane.sort();
                self.project.master.automation.push(lane);
                self.status = Some(format!("Master más bajo en {} (⌘Z lo deshace)", cuts.iter().map(|c| format!("«{}» {} dB", c.0, db(-c.3 as f64))).collect::<Vec<_>>().join(", ")));
            }
            Fix::DeEss { track, name, params } if track_ok(self, *track, name) => {
                self.project.tracks[*track].devices.push(Device::new(Effect::DeEsser(*params)));
                self.status = Some(format!("De-esser en «{name}»: {}, umbral {} dB", freq_label(params.freq_hz), num(params.threshold_db as f64)));
            }
            Fix::Ride { track, name, from, phrases, gains } if track_ok(self, *track, name) => {
                let fader = self.project.tracks[*track].mixer.gain;
                let points: Vec<engine::model::AutomationPoint> = ride_points(phrases, gains, fader)
                    .into_iter()
                    .map(|(at, value)| engine::model::AutomationPoint { beat: beat_after(&self.project, *from, at), value, curve: 0.0 })
                    .collect();
                let automation = &mut self.project.tracks[*track].automation;
                automation.retain(|l| l.param != Param::Volume);
                let mut lane = engine::model::AutomationLane::new(Param::Volume);
                lane.points = points;
                lane.sort();
                automation.push(lane);
                self.sync_automation(engine::Target::Track(*track));
                self.status = Some(format!("«{name}» nivelada: {} frases con automatización de volumen (⌘Z lo deshace)", gains.iter().filter(|g| **g != 0.0).count()));
            }
            _ => {
                self.status = Some("Esa pista ya no está donde estaba: vuelve a analizar".into());
                self.mix.auto = None;
                return false;
            }
        }
        true
    }
}

// ----- Window -------------------------------------------------------------------------

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.mix.open {
        if app.mix.listening.is_some() {
            app.stop_listening();
        }
        return;
    }
    let mut open = true;
    egui::Window::new("Asistente de mezcla y master")
        .id(egui::Id::new("mix assistant"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([640.0, 640.0_f32.min(ctx.content_rect().height() - 80.0)])
        // On a small screen (1440×900) the whole report is taller than it: it scrolls.
        .max_height(ctx.content_rect().height() - 60.0)
        .constrain_to(ctx.content_rect())
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| contents(app, ui)));
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
    compare_row(app, ui);
    let reference = app.mix.reference.as_ref().map(|(_, m)| m.clone());
    let list = advise(&report, app.mix.target, reference.as_ref());
    let mut chosen = None;
    let busy = app.mix.job.is_some();
    let mut all = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Consejos").strong());
        match &app.mix.auto {
            Some(done) => {
                ui.label(RichText::new(format!("Mezclando: paso {}…", done.len())).color(ACCENT));
            }
            None => {
                let any = next_fix(&list, &[]).is_some();
                all = ui
                    .add_enabled(!busy && any, egui::Button::new("Aplicar todo").small())
                    .on_hover_text("Hace los cambios de los consejos en el orden de una mezcla: picos, volúmenes y panoramas, graves, medios, compresión, eses, voz nivelada, reverb y delay, ecos, secciones, y el master. Vuelve a medir después de cada uno")
                    .clicked();
            }
        }
    });
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
    if all {
        app.apply_all();
    }
    if let Some(fix) = chosen {
        app.mix.note = None;
        match (&fix, app.mix.target.levels(reference.as_ref())) {
            (Fix::MasterLimiter { .. }, Some((goal, peak))) => app.reach_loudness(goal, peak),
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

/// «Escuchar: Antes · Después», once the assistant has changed the mix.
fn compare_row(app: &mut PulseApp, ui: &mut egui::Ui) {
    let Some((old, old_lufs)) = &app.mix.before else { return };
    if *old == app.project && app.mix.listening.is_none() {
        return;
    }
    let old_lufs = *old_lufs;
    let listening = app.mix.listening.as_ref().map(|l| l.before);
    let now = app.mix.report.as_ref().filter(|_| !app.mix.stale && app.mix.job.is_none()).and_then(|r| r.mix.lufs);
    let mut choice = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Escuchar").color(TEXT_DIM));
        if ui.selectable_label(listening == Some(true), "Antes").on_hover_text("La mezcla como estaba antes de los cambios del asistente").clicked() {
            choice = Some(Some(true));
        }
        if ui.selectable_label(listening == Some(false), "Después").on_hover_text("La mezcla como está ahora").clicked() {
            choice = Some(Some(false));
        }
        if listening.is_some() && ui.small_button("Dejar de comparar").clicked() {
            choice = Some(None);
        }
        let text = match (old_lufs, now) {
            (Some(b), Some(n)) => format!("al mismo volumen (antes {} LUFS, ahora {}), para que lo más fuerte no te engañe", num(b), num(n)),
            _ => "sin igualar el volumen: vuelve a analizar".into(),
        };
        ui.label(RichText::new(text).color(TEXT_DIM).size(10.5));
    });
    match choice {
        Some(Some(before)) => {
            app.listen(before);
            if !app.audio.is_playing() {
                app.status = Some("Dale a reproducir para oírlas".into());
            }
        }
        Some(None) => app.stop_listening(),
        None => {}
    }
}

/// A track `db` louder: its fader, and its volume automation if it has
/// some (which would hold it where it was).
fn louder(track: &mut crate::project::Track, db: f32) {
    let factor = 10f32.powf(db / 20.0);
    track.mixer.gain *= factor;
    for lane in track.automation.iter_mut().filter(|l| l.param == Param::Volume) {
        for p in &mut lane.points {
            p.value = (p.value * factor).min(FADER_TOP);
        }
    }
}

fn fix_hint(fix: &Fix) -> String {
    match fix {
        Fix::Stereo { name, params, .. } => {
            let mut what = Vec::new();
            if params.flip_right {
                what.push("invierte el canal derecho".to_string());
            }
            if params.width < 1.0 {
                what.push(format!("ancho al {:.0} %", params.width * 100.0));
            }
            if params.bass_mono_hz > 0.0 {
                what.push(format!("graves en mono por debajo de {:.0} Hz", params.bass_mono_hz));
            }
            format!("Utility en «{name}»: {}", what.join(", "))
        }
        Fix::Grow { .. } => "Escribe automatización de volumen en el master: baja lo que lleva al estribillo o al drop para que estos suenen más grandes, sin tocar el limitador".into(),
        Fix::Throws { name, .. } => format!("Escribe automatización en el envío de «{name}» al delay: se abre al {:.0} % en la última palabra de cada frase que acaba en silencio y vuelve a donde estaba al acabar", THROW * 100.0),
        Fix::DeEss { name, params, .. } => format!("Añade un De-esser a «{name}»: por encima de {}, lo que pase de {} dB baja (como mucho {} dB)", freq_label(params.freq_hz), num(params.threshold_db as f64), num(params.range_db as f64)),
        Fix::Ride { name, .. } => format!("Escribe automatización de volumen en «{name}»: cada frase sube o baja (como mucho {RIDE_MAX} dB) para quedar igual de por encima de la base. Los cambios caen en los silencios entre frases"),
        Fix::MasterLimiter { gain_db, ceiling_db } => format!("Limiter en el master: ganancia {gain_db:+.1} dB, techo {ceiling_db:.1} dB"),
        Fix::TrackGain { name, db, .. } => format!("Volumen de «{name}» {db:+.1} dB"),
        Fix::Duck { name, source_name, .. } => format!("Añade un Ducker a «{name}» que escucha a «{source_name}» (sidechain): baja cuando suena, para dejarle sitio"),
        Fix::CutLows { name, .. } => format!("Añade un EQ Three a «{name}» con los graves (bajo 150 Hz) a −9 dB"),
        Fix::MasterEq { low, mid, high } => format!("EQ Three en el master: graves {low:+.1}, medios {mid:+.1}, agudos {high:+.1} dB"),
        Fix::Carve { name, hz, for_name, .. } => format!("Baja {} dB «{name}» en {} (un EQ Eight, campana estrecha) para que «{for_name}» se oiga ahí", -CARVE_DB, freq_label(*hz)),
        Fix::Compress { name, params, .. } => format!(
            "Añade un Compressor a «{name}»: umbral {:.1} dB, ratio {}:1, ataque {:.0} ms, release {:.0} ms (una semicorchea al tempo; el bajo, una corchea) y {:+.1} dB de ganancia para que siga igual de fuerte",
            params.threshold_db, params.ratio, params.attack_ms, params.release_ms, params.makeup_db
        ),
        Fix::Space { .. } => "Pone los envíos de cada pista a la reverb y al delay según su papel (y crea esos retornos si faltan): un punto de partida para seguir mezclando a oído. ⌘Z lo deshace todo".into(),
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
        TrackMeasures { index, id: index as u32 + 10, name: name.into(), drums, kick: false, listens: Vec::new(), peak, lufs: Some(-20.0), correlation: 0.9, low_end: low, role: Role::Other, pan: 0.0, cut: false, bands: [0.0; 10], gain: 0.5, mids: Vec::new(), carves: Vec::new(), dynamics: None, compressed: false, group: false, sends: [0.0; MAX_RETURNS], levels: Vec::new(), automated: false, esses: None, deessed: false, thrown: false, stereo: Stereo::default(), utility: None }
    }

    fn report(lufs: f64, true_peak: f64, tracks: Vec<TrackMeasures>) -> Report {
        Report { mix: Measures { lufs: Some(lufs), range: Some(6.0), short_max: Some(lufs + 3.0), true_peak, peak: true_peak, correlation: 0.8, bands: [0.0; 10] }, tracks, tempo: 120.0, space: Space { reverb: Some(0), delay: Some(1), returns: 2 }, start: 0.0, echo: 0.75, sections: Vec::new(), master_automated: false, limited: None, carved: Vec::new() }
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
        let (notes, groove) = drum_gen::generate(&kit, &settings, 32.0, &[], 0, engine::meter::TimeSig::COMMON);
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
        assert!(list.iter().any(|a| matches!(a.fix, Some(Fix::MasterLimiter { .. }))), "a limiter fix: {list:?}");
        app.reach_loudness(-7.0, -1.0);
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
        let loud = Report { mix: Measures { lufs: Some(-5.0), true_peak: -1.2, ..mix }, tracks: Vec::new(), tempo: 150.0, space: Space::default(), start: 0.0, echo: 0.75, sections: Vec::new(), master_automated: false, limited: None, carved: Vec::new() };
        let told = advise(&loud, Target::HardTechno, None);
        assert!(told.iter().any(|a| a.text.contains("pierde pegada")) && !told.iter().any(|a| a.text.contains("plataforma")), "{told:?}");
    }

    #[test]
    fn quiet_mixes_get_a_limiter_and_loud_ones_are_told() {
        let quiet = advise(&report(-20.0, -6.0, Vec::new()), Target::Streaming, None);
        assert!(quiet.iter().any(|a| a.fix == Some(Fix::MasterLimiter { gain_db: 6.0, ceiling_db: -1.3 })), "{quiet:?}");
        let right = advise(&report(-14.3, -1.5, Vec::new()), Target::Streaming, None);
        assert!(right.iter().all(|a| a.level == Level::Good), "{right:?}");
        // On target but peaking over -1 dBTP: the ceiling comes down.
        let peaky = advise(&report(-14.0, 0.4, Vec::new()), Target::Streaming, None);
        assert!(peaky.iter().any(|a| a.fix == Some(Fix::MasterLimiter { gain_db: 0.0, ceiling_db: -1.3 })));
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
        assert!(list.iter().any(|a| matches!(a.fix, Some(Fix::MasterLimiter { .. }))), "a limiter fix");
        // -7 is more than this loop gives without squashing it: it stops short
        // and says so, without the limiter driven to the end.
        app.reach_loudness(-7.0, -1.0);
        wait(&mut app);
        let after = app.mix.report.as_ref().unwrap().mix.lufs.unwrap();
        println!("demo: {before:.1} → {after:.1} LUFS (objetivo -7), pico real {:.1} dBTP", app.mix.report.as_ref().unwrap().mix.true_peak);
        assert!(after > before + 1.5, "{before} → {after}");
        assert!(app.mix.report.as_ref().unwrap().mix.true_peak <= -1.0, "under -1 dBTP");
        assert!(app.mix.report.as_ref().unwrap().limited.is_some(), "it stopped short on purpose");
        assert!(app.status.as_deref().is_some_and(|s| s.contains("sin aplastarla")), "{:?}", app.status);
        // A loudness it can reach, it reaches.
        app.reach_loudness(-11.0, -1.0);
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
        // (Its lows in mono, a Utility, are another matter.)
        assert!(!advice.iter().any(|a| a.fix.is_some() && !matches!(a.fix, Some(Fix::Stereo { .. })) && a.text.contains("Rumble")), "the rumble ducks to its kick already");
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
            // The track Pulse puts its risers and impacts on.
            ("Transiciones", Role::Fx),
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

    /// The dry tracks get their role's sends; the bass loses the reverb that
    /// muddies it; what sends already, and the kick, are left alone.
    #[test]
    fn dry_tracks_get_their_room() {
        let t = |index: usize, name: &str, role: Role| TrackMeasures { role, ..track(index, name, false, -6.0, Vec::new()) };
        let mut bass = t(2, "Bajo", Role::Bass);
        bass.sends[0] = 0.4;
        let mut chords = t(3, "Acordes", Role::Chords);
        chords.sends[1] = 0.2;
        let tracks = vec![t(0, "Voz", Role::Vocal), t(1, "Pad", Role::Pad), bass, chords, t(4, "Kick", Role::Kick), TrackMeasures { lufs: None, ..t(5, "Coros", Role::Backing) }, TrackMeasures { group: true, ..t(6, "Voces", Role::Vocal) }];
        let space = Space { reverb: Some(0), delay: Some(1), returns: 2 };
        let changes = space_changes(&tracks, space);
        let change = |track: usize, name: &str, role: Role, reverb: Option<f32>, delay: Option<f32>| SpaceChange { track, name: name.into(), role, reverb, delay };
        assert_eq!(
            changes,
            [change(0, "Voz", Role::Vocal, Some(0.22), Some(0.14)), change(1, "Pad", Role::Pad, Some(0.28), None), change(2, "Bajo", Role::Bass, Some(0.0), None)]
        );
        let list = advise(&report(-14.0, -1.5, tracks.clone()), Target::Streaming, None);
        let a = list.iter().find(|a| matches!(a.fix, Some(Fix::Space { .. }))).expect("space advice");
        assert!(a.text.starts_with("Espacio de partida: «Voz», «Pad» suenan secas"), "{}", a.text);
        assert_eq!(a.details[0], "«Voz» (voz): reverb 22 %, delay 14 %");
        assert!(a.details[2].contains("sin reverb"), "{:?}", a.details);
        // Only the bass to fix: a warning about it.
        let only_bass = vec![tracks[2].clone(), tracks[3].clone()];
        let list = advise(&report(-14.0, -1.5, only_bass), Target::Streaming, None);
        assert!(list.iter().any(|a| a.level == Level::Warning && a.text.starts_with("«Bajo» (bajo) manda mucho a la reverb")), "{list:?}");
        // Everything sending: fine.
        let list = advise(&report(-14.0, -1.5, vec![tracks[3].clone(), tracks[4].clone()]), Target::Streaming, None);
        assert!(list.iter().any(|a| a.level == Level::Good && a.text.starts_with("Las pistas tienen su espacio")), "{list:?}");
    }

    /// With no returns, it makes a reverb and a delay; with the returns full
    /// and none a reverb, there is nothing it can do.
    #[test]
    fn missing_returns_are_made_if_there_is_room() {
        let voice = TrackMeasures { role: Role::Vocal, ..track(0, "Voz", false, -6.0, Vec::new()) };
        let pad = TrackMeasures { role: Role::Pad, ..track(1, "Pad", false, -6.0, Vec::new()) };
        let none = Space { reverb: None, delay: None, returns: 0 };
        assert_eq!(space_changes(&[voice.clone(), pad.clone()], none).len(), 2);
        let list = advise(&Report { space: none, ..report(-14.0, -1.5, vec![voice.clone()]) }, Target::Streaming, None);
        assert!(list.iter().any(|a| a.text.ends_with("Crea un retorno de reverb y otro de delay")), "{list:?}");
        // One return left: the reverb, not the delay.
        let one = Space { reverb: None, delay: None, returns: MAX_RETURNS - 1 };
        let changes = space_changes(&[voice.clone()], one);
        assert_eq!(changes[0].reverb, Some(0.22));
        assert_eq!(changes[0].delay, None);
        // Full, with a delay only: the delay.
        let full = Space { reverb: None, delay: Some(0), returns: MAX_RETURNS };
        let changes = space_changes(&[voice.clone(), pad.clone()], full);
        assert_eq!(changes, [SpaceChange { track: 0, name: "Voz".into(), role: Role::Vocal, reverb: None, delay: Some(0.14) }]);
        // The returns found by what they have.
        let mut project = crate::project::Project::empty();
        assert_eq!(Space::of(&project.returns), Space { reverb: Some(0), delay: Some(1), returns: 2 });
        project.returns.swap(0, 1);
        project.returns[1].devices[0].enabled = false;
        assert_eq!(Space::of(&project.returns), Space { reverb: None, delay: Some(0), returns: 2 });
    }

    /// For real: a voice in short phrases and a bass, both dry, in a project
    /// without returns. Applied, there is a reverb and a delay, the voice
    /// sends to both and the bass to none, and the silences between the
    /// phrases fill with their tail.
    #[test]
    fn a_dry_voice_gets_its_room() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let tone = |hz: f32, on: f32| -> Vec<f32> {
            (0..(8.0 * rate) as usize)
                .map(|i| {
                    let t = i as f32 / rate;
                    if t % 2.0 < on {
                        0.3 * (std::f32::consts::TAU * hz * t).sin()
                    } else {
                        0.0
                    }
                })
                .collect()
        };
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let clip = |name: &str, audio: Vec<f32>| {
            let path = dir.join(format!("espacio-{name}-{}.wav", std::process::id()));
            crate::export::write(&path, &audio, &audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
            engine::model::ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: name.into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) }
        };
        let mut project = Project::empty();
        project.tempo = 120.0;
        project.returns.clear();
        let mut voice = Track::new_audio("Voz", [0; 3]);
        voice.arrangement.push(clip("voz", tone(440.0, 0.5)));
        let mut bass = Track::new_audio("Bajo", [0; 3]);
        bass.arrangement.push(clip("bajo", tone(55.0, 0.5)));
        project.tracks = vec![voice, bass];
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
        // The mix's level (dB) between 0.7 and 1.9 s of each 2 s, where only a tail can sound.
        let gaps = |app: &mut PulseApp| {
            let settings = export::Settings::default();
            let plan = export::plan(&app.project, &mut app.samples, &settings, std::path::Path::new("espacio.wav"));
            let mut mix = export::render_each(plan, RATE, &AtomicU32::new(0), &AtomicBool::new(false), |_, left, _| Ok(left)).unwrap();
            let left = mix.remove(0);
            let tail: Vec<f32> = left.iter().enumerate().filter(|(i, _)| (0.7..1.9).contains(&((*i as f32 / RATE) % 2.0))).map(|(_, &x)| x).collect();
            10.0 * ((tail.iter().map(|x| x * x).sum::<f32>() / tail.len().max(1) as f32).max(1e-12)).log10()
        };
        let before = gaps(&mut app);
        app.analyze_mix();
        wait(&mut app);
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Space { .. }))).expect("room to give");
        let Fix::Space { changes, .. } = &fix else { unreachable!() };
        assert_eq!(changes.len(), 1, "{changes:?}");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let names: Vec<&str> = app.project.returns.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["A Reverb", "B Delay"]);
        assert_eq!(&app.project.tracks[0].sends[..2], &[0.22, 0.14]);
        assert!(app.project.tracks[1].sends.iter().all(|&s| s == 0.0), "the bass stays dry");
        let after = gaps(&mut app);
        println!("silencios: {before:.1} → {after:.1} dB");
        assert!(before < -100.0 && after > -50.0, "{before} → {after}");
        assert!(advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None).iter().any(|a| a.level == Level::Good && a.text.starts_with("Las pistas tienen su espacio")));
    }

    /// The order of «Aplicar todo»: the earliest stage first, nothing twice,
    /// the balance once, and nothing after the master's loudness.
    #[test]
    fn everything_is_applied_in_the_order_of_a_mix() {
        let fix = |f: Fix| advice(Level::Note, String::new(), Some(f));
        let limiter = Fix::MasterLimiter { gain_db: 3.0, ceiling_db: -1.3 };
        let space = Fix::Space { reverb: Some(0), delay: Some(1), changes: Vec::new() };
        let gain = Fix::TrackGain { track: 0, name: "Voz".into(), db: -2.0 };
        let list = vec![fix(limiter.clone()), fix(space.clone()), fix(Fix::Balance(Vec::new())), fix(gain.clone()), advice(Level::Good, String::new(), None)];
        assert_eq!(next_fix(&list, &[]), Some(gain.clone()));
        assert_eq!(next_fix(&list, &[gain.clone()]), Some(Fix::Balance(Vec::new())));
        let other_balance = Fix::Balance(vec![Change { track: 0, name: "Voz".into(), role: Role::Vocal, gain_db: 1.0, pan: None, low_cut: None }]);
        let again = vec![fix(other_balance), fix(space.clone())];
        // Balanced again after the space, before the master; not a third time.
        assert_eq!(next_fix(&again, &[Fix::Balance(Vec::new())]), Some(space.clone()));
        assert_eq!(next_fix(&again, &[Fix::Balance(Vec::new()), space.clone()]), Some(again[0].fix.clone().unwrap()));
        let third = vec![fix(Fix::Balance(vec![]))];
        assert_eq!(next_fix(&third, &[Fix::Balance(vec![]), again[0].fix.clone().unwrap()]), None);
        // One carve a pair.
        let carve = |hz: f32, to: &str| Fix::Carve { track: 1, name: "Acordes".into(), hz, for_name: to.into() };
        let carves = vec![fix(carve(1000.0, "Voz")), fix(carve(2000.0, "Voz")), fix(carve(500.0, "Batería"))];
        assert_eq!(next_fix(&carves, &[carve(1000.0, "Voz")]), Some(carve(500.0, "Batería")));
        assert_eq!(next_fix(&list, &[gain, limiter]), None);
    }

    /// For real: the song from «De tarareo a canción», out of balance and
    /// quiet, mixed in one click. It ends balanced, with its space, at the
    /// loudness for streaming, and says what it did.
    #[test]
    fn a_song_is_mixed_in_one_click() {
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
        app.project.tracks[0].mixer.active = false;
        app.project.tracks[2].mixer.gain *= 4.0;
        app.project.tracks[3].mixer.gain *= 0.1;
        for t in &mut app.project.tracks {
            t.sends = [0.0; MAX_RETURNS];
        }
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() || app.mix.auto.is_some() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 300);
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let started = std::time::Instant::now();
        app.apply_all();
        wait(&mut app);
        println!("aplicar todo: {:.1} s", started.elapsed().as_secs_f64());
        let note = app.mix.note.clone().unwrap();
        println!("{note}");
        assert!(note.starts_with("Mezcla completa en "), "{note}");
        for part in ["volúmenes y panoramas", "volúmenes reajustados", "reverb y delay", "sonoridad con el limitador del master"] {
            assert!(note.contains(part), "{part}: {note}");
        }
        let report = app.mix.report.clone().unwrap();
        let (_, left) = balance_tracks(&report.tracks);
        assert!(left.is_empty(), "{left:?}");
        let lufs = report.mix.lufs.unwrap();
        println!("{lufs:.1} LUFS, {:.1} dBTP", report.mix.true_peak);
        assert!((lufs + 14.0).abs() <= 1.0 && report.mix.true_peak <= -1.0, "{lufs} LUFS, {} dBTP", report.mix.true_peak);
        let list = advise(&report, Target::Streaming, None);
        assert!(list.iter().any(|a| a.level == Level::Good && a.text.starts_with("Las pistas tienen su espacio")), "{list:?}");
        // Done: what it advises now is nothing it would change (it doesn't
        // ask again for what it just did, nor undo it).
        for a in list.iter().filter(|a| a.fix.is_some()) {
            println!("queda: {}", a.text);
        }
        assert_eq!(next_fix(&list, &[]), None, "{list:?}");
        // Measured again from nothing, it says the same.
        app.mix.keys = None;
        app.analyze_mix();
        wait(&mut app);
        let again = app.mix.report.clone().unwrap();
        assert!((again.mix.lufs.unwrap() - lufs).abs() < 0.1, "{:?} {lufs}", again.mix.lufs);
        assert_eq!(next_fix(&advise(&again, Target::Streaming, None), &[]), None);
    }

    /// The parts of a song and its range, through the limiter: kept or lost.
    #[test]
    fn a_limiter_that_squashes_the_song_is_told_apart() {
        let names: Vec<String> = ["Intro", "Subida", "Drop", "Break", "Drop"].iter().map(|s| s.to_string()).collect();
        let at = |sections: [f64; 5], range: f64| Trial { gain_db: 0.0, lufs: Some(-10.0), true_peak: -1.0, range: Some(range), sections: sections.iter().map(|&l| Some(l)).collect() };
        // The audit's song before its master, and as the old assistant left it.
        let before = at([-17.8, -10.3, -8.3, -10.5, -8.3], 9.7);
        let squashed_by_12 = at([-10.7, -10.6, -10.1, -9.3, -10.2], 1.5);
        assert_eq!(squashed(&before, &squashed_by_12, &names), Some(Held::Section { peak: "Drop".into(), other: "Intro".into() }));
        // Everything 4 dB up, the drop 1 dB less (its peaks limited): fine.
        let fine = at([-13.8, -6.8, -5.3, -6.8, -5.3], 8.5);
        assert_eq!(squashed(&before, &fine, &names), None);
        // The break catching the drop up: not, whatever else.
        let break_up = at([-13.8, -6.3, -5.3, -5.6, -5.3], 8.5);
        assert_eq!(squashed(&before, &break_up, &names), Some(Held::Section { peak: "Drop".into(), other: "Subida".into() }));
        // The build-up keeps its 1.5 LU under the drop it had.
        let close = at([-13.8, -5.9, -5.3, -7.5, -5.3], 8.5);
        assert!(squashed(&before, &close, &names).is_some());
        // Without parts, the range: a third of it may go.
        let (a, b) = (at([0.0; 5], 9.0), at([0.0; 5], 6.5));
        assert_eq!(squashed(&Trial { sections: Vec::new(), ..a.clone() }, &Trial { sections: Vec::new(), ..b }, &[]), None);
        assert_eq!(squashed(&Trial { sections: Vec::new(), ..a }, &Trial { sections: Vec::new(), ..at([0.0; 5], 5.0) }, &[]), Some(Held::Range));
    }

    /// A techno song (an intro of kick alone, a drop of kick and a loud
    /// synth, a break of the synth alone) taken to a club's -9 LUFS by the
    /// limiter: no more than 6 dB, the drop still over the break and the
    /// intro by about what it was, and it says why it stopped. The old way
    /// (12 dB) squashed it.
    #[test]
    fn the_limiter_keeps_the_song_s_shape() {
        let rate = RATE as usize;
        let part = 8 * rate;
        let (mut left, mut right) = (vec![0f32; 3 * part], vec![0f32; 3 * part]);
        for i in 0..3 * part {
            let t = i as f32 / RATE;
            let beat = (t * 2.0).fract() / 2.0;
            let kick = (std::f32::consts::TAU * 50.0 * t).sin() * (-beat * 18.0).exp() * 0.7;
            // A detuned saw chord, its level breathing a little.
            let saw = |hz: f32| 2.0 * (t * hz).fract() - 1.0;
            let synth = (saw(220.0) + saw(220.9) + saw(261.6) + saw(329.6)) * 0.1 * (1.0 + 0.2 * (t * 0.5).sin());
            let (k, s) = match i / part {
                0 => (kick, 0.0),
                1 => (kick, synth),
                _ => (0.0, synth * 0.7),
            };
            left[i] = k + s;
            right[i] = k + s * 0.95;
        }
        let sections = vec![("Intro".to_string(), 0, part), ("Drop".to_string(), part, 2 * part), ("Break".to_string(), 2 * part, 3 * part)];
        let fader = vec![1.0; 3 * part / FADER_BLOCK + 1];
        let limiter = LimiterParams { ceiling_db: -1.3, ..Default::default() };
        let spans: Vec<(usize, usize)> = sections.iter().map(|s| (s.1, s.2)).collect();
        let names: Vec<String> = sections.iter().map(|s| s.0.clone()).collect();
        let base = trial(&left, &right, &fader, None, &spans);
        let (chosen, held) = loudest_gain(&left, &right, &fader, limiter, &sections, -9.0, -1.0, &AtomicBool::new(false)).unwrap();
        let old = trial(&left, &right, &fader, Some(LimiterParams { gain_db: 12.0, ..limiter }), &spans);
        let lead = |t: &Trial| (t.sections[1].unwrap() - t.sections[0].unwrap(), t.sections[1].unwrap() - t.sections[2].unwrap());
        println!("antes: {:.1} LUFS, el drop {:.1} LU sobre la intro y {:.1} sobre el break", base.lufs.unwrap(), lead(&base).0, lead(&base).1);
        println!("ahora: {:+.1} dB → {:.1} LUFS, pico real {:.1} dBTP, {:.1} y {:.1} LU · {held:?}", chosen.gain_db, chosen.lufs.unwrap(), chosen.true_peak, lead(&chosen).0, lead(&chosen).1);
        println!("a +12: {:.1} LUFS, {:.1} y {:.1} LU", old.lufs.unwrap(), lead(&old).0, lead(&old).1);
        // Its peaks at most 6 dB into the limiter (what they had under the
        // ceiling comes free).
        assert!(chosen.gain_db - headroom(&left, &right, limiter.ceiling_db) <= MAX_LIMITER_GAIN + 1e-3);
        assert!(chosen.true_peak <= -1.0 + 0.05);
        assert!(chosen.lufs.unwrap() > base.lufs.unwrap() + 2.0, "louder");
        assert_eq!(squashed(&base, &chosen, &names), None);
        assert!(lead(&chosen).0 > 0.5 * lead(&base).0 && lead(&chosen).1 > 0.5 * lead(&base).1);
        assert!(held.is_some() && chosen.lufs.unwrap() < -9.5, "short of -9, and said why");
        assert!(squashed(&base, &old, &names).is_some(), "12 dB squash it");
    }

    /// A track only its fader moved isn't rendered again: its measures move
    /// by as many dB, as a render of it would say.
    #[test]
    fn a_track_only_louder_is_measured_without_rendering_it() {
        let mut app = PulseApp::headless(crate::project::Project::demo());
        for t in 0..app.project.tracks.len() {
            if let Some(clip) = app.project.tracks[t].clips[0].clone() {
                app.project.tracks[t].arrangement = vec![engine::model::ArrangementClip { start: 0.0, length: 8.0, clip }];
            }
        }
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            while app.mix.job.is_some() || app.mix.report.is_none() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        app.analyze_mix();
        wait(&mut app);
        let first = app.mix.report.clone().unwrap();
        let t = first.tracks[0].index;
        louder(&mut app.project.tracks[t], -4.5);
        let started = std::time::Instant::now();
        app.analyze(false);
        wait(&mut app);
        let quick = started.elapsed().as_secs_f64();
        let kept = app.mix.report.clone().unwrap();
        app.mix.keys = None;
        app.analyze_mix();
        wait(&mut app);
        let rendered = app.mix.report.clone().unwrap();
        let (a, b) = (&kept.tracks[0], &rendered.tracks[0]);
        println!("sin renderizar ({quick:.2} s): {:.2} LUFS, pico {:.2}; renderizada: {:.2} LUFS, pico {:.2}", a.lufs.unwrap(), a.peak, b.lufs.unwrap(), b.peak);
        assert!((a.lufs.unwrap() - b.lufs.unwrap()).abs() < 0.05 && (a.peak - b.peak).abs() < 0.05);
        assert!((a.lufs.unwrap() - (first.tracks[0].lufs.unwrap() - 4.5)).abs() < 0.05);
        assert!(a.bands.iter().zip(&b.bands).all(|(x, y)| (x - y).abs() < 0.1));
        assert_eq!(a.role, b.role);
    }

    /// The louder one comes down to the softer one; without both, nothing.

    /// The louder one comes down to the softer one; without both, nothing.
    #[test]
    fn before_and_after_meet_at_the_softer_loudness() {
        assert_eq!(matched(Some(-20.0), Some(-14.0)), (0.0, -6.0));
        assert_eq!(matched(Some(-10.0), Some(-14.0)), (-4.0, 0.0));
        assert_eq!(matched(None, Some(-14.0)), (0.0, 0.0));
    }

    /// For real: a dry, quiet voice and bass mixed with «Aplicar todo». What
    /// the engine plays for «Antes» is the mix as it was (no returns), and
    /// both sound as loud; changing the engine over to it with only what
    /// differs sounds as it would from the start. The project is left alone,
    /// and an edit ends the comparison.
    #[test]
    fn before_and_after_are_heard_as_loud() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        use engine::{Command, OfflineRenderer};
        let rate = 48_000.0f32;
        let tone = |hz: f32| -> Vec<f32> { (0..(8.0 * rate) as usize).map(|i| i as f32 / rate).map(|t| if t % 2.0 < 0.5 { 0.1 * (std::f32::consts::TAU * hz * t).sin() } else { 0.0 }).collect() };
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let clip = |name: &str, audio: Vec<f32>| {
            let path = dir.join(format!("antes-{name}-{}.wav", std::process::id()));
            crate::export::write(&path, &audio, &audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
            engine::model::ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: name.into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) }
        };
        let mut project = Project::empty();
        project.tempo = 120.0;
        project.returns.clear();
        let mut voice = Track::new_audio("Voz", [0; 3]);
        voice.arrangement.push(clip("voz", tone(440.0)));
        let mut bass = Track::new_audio("Bajo", [0; 3]);
        bass.arrangement.push(clip("bajo", tone(55.0)));
        project.tracks = vec![voice, bass];
        let mut app = PulseApp::headless(project);
        let ctx = egui::Context::default();
        let wait = |app: &mut PulseApp| {
            let started = std::time::Instant::now();
            while app.mix.job.is_some() || app.mix.report.is_none() || app.mix.auto.is_some() {
                app.follow_mix(&ctx);
                std::thread::sleep(std::time::Duration::from_millis(20));
                assert!(started.elapsed().as_secs() < 120);
            }
        };
        assert!(app.heard(true).is_none(), "nothing to compare yet");
        app.analyze_mix();
        wait(&mut app);
        let original = app.project.clone();
        app.apply_all();
        wait(&mut app);
        assert!(!app.project.returns.is_empty());
        let (before, after) = (app.heard(true).unwrap(), app.heard(false).unwrap());
        assert!(before.returns.is_empty() && before.tracks == original.tracks);
        // Loudness of a project as rendered.
        let lufs = |app: &mut PulseApp, p: &Project| {
            let plan = export::plan(p, &mut app.samples, &export::Settings::default(), std::path::Path::new("ab.wav"));
            let mix = export::render_each(plan, RATE, &AtomicU32::new(0), &AtomicBool::new(false), |_, l, r| Ok((l, r))).unwrap();
            loudness::integrated(&mix[0].0, &mix[0].1, RATE).unwrap()
        };
        let now = app.project.clone();
        let (raw_before, raw_after) = (lufs(&mut app, &original), lufs(&mut app, &now));
        let (heard_before, heard_after) = (lufs(&mut app, &before), lufs(&mut app, &after));
        println!("sin igualar {raw_before:.1} / {raw_after:.1}; igualadas {heard_before:.1} / {heard_after:.1}");
        assert!(raw_after - raw_before > 3.0);
        assert!((heard_before - heard_after).abs() < 1.0, "{heard_before} / {heard_after}");
        // The engine moved over from «Después» to «Antes» sounds as «Antes» from the start.
        let play = |app: &mut PulseApp, start: &Project, then: Option<&Project>| {
            let mut renderer = OfflineRenderer::new(RATE);
            let none = std::collections::HashSet::new();
            for c in crate::sync::project_commands(start, &mut app.samples, RATE, &none) {
                renderer.send(c);
            }
            if let Some(then) = then {
                for c in crate::sync::changed_commands(start, then, &mut app.samples, RATE, &none) {
                    renderer.send(c);
                }
            }
            for c in [Command::SetOutputLimit(false), Command::BackToArrangement, Command::SetPosition(0.0), Command::Play] {
                renderer.send(c);
            }
            let (mut l, mut r) = (Vec::new(), Vec::new());
            renderer.render(2 * RATE as usize, &mut l, &mut r);
            l
        };
        let moved = play(&mut app, &after, Some(&before));
        let fresh = play(&mut app, &before, None);
        let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
        let diff: f32 = moved.iter().zip(&fresh).map(|(a, b)| (a - b) * (a - b)).sum();
        assert!(energy(&fresh) > 1.0 && diff < energy(&fresh) * 1e-4, "{} vs {}", diff, energy(&fresh));
        // Listening touches nothing in the project; an edit ends it.
        let kept = app.project.clone();
        app.listen(true);
        assert_eq!(app.mix.listening.as_ref().map(|l| l.before), Some(true));
        app.listen(false);
        assert_eq!(app.project, kept);
        app.project.tracks[0].mixer.gain *= 0.5;
        app.follow_mix(&ctx);
        assert!(app.mix.listening.is_none());
    }

    /// Phrases found where the voice sounds, each brought to as far over the
    /// rest as most are; the a cappella one is not measured.
    #[test]
    fn each_phrase_is_brought_to_the_others() {
        // 1 s phrases with 0.5 s gaps: -20, -26, -20, -14 dB over a base at -30;
        // then one more over silence.
        let mut voice = Vec::new();
        for level in [-20.0, -26.0, -20.0, -14.0, -20.0] {
            voice.extend(std::iter::repeat(level).take(20));
            voice.extend(std::iter::repeat(-90.0).take(10));
        }
        let mut base = vec![-30.0f32; 120];
        base.extend(std::iter::repeat(-90.0).take(30));
        let found = phrases(&voice, &base);
        assert_eq!(found.len(), 4, "{found:?}");
        let overs: Vec<f32> = found.iter().map(|p| p.over.round()).collect();
        assert_eq!(overs, [10.0, 4.0, 10.0, 16.0]);
        assert!((found[1].start - 1.65).abs() < 1e-9 && (found[1].end - 2.5).abs() < 1e-9, "{:?}", found[1]);
        assert_eq!(ride(&found), [0.0, 6.0, 0.0, -6.0]);
        // Changes in the gaps, each phrase held at its own.
        let points = ride_points(&found, &ride(&found), 0.5);
        assert_eq!(points.len(), 8);
        assert!(points[2].0 > found[0].end && points[2].0 < found[1].start);
        assert!((points[2].1 - 0.5 * 10f32.powf(0.3)).abs() < 1e-6);
        // Beats: 1 s is two at 120, one with the tempo automated to 60.
        let mut project = crate::project::Project::empty();
        assert!((beat_after(&project, 4.0, 1.0) - 6.0).abs() < 1e-9);
        let mut lane = engine::model::AutomationLane::new(Param::Tempo);
        lane.points.push(engine::model::AutomationPoint { beat: 0.0, value: 60.0, curve: 0.0 });
        project.master.automation.push(lane);
        assert!((beat_after(&project, 4.0, 1.0) - 5.0).abs() < 1e-3);
    }

    /// For real: a voice singing phrases loud and soft over a steady pad.
    /// Ridden, every phrase sits within a couple of dB of the rest, and its
    /// fader moved afterwards takes the ride with it.
    #[test]
    fn a_voice_is_ridden_phrase_by_phrase() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let levels_of = [0.3f32, 0.15, 0.3, 0.55, 0.16, 0.3, 0.3, 0.17];
        let voice: Vec<f32> = (0..(16.0 * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                let (phrase, within) = ((t / 2.0) as usize, t % 2.0);
                if within < 1.4 {
                    let x = std::f32::consts::TAU * 330.0 * t;
                    levels_of[phrase.min(7)] * (x.sin() + 0.4 * (2.0 * x).sin()) / 1.4
                } else {
                    0.0
                }
            })
            .collect();
        let pad: Vec<f32> = (0..(16.0 * rate) as usize).map(|i| i as f32 / rate).map(|t| 0.05 * ((std::f32::consts::TAU * 220.0 * t).sin() + (std::f32::consts::TAU * 277.2 * t).sin())).collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let clip = |name: &str, audio: &[f32]| {
            let path = dir.join(format!("nivelar-{name}-{}.wav", std::process::id()));
            crate::export::write(&path, audio, audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
            engine::model::ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: name.into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 32.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) }
        };
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut v = Track::new_audio("Voz", [0; 3]);
        v.arrangement.push(clip("voz", &voice));
        let mut p = Track::new_audio("Pad", [0; 3]);
        p.arrangement.push(clip("pad", &pad));
        project.tracks = vec![v, p];
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
        let spread = |report: &Report| {
            let found = phrases(&report.tracks[0].levels, &base_levels(&report.tracks, 0));
            let (lo, hi) = found.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.over), b.max(p.over)));
            (found.len(), hi - lo)
        };
        app.analyze_mix();
        wait(&mut app);
        let (count, before) = spread(app.mix.report.as_ref().unwrap());
        assert_eq!(count, 8);
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Ride { .. }))).expect("a ride");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let (count, after) = spread(app.mix.report.as_ref().unwrap());
        println!("de frase a frase: {before:.1} → {after:.1} dB");
        assert_eq!(count, 8);
        assert!(before > 10.0 && after < 2.0, "{before} → {after}");
        assert!(!advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None).iter().any(|a| matches!(a.fix, Some(Fix::Ride { .. }))));
        // The fader 6 dB up: the ride comes with it.
        let lane = |app: &PulseApp| app.project.tracks[0].automation.iter().find(|l| l.param == Param::Volume).unwrap().points.iter().map(|p| p.value).collect::<Vec<f32>>();
        let was = lane(&app);
        app.apply_fix(&Fix::TrackGain { track: 0, name: "Voz".into(), db: 6.0 });
        let now = lane(&app);
        assert!(was.iter().zip(&now).all(|(a, b)| (b / a - 10f32.powf(0.3)).abs() < 1e-3 || *b == FADER_TOP));
    }

    /// For real: a voice whose «s» are as loud as its vowels. The assistant
    /// finds them, where they are, and its De-esser brings them down about
    /// what it said, the vowels as they were.
    #[test]
    fn a_voices_esses_come_down() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let n = (8.0 * rate) as usize;
        // White noise, its top over 6.5 kHz: the «s».
        let mut seed = 1u32;
        let noise: Vec<f64> = (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f64 / (1u32 << 24) as f64 * 2.0 - 1.0
            })
            .collect();
        let hiss = filtered(&filtered(&noise, rate, 6500.0, 0.7, 0), rate, 6500.0, 0.7, 0);
        let rms = |x: &[f64]| (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
        let hiss_rms = rms(&hiss);
        // Each half second: a vowel (a 200 Hz voice) for 0.35 s, an «s» for 0.15 s, as loud.
        let voice: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                if t % 0.5 < 0.35 {
                    let x = std::f32::consts::TAU * 200.0 * t;
                    0.25 * (x.sin() + 0.5 * (2.0 * x).sin() + 0.3 * (3.0 * x).sin())
                } else {
                    (hiss[i] / hiss_rms * 0.2) as f32
                }
            })
            .collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("eses-{}.wav", std::process::id()));
        crate::export::write(&path, &voice, &voice, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut v = Track::new_audio("Voz", [0; 3]);
        v.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: "Voz".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) });
        project.tracks = vec![v];
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
        let before = app.mix.report.clone().unwrap().tracks[0].esses.expect("esses found");
        println!("antes: {before:?}");
        assert!(before.over_vowels > -3.0, "{before:?}");
        assert!(before.freq >= 6000.0, "{before:?}");
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::DeEss { .. }))).expect("a de-esser");
        println!("{fix:?}");
        let vowels = |app: &PulseApp| {
            let t = &app.mix.report.as_ref().unwrap().tracks[0];
            t.levels.iter().enumerate().filter(|(k, _)| { let s = *k as f64 * HOP % 0.5; (0.05..0.1).contains(&s) }).map(|(_, l)| *l).sum::<f32>()
        };
        let vowels_before = vowels(&app);
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let after = app.mix.report.clone().unwrap().tracks[0].esses.expect("esses still there");
        println!("después: {after:?}");
        let down = before.over_vowels - after.over_vowels;
        assert!((4.0..=9.0).contains(&down), "{down} dB");
        let count = app.mix.report.as_ref().unwrap().tracks[0].levels.iter().enumerate().filter(|(k, _)| { let s = *k as f64 * HOP % 0.5; (0.05..0.1).contains(&s) }).count() as f32;
        assert!(((vowels(&app) - vowels_before) / count).abs() < 0.3, "the vowels as they were");
        assert!(advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None).iter().any(|a| a.level == Level::Good && a.text == "«Voz» ya tiene De-esser"));
    }

    /// The phrases a long enough silence follows get a throw (the last one
    /// too); a breath between two doesn't. The send opens on the last word
    /// and comes back to where it was.
    #[test]
    fn throws_go_where_a_silence_follows() {
        // Frames of 50 ms: sings 0–1 s, 1.2–2.2 s, 3–4 s, 6–7 s.
        let mut levels = vec![-90.0f32; 160];
        for (a, b) in [(0, 16), (24, 40), (60, 76), (120, 136)] {
            levels[a..b].fill(-20.0);
        }
        let ends = throws(&levels, 0.375);
        let found = phrases(&levels, &vec![0.0; levels.len()]);
        assert_eq!(found.len(), 4);
        // A breath (0.55 s) gets none; 1.15 s and more, one.
        assert_eq!(ends, [found[1].end, found[2].end, found[3].end], "{found:?}");
        // An echo of 1.2 s: the 1.15 s gap is too short for it.
        assert_eq!(throws(&levels, 1.2), [found[2].end, found[3].end]);
        let points = throw_points(&[4.0], 0.1);
        let expect = [(3.6, 0.1), (3.65, THROW), (4.05, THROW), (4.15, 0.1)];
        assert_eq!(points.len(), 4);
        for ((at, v), (eat, ev)) in points.iter().zip(expect) {
            assert!((at - eat).abs() < 1e-9 && (v - ev).abs() < 1e-6, "{points:?}");
        }
    }

    /// For real: a voice singing phrases with silences after them, sending
    /// nothing to the delay. Applied, its echo fills the silences, and the
    /// send is automated on its last words only.
    #[test]
    fn the_last_words_echo_into_the_silence() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let voice: Vec<f32> = (0..(16.0 * rate) as usize)
            .map(|i| {
                let t = i as f32 / rate;
                if t % 2.0 < 1.2 && t < 14.0 {
                    let x = std::f32::consts::TAU * 330.0 * t;
                    0.3 * (x.sin() + 0.4 * (2.0 * x).sin()) / 1.4
                } else {
                    0.0
                }
            })
            .collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("ecos-{}.wav", std::process::id()));
        crate::export::write(&path, &voice, &voice, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut v = Track::new_audio("Voz", [0; 3]);
        v.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: "Voz".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 32.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) });
        project.tracks = vec![v];
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
        // The mix's level (dB) in the silences: 0.3 to 0.7 s after each phrase.
        let gaps = |app: &mut PulseApp| {
            let plan = export::plan(&app.project, &mut app.samples, &export::Settings::default(), std::path::Path::new("ecos.wav"));
            let mut mix = export::render_each(plan, RATE, &AtomicU32::new(0), &AtomicBool::new(false), |_, left, _| Ok(left)).unwrap();
            let left = mix.remove(0);
            let tail: Vec<f32> = left.iter().enumerate().filter(|(i, _)| { let t = *i as f32 / RATE; t < 14.0 && (1.5..1.9).contains(&(t % 2.0)) }).map(|(_, &x)| x).collect();
            10.0 * ((tail.iter().map(|x| x * x).sum::<f32>() / tail.len() as f32).max(1e-12)).log10()
        };
        let before = gaps(&mut app);
        app.analyze_mix();
        wait(&mut app);
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Throws { .. }))).expect("throws");
        let Fix::Throws { ends, delay, .. } = &fix else { unreachable!() };
        assert_eq!(*delay, Some(1));
        assert_eq!(ends.len(), 7, "{ends:?}");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let lane = app.project.tracks[0].automation.iter().find(|l| l.param == Param::Send(1)).expect("the send automated");
        assert_eq!(lane.points.len(), 28);
        // Open at the end of the first phrase (beat 2.4 at 120), shut in the middle of the next.
        assert_eq!(lane.value_at(2.3), Some(THROW));
        assert_eq!(lane.value_at(5.0), Some(0.0));
        let after = gaps(&mut app);
        println!("silencios: {before:.1} → {after:.1} dB");
        assert!(before < -100.0 && after > -40.0, "{before} → {after}");
        let list = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        assert!(list.iter().any(|a| a.level == Level::Good && a.text == "«Voz» ya tiene sus envíos automatizados"), "{list:?}");
        // Measured dry: its echoes are not its soft parts, so no compressor turns up.
        assert!(!list.iter().any(|a| matches!(a.fix, Some(Fix::Compress { .. }))), "{list:?}");
    }

    /// The parts by their names, and how much what leads to the peak comes
    /// down: only what is too close to it.
    #[test]
    fn what_leads_to_the_chorus_comes_down() {
        assert_eq!(section_kind("Estribillo 2"), Some(true));
        assert_eq!(section_kind("DROP"), Some(true));
        assert_eq!(section_kind("Coro final"), Some(true));
        assert_eq!(section_kind("Pre-estribillo"), Some(false));
        assert_eq!(section_kind("Subida al drop"), Some(false));
        assert_eq!(section_kind("Estrofa 1"), Some(false));
        assert_eq!(section_kind("Intro"), None);
        assert_eq!(section_kind("Break"), None);
        let part = |name: &str, lufs: f64| SectionLevel { name: name.into(), start: 0.0, end: 1.0, lufs: Some(lufs) };
        let song = [part("Intro", -20.0), part("Estrofa", -13.8), part("Estribillo", -13.0), part("Estrofa 2", -16.0), part("Drop", -12.0)];
        // The quietest peak, -13: what leads goes to -14.5 at most.
        assert_eq!(growth(&song), [(1, 0.5)]);
        // A LU under it grows enough.
        assert_eq!(growth(&[part("Estrofa", -14.0), part("Estribillo", -13.0)]), []);
        let flat = [part("Estrofa", -12.0), part("Estribillo", -12.0)];
        assert_eq!(growth(&flat), [(0, 1.5)]);
        assert_eq!(growth(&[part("Estrofa", -12.0), part("Puente", -10.0)]), []);
        let way = [part("Subida", -2.0), part("Drop", -12.0)];
        assert_eq!(growth(&way), [(0, 6.0)], "6 dB at most");
    }

    /// For real: a song whose chorus is as loud as its verse. Applied, the
    /// master comes down in the verse and the chorus is 1.5 LU over it.
    #[test]
    fn a_chorus_grows_over_its_verse() {
        use crate::project::{Project, SongMarker, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let pad: Vec<f32> = (0..(16.0 * rate) as usize).map(|i| i as f32 / rate).map(|t| 0.1 * ((std::f32::consts::TAU * 220.0 * t).sin() + (std::f32::consts::TAU * 330.0 * t).sin())).collect();
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("secciones-{}.wav", std::process::id()));
        crate::export::write(&path, &pad, &pad, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut t = Track::new_audio("Pad", [0; 3]);
        t.arrangement.push(engine::model::ArrangementClip { start: 0.0, length: 32.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: "Pad".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 32.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) });
        project.tracks = vec![t];
        project.markers = vec![SongMarker { beat: 0.0, name: "Estrofa".into() }, SongMarker { beat: 16.0, name: "Estribillo".into() }];
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
        let parts = app.mix.report.clone().unwrap().sections;
        println!("{parts:?}");
        assert_eq!(parts.len(), 2);
        assert!((parts[0].lufs.unwrap() - parts[1].lufs.unwrap()).abs() < 0.3);
        let advice = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        let fix = advice.iter().find_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Grow { .. }))).expect("growth");
        app.mix.report = None;
        app.apply_fix(&fix);
        wait(&mut app);
        let parts = app.mix.report.clone().unwrap().sections;
        let grown = parts[1].lufs.unwrap() - parts[0].lufs.unwrap();
        println!("el estribillo sobre la estrofa: {grown:.2} LU");
        assert!((grown - 1.5).abs() < 0.3, "{grown}");
        let list = advise(app.mix.report.as_ref().unwrap(), Target::Streaming, None);
        assert!(!list.iter().any(|a| matches!(a.fix, Some(Fix::Grow { .. }))), "{list:?}");
        assert!(list.iter().any(|a| a.level == Level::Good && a.text == "«Estribillo» suena por encima de lo que lleva a él: la canción crece"), "{list:?}");
        // «Antes / Después» levels the master's automation too.
        let lane = |p: &crate::project::Project| p.master.automation.iter().find(|l| l.param == Param::Volume).unwrap().points[0].value;
        let quieter = levelled(&app.project, -6.0);
        assert!((lane(&quieter) / lane(&app.project) - 0.5012).abs() < 1e-3);
    }

    /// A channel the wrong way is flipped; too wide, narrowed to where its
    /// correlation is 0.2; its lows in stereo, in mono. A Utility it has is
    /// built on.
    #[test]
    fn a_track_that_vanishes_in_mono_is_put_right() {
        let t = |correlation: f64, stereo: Stereo| TrackMeasures { correlation, stereo, ..track(0, "Pad", false, -6.0, Vec::new()) };
        let none = Stereo { mid: 1.0, side: 0.1, low_mid: 0.01, low_side: 0.0 };
        assert_eq!(stereo_fix(&t(0.8, none)), None);
        let flipped = stereo_fix(&t(-0.9, Stereo { mid: 0.05, side: 1.0, low_mid: 0.0, low_side: 0.0 })).unwrap();
        assert!(flipped.flip_right && flipped.width == 1.0 && flipped.bass_mono_hz == 0.0);
        // M = 1, S = 2: correlation -1/3. Narrowed: w² = 0.8 / (2 · 1.2), w = 0.577 → 0.6.
        let wide = stereo_fix(&t(-0.33, Stereo { mid: 1.0, side: 2.0, low_mid: 0.0, low_side: 0.0 })).unwrap();
        assert_eq!(wide.width, 0.6);
        assert!(!wide.flip_right);
        let lows = stereo_fix(&t(0.5, Stereo { mid: 1.0, side: 0.3, low_mid: 0.5, low_side: 0.2 })).unwrap();
        assert_eq!(lows.bass_mono_hz, 120.0);
        // Already at 50 % wide: narrowed from there.
        let mut half = t(-0.33, Stereo { mid: 1.0, side: 2.0, low_mid: 0.0, low_side: 0.0 });
        half.utility = Some(UtilityParams { width: 0.5, gain_db: -3.0, ..Default::default() });
        let narrower = stereo_fix(&half).unwrap();
        assert_eq!((narrower.width, narrower.gain_db), (0.3, -3.0));
    }

    /// For real: a pad with its right channel upside down, and a bass whose
    /// lows are only on the left. Applied, the pad is whole in mono and the
    /// bass's lows are in both channels.
    #[test]
    fn a_pad_the_wrong_way_and_a_bass_in_stereo_are_put_right() {
        use crate::project::{Project, Track};
        use engine::model::{AudioClip, Clip, WarpMode};
        let rate = 48_000.0f32;
        let n = (8.0 * rate) as usize;
        let wave = |hz: f32, a: f32| -> Vec<f32> { (0..n).map(|i| a * (std::f32::consts::TAU * hz * i as f32 / rate).sin()).collect() };
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let clip = |name: &str, left: &[f32], right: &[f32]| {
            let path = dir.join(format!("fase-{name}-{}.wav", std::process::id()));
            crate::export::write(&path, left, right, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
            engine::model::ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: name.into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 16.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }) }
        };
        let pad = wave(440.0, 0.2);
        let upside: Vec<f32> = pad.iter().map(|x| -x).collect();
        let low = wave(55.0, 0.3);
        let top = wave(880.0, 0.05);
        let bass_left: Vec<f32> = low.iter().zip(&top).map(|(a, b)| a + b).collect();
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut p = Track::new_audio("Pad", [0; 3]);
        p.arrangement.push(clip("pad", &pad, &upside));
        let mut b = Track::new_audio("Bajo", [0; 3]);
        b.arrangement.push(clip("bajo", &bass_left, &top));
        project.tracks = vec![p, b];
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
        let before = app.mix.report.clone().unwrap();
        println!("antes: pad {:.2}, mezcla {:.2}", before.tracks[0].correlation, before.mix.correlation);
        let advice = advise(&before, Target::Streaming, None);
        let fixes: Vec<Fix> = advice.iter().filter_map(|a| a.fix.clone().filter(|f| matches!(f, Fix::Stereo { .. }))).collect();
        assert_eq!(fixes.len(), 2, "{advice:?}");
        for fix in &fixes {
            app.mix.report = None;
            app.apply_fix(fix);
            wait(&mut app);
        }
        let after = app.mix.report.clone().unwrap();
        let (pad, bass) = (&after.tracks[0], &after.tracks[1]);
        println!("después: pad {:.2}, mezcla {:.2}, graves del bajo: lado/centro {:.3}", pad.correlation, after.mix.correlation, bass.stereo.low_side / bass.stereo.low_mid);
        assert!(before.tracks[0].correlation < -0.9 && pad.correlation > 0.9);
        assert!(bass.stereo.low_side < 0.01 * bass.stereo.low_mid, "the lows in mono");
        assert!(after.mix.correlation > 0.5);
        assert!(advise(&after, Target::Streaming, None).iter().all(|a| !matches!(a.fix, Some(Fix::Stereo { .. }))));
    }
}
