//! Rendering the arrangement to WAV files: the full mix or one file per track (stems).
//!
//! Rendering runs on a background thread with its own offline engine, faster than
//! real time and independent of the audio device. The output is not clipped while
//! rendering, so the true peak can be reported and the mix normalized.

use crate::app::PulseApp;
use crate::project::Project;
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
}

impl Default for Settings {
    fn default() -> Self {
        Self { range: Range::Song, stems: false, bits: BitDepth::Int24, sample_rate: 48_000, normalize: None }
    }
}

/// One file to render: its path and the commands that set the engine up for it.
pub struct Render {
    pub path: PathBuf,
    pub commands: Vec<Command>,
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
        return vec![Render { path: destination.to_path_buf(), commands: commands(samples) }];
    }
    let base = destination.file_stem().map_or("Pulse".into(), |s| s.to_string_lossy().into_owned());
    let folder = destination.parent().unwrap_or(Path::new("."));
    project
        .tracks
        .iter()
        .enumerate()
        .map(|(t, track)| {
            let mut list = commands(samples);
            // Soloing one track keeps its sends, so its reverb and delay are part of the stem.
            list.extend((0..MAX_TRACKS).map(|other| Command::SetSolo { track: other, solo: other == t }));
            list.push(Command::SetMute { target: engine::Target::Track(t), mute: false });
            let name = format!("{base} - {} {}.wav", t + 1, sanitize(&track.name));
            Render { path: folder.join(name), commands: list }
        })
        .collect()
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect()
}

/// Renders `[start, end)` beats plus the effect tail. `progress` receives 0..1.
pub fn render(
    commands: Vec<Command>,
    (start, end): (f64, f64),
    tempo: f64,
    sample_rate: f32,
    cancel: &AtomicBool,
    progress: &dyn Fn(f32),
) -> Option<(Vec<f32>, Vec<f32>)> {
    let mut renderer = OfflineRenderer::new(sample_rate);
    for command in commands {
        renderer.send(command);
    }
    for command in [
        Command::SetMetronome(false),
        Command::SetLoop { enabled: false, start: 0.0, end: 0.0 },
        Command::SetOutputLimit(false),
        Command::BackToArrangement,
        Command::SetPosition(start),
        Command::Play,
    ] {
        renderer.send(command);
    }

    let frames = ((end - start) * 60.0 / tempo * sample_rate as f64).ceil() as usize;
    let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
    let chunk = (sample_rate as usize / 10).max(1);
    while left.len() < frames {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        renderer.render(chunk.min(frames - left.len()), &mut left, &mut right);
        progress(left.len() as f32 / frames as f32);
    }

    // Let notes release and reverbs/delays ring out until the output falls silent.
    renderer.send(Command::Stop);
    let max_tail = (MAX_TAIL_SECONDS * sample_rate as f64) as usize;
    let quiet_needed = sample_rate as usize / 2;
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

/// A running export.
pub struct Job {
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<Report, String>>>,
}

impl Job {
    pub fn start(renders: Vec<Render>, range: (f64, f64), tempo: f64, settings: Settings) -> Self {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared_progress, shared_cancel) = (progress.clone(), cancel.clone());
        let handle = std::thread::spawn(move || run(renders, range, tempo, settings, &shared_progress, &shared_cancel));
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

fn run(
    renders: Vec<Render>,
    range: (f64, f64),
    tempo: f64,
    settings: Settings,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Report, String> {
    let count = renders.len().max(1);
    let mut report = Report::default();
    let mut peak_overall = 0f32;
    for (index, Render { path, commands }) in renders.into_iter().enumerate() {
        let step = |p: f32| progress.store(((index as f32 + p) / count as f32).to_bits(), Ordering::Relaxed);
        let Some((left, right)) = render(commands, range, tempo, settings.sample_rate as f32, cancel, &step) else {
            return Err("Exportación cancelada".into());
        };
        let peak = peak(&left, &right);
        peak_overall = peak_overall.max(peak);
        // Stems keep their relative levels, so only the full mix is normalized.
        let gain = match settings.normalize {
            Some(target) if !settings.stems && peak > 0.0 => 10f32.powf(target / 20.0) / peak,
            _ => 1.0,
        };
        report.clipped += write(&path, &left, &right, settings.sample_rate, settings.bits, gain)?;
        report.seconds = report.seconds.max(left.len() as f64 / settings.sample_rate as f64);
        report.files.push(path);
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
    let report = run(renders, range, project.tempo, settings, &progress, &AtomicBool::new(false))?;
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
        let dialog = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name(&name);
        let Some(destination) = dialog.save_file() else { return };
        let renders = plan(&self.project, &mut self.samples, &settings, &destination);
        self.export.job = Some(Job::start(renders, range, self.project.tempo, settings));
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
                    egui::RichText::new("Cada pista se exporta en solo, con sus envíos de reverb y delay y la cadena del master.")
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

    #[test]
    fn the_demo_song_renders_with_its_tail_and_without_problems() {
        let settings = Settings::default();
        let (mut renders, range, tempo) = render_demo(&settings);
        let render = renders.remove(0);
        let cancel = AtomicBool::new(false);
        let (left, right) = super::render(render.commands, range, tempo, 48_000.0, &cancel, &|_| {}).unwrap();

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

    #[test]
    fn stems_solo_each_track() {
        let settings = Settings { stems: true, range: Range::Loop, ..Default::default() };
        let (renders, _, tempo) = render_demo(&settings);
        assert_eq!(renders.len(), 4);
        assert!(renders[0].path.to_string_lossy().ends_with("pulse-demo - 1 Drums.wav"));
        // The bass only plays from bar 5: its stem is silent over the first four bars.
        let bass = renders.into_iter().nth(1).unwrap();
        let cancel = AtomicBool::new(false);
        let (left, _) = super::render(bass.commands, (0.0, 16.0), tempo, 48_000.0, &cancel, &|_| {}).unwrap();
        let first_bars = (16.0 * 60.0 / tempo * 48_000.0) as usize;
        assert!(left[..first_bars].iter().all(|s| s.abs() < 1e-3), "only the bass should be in its stem");
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
        for (render, track) in renders.into_iter().zip(&project.tracks) {
            let (l, r) = super::render(render.commands, range, project.tempo, 48_000.0, &AtomicBool::new(false), &|_| {}).unwrap();
            let rms = (l.iter().chain(&r).map(|s| s * s).sum::<f32>() / (l.len() * 2) as f32).sqrt();
            println!("{:8} pico {:6.1} dBFS  rms {:6.1} dBFS", track.name, db(peak(&l, &r)), db(rms));
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
