//! Rendering the arrangement to WAV files: the full mix or one file per track (stems).
//!
//! Rendering runs on a background thread with its own offline engine, faster than
//! real time and independent of the audio device. The output is not clipped while
//! rendering, so the true peak can be reported and the mix normalized.

use crate::app::PulseApp;
use crate::project::{Project, Track};
use engine::model::InstrumentKind;
use crate::sync::{self, SampleCache};
use crate::theme::TEXT_DIM;
use eframe::egui;
use engine::{Command, OfflineRenderer, MAX_TRACKS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Longest effect tail rendered after the end of the song.
const MAX_TAIL_SECONDS: f64 = 10.0;
/// The tail ends once the output stays below this level (-80 dBFS) for a while.
const SILENCE: f32 = 1e-4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    Song,
    Loop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BitDepth {
    Int16,
    Int24,
    Float32,
}

impl BitDepth {
    fn label(self) -> &'static str {
        match self {
            BitDepth::Int16 => "16 bits",
            BitDepth::Int24 => "24 bits",
            BitDepth::Float32 => "32 bits (float)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub range: Range,
    pub stems: bool,
    pub bits: BitDepth,
    pub sample_rate: u32,
    /// Normalize the mix so its peak sits at this level (dBFS).
    pub normalize: Option<f32>,
    /// Stems through the master's effects (as heard), or without them (they
    /// then add up to the mix).
    pub stems_master: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { range: Range::Song, stems: false, bits: BitDepth::Int24, sample_rate: 48_000, normalize: None, stems_master: true }
    }
}

/// One file to render: its path and the commands that set the engine up for it.
pub struct Render {
    pub path: PathBuf,
    pub commands: Vec<Command>,
    /// The track a stem is of (none for the mix).
    pub track: Option<usize>,
    /// It plays plugins (see [`threads`]).
    pub plugins: bool,
}

#[derive(Debug, Default)]
pub struct Report {
    pub files: Vec<PathBuf>,
    pub seconds: f64,
    /// Highest peak before normalization, in dBFS.
    pub peak_db: f32,
    /// Samples that had to be clipped for integer formats.
    pub clipped: usize,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut text = format!(
            "Exportado{} {} archivo(s), {:.1} s, pico {:.1} dBFS",
            if self.files.len() == 1 { "" } else { "s" },
            self.files.len(),
            self.seconds,
            self.peak_db
        );
        if self.clipped > 0 {
            text.push_str(&format!(" · ¡{} muestras saturadas! Baja el master o normaliza", self.clipped));
        }
        if let Some(dir) = self.files.first().and_then(|f| f.parent()) {
            text.push_str(&format!(" · {}", dir.display()));
        }
        text
    }
}

/// The beat range to render, or `None` if there is nothing on the arrangement.
pub fn beat_range(project: &Project, range: Range) -> Option<(f64, f64)> {
    match range {
        Range::Loop => (project.loop_end > project.loop_start).then_some((project.loop_start, project.loop_end)),
        Range::Song => {
            let end = project.tracks.iter().map(|t| t.arrangement_end()).fold(0.0, f64::max);
            (end > 0.0).then_some((0.0, end))
        }
    }
}

/// Builds the renders for the settings: the mix, or one solo render per track.
pub fn plan(
    project: &Project,
    samples: &mut SampleCache,
    settings: &Settings,
    destination: &Path,
) -> Vec<Render> {
    let sample_rate = settings.sample_rate as f32;
    // Exports always play the automation as written, ignoring knobs moved by hand.
    let commands = |samples: &mut SampleCache| sync::project_commands(project, samples, sample_rate, &HashSet::new());
    if !settings.stems {
        return vec![Render { path: destination.to_path_buf(), commands: commands(samples), track: None, plugins: uses_plugins(project) }];
    }
    let base = destination.file_stem().map_or("Pulse".into(), |s| s.to_string_lossy().into_owned());
    let folder = destination.parent().unwrap_or(Path::new("."));
    // What is heard, one track at a time: not the ones switched off (or in a
    // group that is), and not groups, whose tracks come out each through it.
    let heard = |t: usize| {
        let mut at = Some(t);
        while let Some(i) = at {
            if !project.tracks[i].mixer.active {
                return false;
            }
            at = crate::groups::parent(&project.tracks, i);
        }
        true
    };
    project
        .tracks
        .iter()
        .enumerate()
        .filter(|&(t, track)| !track.is_group() && heard(t))
        .map(|(t, track)| {
            // Only what can be heard in it: the rest is rendered as empty tracks.
            let only = only(project, t);
            let mut list = sync::project_commands(&only, samples, sample_rate, &HashSet::new());
            // Soloing one track keeps its sends, so its reverb and delay are part of the stem.
            list.extend((0..MAX_TRACKS).map(|other| Command::SetSolo { track: other, solo: other == t }));
            if !settings.stems_master {
                list.push(Command::ReplaceChain { target: engine::Target::Master, chain: Box::default() });
            }
            let name = format!("{base} - {} {}.wav", t + 1, sanitize(&track.name));
            Render { path: folder.join(name), commands: list, track: Some(t), plugins: uses_plugins(&only) }
        })
        .collect()
}

/// The project as track `t`'s stem hears it: the tracks that can't sound in it
/// are left empty, so rendering it costs little more than the track itself.
/// What stays: the track, the groups it is in, and the tracks whose sound
/// their effects, the returns' or the master's listen to (sidechains), and
/// theirs in turn. Nothing else reaches a soloed track's stem: the others are
/// silent, their sends too, and a track's sound doesn't depend on another's
/// (there is no latency compensation between them).
pub fn only(project: &Project, t: usize) -> Project {
    use engine::model::Effect;
    let tracks = &project.tracks;
    let listens = |devices: &[engine::model::Device]| -> Vec<usize> {
        devices
            .iter()
            .filter_map(|d| match &d.effect {
                Effect::Compressor(p) => p.sidechain,
                Effect::Ducker(p) => p.sidechain,
                _ => None,
            })
            .filter_map(|id| tracks.iter().position(|o| o.id == id && id != 0))
            .collect()
    };
    let mut keep = vec![false; tracks.len()];
    let mut queue = vec![t];
    let mut at = crate::groups::parent(tracks, t);
    while let Some(group) = at {
        queue.push(group);
        at = crate::groups::parent(tracks, group);
    }
    queue.extend(project.returns.iter().map(|r| &r.devices).chain([&project.master.devices]).flat_map(|d| listens(d)));
    while let Some(i) = queue.pop() {
        if i < keep.len() && !keep[i] {
            keep[i] = true;
            // What a frozen track's effects listened to is in its audio already,
            // but keeping it costs nothing it wouldn't otherwise.
            queue.extend(listens(&tracks[i].devices));
        }
    }
    let mut only = project.clone();
    for (track, _) in only.tracks.iter_mut().zip(&keep).filter(|(_, k)| !**k) {
        let mut empty = Track::new(track.name.clone(), track.color, InstrumentKind::Synth(Default::default()));
        (empty.kind, empty.depth, empty.id) = (track.kind, track.depth, track.id);
        *track = empty;
    }
    only
}

/// Whether anything in the project is a plugin.
fn uses_plugins(project: &Project) -> bool {
    let effects = |devices: &[engine::model::Device]| devices.iter().any(|d| matches!(d.effect, engine::model::Effect::Plugin(_)));
    project.tracks.iter().any(|t| matches!(t.instrument, InstrumentKind::Plugin(_)) || effects(&t.devices))
        || project.returns.iter().map(|r| &r.devices).chain([&project.master.devices]).any(|d| effects(d))
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect()
}

/// Renders `[start, end)` beats plus the effect tail. `progress` receives 0..1.
/// Beats a render of part of the song starts before it (see [`render_mapped`]).
const PREROLL_BEATS: f64 = 16.0;

/// Where a render that has to start at `start` begins playing.
fn preroll(start: f64) -> f64 {
    (start - PREROLL_BEATS).max(0.0)
}

/// What a render needs to know of the song besides its commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Song {
    pub tempo: f64,
    /// Seconds of silence after which the tail is over: longer than the gap
    /// between a delay's echoes (see [`Song::of`]).
    pub ring_out: f64,
}

impl Song {
    pub fn of(project: &crate::project::Project) -> Self {
        Song { tempo: project.tempo, ring_out: ring_out(project) }
    }
}

/// How long a tail can stay silent and still have echoes to come: the longest
/// delay in the project at its slowest tempo, plus a margin.
pub fn ring_out(project: &crate::project::Project) -> f64 {
    use engine::model::{Device, Effect, InstrumentKind};
    let slowest = project.master.automation.iter().filter(|l| l.param == engine::model::Param::Tempo).flat_map(|l| l.points.iter().map(|p| p.value as f64)).fold(project.tempo, f64::min).max(20.0);
    let delay = |d: &Device| match d.effect {
        Effect::Delay(p) if d.enabled => p.time_beats * 60.0 / slowest,
        _ => 0.0,
    };
    let mut longest = 0f64;
    for track in &project.tracks {
        longest = track.devices.iter().map(delay).fold(longest, f64::max);
        if let InstrumentKind::Wavetable(p) | InstrumentKind::Polychrome(p) = &track.instrument {
            longest = p.fx.iter().flatten().map(delay).fold(longest, f64::max);
        }
    }
    for devices in project.returns.iter().map(|r| &r.devices).chain([&project.master.devices]) {
        longest = devices.iter().map(delay).fold(longest, f64::max);
    }
    longest + 0.25
}

pub fn render(
    commands: Vec<Command>,
    range: (f64, f64),
    song: Song,
    sample_rate: f32,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Option<(Vec<f32>, Vec<f32>)> {
    render_mapped(commands, range, song, sample_rate, cancel, progress, &mut Vec::new())
}

/// [`render`], noting in `map` where the transport was (beat) at frames along
/// the way: with tempo automation, where each beat fell in the file.
pub fn render_mapped(
    commands: Vec<Command>,
    (start, end): (f64, f64),
    song: Song,
    sample_rate: f32,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
    map: &mut Vec<(f64, usize)>,
) -> Option<(Vec<f32>, Vec<f32>)> {
    let tempo = song.tempo;
    let mut renderer = OfflineRenderer::new(sample_rate);
    for command in commands {
        renderer.send(command);
    }
    for command in [
        Command::SetMetronome(false),
        Command::SetLoop { enabled: false, start: 0.0, end: 0.0 },
        Command::SetOutputLimit(false),
        Command::BackToArrangement,
        Command::SetPosition(preroll(start)),
        Command::Play,
    ] {
        renderer.send(command);
    }
    let chunk = (sample_rate as usize / 10).max(1);
    // From a few bars before (a region of the song): what sounds into it, held
    // notes and the effects' tails, is in it. That part is then dropped.
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while renderer.state.position() < start - 1e-9 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let frames = ((start - renderer.state.position()) * 60.0 / tempo * sample_rate as f64).ceil() as usize;
        renderer.render(frames.clamp(1, chunk), &mut left, &mut right);
    }

    // Until the transport reaches the end: with tempo automation the song is as
    // long as its tempos make it. Near the end, in steps the size of what is
    // left at `tempo` (the last few samples land on the end).
    let frames = ((end - start) * 60.0 / tempo * sample_rate as f64).ceil() as usize;
    let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
    // However slow it gets (20 BPM), it ends.
    let most = ((end - start) * 3.0 * sample_rate as f64).ceil() as usize + chunk;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let at = renderer.state.position();
        map.push((at, left.len()));
        if at >= end - 1e-9 || left.len() >= most {
            break;
        }
        let left_frames = ((end - at) * 60.0 / tempo * sample_rate as f64).ceil() as usize;
        renderer.render(chunk.min(left_frames.max(1)), &mut left, &mut right);
        progress(((renderer.state.position() - start) / (end - start)).clamp(0.0, 1.0) as f32);
    }
    let frames = left.len();

    // Let notes release and reverbs/delays ring out until the output falls silent.
    renderer.send(Command::Stop);
    let max_tail = (MAX_TAIL_SECONDS * sample_rate as f64) as usize;
    // Silence long enough to be the end, not the gap before a delay's next echo.
    let quiet_needed = ((song.ring_out.max(0.5) * sample_rate as f64) as usize).min(max_tail);
    let (mut tail, mut quiet) = (0, 0);
    while tail < max_tail && quiet < quiet_needed {
        let from = left.len();
        renderer.render(chunk, &mut left, &mut right);
        let peak = left[from..].iter().chain(&right[from..]).fold(0f32, |p, s| p.max(s.abs()));
        quiet = if peak < SILENCE { quiet + chunk } else { 0 };
        tail += chunk;
    }
    // Trim the silent end, keeping a few milliseconds.
    let keep = left.iter().zip(&right).rposition(|(l, r)| l.abs().max(r.abs()) >= SILENCE).map_or(frames, |i| i + 1);
    let keep = (keep + (sample_rate as usize / 100)).max(frames).min(left.len());
    left.truncate(keep);
    right.truncate(keep);
    Some((left, right))
}

pub fn peak(left: &[f32], right: &[f32]) -> f32 {
    left.iter().chain(right).fold(0f32, |p, s| p.max(s.abs()))
}

fn db(gain: f32) -> f32 {
    if gain > 0.0 { 20.0 * gain.log10() } else { f32::NEG_INFINITY }
}

/// Writes a stereo WAV applying `gain`. Returns how many samples had to be clipped.
pub fn write(path: &Path, left: &[f32], right: &[f32], sample_rate: u32, bits: BitDepth, gain: f32) -> Result<usize, String> {
    let (bits_per_sample, sample_format) = match bits {
        BitDepth::Int16 => (16, hound::SampleFormat::Int),
        BitDepth::Int24 => (24, hound::SampleFormat::Int),
        BitDepth::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: 2, sample_rate, bits_per_sample, sample_format };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|e| format!("No se pudo crear {}: {e}", path.display()))?;
    let mut clipped = 0;
    // TPDF dither for 16-bit, from a small xorshift generator.
    let mut seed: u32 = 0x1234_5678;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for (&l, &r) in left.iter().zip(right) {
        for sample in [l * gain, r * gain] {
            let result = match bits {
                BitDepth::Float32 => writer.write_sample(sample),
                BitDepth::Int16 | BitDepth::Int24 => {
                    let full_scale = if bits == BitDepth::Int16 { 32_767.0 } else { 8_388_607.0 };
                    let dither = if bits == BitDepth::Int16 { (noise() - noise()) / 32_768.0 } else { 0.0 };
                    let value = sample + dither;
                    if value.abs() > 1.0 {
                        clipped += 1;
                    }
                    writer.write_sample((value.clamp(-1.0, 1.0) * full_scale).round() as i32)
                }
            };
            result.map_err(|e| e.to_string())?;
        }
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(clipped)
}

/// Adds `frames` of silence at the end of a WAV written by [`write`].
fn pad(path: &Path, frames: usize, bits: BitDepth) -> Result<(), String> {
    let mut writer = hound::WavWriter::append(path).map_err(|e| format!("No se pudo alargar {}: {e}", path.display()))?;
    for _ in 0..frames * 2 {
        match bits {
            BitDepth::Float32 => writer.write_sample(0f32),
            BitDepth::Int16 | BitDepth::Int24 => writer.write_sample(0i32),
        }
        .map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())
}

/// A running export.
pub struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<Report, String>>>,
}

impl Job {
    pub fn start(renders: Vec<Render>, range: (f64, f64), song: Song, settings: Settings) -> Self {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared_progress, shared_cancel) = (progress.clone(), cancel.clone());
        let handle = std::thread::spawn(move || run(renders, range, song, settings, &shared_progress, &shared_cancel));
        Self { progress, cancel, handle: Some(handle) }
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The result once the job has finished.
    pub fn finished(&mut self) -> Option<Result<Report, String>> {
        if !self.handle.as_ref()?.is_finished() {
            return None;
        }
        let handle = self.handle.take()?;
        Some(handle.join().unwrap_or_else(|_| Err("La exportación falló inesperadamente".into())))
    }
}

/// Renders at once on as many threads as the computer has cores: each render
/// has its own engine. Not with plugins, which aren't all safe to run on
/// several threads at the same time.
fn threads(renders: &[Render]) -> usize {
    if renders.iter().any(|r| r.plugins) {
        return 1;
    }
    std::thread::available_parallelism().map_or(1, |n| n.get()).min(renders.len()).max(1)
}

/// Renders each of `renders` (on [`threads`] threads) and hands it to `each`
/// as it finishes, with what it was. The results in the order of `renders`, or
/// the first error: a cancellation, or one `each` returned (which stops the rest).
pub fn render_each<T: Send>(
    renders: Vec<Render>,
    range: (f64, f64),
    song: Song,
    sample_rate: f32,
    progress: &AtomicU32,
    cancel: &AtomicBool,
    each: impl Fn(&Render, Vec<f32>, Vec<f32>) -> Result<T, String> + Sync,
) -> Result<Vec<T>, String> {
    const CANCELLED: &str = "Cancelado";
    let count = renders.len().max(1);
    let threads = threads(&renders);
    // How far each render has got, 0..1, to add up into the progress.
    let done: Vec<AtomicU32> = (0..count).map(|_| AtomicU32::new(0)).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let queue: Vec<std::sync::Mutex<Option<Render>>> = renders.into_iter().map(|r| std::sync::Mutex::new(Some(r))).collect();
    let results: Vec<std::sync::Mutex<Option<Result<T, String>>>> = (0..queue.len()).map(|_| std::sync::Mutex::new(None)).collect();
    let work = || loop {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(mut render) = queue.get(index).and_then(|r| r.lock().ok()?.take()) else { return };
        let step = |p: f32| {
            done[index].store(p.to_bits(), Ordering::Relaxed);
            let total: f32 = done.iter().map(|d| f32::from_bits(d.load(Ordering::Relaxed))).sum();
            progress.store((total / count as f32).to_bits(), Ordering::Relaxed);
        };
        let commands = std::mem::take(&mut render.commands);
        let result = match self::render(commands, range, song, sample_rate, cancel, &step) {
            None => Err(CANCELLED.to_string()),
            Some((left, right)) => each(&render, left, right),
        };
        let failed = result.is_err();
        if let Ok(mut slot) = results[index].lock() {
            *slot = Some(result);
        }
        if failed {
            cancel.store(true, Ordering::Relaxed);
            return;
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads {
            scope.spawn(work);
        }
        work();
    });
    let mut out = Vec::new();
    let mut error: Option<String> = None;
    for result in results.into_iter().map(|r| r.into_inner().ok().flatten()) {
        match result {
            Some(Ok(value)) => out.push(value),
            // What went wrong, rather than the cancellation it caused.
            Some(Err(e)) if e != CANCELLED => error = error.filter(|e| e != CANCELLED).or(Some(e)),
            _ => error = error.or(Some(CANCELLED.to_string())),
        }
    }
    match error {
        Some(e) => Err(e),
        None => {
            progress.store(1f32.to_bits(), Ordering::Relaxed);
            Ok(out)
        }
    }
}

fn run(
    renders: Vec<Render>,
    range: (f64, f64),
    song: Song,
    settings: Settings,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Report, String> {
    let written = render_each(renders, range, song, settings.sample_rate as f32, progress, cancel, |render, left, right| {
        let peak = peak(&left, &right);
        // Stems keep their relative levels, so only the full mix is normalized.
        let gain = match settings.normalize {
            Some(target) if !settings.stems && peak > 0.0 => 10f32.powf(target / 20.0) / peak,
            _ => 1.0,
        };
        let clipped = write(&render.path, &left, &right, settings.sample_rate, settings.bits, gain)?;
        Ok((render.path.clone(), left.len(), peak, clipped))
    })
    .map_err(|e| if e == "Cancelado" { "Exportación cancelada".to_string() } else { e })?;
    let mut report = Report::default();
    let mut peak_overall = 0f32;
    let mut lengths = Vec::new();
    for (path, frames, peak, clipped) in written {
        peak_overall = peak_overall.max(peak);
        report.clipped += clipped;
        report.seconds = report.seconds.max(frames as f64 / settings.sample_rate as f64);
        lengths.push(frames);
        report.files.push(path);
    }
    // Stems as long as the longest (each ends with its own tail), so they line up
    // end to end too in another program.
    let longest = lengths.iter().copied().max().unwrap_or(0);
    for (path, &frames) in report.files.iter().zip(&lengths) {
        if frames < longest {
            pad(path, longest - frames, settings.bits)?;
        }
    }
    report.peak_db = db(peak_overall);
    progress.store(1f32.to_bits(), Ordering::Relaxed);
    Ok(report)
}

/// Headless export for `pulse --render <out.wav> [project.pulse]`: renders the whole
/// song (the demo if no project is given) as 24-bit, 48 kHz and prints a report.
pub fn render_from_command_line(output: &Path, project: Option<&Path>) -> Result<String, String> {
    let project = match project {
        Some(path) => Project::load(path)?,
        None => Project::demo(),
    };
    let settings = Settings::default();
    let range = beat_range(&project, settings.range).ok_or("El Arrangement está vacío")?;
    let mut samples = SampleCache::default();
    let renders = plan(&project, &mut samples, &settings, output);
    if let Some(error) = samples.errors.first() {
        eprintln!("{error}");
    }
    let progress = AtomicU32::new(0);
    let report = run(renders, range, Song::of(&project), settings, &progress, &AtomicBool::new(false))?;
    Ok(report.summary())
}

// ----- Dialog ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Dialog {
    pub open: bool,
    pub settings: Settings,
    job: Option<Job>,
}

impl PulseApp {
    fn start_export(&mut self) {
        let settings = self.export.settings;
        let Some(range) = beat_range(&self.project, settings.range) else {
            self.status = Some("El Arrangement está vacío: coloca clips en él (o copia clips de la sesión) para exportar".into());
            return;
        };
        let name = format!("{}.wav", self.file_name());
        let dialog = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_directory(crate::library::folder(crate::library::EXPORTS)).set_file_name(&name);
        let Some(destination) = crate::files::save_file(dialog) else { return };
        self.capture_plugins();
        // The renders load plugin instances of their own; the live ones stay as they are.
        let live = std::mem::take(&mut self.samples.plugins);
        let renders = plan(&self.project, &mut self.samples, &settings, &destination);
        self.samples.plugins = live;
        self.export.job = Some(Job::start(renders, range, Song::of(&self.project), settings));
    }
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if let Some(job) = &mut app.export.job {
        if let Some(result) = job.finished() {
            app.status = Some(match result {
                Ok(report) => report.summary(),
                Err(error) => error,
            });
            app.export.job = None;
            app.export.open = false;
        }
    }
    if !app.export.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Exportar audio")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            if let Some(job) = &app.export.job {
                ui.label("Renderizando…");
                ui.add(egui::ProgressBar::new(job.progress()).show_percentage().desired_width(320.0));
                if ui.button("Cancelar").clicked() {
                    job.cancel();
                }
                return;
            }
            let s = &mut app.export.settings;
            egui::Grid::new("export").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Qué");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.stems, false, "Mezcla completa");
                    ui.radio_value(&mut s.stems, true, "Una pista por archivo (stems)");
                });
                ui.end_row();
                ui.label("Rango");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.range, Range::Song, "Toda la canción");
                    ui.radio_value(&mut s.range, Range::Loop, "Región del loop");
                });
                ui.end_row();
                ui.label("Formato");
                ui.horizontal(|ui| {
                    for bits in [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32] {
                        ui.radio_value(&mut s.bits, bits, bits.label());
                    }
                });
                ui.end_row();
                ui.label("Frecuencia");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.sample_rate, 44_100, "44.1 kHz");
                    ui.radio_value(&mut s.sample_rate, 48_000, "48 kHz");
                });
                ui.end_row();
                ui.label("Normalizar");
                ui.add_enabled_ui(!s.stems, |ui| {
                    ui.horizontal(|ui| {
                        let mut on = s.normalize.is_some();
                        ui.checkbox(&mut on, "Pico a");
                        let mut target = s.normalize.unwrap_or(-1.0);
                        ui.add_enabled(on, egui::DragValue::new(&mut target).range(-24.0..=0.0).speed(0.1).suffix(" dBFS"));
                        s.normalize = on.then_some(target);
                    });
                });
                ui.end_row();
                if s.stems {
                    ui.label("Master");
                    ui.checkbox(&mut s.stems_master, "Con la cadena del master").on_hover_text("Sin ella, los stems sumados dan la mezcla (sin los efectos del master)");
                    ui.end_row();
                }
            });
            if let Some((start, end)) = beat_range(&app.project, app.export.settings.range) {
                let seconds = (end - start) * 60.0 / app.project.tempo;
                ui.label(
                    egui::RichText::new(format!(
                        "{:.0} compases · {:.1} s, más la cola de los efectos. La automatización se exporta tal como está escrita.",
                        (end - start) / 4.0,
                        seconds
                    ))
                    .color(TEXT_DIM)
                    .size(10.5),
                );
            }
            if app.export.settings.stems {
                ui.label(
                    egui::RichText::new(if app.export.settings.stems_master {
                        "Cada pista que suena se exporta en solo, con sus envíos de reverb y delay, su grupo y la cadena del master."
                    } else {
                        "Cada pista que suena se exporta en solo, con sus envíos de reverb y delay y su grupo, sin los efectos del master: sumados dan la mezcla."
                    })
                        .color(TEXT_DIM)
                        .size(10.5),
                );
            }
            ui.add_space(6.0);
            if ui.button("Exportar…").clicked() {
                app.start_export();
            }
        });
    if !open {
        if let Some(job) = &app.export.job {
            job.cancel();
        }
        app.export.open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_demo(settings: &Settings) -> (Vec<Render>, (f64, f64), f64) {
        let project = Project::demo();
        let mut samples = SampleCache::default();
        let range = beat_range(&project, settings.range).unwrap();
        let renders = plan(&project, &mut samples, settings, Path::new("/tmp/pulse-demo.wav"));
        (renders, range, project.tempo)
    }

    /// Exporting the loop region: a note held from before it sounds from its
    /// first sample (the render started at the region, after every note began).
    #[test]
    fn a_region_has_what_sounds_into_it() {
        use engine::model::{ArrangementClip, Clip, InstrumentKind, MidiClip, Note};
        let mut project = Project::empty();
        let mut track = crate::project::Track::new("Pad", [0, 0, 0], InstrumentKind::SuperSaw(Default::default()));
        let mut clip = MidiClip::new("A", 16.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 16.0, pitch: 57, velocity: 0.8 });
        track.arrangement.push(ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(clip) });
        project.tracks = vec![track];
        (project.loop_start, project.loop_end) = (8.0, 12.0);
        let range = beat_range(&project, Range::Loop).unwrap();
        let commands = sync::project_commands(&project, &mut SampleCache::default(), 48_000.0, &HashSet::new());
        let (left, _) = render(commands, range, Song::of(&project), 48_000.0, &AtomicBool::new(false), &|_| {}).unwrap();
        let first = left[..480].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(first > 0.05, "the held note from the first 10 ms: {first}");
        // Two seconds of region, its tail aside.
        assert!(left.len() >= 2 * 48_000 && left.len() < 2 * 48_000 + 10 * 48_000);
    }

    /// A delay with echoes two seconds apart: the export waits for them instead of
    /// stopping at the first half second of silence.
    #[test]
    fn a_long_delays_echoes_are_in_the_tail() {
        use engine::model::{ArrangementClip, Clip, DelayParams, Device, Effect, InstrumentKind, MidiClip, Note};
        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut track = crate::project::Track::new("Synth", [0, 0, 0], InstrumentKind::Synth(Default::default()));
        let mut clip = MidiClip::new("A", 1.0, [0, 0, 0]);
        clip.notes.push(Note { start: 0.0, length: 0.25, pitch: 60, velocity: 1.0 });
        track.arrangement.push(ArrangementClip { start: 0.0, length: 1.0, clip: Clip::Midi(clip) });
        // 4 beats at 120: an echo every 2 s.
        track.devices.push(Device::new(Effect::Delay(DelayParams { time_beats: 4.0, feedback: 0.5, mix: 0.5, ping_pong: false, ..Default::default() })));
        project.tracks = vec![track];
        assert!((ring_out(&project) - 2.25).abs() < 1e-9);
        let mut samples = SampleCache::default();
        let commands = sync::project_commands(&project, &mut samples, 48_000.0, &HashSet::new());
        let (left, _) = render(commands, (0.0, 1.0), Song::of(&project), 48_000.0, &AtomicBool::new(false), &|_| {}).unwrap();
        let level = |from: f64, to: f64| left[(from * 48_000.0) as usize..((to * 48_000.0) as usize).min(left.len())].iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(left.len() > 6 * 48_000, "{} s", left.len() as f64 / 48_000.0);
        assert!(level(1.9, 2.4) > 0.01 && level(3.9, 4.4) > 0.005, "the echoes at 2 and 4 s");
    }

    /// With tempo automation the song lasts what its tempos make it: 4 beats at
    /// 120 BPM and 4 at 60 are 2 + 4 seconds.
    #[test]
    fn a_song_with_tempo_automation_renders_its_real_length() {
        let mut project = Project::demo();
        let mut lane = engine::model::AutomationLane::new(engine::model::Param::Tempo);
        for (beat, value) in [(0.0, 120.0), (3.99, 120.0), (4.0, 60.0)] {
            lane.points.push(engine::model::AutomationPoint { beat, value, curve: 0.0 });
        }
        project.master.automation.push(lane);
        project.tempo = 120.0;
        let mut samples = SampleCache::default();
        let commands = sync::project_commands(&project, &mut samples, 48_000.0, &HashSet::new());
        let (left, _) = render(commands, (0.0, 8.0), Song::of(&project), 48_000.0, &AtomicBool::new(false), &|_| {}).unwrap();
        let seconds = left.len() as f64 / 48_000.0;
        assert!(seconds >= 5.95, "{seconds}");
    }

    #[test]
    fn the_demo_song_renders_with_its_tail_and_without_problems() {
        let settings = Settings::default();
        let (mut renders, range, tempo) = render_demo(&settings);
        let render = renders.remove(0);
        let cancel = AtomicBool::new(false);
        let (left, right) = super::render(render.commands, range, Song { tempo, ring_out: 0.5 }, 48_000.0, &cancel, &|_| {}).unwrap();

        let song_frames = (range.1 - range.0) * 60.0 / tempo * 48_000.0;
        assert!(left.len() as f64 >= song_frames, "the whole song is rendered");
        assert!((left.len() as f64) < song_frames + 11.0 * 48_000.0, "the tail is bounded");
        assert!(left.iter().chain(&right).all(|s| s.is_finite()));

        // Every bar of the demo has something playing.
        let bar = (4.0 * 60.0 / tempo * 48_000.0) as usize;
        for (i, chunk) in left.chunks(bar).take(8).enumerate() {
            let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt();
            assert!(rms > 0.01, "bar {} is silent (rms {rms})", i + 1);
        }
        let peak = peak(&left, &right);
        assert!(peak > 0.1 && peak < 4.0, "unexpected peak {peak}");
    }

    /// Each stem ends with its own tail; written, they all last as long as the longest.
    #[test]
    fn stems_are_written_all_the_same_length() {
        let dir = std::env::temp_dir().join(format!("pulse-stem-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = Settings { stems: true, range: Range::Loop, bits: BitDepth::Int16, ..Default::default() };
        let project = Project::demo();
        let mut samples = SampleCache::default();
        let range = beat_range(&project, settings.range).unwrap();
        let renders = plan(&project, &mut samples, &settings, &dir.join("demo.wav"));
        let report = run(renders, range, Song::of(&project), settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
        let lengths: Vec<u32> = report.files.iter().map(|f| hound::WavReader::open(f).unwrap().duration()).collect();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(lengths.len(), 4);
        assert!(lengths.iter().all(|&n| n == lengths[0]), "{lengths:?}");
        assert!(lengths[0] as f64 >= (range.1 - range.0) * 60.0 / project.tempo * 48_000.0);
    }

    #[test]
    fn stems_solo_each_track() {
        let settings = Settings { stems: true, range: Range::Loop, ..Default::default() };
        let (renders, _, tempo) = render_demo(&settings);
        assert_eq!(renders.len(), 4);
        assert!(renders[0].path.to_string_lossy().ends_with("pulse-demo - 1 Drums.wav"));
        // The bass only plays from bar 5: its stem is silent over the first four bars.
        let bass = renders.into_iter().nth(1).unwrap();
        let cancel = AtomicBool::new(false);
        let (left, _) = super::render(bass.commands, (0.0, 16.0), Song { tempo, ring_out: 0.5 }, 48_000.0, &cancel, &|_| {}).unwrap();
        let first_bars = (16.0 * 60.0 / tempo * 48_000.0) as usize;
        assert!(left[..first_bars].iter().all(|s| s.abs() < 1e-3), "only the bass should be in its stem");
    }

    /// The demo with a group around keys and lead (compressed), a ducker on the
    /// bass listening to the drums, and a compressor on the master listening to
    /// the bass: everything that makes one track's stem depend on another.
    fn tangled_demo() -> Project {
        use engine::model::{CompressorParams, Device, DuckerParams, Effect};
        let mut project = Project::demo();
        let mut group = crate::project::Track::new_group("Grupo", [0, 0, 0]);
        group.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -24.0, ratio: 6.0, ..Default::default() })));
        project.tracks.insert(2, group);
        project.tracks[3].depth = 1;
        project.tracks[4].depth = 1;
        project.assign_ids();
        let drums = project.tracks[0].id;
        project.tracks[1].devices.push(Device::new(Effect::Ducker(DuckerParams { depth: 0.9, sidechain: Some(drums), ..Default::default() })));
        let bass = project.tracks[1].id;
        project.master.devices.push(Device::new(Effect::Compressor(CompressorParams { threshold_db: -30.0, ratio: 4.0, sidechain: Some(bass), ..Default::default() })));
        project
    }

    /// What each stem keeps of the project: itself, its groups and what is
    /// listened to; the rest is left empty.
    #[test]
    fn a_stem_keeps_only_what_it_can_hear() {
        let project = tangled_demo();
        let kept = |t: usize| -> Vec<usize> {
            let only = only(&project, t);
            (0..only.tracks.len()).filter(|&i| only.tracks[i] == project.tracks[i]).collect()
        };
        // The master listens to the bass, the bass to the drums: in every stem.
        assert_eq!(kept(0), [0, 1]);
        assert_eq!(kept(1), [0, 1]);
        assert_eq!(kept(3), [0, 1, 2, 3], "chords: its group too");
        assert_eq!(kept(4), [0, 1, 2, 4], "lead: not the chords beside it");
        // The emptied ones stay where they were, with their ids and groups.
        let only = only(&project, 0);
        assert!(only.tracks[2].is_group() && only.tracks[2].devices.is_empty());
        assert_eq!((only.tracks[3].depth, only.tracks[3].id), (1, project.tracks[3].id));
        assert!(only.tracks[3].arrangement.is_empty() && only.tracks[3].clips.iter().all(|c| c.is_none()));
    }

    /// Rendered with only what it hears, each stem is the same, sample for
    /// sample, as rendered with the whole project.
    #[test]
    fn stems_sound_the_same_with_only_what_they_hear() {
        let project = tangled_demo();
        let range = (8.0, 24.0);
        let song = Song::of(&project);
        let mut samples = SampleCache::default();
        let settings = Settings { stems: true, ..Default::default() };
        let renders = plan(&project, &mut samples, &settings, Path::new("/tmp/pulse-iguales.wav"));
        assert_eq!(renders.iter().map(|r| r.track.unwrap()).collect::<Vec<_>>(), [0, 1, 3, 4]);
        for render in renders {
            let t = render.track.unwrap();
            let mut whole = sync::project_commands(&project, &mut samples, 48_000.0, &HashSet::new());
            whole.extend((0..MAX_TRACKS).map(|other| Command::SetSolo { track: other, solo: other == t }));
            let cancel = AtomicBool::new(false);
            let (l1, r1) = super::render(whole, range, song, 48_000.0, &cancel, &|_| {}).unwrap();
            let (l2, r2) = super::render(render.commands, range, song, 48_000.0, &cancel, &|_| {}).unwrap();
            assert!(peak(&l1, &r1) > 0.01, "track {t} sounds");
            assert_eq!(l1.len(), l2.len(), "track {t}");
            let difference = l1.iter().zip(&l2).chain(r1.iter().zip(&r2)).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(difference == 0.0, "track {t}: differs by {difference}");
        }
    }

    /// Stems render on several threads and come out in order, all of them.
    #[test]
    fn stems_render_side_by_side() {
        let dir = std::env::temp_dir().join(format!("pulse-stems-hilos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = tangled_demo();
        let settings = Settings { stems: true, range: Range::Loop, ..Default::default() };
        let range = beat_range(&project, settings.range).unwrap();
        let renders = plan(&project, &mut SampleCache::default(), &settings, &dir.join("demo.wav"));
        assert!(threads(&renders) > 1 || std::thread::available_parallelism().map_or(1, |n| n.get()) == 1);
        let progress = AtomicU32::new(0);
        let report = run(renders, range, Song::of(&project), settings, &progress, &AtomicBool::new(false)).unwrap();
        let names: Vec<String> = report.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["demo - 1 Drums.wav", "demo - 2 Bass.wav", "demo - 4 Chords.wav", "demo - 5 Lead.wav"]);
        assert_eq!(f32::from_bits(progress.load(Ordering::Relaxed)), 1.0);
        // Cancelled, it says so and stops all of them.
        let renders = plan(&project, &mut SampleCache::default(), &settings, &dir.join("otra.wav"));
        let error = run(renders, range, Song::of(&project), settings, &AtomicU32::new(0), &AtomicBool::new(true)).unwrap_err();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(error, "Exportación cancelada");
    }

    #[test]
    fn written_files_have_the_requested_format_and_normalization() {
        let dir = std::env::temp_dir().join(format!("pulse-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let left: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.25).collect();
        let right = left.clone();
        let target_gain = 10f32.powf(-1.0 / 20.0) / peak(&left, &right);
        for bits in [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32] {
            let path = dir.join(format!("{bits:?}.wav"));
            let clipped = write(&path, &left, &right, 44_100, bits, target_gain).unwrap();
            assert_eq!(clipped, 0);
            let reader = hound::WavReader::open(&path).unwrap();
            assert_eq!(reader.spec().sample_rate, 44_100);
            assert_eq!(reader.len(), 9600);
            let sample = engine::Sample::load(&path).unwrap();
            let p = peak(&sample.left, &sample.right);
            assert!((db(p) + 1.0).abs() < 0.05, "{bits:?} peak {} dBFS", db(p));
        }
        // A hot signal is reported as clipped in integer formats.
        let clipped = write(&dir.join("hot.wav"), &[1.5, 0.2], &[0.2, 0.2], 44_100, BitDepth::Int24, 1.0).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(clipped, 1);
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "diagnostic: prints the peak of each demo stem"]
    fn demo_stem_peaks() {
        let project = Project::demo();
        let settings = Settings { stems: true, ..Default::default() };
        let range = beat_range(&project, settings.range).unwrap();
        let renders = plan(&project, &mut SampleCache::default(), &settings, Path::new("/tmp/x.wav"));
        for render in renders {
            let name = render.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let (l, r) = super::render(render.commands, range, Song::of(&project), 48_000.0, &AtomicBool::new(false), &|_| {}).unwrap();
            let rms = (l.iter().chain(&r).map(|s| s * s).sum::<f32>() / (l.len() * 2) as f32).sqrt();
            println!("{name:12} pico {:6.1} dBFS  rms {:6.1} dBFS", db(peak(&l, &r)), db(rms));
        }
    }

    #[test]
    #[ignore = "diagnostic: writes every kick designer preset, one per second, to $PULSE_OUT"]
    fn kick_presets_to_wav() {
        let out = std::env::var("PULSE_OUT").unwrap();
        let mut audio = Vec::new();
        for (name, preset) in engine::model::KickParams::presets() {
            let mut hit = engine::render_kick(preset, None, 48_000.0, 120.0);
            let peak = peak(&hit, &hit);
            println!("{name:14} {:5.0} ms  pico {:6.1} dBFS", engine::kick_length_ms(&preset), db(peak));
            hit.resize(96_000, 0.0);
            audio.extend(hit);
        }
        crate::capture::write_wav(Path::new(&out), &audio, &audio, 48_000.0).unwrap();
    }
}

#[cfg(test)]
mod playback {
    use super::*;
    use crate::project::{Track, PALETTE};
    use engine::model::{Clip, InstrumentKind, MidiClip, Note, SynthParams};
    use std::sync::atomic::Ordering;

    /// A track added like `PulseApp::add_track`, holding a C major chord.
    fn project_with_new_chord_track() -> (Project, usize) {
        let mut project = Project::demo();
        let index = project.tracks.len();
        let mut track = Track::new(format!("{} MIDI", index + 1), PALETTE[1], InstrumentKind::Synth(SynthParams::default()));
        let mut clip = MidiClip::new("MIDI", 4.0, PALETTE[1]);
        for pitch in [60, 64, 67] {
            clip.notes.push(Note { start: 0.0, length: 2.0, pitch, velocity: 0.8 });
        }
        track.clips[0] = Some(Clip::Midi(clip));
        project.tracks.push(track);
        (project, index)
    }

    #[test]
    fn a_new_synth_track_plays_its_chord_clip() {
        let (project, index) = project_with_new_chord_track();
        let mut renderer = OfflineRenderer::new(48_000.0);
        for command in sync::project_commands(&project, &mut SampleCache::default(), 48_000.0, &HashSet::new()) {
            renderer.send(command);
        }
        renderer.send(Command::LaunchClip { track: index, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render(24_000, &mut l, &mut r);
        let state = renderer.state.clone();
        let playing = state.tracks[index].playing.load(Ordering::Relaxed);
        let track_peak = state.tracks[index].peak.take();
        assert_eq!(playing, 0);
        assert!(track_peak > 0.01, "the new track should sound");
    }

    /// Renders `project` for `frames` with clip 0 of `track` launched; returns the track's peak.
    fn track_peak(project: &Project, track: usize, frames: usize) -> f32 {
        let mut renderer = OfflineRenderer::new(48_000.0);
        for command in sync::project_commands(project, &mut SampleCache::default(), 48_000.0, &HashSet::new()) {
            renderer.send(command);
        }
        renderer.send(Command::LaunchClip { track, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render(frames, &mut l, &mut r);
        renderer.state.tracks[track].peak.take()
    }

    fn clip_with(pitches: &[u8]) -> Clip {
        let mut clip = MidiClip::new("MIDI", 4.0, PALETTE[1]);
        for (i, &pitch) in pitches.iter().enumerate() {
            clip.notes.push(Note { start: i as f64 * 0.25, length: 0.25, pitch, velocity: 1.0 });
        }
        Clip::Midi(clip)
    }

    #[test]
    fn a_drum_rack_with_the_basic_kit_plays_its_pads() {
        let dir = std::env::temp_dir().join(format!("pulse-rack-{}", std::process::id()));
        let (params, files) = crate::kits::basic_kit(&dir).unwrap();
        let mut project = Project::empty();
        let mut track = Track::new("Rack", PALETTE[1], InstrumentKind::DrumRack(params));
        track.clips[0] = Some(clip_with(&[36, 38, 42]));
        project.tracks = vec![track.clone()];
        assert_eq!(track_peak(&project, 0, 24_000), 0.0, "no samples, no sound");

        track.samples = files;
        project.tracks = vec![track];
        let peak = track_peak(&project, 0, 24_000);
        std::fs::remove_dir_all(&dir).ok();
        assert!(peak > 0.1 && peak < 2.0, "kick, snare and hat should sound: {peak}");
    }

    #[test]
    fn a_sampler_plays_a_file_chromatically() {
        let dir = std::env::temp_dir().join(format!("pulse-sampler-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let tone: Vec<f32> = (0..44_100).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.5).collect();
        crate::capture::write_wav(&path, &tone, &tone, 44_100.0).unwrap();

        let mut project = Project::empty();
        let mut track = Track::new("Keys", PALETTE[1], InstrumentKind::Sampler(Default::default()));
        track.clips[0] = Some(clip_with(&[60, 64, 67]));
        track.set_sample(0, Some(path.to_string_lossy().into_owned()));
        project.tracks = vec![track];
        let peak = track_peak(&project, 0, 24_000);
        std::fs::remove_dir_all(&dir).ok();
        assert!(peak > 0.2 && peak < 1.5, "the sampler should sound: {peak}");
    }

    #[test]
    fn a_sliced_loop_replays_like_the_original() {
        // Four hits of different loudness, one per beat at 120 BPM.
        let rate = 48_000.0;
        let mut audio = vec![0.0f32; 96_000];
        for (k, amp) in [0.9f32, 0.3, 0.6, 0.15].into_iter().enumerate() {
            for i in 0..12_000 {
                let t = i as f32 / rate;
                audio[k * 24_000 + i] += amp * (t * 180.0 * std::f32::consts::TAU).sin() * (-t / 0.04).exp();
            }
        }
        let sample = engine::Sample::from_channels(audio.clone(), audio.clone(), rate);
        let dir = std::env::temp_dir().join(format!("pulse-slice-play-{}", std::process::id()));
        let mode = crate::slicing::SliceMode::Transients;
        let (sliced, _) = crate::slicing::slice(&sample, (0.0, 2.0), 120.0, mode, "Loop", &dir, PALETTE[1]).unwrap();
        assert_eq!(sliced.clip.notes.len(), 4);

        let mut project = Project::empty();
        project.tempo = 120.0;
        let mut track = Track::new("Slices", PALETTE[1], InstrumentKind::DrumRack(sliced.params));
        track.samples = sliced.files;
        track.clips[0] = Some(Clip::Midi(sliced.clip));
        project.tracks = vec![track];
        let mut renderer = OfflineRenderer::new(rate);
        for command in sync::project_commands(&project, &mut SampleCache::default(), rate, &HashSet::new()) {
            renderer.send(command);
        }
        renderer.send(Command::LaunchClip { track: 0, slot: 0 });
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render(96_000, &mut l, &mut r);
        std::fs::remove_dir_all(&dir).ok();

        // Each beat keeps its hit's loudness. The first hit is left out: the channel
        // gain ramps up from silence when playback starts.
        let peak = |x: &[f32]| x.iter().fold(0f32, |m, s| m.max(s.abs()));
        let original: Vec<f32> = audio.chunks(24_000).map(peak).collect();
        let replayed: Vec<f32> = l.chunks(24_000).map(peak).collect();
        assert!(replayed[0] > 0.1);
        for k in 1..4 {
            let (a, b) = (original[k] / original[2], replayed[k] / replayed[2]);
            assert!((a - b).abs() < 0.03, "beat {k}: original {a:.2}, sliced {b:.2}");
        }
    }

    #[test]
    fn a_kick_designer_track_plays_every_preset() {
        for (name, preset) in engine::model::KickParams::presets() {
            let mut project = Project::empty();
            let mut track = Track::new("Kick", PALETTE[1], InstrumentKind::Kick(preset));
            track.clips[0] = Some(clip_with(&[60, 60, 60, 60]));
            project.tracks = vec![track];
            let peak = track_peak(&project, 0, 24_000);
            assert!(peak > 0.1 && peak < 1.5, "{name}: peak {peak}");
        }
    }
}

