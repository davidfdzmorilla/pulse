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

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Range {
    Song,
    Loop,
    /// One file for each part of the song between its markers.
    Sections,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
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

/// The kind of file written.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Format {
    #[default]
    Wav,
    /// Lossless and about half the size (16 or 24 bits).
    Flac,
    /// For listening and sharing (CBR at `Settings::mp3_kbps`).
    Mp3,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Wav, Format::Flac, Format::Mp3];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Format::Wav => "WAV",
            Format::Flac => "FLAC",
            Format::Mp3 => "MP3",
        }
    }
}

/// The export dialog's choices; the last ones are kept in the app's settings.
#[derive(Clone, Copy, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    pub format: Format,
    /// MP3's bit rate (kbps).
    pub mp3_kbps: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { range: Range::Song, stems: false, bits: BitDepth::Int24, sample_rate: 48_000, normalize: None, stems_master: true, format: Format::Wav, mp3_kbps: 320 }
    }
}

/// One file to render: its path and the commands that set the engine up for it.
pub struct Render {
    pub path: PathBuf,
    pub commands: Vec<Command>,
    /// The track a stem is of (none for the mix).
    pub track: Option<usize>,
    /// The beats it plays, and what it needs of the song.
    pub range: (f64, f64),
    pub song: Song,
    /// Files of the same group (the stems of one section of one version) are
    /// padded to the same length.
    pub group: usize,
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

/// The beat range to render, or `None` if there is nothing on the arrangement
/// (for [`Range::Sections`], the whole song).
pub fn beat_range(project: &Project, range: Range) -> Option<(f64, f64)> {
    match range {
        Range::Loop => (project.loop_end > project.loop_start).then_some((project.loop_start, project.loop_end)),
        Range::Song | Range::Sections => {
            let end = project.tracks.iter().map(|t| t.arrangement_end()).fold(0.0, f64::max);
            (end > 0.0).then_some((0.0, end))
        }
    }
}

/// The parts of the song between its markers, named after them: from each
/// marker to the next, the last to the end of the song. What comes before the
/// first marker is «Inicio». Empty if the song is, or has no markers in it.
pub fn sections(project: &Project) -> Vec<(String, (f64, f64))> {
    let Some((_, end)) = beat_range(project, Range::Song) else { return Vec::new() };
    let mut markers: Vec<_> = project.markers.iter().filter(|m| m.beat < end - 1e-9).collect();
    markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    if markers.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if markers[0].beat > 1e-9 {
        out.push(("Inicio".to_string(), (0.0, markers[0].beat)));
    }
    for (i, marker) in markers.iter().enumerate() {
        let to = markers.get(i + 1).map_or(end, |next| next.beat);
        // Two markers in the same place: the part is the second's.
        if to > marker.beat + 1e-9 {
            let name = if marker.name.trim().is_empty() { format!("Sección {}", i + 1) } else { marker.name.clone() };
            out.push((name, (marker.beat.max(0.0), to)));
        }
    }
    out
}

/// `destination` with `label` added to its name: «Tema.wav» → «Tema - Drop.wav».
fn labelled(destination: &Path, label: &str) -> PathBuf {
    let base = destination.file_stem().map_or("Pulse".into(), |s| s.to_string_lossy().into_owned());
    destination.with_file_name(format!("{base} - {}.wav", sanitize(label)))
}

/// Builds the renders for the settings: the mix, or one solo render per track,
/// of the song, the loop, or each section.
pub fn plan(
    project: &Project,
    samples: &mut SampleCache,
    settings: &Settings,
    destination: &Path,
) -> Vec<Render> {
    plan_from(project, samples, settings, destination, 0)
}

/// [`plan`], numbering its groups from `group`.
fn plan_from(project: &Project, samples: &mut SampleCache, settings: &Settings, destination: &Path, group: usize) -> Vec<Render> {
    let parts: Vec<(Option<String>, (f64, f64))> = match settings.range {
        Range::Sections => sections(project).into_iter().enumerate().map(|(i, (name, range))| (Some(format!("{} {name}", i + 1)), range)).collect(),
        range => beat_range(project, range).map(|r| (None, r)).into_iter().collect(),
    };
    let mut renders = Vec::new();
    for (k, (label, range)) in parts.into_iter().enumerate() {
        let destination = label.map_or_else(|| destination.to_path_buf(), |l| labelled(destination, &l));
        renders.extend(plan_part(project, samples, settings, &destination, range, group + k));
    }
    renders
}

/// The versions to export besides what there is now: each one's mix (or
/// stems), named after it.
pub fn plan_versions(
    versions: &[(String, Project)],
    samples: &mut SampleCache,
    settings: &Settings,
    destination: &Path,
    group: usize,
) -> Vec<Render> {
    let mut renders = Vec::new();
    for (name, project) in versions {
        let first = group + renders.iter().map(|r: &Render| r.group + 1).max().unwrap_or(0);
        renders.extend(plan_from(project, samples, settings, &labelled(destination, name), first));
    }
    renders
}

fn plan_part(project: &Project, samples: &mut SampleCache, settings: &Settings, destination: &Path, range: (f64, f64), group: usize) -> Vec<Render> {
    let sample_rate = settings.sample_rate as f32;
    let song = Song::of(project);
    // Exports always play the automation as written, ignoring knobs moved by hand.
    if !settings.stems {
        let commands = sync::project_commands(project, samples, sample_rate, &HashSet::new());
        return vec![Render { path: destination.to_path_buf(), commands, track: None, range, song, group, plugins: uses_plugins(project) }];
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
            Render { path: folder.join(name), commands: list, track: Some(t), range, song, group, plugins: uses_plugins(&only) }
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

/// Where a file to be encoded is first written: beside it, hidden.
fn temporary(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(|| "export".into(), |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.tmp.wav"))
}

/// Encodes the temporary WAVs to the settings' format, a few at a time, and
/// removes them. The files written; `clipped` counts the samples over full scale.
fn encode_all(temporaries: Vec<PathBuf>, settings: &Settings, clipped: &mut usize, cancel: &AtomicBool) -> Result<Vec<PathBuf>, String> {
    let finals: Vec<PathBuf> = temporaries.iter().map(|t| {
        let name = t.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        t.with_file_name(name.trim_start_matches('.').trim_end_matches(".tmp.wav"))
    }).collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<usize, String>>>> = temporaries.iter().map(|_| std::sync::Mutex::new(None)).collect();
    // FLAC holds a file's frames in memory until it is written: not too many at once.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4).min(temporaries.len()).max(1);
    let work = || loop {
        let i = next.fetch_add(1, Ordering::Relaxed);
        let Some(temporary) = temporaries.get(i) else { return };
        let result = if cancel.load(Ordering::Relaxed) { Err("Exportación cancelada".to_string()) } else { encode(temporary, &finals[i], settings) };
        if let Ok(mut slot) = results[i].lock() {
            *slot = Some(result);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads {
            scope.spawn(work);
        }
        work();
    });
    for temporary in &temporaries {
        let _ = std::fs::remove_file(temporary);
    }
    for result in results {
        *clipped += result.into_inner().ok().flatten().unwrap_or_else(|| Err("Exportación cancelada".into()))?;
    }
    Ok(finals)
}

/// Encodes a 32-bit float WAV to FLAC or MP3 at `to`. How many samples were
/// over full scale (clipped).
fn encode(from: &Path, to: &Path, settings: &Settings) -> Result<usize, String> {
    let reader = hound::WavReader::open(from).map_err(|e| format!("No se pudo leer {}: {e}", from.display()))?;
    let rate = reader.spec().sample_rate;
    let mut samples = reader.into_samples::<f32>().map(|s| s.unwrap_or(0.0));
    let mut clipped = 0;
    let bytes = match settings.format {
        Format::Wav => return Ok(0),
        Format::Flac => {
            let bits = if settings.bits == BitDepth::Int16 { 16 } else { 24 };
            let mut source = FloatSource { samples: &mut samples, bits, rate, clipped: 0, seed: 0x1234_5678, buffer: Vec::new() };
            use flacenc::error::Verify;
            let config = flacenc::config::Encoder::default().into_verified().map_err(|(_, e)| format!("FLAC: {e}"))?;
            let block = config.block_size;
            let stream = flacenc::encode_with_fixed_block_size(&config, &mut source, block).map_err(|e| format!("No se pudo codificar en FLAC: {e}"))?;
            clipped = source.clipped;
            let mut sink = flacenc::bitsink::ByteSink::new();
            use flacenc::component::BitRepr;
            stream.write(&mut sink).map_err(|e| format!("No se pudo codificar en FLAC: {e:?}"))?;
            let mut bytes = sink.into_inner();
            // Every block but the last is `block` long: STREAMINFO's smallest
            // block size says so, as libFLAC writes it (flacenc puts the last
            // one's there, and readers then take the stream for variable).
            if bytes.len() > 12 && &bytes[..4] == b"fLaC" && bytes[4] & 0x7f == 0 {
                let largest = [bytes[10], bytes[11]];
                bytes[8..10].copy_from_slice(&largest);
            }
            bytes
        }
        Format::Mp3 => {
            let mut encoder = rusty_mp3::Mp3Encoder::new(rusty_mp3::Mp3EncoderConfig { bitrate_kbps: settings.mp3_kbps, vbr_quality: None });
            let mut out = Vec::new();
            let mut chunk = Vec::with_capacity(2 * 4096);
            loop {
                chunk.clear();
                chunk.extend(samples.by_ref().take(2 * 4096).map(|s| {
                    if s.abs() > 1.0 {
                        clipped += 1;
                    }
                    s.clamp(-1.0, 1.0)
                }));
                if chunk.is_empty() {
                    break;
                }
                encoder.push_pcm_f32(&chunk, 2, rate).map_err(|e| format!("No se pudo codificar en MP3: {e:?}"))?;
                while let Ok(packet) = encoder.next_packet() {
                    out.extend_from_slice(&packet);
                }
            }
            encoder.finish();
            while let Ok(packet) = encoder.next_packet() {
                out.extend_from_slice(&packet);
            }
            out
        }
    };
    crate::store::write_atomic(to, &bytes)?;
    Ok(clipped)
}

/// Float samples as FLAC's integers, with TPDF dither at 16 bits (as WAV).
struct FloatSource<'a, I: Iterator<Item = f32>> {
    samples: &'a mut I,
    bits: usize,
    rate: u32,
    clipped: usize,
    seed: u32,
    buffer: Vec<i32>,
}

impl<I: Iterator<Item = f32>> flacenc::source::Source for FloatSource<'_, I> {
    fn channels(&self) -> usize {
        2
    }

    fn bits_per_sample(&self) -> usize {
        self.bits
    }

    fn sample_rate(&self) -> usize {
        self.rate as usize
    }

    fn read_samples<F: flacenc::source::Fill>(&mut self, block_size: usize, dest: &mut F) -> Result<usize, flacenc::error::SourceError> {
        let full_scale = if self.bits == 16 { 32_767.0 } else { 8_388_607.0 };
        self.buffer.clear();
        for sample in self.samples.by_ref().take(2 * block_size) {
            let dither = if self.bits == 16 {
                let mut noise = || {
                    self.seed ^= self.seed << 13;
                    self.seed ^= self.seed >> 17;
                    self.seed ^= self.seed << 5;
                    self.seed as f32 / u32::MAX as f32
                };
                (noise() - noise()) / 32_768.0
            } else {
                0.0
            };
            let value = sample + dither;
            if value.abs() > 1.0 {
                self.clipped += 1;
            }
            self.buffer.push((value.clamp(-1.0, 1.0) * full_scale).round() as i32);
        }
        dest.fill_interleaved(&self.buffer)?;
        Ok(self.buffer.len() / 2)
    }
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
    pub fn start(renders: Vec<Render>, settings: Settings) -> Self {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (shared_progress, shared_cancel) = (progress.clone(), cancel.clone());
        let handle = std::thread::spawn(move || run(renders, settings, &shared_progress, &shared_cancel));
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
        let result = match self::render(commands, render.range, render.song, sample_rate, cancel, &step) {
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
    settings: Settings,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<Report, String> {
    // FLAC and MP3 are written as WAV (32-bit float) first, so stems can be
    // padded to the same length, then encoded.
    let format = settings.format;
    let written_as = |path: &Path| match format {
        Format::Wav => path.with_extension("wav"),
        _ => temporary(&path.with_extension(format.extension())),
    };
    let bits = if format == Format::Wav { settings.bits } else { BitDepth::Float32 };
    let written = render_each(renders, settings.sample_rate as f32, progress, cancel, |render, left, right| {
        let peak = peak(&left, &right);
        // Stems keep their relative levels, so only the full mix is normalized.
        let gain = match settings.normalize {
            Some(target) if !settings.stems && peak > 0.0 => 10f32.powf(target / 20.0) / peak,
            _ => 1.0,
        };
        let path = written_as(&render.path);
        let clipped = write(&path, &left, &right, settings.sample_rate, bits, gain)?;
        Ok((path, render.group, left.len(), peak, clipped))
    })
    .map_err(|e| if e == "Cancelado" { "Exportación cancelada".to_string() } else { e })?;
    let mut report = Report::default();
    let mut peak_overall = 0f32;
    let mut lengths = Vec::new();
    for (path, group, frames, peak, clipped) in written {
        peak_overall = peak_overall.max(peak);
        report.clipped += clipped;
        report.seconds = report.seconds.max(frames as f64 / settings.sample_rate as f64);
        lengths.push((group, frames));
        report.files.push(path);
    }
    // Stems as long as the longest of theirs (each ends with its own tail), so
    // they line up end to end too in another program.
    for (path, &(group, frames)) in report.files.iter().zip(&lengths) {
        let longest = lengths.iter().filter(|(g, _)| *g == group).map(|&(_, n)| n).max().unwrap_or(0);
        if frames < longest {
            pad(path, longest - frames, bits)?;
        }
    }
    if format != Format::Wav {
        report.clipped = 0;
        report.files = encode_all(std::mem::take(&mut report.files), &settings, &mut report.clipped, cancel)?;
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
    beat_range(&project, settings.range).ok_or("El Arrangement está vacío")?;
    let mut samples = SampleCache::default();
    let renders = plan(&project, &mut samples, &settings, output);
    if let Some(error) = samples.errors.first() {
        eprintln!("{error}");
    }
    let progress = AtomicU32::new(0);
    let report = run(renders, settings, &progress, &AtomicBool::new(false))?;
    Ok(report.summary())
}

// ----- Dialog ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Dialog {
    pub open: bool,
    pub settings: Settings,
    /// Saved versions (their files) to export too, each named after itself.
    pub versions: std::collections::BTreeSet<String>,
    /// Only those versions, not what there is now.
    pub only_versions: bool,
    job: Option<Job>,
    /// The settings were taken from the app's (see `window`).
    loaded: bool,
}

impl PulseApp {
    /// What the export dialog's settings make: what there is now and the
    /// versions chosen, to files beside `destination`.
    pub(crate) fn export_renders(&mut self, destination: &Path) -> Result<Vec<Render>, String> {
        let settings = self.export.settings;
        let mut versions = Vec::new();
        if !self.export.versions.is_empty() {
            let dir = self.versions_dir()?;
            for version in crate::versions::list(&dir).into_iter().filter(|v| self.export.versions.contains(&v.file)) {
                versions.push((version.name.clone(), crate::versions::load(&dir, &version)?));
            }
        }
        let now = !self.export.only_versions || versions.is_empty();
        if now {
            if beat_range(&self.project, settings.range).is_none() {
                return Err("El Arrangement está vacío: coloca clips en él (o copia clips de la sesión) para exportar".into());
            }
            if settings.range == Range::Sections && sections(&self.project).is_empty() {
                return Err("No hay marcadores en la canción: añade alguno (doble clic en la franja de marcadores) para exportar por secciones".into());
            }
        }
        self.capture_plugins();
        // The renders load plugin instances of their own; the live ones stay as they are.
        let live = std::mem::take(&mut self.samples.plugins);
        let mut renders = if now { plan(&self.project, &mut self.samples, &settings, destination) } else { Vec::new() };
        let next = renders.iter().map(|r| r.group + 1).max().unwrap_or(0);
        renders.extend(plan_versions(&versions, &mut self.samples, &settings, destination, next));
        self.samples.plugins = live;
        if renders.is_empty() {
            return Err("Esas versiones no tienen nada que exportar en ese rango".into());
        }
        Ok(renders)
    }

    fn start_export(&mut self) {
        let format = self.export.settings.format;
        let name = format!("{}.{}", self.file_name(), format.extension());
        let dialog = rfd::FileDialog::new().add_filter(format.label(), &[format.extension()]).set_directory(crate::library::folder(crate::library::EXPORTS)).set_file_name(&name);
        let Some(destination) = crate::files::save_file(dialog) else { return };
        match self.export_renders(&destination) {
            Ok(renders) => self.export.job = Some(Job::start(renders, self.export.settings)),
            Err(e) => self.status = Some(e),
        }
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
    // The choices made last time.
    if !app.export.loaded {
        app.export.settings = app.settings.export;
        app.export.loaded = true;
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
                    ui.radio_value(&mut s.range, Range::Sections, "Por secciones").on_hover_text("Un archivo por cada parte de la canción, de un marcador al siguiente");
                });
                ui.end_row();
                ui.label("Formato");
                ui.horizontal(|ui| {
                    for format in Format::ALL {
                        ui.radio_value(&mut s.format, format, format.label()).on_hover_text(match format {
                            Format::Wav => "Sin pérdida, el de siempre",
                            Format::Flac => "Sin pérdida y la mitad de grande (16 o 24 bits)",
                            Format::Mp3 => "Para escuchar y compartir",
                        });
                    }
                });
                ui.end_row();
                if s.format == Format::Mp3 {
                    ui.label("Calidad");
                    ui.horizontal(|ui| {
                        for kbps in [192, 256, 320] {
                            ui.radio_value(&mut s.mp3_kbps, kbps, format!("{kbps} kbps"));
                        }
                    });
                } else {
                    ui.label("Bits");
                    ui.horizontal(|ui| {
                        for bits in [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32] {
                            if bits == BitDepth::Float32 && s.format == Format::Flac {
                                continue;
                            }
                            ui.radio_value(&mut s.bits, bits, bits.label());
                        }
                    });
                }
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
            versions_list(app, ui);
            if app.export.settings.range == Range::Sections {
                let parts = sections(&app.project);
                let text = if parts.is_empty() {
                    "No hay marcadores en la canción: añade alguno (doble clic en la franja de marcadores).".to_string()
                } else {
                    let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
                    format!("{} secciones: {}. Cada una con lo que suena en ella desde antes y su cola.", parts.len(), names.join(", "))
                };
                ui.label(egui::RichText::new(text).color(TEXT_DIM).size(10.5));
            } else if let Some((start, end)) = beat_range(&app.project, app.export.settings.range) {
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
    if app.export.settings != app.settings.export {
        let mut next = app.settings.clone();
        next.export = app.export.settings;
        app.set_settings(next);
    }
    if !open {
        if let Some(job) = &app.export.job {
            job.cancel();
        }
        app.export.open = false;
    }
}

/// The saved versions to export too, when the project has any.
fn versions_list(app: &mut PulseApp, ui: &mut egui::Ui) {
    let Ok(dir) = app.versions_dir() else { return };
    // Named ones: the ones made on saving are too many to choose from.
    let versions: Vec<_> = crate::versions::list(&dir).into_iter().filter(|v| !v.auto).collect();
    let d = &mut app.export;
    d.versions.retain(|f| versions.iter().any(|v| &v.file == f));
    if versions.is_empty() {
        return;
    }
    ui.add_space(4.0);
    egui::CollapsingHeader::new(format!("Versiones ({} marcadas)", d.versions.len())).id_salt("export_versions").show(ui, |ui| {
        ui.label(egui::RichText::new("Las marcadas se exportan también, cada una con su nombre al final del archivo.").color(TEXT_DIM).size(10.5));
        for v in versions.iter().rev() {
            let mut on = d.versions.contains(&v.file);
            if ui.checkbox(&mut on, &v.name).changed() {
                if on {
                    d.versions.insert(v.file.clone());
                } else {
                    d.versions.remove(&v.file);
                }
            }
        }
        ui.add_enabled(!d.versions.is_empty(), egui::Checkbox::new(&mut d.only_versions, "Solo las versiones (no lo de ahora)"));
    });
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
        let report = run(renders, settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
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
        let renders = plan(&project, &mut SampleCache::default(), &settings, &dir.join("demo.wav"));
        assert!(threads(&renders) > 1 || std::thread::available_parallelism().map_or(1, |n| n.get()) == 1);
        let progress = AtomicU32::new(0);
        let report = run(renders, settings, &progress, &AtomicBool::new(false)).unwrap();
        let names: Vec<String> = report.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["demo - 1 Drums.wav", "demo - 2 Bass.wav", "demo - 4 Chords.wav", "demo - 5 Lead.wav"]);
        assert_eq!(f32::from_bits(progress.load(Ordering::Relaxed)), 1.0);
        // Cancelled, it says so and stops all of them.
        let renders = plan(&project, &mut SampleCache::default(), &settings, &dir.join("otra.wav"));
        let error = run(renders, settings, &AtomicU32::new(0), &AtomicBool::new(true)).unwrap_err();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(error, "Exportación cancelada");
    }

    fn marked(project: &mut Project, marks: &[(f64, &str)]) {
        project.markers = marks.iter().map(|&(beat, name)| crate::project::SongMarker { beat, name: name.into() }).collect();
    }

    /// The song's parts, from marker to marker, the last to the end; what is
    /// before the first is «Inicio», and markers past the end are left out.
    #[test]
    fn sections_go_from_marker_to_marker() {
        let mut project = Project::demo();
        let (_, end) = beat_range(&project, Range::Song).unwrap();
        assert!(sections(&project).is_empty(), "no markers, no sections");
        marked(&mut project, &[(16.0, "Drop"), (4.0, "Intro"), (end + 8.0, "Después"), (16.0, "")]);
        let parts = sections(&project);
        assert_eq!(parts, [("Inicio".to_string(), (0.0, 4.0)), ("Intro".to_string(), (4.0, 16.0)), ("Sección 3".to_string(), (16.0, end))]);
    }

    /// Exported by sections, each is a file of its own, as long as its part
    /// (plus its tail); its stems are as long as each other, not as the others'.
    #[test]
    fn each_section_is_a_file() {
        let dir = std::env::temp_dir().join(format!("pulse-secciones-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut project = Project::demo();
        marked(&mut project, &[(0.0, "Intro"), (16.0, "Drop")]);
        let (_, end) = beat_range(&project, Range::Song).unwrap();
        let seconds = |beats: f64| beats * 60.0 / project.tempo;
        let settings = Settings { range: Range::Sections, bits: BitDepth::Int16, ..Default::default() };
        let report = run(plan(&project, &mut SampleCache::default(), &settings, &dir.join("Tema.wav")), settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
        let names: Vec<String> = report.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Tema - 1 Intro.wav", "Tema - 2 Drop.wav"]);
        let length = |f: &Path| hound::WavReader::open(f).unwrap().duration() as f64 / 48_000.0;
        let (intro, drop) = (length(&report.files[0]), length(&report.files[1]));
        assert!(intro >= seconds(16.0) && intro < seconds(16.0) + 10.0, "{intro}");
        assert!(drop >= seconds(end - 16.0) && drop < seconds(end - 16.0) + 10.0, "{drop}");
        // Stems by section: «Tema - 1 Intro - 1 Drums.wav»…, the same length within a section.
        let settings = Settings { stems: true, ..settings };
        let report = run(plan(&project, &mut SampleCache::default(), &settings, &dir.join("Tema.wav")), settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
        let files: Vec<(String, f64)> = report.files.iter().map(|f| (f.file_name().unwrap().to_string_lossy().into_owned(), length(f))).collect();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(files.len(), 8);
        assert_eq!(files[0].0, "Tema - 1 Intro - 1 Drums.wav");
        assert_eq!(files[4].0, "Tema - 2 Drop - 1 Drums.wav");
        assert!(files[..4].iter().all(|f| f.1 == files[0].1) && files[4..].iter().all(|f| f.1 == files[4].1), "{files:?}");
    }

    /// Versions chosen in the dialog are exported too, each as it was (its own
    /// tempo) and named after itself; or only them.
    #[test]
    fn chosen_versions_are_exported_too() {
        let root = std::env::temp_dir().join(format!("pulse-versiones-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("Tema").join("Tema.pulse");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut app = PulseApp::headless(Project::demo());
        app.file = Some(file);
        app.project.tempo = 100.0;
        app.save_version("Lenta", "");
        app.project.tempo = 140.0;
        let dir = app.versions_dir().unwrap();
        let lenta = crate::versions::list(&dir)[0].file.clone();
        app.export.settings = Settings { bits: BitDepth::Int16, ..Default::default() };
        app.export.versions.insert(lenta);
        let out = root.join("Exportado.wav");
        let renders = app.export_renders(&out).unwrap();
        let report = run(renders, app.export.settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
        let names: Vec<String> = report.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Exportado.wav", "Exportado - Lenta.wav"]);
        let length = |f: &Path| hound::WavReader::open(f).unwrap().duration() as f64;
        let (now, slow) = (length(&report.files[0]), length(&report.files[1]));
        // The same song at 100 instead of 140 BPM (the tails aside).
        assert!((slow / now - 1.4).abs() < 0.1, "{slow} / {now}");
        app.export.only_versions = true;
        assert_eq!(app.export_renders(&out).unwrap().len(), 1);
        // Sections with no markers: said, nothing done.
        app.export.only_versions = false;
        app.export.settings.range = Range::Sections;
        assert!(app.export_renders(&out).err().unwrap().contains("marcadores"));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Band energies (dB) of a mono mix: below 200 Hz, 200 Hz–5 kHz, above 5 kHz.
    fn bands(sample: &engine::Sample) -> [f32; 3] {
        let rate = sample.sample_rate;
        let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(l, r)| 0.5 * (l + r)).collect();
        let one_pole = |cutoff: f32, x: &[f32]| -> Vec<f32> {
            let a = (-std::f32::consts::TAU * cutoff / rate).exp();
            let mut y = 0.0;
            x.iter().map(|&v| { y = v + a * (y - v); y }).collect()
        };
        let low = one_pole(200.0, &one_pole(200.0, &mono));
        let below5k = one_pole(5000.0, &one_pole(5000.0, &mono));
        let high: Vec<f32> = mono.iter().zip(&below5k).map(|(m, l)| m - l).collect();
        let mid: Vec<f32> = below5k.iter().zip(&low).map(|(a, b)| a - b).collect();
        let db = |x: &[f32]| 10.0 * (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32 + 1e-20).log10();
        [db(&low), db(&mid), db(&high)]
    }

    /// FLAC holds the same samples as a WAV at its bits; MP3 sounds like it
    /// (each band within a dB), as long; nothing temporary is left behind.
    #[test]
    fn flac_and_mp3_are_written_too() {
        let dir = std::env::temp_dir().join(format!("pulse-formatos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = Project::demo();
        let export = |format: Format| {
            let settings = Settings { range: Range::Loop, format, ..Default::default() };
            let renders = plan(&project, &mut SampleCache::default(), &settings, &dir.join(format!("Tema.{}", format.extension())));
            let report = run(renders, settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
            assert_eq!(report.files.len(), 1);
            assert_eq!(report.files[0].extension().unwrap(), format.extension());
            engine::Sample::load(&report.files[0]).unwrap()
        };
        let wav = export(Format::Wav);
        let flac = export(Format::Flac);
        let mp3 = export(Format::Mp3);
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        std::fs::remove_dir_all(&dir).ok();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(sorted, ["Tema.flac", "Tema.mp3", "Tema.wav"], "no temporary files: {names:?}");
        assert_eq!(flac.frames(), wav.frames());
        let difference = wav.left.iter().zip(&flac.left).chain(wav.right.iter().zip(&flac.right)).fold(0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(difference < 1e-6, "FLAC at 24 bits is the WAV at 24 bits: {difference}");
        let seconds = |s: &engine::Sample| s.duration();
        assert!((seconds(&mp3) - seconds(&wav)).abs() < 0.1, "{} vs {}", seconds(&mp3), seconds(&wav));
        let (a, b) = (bands(&wav), bands(&mp3));
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1.0, "band {k}: WAV {:.1} dB, MP3 {:.1} dB", a[k], b[k]);
        }
    }

    /// Stems in FLAC are as long as each other, as in WAV.
    #[test]
    fn flac_stems_line_up() {
        let dir = std::env::temp_dir().join(format!("pulse-stems-flac-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = Project::demo();
        let settings = Settings { range: Range::Loop, stems: true, format: Format::Flac, bits: BitDepth::Int16, ..Default::default() };
        let report = run(plan(&project, &mut SampleCache::default(), &settings, &dir.join("Tema.flac")), settings, &AtomicU32::new(0), &AtomicBool::new(false)).unwrap();
        let lengths: Vec<usize> = report.files.iter().map(|f| engine::Sample::load(f).unwrap().frames()).collect();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(lengths.len(), 4);
        assert!(lengths.iter().all(|&n| n == lengths[0]), "{lengths:?}");
        assert!(report.files[0].to_string_lossy().ends_with("Tema - 1 Drums.flac"));
    }

    /// The dialog opens with the choices made last time, and changing them
    /// keeps them; settings from before have the defaults.
    #[test]
    fn the_export_choices_are_remembered() {
        let mut app = PulseApp::headless(Project::demo());
        app.settings.export = Settings { format: Format::Mp3, mp3_kbps: 256, stems: true, ..Default::default() };
        app.export.open = true;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, &ui.ctx().clone()));
        assert_eq!(app.export.settings.format, Format::Mp3);
        assert_eq!(app.export.settings.mp3_kbps, 256);
        app.export.settings.range = Range::Sections;
        let _ = ctx.run_ui(Default::default(), |ui| window(&mut app, &ui.ctx().clone()));
        assert_eq!(app.settings.export.range, Range::Sections);
        let text = serde_json::to_string(&app.settings).unwrap();
        let back: crate::settings::Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(back.export, app.settings.export);
        let old: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.export, Settings::default());
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

