//! «Sonidos de Pulse»: the sample library that comes with Pulse, made by Pulse.
//!
//! Every sound is synthesized by Pulse's own instruments and effects (see
//! `factory_recipes`) the first time it is needed, and written as a 24-bit WAV
//! (mono when both sides are the same) into the data folder, in a folder per
//! version of the library (`Sonidos de Pulse/v1`): a later version with other
//! sounds is made anew beside it, and files already made are never made again.
//!
//! - One-shots in folders (Bombos, Cajas y palmas, Charles y platos…), each
//!   cleaned the same way: no DC, its silence trimmed from both ends, short
//!   fades, a loudness common to its kind (kicks with kicks, hats with hats),
//!   peaks under -0.3 dBFS.
//! - Kits for the Drum Pads, one a genre, on the General MIDI layout, made of
//!   those one-shots: as presets in `Kits/` (double click in the browser) and
//!   in the Drum Pads' «Kits» menu.
//! - A loop a kit (`Bucles/House 124`), its pattern played by its kit, named
//!   with its tempo so the browser plays it in time.
//!
//! Making all of it takes a few seconds of every core, so it is done in the
//! background, the first time the library is opened in the browser; a kit
//! asked for before that makes just its sounds, also in the background.

use crate::factory_recipes::{recipes, render_project, Recipe, Role, Stereo, RATE};
use crate::project::{Project, Track};
use engine::model::{ArrangementClip, Clip, Effect, InstrumentKind, LimiterParams, MidiClip, Note, RackParams, RACK_FIRST_NOTE, RACK_PADS};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// The name the library goes by (its folder and its place in the browser).
pub const NAME: &str = "Sonidos de Pulse";
/// Bumped when sounds change: the new version is made in a folder of its own.
pub const VERSION: &str = "v1";
pub const KITS: &str = "Kits";
pub const LOOPS: &str = "Bucles";
/// Written once everything is made.
const DONE: &str = ".completo";

/// Where the library is made (tests use their own folder).
pub fn root() -> PathBuf {
    if cfg!(test) {
        return std::env::temp_dir().join(format!("pulse-test-sonidos-{}", std::process::id())).join(VERSION);
    }
    paths::data_dir().join(NAME).join(VERSION)
}

pub fn file(root: &Path, recipe: &Recipe) -> PathBuf {
    root.join(recipe.folder).join(format!("{}.wav", recipe.name))
}

// ---------------------------------------------------------------- finishing

/// Loudness each kind of sound is brought to: its loudest 50 ms, dBFS RMS.
pub fn target(role: Role) -> f32 {
    match role {
        Role::Kick => -8.0,
        Role::Snare => -12.0,
        Role::Rim => -16.0,
        Role::Hat => -17.0,
        Role::Cymbal => -16.0,
        Role::Perc => -14.0,
        Role::Sub => -7.0,
        Role::Bass => -10.0,
        Role::Stab => -12.0,
        Role::Fx => -12.0,
    }
}

/// The longest each kind of sound may last, in seconds.
fn longest(role: Role) -> f32 {
    match role {
        Role::Kick | Role::Perc => 2.0,
        Role::Snare | Role::Rim | Role::Hat => 2.5,
        Role::Cymbal | Role::Sub | Role::Bass => 4.0,
        Role::Stab => 5.0,
        Role::Fx => 20.0,
    }
}

/// The highest peak a file reaches, dBFS.
pub const CEILING_DB: f32 = -0.3;
/// Trimmed where it falls this far under its peak.
pub const TAIL_DB: f32 = -60.0;
const WINDOW_SECONDS: f32 = 0.05;

pub fn db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

pub fn peak(l: &[f32], r: &[f32]) -> f32 {
    l.iter().chain(r).fold(0f32, |m, s| m.max(s.abs()))
}

/// Its loudest 50 ms, dBFS RMS (both channels).
pub fn loudness(l: &[f32], r: &[f32]) -> f32 {
    let window = ((WINDOW_SECONDS * RATE) as usize).min(l.len()).max(1);
    let mut sum = 0f64;
    let mut most = 0f64;
    for i in 0..l.len() {
        sum += (l[i] as f64).powi(2) + (r[i] as f64).powi(2);
        if i >= window {
            sum -= (l[i - window] as f64).powi(2) + (r[i - window] as f64).powi(2);
        }
        most = most.max(sum);
    }
    10.0 * (most.max(1e-18) / (2 * window) as f64).log10() as f32
}

/// How a sound came out of `finish`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    /// What the limiter took off its loudest peak, dB.
    pub limited_db: f32,
}

/// A raw render made into a library file: finite, without DC, trimmed, at
/// its kind's loudness under the ceiling, with fades at both ends.
pub fn finish((mut l, mut r): Stereo, role: Role, length: Option<f32>) -> (Stereo, Report) {
    for s in l.iter_mut().chain(r.iter_mut()) {
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    // DC and the inaudible lowest end: a gentle high-pass at 10 Hz.
    let a = (-std::f32::consts::TAU * 10.0 / RATE).exp();
    for channel in [&mut l, &mut r] {
        let (mut x1, mut y1) = (0f32, 0f32);
        for s in channel.iter_mut() {
            let y = a * (y1 + *s - x1);
            x1 = *s;
            y1 = y;
            *s = y;
        }
    }
    let top = peak(&l, &r);
    if top <= 0.0 {
        return ((l, r), Report::default());
    }
    // From where it starts (an attack the renderer delayed would be a gap).
    let floor = top * 10f32.powf(TAIL_DB / 20.0);
    let first = l.iter().zip(&r).position(|(a, b)| a.abs().max(b.abs()) > floor).unwrap_or(0);
    // A riser keeps its quiet start: it is as long as its bars.
    if length.is_none() {
        l.drain(..first);
        r.drain(..first);
    }
    // A riser as long as its bars; anything else no longer than its kind lasts.
    match length {
        Some(seconds) => {
            l.resize((seconds * RATE) as usize, 0.0);
            r.resize((seconds * RATE) as usize, 0.0);
        }
        None => {
            l.truncate((longest(role) * RATE) as usize);
            r.truncate((longest(role) * RATE) as usize);
        }
    }

    // To its loudness; what then goes over the ceiling, limited.
    let gain = 10f32.powf((target(role) - loudness(&l, &r)) / 20.0);
    for s in l.iter_mut().chain(r.iter_mut()) {
        *s *= gain;
    }
    let mut report = Report::default();
    let over = peak(&l, &r);
    let ceiling = 10f32.powf((CEILING_DB - 0.3) / 20.0);
    if over > ceiling {
        let limiter = Effect::Limiter(LimiterParams { gain_db: 0.0, ceiling_db: CEILING_DB - 0.3, release_ms: 60.0 });
        let (a, b) = engine::render_effects(&[limiter], &l, &r, RATE, 120.0, 0.0);
        report.limited_db = db(over) - db(peak(&a, &b));
        l = a;
        r = b;
    }
    // Its tail, to where it falls under -60 dB of its (final) peak, and a
    // fade after that (over what is already under it).
    let fade_out = ((if length.is_some() { 0.01 } else { 0.02 }) * RATE) as usize;
    if length.is_none() {
        let floor = peak(&l, &r) * 10f32.powf(TAIL_DB / 20.0);
        let last = l.iter().zip(&r).rposition(|(a, b)| a.abs().max(b.abs()) > floor).map_or(0, |i| i + 1);
        let end = (last + fade_out).min(l.len());
        l.truncate(end);
        r.truncate(end);
    }
    // Fades: half a millisecond in, and out over the last of it.
    let fade_in = ((0.0005 * RATE) as usize).min(l.len());
    for i in 0..fade_in {
        let g = 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / fade_in as f32).cos();
        l[i] *= g;
        r[i] *= g;
    }
    let fade_out = fade_out.min(l.len() / 4).max(1);
    let n = l.len();
    for i in 0..fade_out {
        let g = 0.5 - 0.5 * (std::f32::consts::PI * i as f32 / fade_out as f32).cos();
        l[n - 1 - i] *= g;
        r[n - 1 - i] *= g;
    }
    // Whatever the limiter let by.
    let safe = 10f32.powf(CEILING_DB / 20.0) * 0.999;
    let top = peak(&l, &r);
    if top > safe {
        for s in l.iter_mut().chain(r.iter_mut()) {
            *s *= safe / top;
        }
    }
    ((l, r), report)
}

/// Writes a 24-bit WAV, mono when both sides are the same, through a hidden
/// file renamed into place (two makers of it never leave half a file).
pub fn write_wav(path: &Path, l: &[f32], r: &[f32]) -> Result<(), String> {
    let dir = path.parent().ok_or("Sin carpeta")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
    let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    // Its own name for each writer, in this process and any other.
    static WRITERS: AtomicUsize = AtomicUsize::new(0);
    let part = dir.join(format!(".{name}.{}-{}.part", std::process::id(), WRITERS.fetch_add(1, Ordering::Relaxed)));
    let mono = l.iter().zip(r).all(|(a, b)| a == b);
    let spec = hound::WavSpec { channels: if mono { 1 } else { 2 }, sample_rate: RATE as u32, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let write = || -> Result<(), String> {
        let mut writer = hound::WavWriter::create(&part, spec).map_err(|e| format!("No se pudo crear {}: {e}", part.display()))?;
        let value = |s: f32| (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
        for (&a, &b) in l.iter().zip(r) {
            writer.write_sample(value(a)).map_err(|e| e.to_string())?;
            if !mono {
                writer.write_sample(value(b)).map_err(|e| e.to_string())?;
            }
        }
        writer.finalize().map_err(|e| e.to_string())
    };
    let result = write().and_then(|_| std::fs::rename(&part, path).map_err(|e| format!("No se pudo guardar {}: {e}", path.display())));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Makes a sound's file unless it is there already.
pub fn make(root: &Path, recipe: &Recipe) -> Result<Option<Report>, String> {
    let path = file(root, recipe);
    if path.exists() {
        return Ok(None);
    }
    let ((l, r), report) = finish((recipe.make)(), recipe.role, recipe.length);
    write_wav(&path, &l, &r)?;
    Ok(Some(report))
}

// ---------------------------------------------------------------- kits

/// A drum kit: a sound for each pad (folder, name, level), and a two-bar
/// pattern to hear it with, at its genre's tempo.
pub struct Kit {
    pub name: &'static str,
    /// What makes it that genre, for the guide and the menu's tooltip.
    pub about: &'static str,
    pub tempo: f64,
    /// How late the off-beat 16ths come, as a fraction of a 16th (0: straight).
    pub swing: f64,
    pub pads: [(&'static str, &'static str, f32); RACK_PADS],
    /// Per pad note: 32 sixteenths, `X` loud, `x` normal, `o` soft, `r` a roll
    /// of two 32nds, `t` of three, `.` none (spaces are ignored).
    pub pattern: &'static [(u8, &'static str)],
}

use crate::factory_recipes::{HATS, KICKS, PERC, SNARES, STABS, SUBS, FX as FX_DIR};

pub fn kits() -> Vec<Kit> {
    vec![
        Kit {
            name: "House",
            about: "Bombo redondo con pegada, palmada larga con sala, charles abierto a contratiempo, shaker, congas y un stab de órgano",
            tempo: 124.0,
            swing: 0.0,
            pads: [
                (KICKS, "Bombo house", 0.95),
                (SNARES, "Aro house", 0.6),
                (SNARES, "Caja house", 0.75),
                (SNARES, "Palmada house", 0.8),
                (SNARES, "Palmada afro", 0.7),
                (PERC, "Tom grave", 0.7),
                (HATS, "Charles cerrado house", 0.6),
                (PERC, "Tom agudo", 0.7),
                (PERC, "Shaker", 0.5),
                (PERC, "Conga aguda", 0.6),
                (HATS, "Charles abierto house", 0.55),
                (PERC, "Bongó", 0.6),
                (PERC, "Pandereta", 0.5),
                (HATS, "Crash brillante", 0.5),
                (STABS, "Stab de órgano (Do menor 7)", 0.6),
                (HATS, "Ride", 0.45),
            ],
            pattern: &[
                (36, "x... x... x... x... x... x... x... x..."),
                (39, ".... x... .... x... .... x... .... x..."),
                (42, "..x. ..x. ..x. ..x. ..x. ..x. ..x. ..xo"),
                (46, ".... .... .... .... .... .... .... ..x."),
                (44, "oxox oxox oxox oxox oxox oxox oxox oxox"),
                (45, ".... ...x .... .... .... ...x ..x. ...."),
                (50, ".... .... .... .... .... .... .x.. ...."),
            ],
        },
        Kit {
            name: "Techno",
            about: "Bombo saturado con rumble, charles metálicos y saturados, palmada en placa, aro y cencerro distorsionados",
            tempo: 130.0,
            swing: 0.0,
            pads: [
                (KICKS, "Bombo techno con rumble", 1.0),
                (SNARES, "Aro techno", 0.55),
                (SNARES, "Caja techno", 0.7),
                (SNARES, "Palmada techno", 0.75),
                (KICKS, "Bombo techno saturado", 0.9),
                (PERC, "Tom electrónico", 0.7),
                (HATS, "Charles cerrado techno", 0.6),
                (PERC, "Tom medio", 0.65),
                (PERC, "Shaker corto", 0.45),
                (PERC, "Cencerro saturado", 0.5),
                (HATS, "Charles abierto techno", 0.55),
                (PERC, "Percusión metálica", 0.55),
                (PERC, "Blip", 0.5),
                (HATS, "Crash oscuro", 0.5),
                (FX_DIR, "Zap", 0.5),
                (HATS, "Ride", 0.5),
            ],
            pattern: &[
                (36, "x... x... x... x... x... x... x... x..."),
                (39, ".... x... .... x... .... x... .... x..."),
                (42, "x.x. x.xx x.x. x.x. x.x. x.xx x.x. x.xo"),
                (46, "..x. ..x. ..x. ..x. ..x. ..x. ..x. ..x."),
                (37, "...x .... ..x. .... ...x .... .... .x.."),
                (47, ".... ..x. .... ...x .... ..x. .... ...x"),
                (51, "x.x. x.x. x.x. x.x. x.x. x.x. x.x. x.x."),
            ],
        },
        Kit {
            name: "Trap",
            about: "Bombo corto y 808 largo afinado en Do, caja y palmada secas y con chasquido, charles nítidos para redobles",
            tempo: 140.0,
            swing: 0.0,
            pads: [
                (KICKS, "Bombo trap", 0.9),
                (SNARES, "Chasquido", 0.6),
                (SNARES, "Caja trap", 0.8),
                (SNARES, "Palmada trap", 0.8),
                (SUBS, "808 largo (Do)", 0.9),
                (PERC, "Tom electrónico", 0.7),
                (HATS, "Charles cerrado trap", 0.55),
                (PERC, "Tom agudo", 0.65),
                (PERC, "Shaker corto", 0.45),
                (PERC, "Cencerro", 0.5),
                (HATS, "Charles abierto trap", 0.5),
                (PERC, "Clave", 0.5),
                (PERC, "Gota", 0.5),
                (HATS, "Crash brillante", 0.5),
                (FX_DIR, "Láser", 0.5),
                (HATS, "Ride seco", 0.45),
            ],
            pattern: &[
                (36, "x... .... ..x. .... .... .... x.x. ...."),
                (40, "x... .... ..x. .... .... .... x.x. ...."),
                (39, ".... .... x... .... .... .... x... ...."),
                (38, ".... .... x... .... .... .... x... ...."),
                (42, "x.x. x.x. x.x. x.rr x.x. x.x. tttt x.x."),
                (46, ".... .... .... .... .... .... .... ..x."),
            ],
        },
        Kit {
            name: "Drum & Bass",
            about: "Bombo seco y agudo, caja tensa que restalla con algo de placa, caja suave para los fantasmas, charles cortos y ride",
            tempo: 174.0,
            swing: 0.0,
            pads: [
                (KICKS, "Bombo drum and bass", 0.95),
                (SNARES, "Aro garage", 0.55),
                (SNARES, "Caja drum and bass", 0.85),
                (SNARES, "Palmada techno", 0.6),
                (SNARES, "Caja suave drum and bass", 0.45),
                (PERC, "Tom grave", 0.65),
                (HATS, "Charles cerrado drum and bass", 0.55),
                (PERC, "Tom agudo", 0.6),
                (PERC, "Shaker", 0.45),
                (PERC, "Conga aguda", 0.5),
                (HATS, "Charles abierto drum and bass", 0.5),
                (PERC, "Pandereta", 0.45),
                (PERC, "Bongó", 0.5),
                (HATS, "Crash brillante", 0.5),
                (FX_DIR, "Crash al revés", 0.5),
                (HATS, "Ride", 0.5),
            ],
            pattern: &[
                (36, "x... .... ..x. .... x... .... .... ...."),
                (38, ".... x... .... x... .... x... .... x..."),
                (40, ".... ...o .o.. ...o .... ...o .... .o.o"),
                (42, "x.x. x.x. x.x. x.xx x.x. x.x. x.x. x.x."),
                (51, "x... x... x... x... x... x... x... x..."),
                (46, ".... .... .... ..x. .... .... .... ..x."),
            ],
        },
        Kit {
            name: "Lo-Fi",
            about: "Todo apagado, saturado y con la resolución reducida: bombo blando, caja grave, charles oscuros y un acorde de piano gastado",
            tempo: 85.0,
            swing: 0.55,
            pads: [
                (KICKS, "Bombo lo-fi", 0.95),
                (SNARES, "Aro lo-fi", 0.6),
                (SNARES, "Caja lo-fi", 0.8),
                (SNARES, "Palmada afro", 0.55),
                (SNARES, "Aro boom bap", 0.55),
                (PERC, "Tom lo-fi", 0.65),
                (HATS, "Charles cerrado lo-fi", 0.55),
                (PERC, "Conga grave", 0.55),
                (PERC, "Shaker corto", 0.4),
                (PERC, "Bloque de madera", 0.5),
                (HATS, "Charles abierto lo-fi", 0.5),
                (PERC, "Conga aguda", 0.5),
                (FX_DIR, "Ruido de vinilo", 0.5),
                (HATS, "Crash lo-fi", 0.45),
                (STABS, "Acorde lo-fi (Do mayor 7)", 0.55),
                (HATS, "Ride seco", 0.4),
            ],
            pattern: &[
                (36, "x... .... ..x. .... x... ...x .... ...."),
                (38, ".... x... .... x... .... x... .... x..."),
                (42, "x.x. x.xo x.x. x.x. x.x. x.xo x.x. x.xx"),
                (37, ".... .... .... ...o .... .... .o.. ...."),
                (48, "x... .... .... .... x... .... .... ...."),
            ],
        },
        Kit {
            name: "UK Garage",
            about: "Bombo corto que rebota, aro y caja nítidos, charles rápidos con swing, shaker y pandereta",
            tempo: 132.0,
            swing: 0.6,
            pads: [
                (KICKS, "Bombo garage", 0.95),
                (SNARES, "Aro garage", 0.65),
                (SNARES, "Caja garage", 0.8),
                (SNARES, "Palmada house", 0.7),
                (SNARES, "Chasquido", 0.55),
                (PERC, "Tom grave", 0.65),
                (HATS, "Charles cerrado garage", 0.55),
                (PERC, "Tom agudo", 0.6),
                (PERC, "Shaker", 0.5),
                (PERC, "Bloque de madera", 0.5),
                (HATS, "Charles abierto garage", 0.5),
                (PERC, "Pandereta", 0.5),
                (PERC, "Clave", 0.5),
                (HATS, "Crash brillante", 0.5),
                (STABS, "Stab de órgano (Do menor 7)", 0.55),
                (HATS, "Ride", 0.45),
            ],
            pattern: &[
                (36, "x... .... ..x. .... x... .... .x.. ...."),
                (38, ".... x... .... x... .... x... .... x..."),
                (37, "...x .... .x.. ..x. ...x .... .... .x.."),
                (42, "xxox xxox xxox xxox xxox xxox xxox xxox"),
                (46, "..x. .... ..x. .... ..x. .... ..x. ...."),
                (44, ".x.x .x.x .x.x .x.x .x.x .x.x .x.x .x.x"),
            ],
        },
        Kit {
            name: "Afro House",
            about: "Bombo cálido y redondo, congas grave y aguda, bongó, shaker, clave, cencerro y toms que llevan el groove",
            tempo: 122.0,
            swing: 0.15,
            pads: [
                (KICKS, "Bombo afro", 0.95),
                (SNARES, "Aro house", 0.55),
                (SNARES, "Caja house", 0.6),
                (SNARES, "Palmada afro", 0.75),
                (PERC, "Bloque de madera", 0.55),
                (PERC, "Tom grave", 0.7),
                (HATS, "Charles cerrado house", 0.5),
                (PERC, "Tom medio", 0.65),
                (PERC, "Shaker", 0.55),
                (PERC, "Conga grave", 0.7),
                (HATS, "Charles abierto house", 0.5),
                (PERC, "Conga aguda", 0.7),
                (PERC, "Bongó", 0.65),
                (HATS, "Crash oscuro", 0.45),
                (PERC, "Cencerro", 0.45),
                (PERC, "Clave", 0.55),
            ],
            pattern: &[
                (36, "x... x... x... x... x... x... x... x..."),
                (39, ".... x... .... x... .... x... .... x..."),
                (44, "xoxo xoxo xoxo xoxo xoxo xoxo xoxo xoxo"),
                (45, "..x. ..x. .x.. ..x. ..x. ..x. .x.. x.x."),
                (47, "x..x .... x... .x.. x..x .... x... .xx."),
                (51, "x..x ..x. ..x. x... x..x ..x. ..x. x..."),
                (48, ".... .... .... ..x. .... .... .... .x.x"),
            ],
        },
        Kit {
            name: "Synthwave",
            about: "Caja y palmada con reverb puerta de los 80, bombo sólido con algo de sala, toms grandes, crash y charles en placa",
            tempo: 105.0,
            swing: 0.0,
            pads: [
                (KICKS, "Bombo ochentero", 0.95),
                (SNARES, "Aro house", 0.5),
                (SNARES, "Caja ochentera", 0.85),
                (SNARES, "Palmada ochentera", 0.75),
                (SNARES, "Caja house", 0.6),
                (PERC, "Tom ochentero", 0.75),
                (HATS, "Charles cerrado ochentero", 0.55),
                (PERC, "Tom electrónico", 0.7),
                (PERC, "Shaker", 0.45),
                (PERC, "Cencerro", 0.45),
                (HATS, "Charles abierto house", 0.5),
                (PERC, "Tom agudo", 0.65),
                (PERC, "Pandereta", 0.45),
                (HATS, "Crash brillante", 0.55),
                (STABS, "Acorde ochentero (Do menor)", 0.55),
                (HATS, "Ride", 0.45),
            ],
            pattern: &[
                (36, "x... .... x... .... x... .... x... ...."),
                (38, ".... x... .... x... .... x... .... x..."),
                (42, "x.x. x.x. x.x. x.x. x.x. x.x. x.x. x.x."),
                (41, ".... .... .... .... .... .... ..x. x..."),
                (49, "x... .... .... .... .... .... .... ...."),
            ],
        },
        Kit {
            name: "Minimal",
            about: "Bombo corto con clic, clics y aros pequeños, charles diminutos, bloque de madera, blips y gotas: pocos sonidos, muy secos",
            tempo: 126.0,
            swing: 0.1,
            pads: [
                (KICKS, "Bombo minimal", 0.95),
                (SNARES, "Clic minimal", 0.6),
                (SNARES, "Aro garage", 0.6),
                (SNARES, "Palmada house", 0.65),
                (SNARES, "Chasquido", 0.55),
                (PERC, "Tom electrónico", 0.6),
                (HATS, "Charles cerrado minimal", 0.55),
                (PERC, "Gota", 0.55),
                (PERC, "Shaker corto", 0.45),
                (PERC, "Bloque de madera", 0.55),
                (HATS, "Charles abierto garage", 0.45),
                (PERC, "Blip", 0.5),
                (PERC, "Percusión metálica", 0.5),
                (HATS, "Crash oscuro", 0.4),
                (FX_DIR, "Zap", 0.45),
                (HATS, "Ride seco", 0.45),
            ],
            pattern: &[
                (36, "x... x... x... x... x... x... x... x..."),
                (37, "..x. ...x .x.. ..x. ..x. ...x .... .x.."),
                (42, "..x. ..x. ..x. ..x. ..x. ..x. ..x. ..x."),
                (47, ".... .... ...x .... .... ..x. .... ...."),
                (45, ".x.. .... .... .x.. .x.. .... ..x. ...."),
                (44, "x.x. x.x. x.x. x.x. x.x. x.x. x.x. x.x."),
            ],
        },
        Kit {
            name: "Hip Hop",
            about: "Boom bap: bombo que golpea, caja gorda con saturación y sala, aro, charles apagados con swing y pandereta",
            tempo: 90.0,
            swing: 0.5,
            pads: [
                (KICKS, "Bombo boom bap", 0.95),
                (SNARES, "Aro boom bap", 0.6),
                (SNARES, "Caja boom bap", 0.85),
                (SNARES, "Palmada afro", 0.65),
                (SNARES, "Caja lo-fi", 0.6),
                (PERC, "Tom grave", 0.65),
                (HATS, "Charles cerrado boom bap", 0.55),
                (PERC, "Tom medio", 0.6),
                (PERC, "Shaker", 0.45),
                (PERC, "Cencerro", 0.45),
                (HATS, "Charles abierto boom bap", 0.5),
                (PERC, "Pandereta", 0.5),
                (FX_DIR, "Ruido de vinilo", 0.45),
                (HATS, "Crash oscuro", 0.45),
                (STABS, "Acorde de piano eléctrico (Do menor 9)", 0.55),
                (HATS, "Ride seco", 0.4),
            ],
            pattern: &[
                (36, "x... .... ..x. ...x .x.. .... ..x. ...."),
                (38, ".... x... .... x... .... x... .... x..o"),
                (42, "x.x. x.x. x.x. x.x. x.x. x.x. x.x. x.x."),
                (47, ".... .... .... x... .... .... .... x..."),
            ],
        },
    ]
}

/// The rack settings and pad files of `kit` under `root` (the files may not
/// be made yet).
pub fn kit_settings(root: &Path, kit: &Kit) -> (RackParams, Vec<String>) {
    let mut params = RackParams::default();
    let mut files = vec![String::new(); RACK_PADS];
    for (pad, &(folder, name, level)) in kit.pads.iter().enumerate() {
        files[pad] = root.join(folder).join(format!("{name}.wav")).to_string_lossy().into_owned();
        params.level[pad] = level;
    }
    // The closed hat cuts the open one, as on drum machines.
    for note in [42u8, 46] {
        params.choke[(note - RACK_FIRST_NOTE) as usize] = 1;
    }
    (params, files)
}

/// The recipes `kit` plays.
fn kit_recipes<'a>(kit: &Kit, all: &'a [Recipe]) -> Vec<&'a Recipe> {
    kit.pads.iter().filter_map(|&(folder, name, _)| all.iter().find(|r| r.folder == folder && r.name == name)).collect()
}

/// Makes the sounds of `kit` that are missing; returns its rack settings and files.
pub fn make_kit(root: &Path, kit: &Kit) -> Result<(RackParams, Vec<String>), String> {
    let all = recipes();
    for recipe in kit_recipes(kit, &all) {
        make(root, recipe)?;
    }
    Ok(kit_settings(root, kit))
}

/// The kit as a preset of the Drum Pads, as the browser applies it.
pub fn kit_preset(root: &Path, kit: &Kit) -> crate::presets::Preset {
    let (params, samples) = kit_settings(root, kit);
    crate::presets::Preset { name: format!("Kit {}", kit.name), instrument: InstrumentKind::DrumRack(params), samples }
}

/// The notes of a kit's pattern: (beat, pitch, velocity).
pub fn pattern_notes(kit: &Kit) -> Vec<(f64, u8, f32)> {
    let mut notes = Vec::new();
    for &(pitch, steps) in kit.pattern {
        for (step, c) in steps.chars().filter(|c| !c.is_whitespace()).enumerate() {
            let at = step as f64 * 0.25 + if step % 2 == 1 { kit.swing * 0.25 * 0.5 } else { 0.0 };
            let hits: &[(f64, f32)] = match c {
                'X' => &[(0.0, 1.0)],
                'x' => &[(0.0, 0.85)],
                'o' => &[(0.0, 0.5)],
                'r' => &[(0.0, 0.7), (0.125, 0.55)],
                't' => &[(0.0, 0.7), (0.25 / 3.0, 0.55), (0.5 / 3.0, 0.6)],
                _ => &[],
            };
            for &(offset, velocity) in hits {
                notes.push((at + offset, pitch, velocity));
            }
        }
    }
    notes
}

pub const LOOP_BARS: f64 = 2.0;

/// The kit's loop: its pattern on its pads at its tempo, exactly two bars, its
/// tail folded onto its start (it loops without a gap or a cut).
pub fn render_loop(root: &Path, kit: &Kit) -> Stereo {
    let (params, files) = kit_settings(root, kit);
    let beats = LOOP_BARS * 4.0;
    let mut project = Project::empty();
    project.tempo = kit.tempo;
    let mut track = Track::new(kit.name, [0, 0, 0], InstrumentKind::DrumRack(params));
    track.samples = files;
    let mut clip = MidiClip::new(kit.name, beats, [0, 0, 0]);
    for (start, pitch, velocity) in pattern_notes(kit) {
        clip.notes.push(Note { start, length: 0.25, pitch, velocity, chance: 1.0 });
    }
    track.arrangement.push(ArrangementClip { start: 0.0, length: beats, clip: Clip::Midi(clip) });
    project.tracks = vec![track];
    let (mut l, mut r) = render_project(&project, beats);
    let frames = (beats * 60.0 / kit.tempo * RATE as f64).round() as usize;
    l.resize(frames.max(l.len()), 0.0);
    r.resize(frames.max(r.len()), 0.0);
    for i in frames..l.len() {
        l[i % frames] += l[i];
        r[i % frames] += r[i];
    }
    l.truncate(frames);
    r.truncate(frames);
    // Peaks at -1 dBFS.
    let gain = 10f32.powf(-1.0 / 20.0) / peak(&l, &r).max(1e-6);
    for s in l.iter_mut().chain(r.iter_mut()) {
        *s *= gain;
    }
    (l, r)
}

pub fn loop_file(root: &Path, kit: &Kit) -> PathBuf {
    root.join(LOOPS).join(format!("{} {}.wav", kit.name, kit.tempo))
}

// ---------------------------------------------------------------- making it all

/// How many steps making everything takes (for its progress).
pub fn steps() -> usize {
    recipes().len() + kits().len()
}

pub fn is_complete(root: &Path) -> bool {
    root.join(DONE).exists()
}

/// Makes every missing file of the library under `root`, on every core,
/// counting what it has done in `done`. Stops early when `cancel` is set.
pub fn make_all(root: &Path, done: &AtomicUsize, cancel: &AtomicBool) -> Result<(), String> {
    let all = recipes();
    let next = AtomicUsize::new(0);
    let failed: Mutex<Option<String>> = Mutex::new(None);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= all.len() || cancel.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(error) = make(root, &all[i]) {
                    *failed.lock().unwrap() = Some(error);
                }
                done.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    if let Some(error) = failed.lock().unwrap().take() {
        return Err(error);
    }
    let kits = kits();
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..threads.min(kits.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= kits.len() || cancel.load(Ordering::Relaxed) {
                    break;
                }
                let kit = &kits[i];
                let preset = root.join(KITS).join(format!("{}.json", kit.name));
                let result = std::fs::create_dir_all(root.join(KITS))
                    .map_err(|e| e.to_string())
                    .and_then(|_| if preset.exists() { Ok(()) } else { crate::store::write_json(&preset, &kit_preset(root, kit)) })
                    .and_then(|_| {
                        let path = loop_file(root, kit);
                        if path.exists() {
                            return Ok(());
                        }
                        let (l, r) = render_loop(root, kit);
                        write_wav(&path, &l, &r)
                    });
                if let Err(error) = result {
                    *failed.lock().unwrap() = Some(error);
                }
                done.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    if let Some(error) = failed.lock().unwrap().take() {
        return Err(error);
    }
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    std::fs::write(root.join(DONE), VERSION).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- in the app

/// The library being made, and a kit waiting for its sounds.
#[derive(Default)]
pub struct State {
    making: Option<Making>,
    /// A kit asked for before its sounds were made: (track, kit, its result).
    waiting: Option<(usize, usize, Arc<Mutex<Option<Result<(RackParams, Vec<String>), String>>>>)>,
    pub error: Option<String>,
}

struct Making {
    done: Arc<AtomicUsize>,
    handle: std::thread::JoinHandle<Result<(), String>>,
}

impl State {
    /// How far making the library has got (done, of), while it is made.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.making.as_ref().map(|m| (m.done.load(Ordering::Relaxed), steps()))
    }

    pub fn busy(&self) -> bool {
        self.making.is_some() || self.waiting.is_some()
    }
}

impl crate::app::PulseApp {
    /// Starts making the library in the background, unless it is made or being made.
    pub fn make_factory_sounds(&mut self) {
        let root = root();
        if self.factory.making.is_some() || self.factory.error.is_some() || is_complete(&root) {
            return;
        }
        let done = Arc::new(AtomicUsize::new(0));
        let counter = done.clone();
        let handle = std::thread::spawn(move || make_all(&root, &counter, &AtomicBool::new(false)));
        self.factory.making = Some(Making { done, handle });
    }

    /// Fills the drum rack of `track` with a factory kit (made first if it isn't).
    pub fn load_factory_kit(&mut self, track: usize, kit: usize) {
        let all = kits();
        let Some(chosen) = all.get(kit) else { return };
        let root = root();
        let (params, files) = kit_settings(&root, chosen);
        if files.iter().all(|f| Path::new(f).exists()) {
            self.apply_factory_kit(track, chosen.name, params, files);
            return;
        }
        let result = Arc::new(Mutex::new(None));
        let shared = result.clone();
        std::thread::spawn(move || {
            let made = make_kit(&root, &kits()[kit]);
            if let Ok(mut slot) = shared.lock() {
                *slot = Some(made);
            }
        });
        self.factory.waiting = Some((track, kit, result));
        self.status = Some(format!("Preparando el kit {}…", chosen.name));
    }

    fn apply_factory_kit(&mut self, track: usize, name: &str, params: RackParams, files: Vec<String>) {
        if track >= self.project.tracks.len() || self.project.tracks[track].is_audio() {
            return;
        }
        self.set_instrument(track, InstrumentKind::DrumRack(params));
        self.project.tracks[track].samples = files;
        self.sync_samples(track);
        self.status = Some(format!("Kit {name} cargado"));
    }

    /// Picks up what was being made in the background, once it is.
    pub fn poll_factory_sounds(&mut self, ctx: &eframe::egui::Context) {
        if self.factory.making.as_ref().is_some_and(|m| m.handle.is_finished()) {
            let making = self.factory.making.take().unwrap();
            match making.handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => self.factory.error = Some(error),
                Err(_) => self.factory.error = Some("No se pudieron preparar los sonidos".into()),
            }
        }
        let ready = self.factory.waiting.as_ref().and_then(|(_, _, result)| result.lock().ok()?.take());
        if let Some(result) = ready {
            let (track, kit, _) = self.factory.waiting.take().unwrap();
            match result {
                Ok((params, files)) => self.apply_factory_kit(track, kits()[kit].name, params, files),
                Err(error) => self.status = Some(error),
            }
        }
        if self.factory.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
