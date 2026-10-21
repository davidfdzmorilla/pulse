//! Freezing a track: what its instrument and effects make, rendered to audio, so
//! it plays that instead (a heavy plugin stops costing CPU). Its mixer (volume,
//! pan, sends) stays live. One file for the arrangement, from its start with the
//! effects' tail, and one per session clip, as a loop.
//!
//! While frozen its instrument, effects and clips are kept as they were (the
//! engine just doesn't get them): unfreezing brings them back. Changing them
//! unfreezes it, so a frozen track never plays stale audio. «Convertir en audio»
//! turns it into an audio track playing those files (Live's Flatten).

use crate::app::PulseApp;
use crate::project::{Project, Track, TrackKind};
use engine::model::{ArrangementClip, AudioClip, AutomationLane, Clip, InstrumentKind, Param, SynthParams, WarpMode};
use engine::{Command, Target, NUM_SCENES};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// A frozen track's audio, and what it was frozen from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frozen {
    /// The arrangement as heard (empty if it had no clips).
    #[serde(default)]
    pub arrangement: Vec<ArrangementClip>,
    /// Each session clip as heard, looping.
    #[serde(default)]
    pub clips: Vec<Option<AudioClip>>,
    /// The track's content when frozen (see [`content`]): changed, it unfreezes.
    pub content: u64,
}

/// What a freeze bakes in: the track's instrument, effects, MIDI effects,
/// clips, samples and the automation that isn't the mixer's; the song's tempo,
/// swing and key; and the tracks its sidechains listen to. Hashed with FNV-1a
/// (the same in every build). Opening a project takes its frozen tracks as
/// they are (see [`refresh_frozen`]), so a new Pulse writing these a little
/// differently doesn't unfreeze them.
pub fn content(project: &Project, t: usize) -> u64 {
    let Some(track) = project.tracks.get(t) else { return 0 };
    let own = |track: &Track| {
        let lanes: Vec<&AutomationLane> = track.automation.iter().filter(|l| !mixer_param(l.param)).collect();
        serde_json::to_string(&(&track.instrument, &track.devices, &track.midi_effects, &track.clips, &track.arrangement, &track.samples, lanes)).unwrap_or_default()
    };
    let tempo: Vec<&AutomationLane> = project.master.automation.iter().filter(|l| l.param == Param::Tempo).collect();
    let mut text = own(track);
    text += &serde_json::to_string(&(project.tempo, project.swing, project.swing_grid, &project.key, tempo)).unwrap_or_default();
    for source in crate::export::sources(project, t) {
        text += &own(&project.tracks[source]);
    }
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| (hash ^ byte as u64).wrapping_mul(0x0100_0000_01b3))
}

/// After opening a project: its frozen tracks are taken as up to date.
pub fn refresh_frozen(project: &mut Project) {
    for t in 0..project.tracks.len() {
        if project.tracks[t].frozen.is_some() {
            let content = content(project, t);
            if let Some(frozen) = &mut project.tracks[t].frozen {
                frozen.content = content;
            }
        }
    }
}

/// Volume, pan and sends: the mixer's, which a freeze leaves live.
fn mixer_param(param: Param) -> bool {
    matches!(param, Param::Volume | Param::Pan | Param::Send(_))
}

/// What the engine plays for a track: itself, or if frozen, its frozen audio
/// through no instrument or effects.
pub fn playing(track: &Track) -> std::borrow::Cow<'_, Track> {
    let Some(frozen) = &track.frozen else { return std::borrow::Cow::Borrowed(track) };
    let mut clips: [Option<Clip>; NUM_SCENES] = Default::default();
    for (slot, clip) in frozen.clips.iter().enumerate().take(NUM_SCENES) {
        clips[slot] = clip.clone().map(Clip::Audio);
    }
    std::borrow::Cow::Owned(Track {
        instrument: InstrumentKind::Synth(SynthParams::default()),
        devices: Vec::new(),
        samples: Vec::new(),
        clips,
        arrangement: frozen.arrangement.clone(),
        automation: track.automation.iter().filter(|l| mixer_param(l.param)).cloned().collect(),
        frozen: None,
        ..track.clone()
    })
}

/// Warp markers from where the render was at each frame (`map`): each beat
/// where it was rendered. Past the song's end (its tail) the beats go on at the
/// tempo `after`, to cover `seconds`.
fn warp_markers(map: &[(f64, usize)], rate: f32, seconds: f64, after: f64) -> Vec<engine::model::WarpMarker> {
    let mut markers: Vec<engine::model::WarpMarker> = Vec::new();
    for &(beat, frame) in map {
        let time = frame as f64 / rate as f64;
        if markers.last().is_none_or(|m| beat > m.beat + 1e-9) {
            markers.push(engine::model::WarpMarker { beat, seconds: time });
        }
    }
    if markers.is_empty() {
        markers.push(engine::model::WarpMarker { beat: 0.0, seconds: 0.0 });
    }
    let last = *markers.last().unwrap();
    if seconds > last.seconds {
        markers.push(engine::model::WarpMarker { beat: last.beat + (seconds - last.seconds) * after / 60.0, seconds });
    }
    markers
}

/// A freeze being rendered in the background.
pub struct FreezeJob {
    track: usize,
    content: u64,
    /// The song changes tempo: the arrangement's audio follows it as rendered.
    tempo_changes: bool,
    /// Where the arrangement's last clip ends, in beats.
    end: f64,
    result: Arc<Mutex<Option<Result<Rendered, String>>>>,
}

/// The renders: the arrangement's (beats long) and each session clip's.
struct Rendered {
    arrangement: Option<(Vec<f32>, Vec<f32>)>,
    clips: Vec<Option<(Vec<f32>, Vec<f32>)>>,
    /// Where the arrangement's render was (beat) at frames along it.
    map: Vec<(f64, usize)>,
}

/// The project as a freeze renders track `t`: it soloed, with its mixer flat
/// (unity, centre, no sends, its lanes left out), through its groups made
/// plain (no effects, unity), the master plain but with its tempo. What its
/// sidechains listen to plays too, unheard (soloed out), with groups, their
/// tracks, and sources of sources, so it ducks as in the song. Its session
/// clips' mixer envelopes stay out: they still move the frozen clips.
fn solo_project(project: &Project, t: usize) -> Project {
    let mut render = crate::export::only(project, t);
    let flat = |track: &mut Track| {
        track.mixer.gain = 1.0;
        track.mixer.pan = 0.0;
        track.mixer.active = true;
        track.sends = Default::default();
        track.automation.retain(|l| !mixer_param(l.param));
    };
    for track in &mut render.tracks {
        track.solo = false;
        track.armed = false;
    }
    let mut at = crate::groups::parent(&render.tracks, t);
    while let Some(group) = at {
        let group_track = &mut render.tracks[group];
        flat(group_track);
        group_track.devices.clear();
        group_track.automation.clear();
        at = crate::groups::parent(&render.tracks, group);
    }
    let track = &mut render.tracks[t];
    flat(track);
    track.solo = true;
    for clip in track.clips.iter_mut().flatten() {
        match clip {
            Clip::Midi(m) => m.envelopes.retain(|l| !mixer_param(l.param)),
            Clip::Audio(a) => a.envelopes.retain(|l| !mixer_param(l.param)),
        }
    }
    for ret in &mut render.returns {
        ret.devices.clear();
        ret.automation.clear();
    }
    render.master.devices.clear();
    render.master.automation.retain(|l| l.param == Param::Tempo);
    render.master.mixer.gain = 1.0;
    render.master.mixer.pan = 0.0;
    render.quantize = 0.0;
    render
}

/// Renders session clip `slot` of the only track as a loop: two laps, the second
/// kept (its start has the end's tail, as when it loops).
fn render_loop(commands: Vec<Command>, slot: usize, beats: f64, tempo: f64, rate: f32, plugins: bool) -> (Vec<f32>, Vec<f32>) {
    // Its plugins are ended on the UI thread.
    let mut renderer = crate::export::OnUi::new(engine::OfflineRenderer::new(rate), plugins);
    for command in commands {
        renderer.send(command);
    }
    renderer.send(Command::SetOutputLimit(false));
    renderer.send(Command::SetMetronome(false));
    // With the tracks its sidechains listen to (they come after it).
    for track in 0..engine::MAX_TRACKS {
        renderer.send(Command::LaunchClip { track, slot });
    }
    let lap = (beats * 60.0 / tempo * rate as f64).round() as usize;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    renderer.render(2 * lap, &mut left, &mut right);
    (left.split_off(lap.min(left.len())), right.split_off(lap.min(right.len())))
}

/// Marks a frozen track's header: an ice-blue frame and a star.
pub fn mark(painter: &eframe::egui::Painter, rect: eframe::egui::Rect) {
    use eframe::egui::{Align2, Stroke, StrokeKind};
    painter.rect_stroke(rect.shrink(1.0), 2.0, Stroke::new(1.5, crate::theme::FROZEN), StrokeKind::Inside);
    painter.text(rect.right_top() + eframe::egui::vec2(-6.0, 4.0), Align2::RIGHT_TOP, "✱", crate::theme::small(), crate::theme::FROZEN);
}

impl PulseApp {
    pub fn is_frozen(&self, track: usize) -> bool {
        self.project.tracks.get(track).is_some_and(|t| t.frozen.is_some())
    }

    /// Starts rendering track `t` to freeze it.
    pub fn freeze(&mut self, t: usize) {
        if self.freeze_job.is_some() {
            self.status = Some("Ya se está congelando una pista".into());
            return;
        }
        let Some(track) = self.project.tracks.get(t) else { return };
        if track.is_audio() && track.devices.is_empty() {
            self.status = Some("Una pista de audio sin efectos no gasta nada: no hace falta congelarla".into());
            return;
        }
        let (tempo, rate) = (self.project.tempo, self.audio.sample_rate());
        let render = solo_project(&self.project, t);
        let tempo_changes = render.master.automation.iter().any(|l| l.param == Param::Tempo && !l.points.is_empty());
        let song = crate::export::Song::of(&render);
        // Plugins load here (with their state), the renders run in the background.
        self.capture_plugins();
        let live = std::mem::take(&mut self.samples.plugins);
        let commands = |app: &mut PulseApp| crate::sync::project_commands(&render, &mut app.samples, rate, &HashSet::new());
        let end = render.tracks[t].arrangement.iter().map(|c| c.end()).fold(0.0, f64::max);
        let arrangement = (end > 0.0).then(|| (commands(self), end));
        let clips: Vec<Option<(Vec<Command>, f64)>> = (0..NUM_SCENES).map(|s| render.tracks[t].clips[s].as_ref().map(|c| (commands(self), c.length().max(0.25)))).collect();
        self.samples.plugins = live;
        let result = Arc::new(Mutex::new(None));
        let shared = result.clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let mut map = Vec::new();
            let arrangement = arrangement.and_then(|(commands, end)| crate::export::render_mapped(commands, (0.0, end), song, rate, &cancel, &|_| {}, &mut map));
            let clips = clips.into_iter().enumerate().map(|(slot, c)| c.map(|(commands, beats)| render_loop(commands, slot, beats, tempo, rate, song.plugins))).collect();
            if let Ok(mut r) = shared.lock() {
                *r = Some(Ok(Rendered { arrangement, clips, map }));
            }
        });
        self.status = Some(format!("Congelando «{}»…", self.project.tracks[t].name));
        self.freeze_job = Some(FreezeJob { track: t, content: content(&self.project, t), tempo_changes, end, result });
    }

    /// Every frame: a finished freeze is written and put in place; a frozen track
    /// whose content changed unfreezes.
    pub fn follow_freeze(&mut self, ctx: &eframe::egui::Context) {
        if let Some(job) = &self.freeze_job {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            let done = job.result.lock().ok().and_then(|mut r| r.take());
            if let Some(result) = done {
                let job = self.freeze_job.take().expect("a job");
                match result.and_then(|rendered| self.finish_freeze(&job, rendered)) {
                    Ok(name) => self.status = Some(format!("«{name}» congelada: suena su audio; su instrumento y efectos no gastan CPU")),
                    Err(error) => self.status = Some(error),
                }
            }
        }
    }

    fn finish_freeze(&mut self, job: &FreezeJob, rendered: Rendered) -> Result<String, String> {
        let t = job.track;
        let track = self.project.tracks.get(t).ok_or("La pista ya no existe")?;
        if content(&self.project, t) != job.content {
            return Err(format!("«{}» cambió mientras se congelaba: vuelve a congelarla", track.name));
        }
        let (name, color, tempo, rate) = (track.name.clone(), track.color, self.project.tempo, self.audio.sample_rate());
        let dir = self.media_dir(crate::library::FROZEN);
        std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
        let clean = crate::files::safe_name(&name);
        let write = |label: &str, (left, right): &(Vec<f32>, Vec<f32>)| -> Result<String, String> {
            let path = crate::library::unique(&dir.join(format!("{} {label}.wav", clean.trim())));
            crate::export::write(&path, left, right, rate as u32, crate::export::BitDepth::Float32, 1.0)?;
            Ok(path.to_string_lossy().into_owned())
        };
        // At the project's tempo, which is theirs: Stretch at the same tempo plays
        // them as they are. A song that changes tempo was rendered following it:
        // warp markers on every beat put each beat where it was rendered, and
        // Re-Pitch plays it as it is (at 1:1).
        let clip = |name: String, sample: String, length: f64, envelopes: Vec<AutomationLane>, warp: WarpMode| AudioClip { hard_start: false,
            name,
            color,
            sample,
            length,
            offset: 0.0,
            warp,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            envelopes, muted: false,
        };
        let mut frozen = Frozen { arrangement: Vec::new(), clips: vec![None; NUM_SCENES], content: job.content };
        if let Some(audio) = &rendered.arrangement {
            let seconds = audio.0.len() as f64 / rate as f64;
            let sample = write("(congelada)", audio)?;
            let mut placed = if job.tempo_changes {
                let last_tempo = self.project.master.automation.iter().find(|l| l.param == Param::Tempo).and_then(|l| l.value_at(job.end)).map_or(tempo, |t| t as f64);
                let markers = warp_markers(&rendered.map, rate, seconds, last_tempo);
                let beats = markers.last().map_or(0.0, |m| m.beat);
                let mut audio = clip(format!("{name} (congelada)"), sample, beats, Vec::new(), WarpMode::Repitch);
                audio.warp_markers = markers;
                ArrangementClip { start: 0.0, length: beats, clip: Clip::Audio(audio) }
            } else {
                let beats = seconds * tempo / 60.0;
                ArrangementClip { start: 0.0, length: beats, clip: Clip::Audio(clip(format!("{name} (congelada)"), sample, beats, Vec::new(), WarpMode::Stretch)) }
            };
            placed.length = placed.length.max(job.end);
            if let Clip::Audio(audio) = &mut placed.clip {
                audio.hard_start = true;
            }
            frozen.arrangement.push(placed);
        }
        for (slot, audio) in rendered.clips.iter().enumerate() {
            let (Some(audio), Some(original)) = (audio, self.project.tracks[t].clips.get(slot).cloned().flatten()) else { continue };
            let sample = write(&format!("(congelada) {}", slot + 1), audio)?;
            // The clip's own envelopes that aren't baked (mixer ones) still move.
            let envelopes = match &original {
                Clip::Midi(m) => m.envelopes.clone(),
                Clip::Audio(a) => a.envelopes.clone(),
            };
            let envelopes = envelopes.into_iter().filter(|l| mixer_param(l.param)).collect();
            frozen.clips[slot] = Some(clip(original.name().to_string(), sample, original.length().max(0.25), envelopes, WarpMode::Stretch));
        }
        self.project.tracks[t].frozen = Some(frozen);
        self.sync_track(t);
        Ok(name)
    }

    /// Plays track `t`'s own instrument, effects and clips again.
    pub fn unfreeze(&mut self, t: usize) {
        if let Some(track) = self.project.tracks.get_mut(t) {
            if track.frozen.take().is_some() {
                let name = track.name.clone();
                self.sync_track(t);
                self.status = Some(format!("«{name}» descongelada"));
            }
        }
    }

    /// A frozen track becomes an audio track playing its frozen audio, for good.
    pub fn flatten(&mut self, t: usize) {
        let Some(track) = self.project.tracks.get(t).filter(|t| t.frozen.is_some()) else { return };
        let playing = playing(track).into_owned();
        self.project.tracks[t] = Track { kind: TrackKind::Audio, frozen: None, ..playing };
        let name = self.project.tracks[t].name.clone();
        self.sync_track(t);
        self.status = Some(format!("«{name}» es ahora una pista de audio"));
    }

    /// Sends everything about track `t` to the engine again (what it plays).
    pub fn sync_track(&mut self, t: usize) {
        let rate = self.audio.sample_rate();
        let track = playing(&self.project.tracks[t]).into_owned();
        let target = Target::Track(t);
        self.audio.send(crate::sync::instrument_command(&self.project, t, track.instrument, &mut self.samples, rate));
        self.audio.send(Command::SetSamples { track: t, samples: crate::sync::instrument_samples(&track, &mut self.samples) });
        self.audio.send(Command::SetWavetables { track: t, tables: crate::sync::instrument_tables(&track, &mut self.samples) });
        self.audio.send(Command::ReplaceChain { target, chain: crate::sync::chain(&self.project, &track.devices, &mut self.samples, rate) });
        self.audio.send(Command::SetArrangement { track: t, clips: crate::sync::placed_clips(&track, &mut self.samples) });
        self.audio.send(Command::SetAutomation { target, lanes: crate::sync::active_lanes(&track.automation, target, &self.overridden) });
        for slot in 0..NUM_SCENES {
            let clip = track.clips[slot].as_ref().map(|c| Box::new(crate::sync::play_clip(c, &mut self.samples)));
            self.audio.send(Command::SetClip { track: t, slot, clip });
        }
    }

    /// After a change: frozen tracks whose content changed unfreeze.
    pub fn check_frozen(&mut self) {
        let changed: Vec<usize> = (0..self.project.tracks.len())
            .filter(|&t| self.project.tracks[t].frozen.as_ref().is_some_and(|f| f.content != content(&self.project, t)))
            .collect();
        for t in changed {
            self.unfreeze(t);
            self.status = Some(format!("«{}» descongelada: has cambiado su contenido", self.project.tracks[t].name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(app: &mut PulseApp) {
        let ctx = eframe::egui::Context::default();
        let started = std::time::Instant::now();
        while app.freeze_job.is_some() {
            app.follow_freeze(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(20));
            assert!(started.elapsed().as_secs() < 60);
        }
    }

    /// The arrangement of `project` rendered for `beats`, only track `t` sounding.
    fn render(project: &Project, t: usize, beats: f64) -> Vec<f32> {
        let mut project = project.clone();
        for (i, track) in project.tracks.iter_mut().enumerate() {
            track.mixer.active = i == t;
        }
        let rate = 48_000.0;
        let mut renderer = engine::OfflineRenderer::new(rate);
        let mut samples = crate::sync::SampleCache::default();
        for command in crate::sync::project_commands(&project, &mut samples, rate, &HashSet::new()) {
            renderer.send(command);
        }
        renderer.send(Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((beats * 60.0 / project.tempo * rate as f64) as usize, &mut l, &mut r);
        l
    }

    /// Loudness in 50 ms windows, in dB.
    fn envelope(x: &[f32]) -> Vec<f32> {
        x.chunks(2400).map(|w| 10.0 * (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32 + 1e-12).log10()).collect()
    }

    #[test]
    fn a_frozen_track_sounds_as_it_did_and_unfreezes_when_changed() {
        let mut app = PulseApp::headless(Project::demo());
        let t = app.project.tracks.iter().position(|t| t.name == "Chords").unwrap();
        // Its volume and pan are left live: set them, they must not be baked in.
        app.project.tracks[t].mixer.gain = 0.5;
        let before = render(&app.project, t, 16.0);
        app.freeze(t);
        wait(&mut app);
        let frozen = app.project.tracks[t].frozen.clone().expect("frozen");
        assert!(!frozen.arrangement.is_empty() && frozen.clips.iter().flatten().count() >= 1, "{:?}", app.status);
        let Clip::Audio(audio) = &frozen.arrangement[0].clip else { panic!() };
        assert!(audio.sample.contains("Congelados"), "{}", audio.sample);
        // The engine gets no instrument or effects for it: they cost nothing.
        let playing = playing(&app.project.tracks[t]);
        assert!(matches!(playing.instrument, InstrumentKind::Synth(_)) && playing.devices.is_empty());
        assert!(playing.clips.iter().flatten().all(|c| matches!(c, Clip::Audio(_))));
        let after = render(&app.project, t, 16.0);
        // The same loudness all along (synth phases are random: not sample for sample).
        let (a, b) = (envelope(&before), envelope(&after));
        let loud: Vec<(f32, f32)> = a.iter().zip(&b).map(|(x, y)| (*x, *y)).filter(|(x, _)| *x > -50.0).collect();
        assert!(loud.len() > 20);
        let worst = loud.iter().map(|(x, y)| (x - y).abs()).fold(0f32, f32::max);
        assert!(worst < 1.5, "{worst} dB");
        // The mixer stays live: twice the gain, about 6 dB louder.
        app.project.tracks[t].mixer.gain = 1.0;
        let louder = envelope(&render(&app.project, t, 16.0));
        let gain: f32 = louder.iter().zip(&b).filter(|(_, y)| **y > -50.0).map(|(x, y)| x - y).sum::<f32>() / loud.len() as f32;
        assert!((gain - 6.0).abs() < 1.0, "{gain} dB");
        assert!(app.project.tracks[t].frozen.is_some(), "the mixer isn't its content");
        app.check_frozen();
        assert!(app.project.tracks[t].frozen.is_some());

        // Its notes changed: it unfreezes.
        if let Some(Clip::Midi(m)) = &mut app.project.tracks[t].clips[0] {
            m.notes.pop();
        }
        app.check_frozen();
        assert!(app.project.tracks[t].frozen.is_none());

        // Frozen again, then turned into audio.
        app.freeze(t);
        wait(&mut app);
        app.flatten(t);
        let track = &app.project.tracks[t];
        assert!(track.is_audio() && track.frozen.is_none() && track.devices.is_empty());
        assert!(matches!(&track.arrangement[0].clip, Clip::Audio(_)));
        assert!(track.clips.iter().flatten().all(|c| matches!(c, Clip::Audio(_))));
        for file in std::fs::read_dir(app.media_dir(crate::library::FROZEN)).into_iter().flatten().flatten() {
            std::fs::remove_file(file.path()).ok();
        }
    }

    /// A frozen track saved by another Pulse (its content noted otherwise: a new
    /// field, another hash) stays frozen when opened, until it is changed.
    #[test]
    fn frozen_tracks_stay_frozen_across_versions() {
        let mut app = PulseApp::headless(Project::demo());
        let t = app.project.tracks.iter().position(|t| t.name == "Chords").unwrap();
        app.freeze(t);
        wait(&mut app);
        let file = std::env::temp_dir().join(format!("pulse-frozen-{}/F/F.pulse", std::process::id()));
        app.save_to_path(&file);
        // As an older Pulse would have noted it.
        let mut json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        json["tracks"][t]["frozen"]["content"] = 12345.into();
        std::fs::write(&file, json.to_string()).unwrap();
        let mut opened = PulseApp::headless(Project::empty());
        opened.open_from(&file);
        opened.project.tracks[0].name = "Otra cosa".into();
        opened.check_frozen();
        assert!(opened.project.tracks[t].frozen.is_some(), "an edit elsewhere doesn't unfreeze it");
        opened.project.tracks[t].devices.clear();
        opened.check_frozen();
        assert!(opened.project.tracks[t].frozen.is_none(), "changing it does");
        let _ = std::fs::remove_dir_all(file.parent().unwrap().parent().unwrap());
    }
}
