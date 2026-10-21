//! The mix assistant: renders the mix and each track on its own (offline, in the
//! background), measures them (loudness, true peak, range, stereo, balance, low
//! end) and says what to fix for where the song is going (Spotify, Apple Music,
//! a club…), each with a button that does it. A reference song can be loaded to
//! compare the loudness and the tonal balance with.

use crate::app::PulseApp;
use crate::export::{self, Range};
use crate::theme::{ACCENT, METER_RED, PLAY, TEXT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, RichText, Sense, Stroke};
use engine::model::{Device, DuckerParams, Effect, Eq3Params, LimiterParams, Param};
use stems::loudness;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const RATE: f32 = 48_000.0;
/// The most the assistant drives the master's limiter (dB): past it, a mix is
/// squashed rather than made louder.
const MAX_LIMITER_GAIN: f32 = 12.0;

/// Where the song is going, and the loudness and true peak it should have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Streaming,
    Apple,
    Club,
    Broadcast,
    /// As loud as the reference song.
    Reference,
}

impl Target {
    const ALL: [Target; 5] = [Target::Streaming, Target::Apple, Target::Club, Target::Broadcast, Target::Reference];

    fn label(self) -> &'static str {
        match self {
            Target::Streaming => "Spotify, YouTube, Tidal (−14 LUFS)",
            Target::Apple => "Apple Music (−16 LUFS)",
            Target::Club => "Club, DJ, SoundCloud (−9 LUFS)",
            Target::Broadcast => "Radio y TV, EBU R128 (−23 LUFS)",
            Target::Reference => "Como la referencia",
        }
    }

    /// The loudness (LUFS) and the highest true peak (dBTP).
    fn levels(self, reference: Option<&Measures>) -> Option<(f64, f64)> {
        match self {
            Target::Streaming => Some((-14.0, -1.0)),
            Target::Apple => Some((-16.0, -1.0)),
            Target::Club => Some((-9.0, -1.0)),
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
    pub drums: bool,
    pub peak: f64,
    pub lufs: Option<f64>,
    pub correlation: f64,
    /// Its level below 120 Hz every 0.1 s (dB).
    low_end: Vec<f64>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub mix: Measures,
    pub tracks: Vec<TrackMeasures>,
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
}

fn advice(level: Level, text: String, fix: Option<Fix>) -> Advice {
    Advice { level, text, fix }
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
                format!("Sonoridad {} LUFS: {} dB por encima del objetivo ({} LUFS). La plataforma la bajará, y lo que hayas apretado de más solo le quita dinámica", num(lufs), num(-off), num(goal)),
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
        let text = format!("«{}» y «{}» llenan los graves a la vez el {:.0} % del tiempo: se tapan y la mezcla se embarra", a.name, b.name, share * 100.0);
        // A kick against the rest: the rest ducks. Otherwise the one with less
        // low end gives it up.
        let fix = match (a.drums, b.drums) {
            (true, false) => Some(Fix::Duck { track: b.index, name: b.name.clone(), source: a.id, source_name: a.name.clone() }),
            (false, true) => Some(Fix::Duck { track: a.index, name: a.name.clone(), source: b.id, source_name: b.name.clone() }),
            _ => {
                let mean = |t: &TrackMeasures| t.low_end.iter().sum::<f64>() / t.low_end.len().max(1) as f64;
                let lesser = if mean(a) < mean(b) { a } else { b };
                Some(Fix::CutLows { track: lesser.index, name: lesser.name.clone() })
            }
        };
        out.push(advice(Level::Warning, text, fix));
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
        let names: Vec<(String, bool, u32)> = self.project.tracks.iter().map(|t| (t.name.clone(), t.is_drums(), t.id)).collect();
        let (progress, cancel, result) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (p, c, r) = (progress.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            // The mix and every track at once (see `export::render_each`).
            let renders: Vec<_> = mix.into_iter().chain(tracks).collect();
            let measured = export::render_each(renders, RATE, &p, &c, |render, left, right| {
                Ok(match render.track {
                    None => Measured::Mix(Measures::of(&left, &right, RATE)),
                    Some(index) => {
                        let (name, drums, id) = names.get(index).cloned().unwrap_or_default();
                        Measured::Track(TrackMeasures {
                            index,
                            id,
                            name,
                            drums,
                            peak: loudness::sample_peak(&left, &right),
                            lufs: loudness::integrated(&left, &right, RATE),
                            correlation: loudness::correlation(&left, &right),
                            low_end: loudness::low_end(&left, &right, RATE),
                        })
                    }
                })
            });
            let outcome = measured.map_err(|_| "Análisis cancelado".to_string()).and_then(|all| {
                let mut all = all.into_iter();
                let Some(Measured::Mix(mix)) = all.next() else { return Err("Nada que analizar".to_string()) };
                let tracks = all.filter_map(|t| if let Measured::Track(t) = t { Some(t) } else { None }).collect();
                Ok(Report { mix, tracks })
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
        let before = self.mix.report.as_ref().and_then(|r| r.mix.lufs);
        let was = self.master_limiter_gain();
        self.apply_fix(&Fix::MasterLimiter { gain_db, ceiling_db });
        let applied = self.master_limiter_gain() - was;
        self.mix.chase = Some(Chase { goal, max_peak, steps: 1, last: (applied, before) });
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
                Level::Warning => ("⚠", METER_RED),
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
        TrackMeasures { index, id: index as u32 + 10, name: name.into(), drums, peak, lufs: Some(-20.0), correlation: 0.9, low_end: low }
    }

    fn report(lufs: f64, true_peak: f64, tracks: Vec<TrackMeasures>) -> Report {
        Report { mix: Measures { lufs: Some(lufs), range: Some(6.0), short_max: Some(lufs + 3.0), true_peak, peak: true_peak, correlation: 0.8, bands: [0.0; 10] }, tracks }
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
}
