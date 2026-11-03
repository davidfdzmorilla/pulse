//! «Convertir el tramo en audio» (bounce) and «Remuestrear» (resampling), as
//! in Live 12: what a stretch of the song sounds on one or several tracks, a
//! group, or the master, rendered to audio on a new track.
//!
//! The stretch is the span of the selected clips on the Arrangement, or the
//! loop if none is selected. A track (or group) is rendered as a freeze renders
//! it (`freeze::solo_project`): its instrument and effects (a group's tracks
//! and its own effects), its fader, pan and sends left out. The new audio track
//! goes right under it (inside the same groups) with its fader, pan, sends and
//! their automation, and the clips that played in the stretch are cut at its
//! edges and deactivated: the song sounds the same, from the audio.
//!
//! Resampling keeps everything as it was: the new track (with what a track,
//! or the whole master, sounds) is made silenced, to be played instead.
//! A group's tracks' sends to the returns aren't in its audio (the returns'
//! effects are not the group's).

use crate::app::PulseApp;
use crate::project::{Project, Track};
use engine::model::{ArrangementClip, AudioClip, Clip, Param, WarpMode};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// What a stretch is rendered from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A track or a group (by index), as it sounds into its fader.
    Track(usize),
    /// The whole song, as exported.
    Master,
}

/// A render of one source: its audio and where the transport was along it.
struct Rendered {
    audio: (Vec<f32>, Vec<f32>),
    map: Vec<(f64, usize)>,
}

/// One source being rendered: its track's id (none for the master) and the
/// arrangements of its tracks when it started (changed meanwhile, it stops).
struct Part {
    id: Option<u32>,
    lanes: Vec<(u32, Vec<ArrangementClip>)>,
}

/// Bounces being rendered in the background.
pub struct BounceJob {
    parts: Vec<Part>,
    range: (f64, f64),
    /// Resampling: the originals stay, the new tracks are silenced.
    resample: bool,
    tempo_changes: bool,
    result: Arc<Mutex<Option<Vec<Option<Rendered>>>>>,
}

/// Near enough to a stretch's edge to be on it (beats).
const EDGE: f64 = 1e-6;

/// The lane cut at `beat`: a clip across it becomes two.
fn cut(lane: &mut Vec<ArrangementClip>, beat: f64, tempo: f64) {
    let mut out = Vec::with_capacity(lane.len() + 1);
    for placed in lane.drain(..) {
        match crate::clip_split::split(&placed, beat, tempo) {
            Some((left, right)) => out.extend([left, right]),
            None => out.push(placed),
        }
    }
    *lane = out;
}

/// Whether `placed` plays inside the stretch (after cutting at its edges).
fn inside(placed: &ArrangementClip, (from, to): (f64, f64)) -> bool {
    placed.start >= from - EDGE && placed.end() <= to + EDGE && placed.end() > from + EDGE
}

/// The project as source `t` is bounced from: soloed as a freeze renders it,
/// its tracks playing only their clips inside the stretch (cut at its edges);
/// a group's tracks send nothing to the returns.
fn bounce_project(project: &Project, t: usize, range: (f64, f64)) -> Project {
    let mut cut_project = project.clone();
    let block = crate::groups::block(&cut_project.tracks, t);
    for i in block.clone() {
        let lane = &mut cut_project.tracks[i].arrangement;
        cut(lane, range.0, project.tempo);
        cut(lane, range.1, project.tempo);
        lane.retain(|p| inside(p, range));
    }
    let mut render = crate::freeze::solo_project(&cut_project, t);
    for i in block.skip(1) {
        render.tracks[i].sends = Default::default();
        render.tracks[i].automation.retain(|l| !matches!(l.param, Param::Send(_)));
    }
    render
}

impl PulseApp {
    /// The stretch a bounce takes: the selected Arrangement clips', or the loop.
    pub fn bounce_range(&self) -> (f64, f64) {
        let spans: Vec<(f64, f64)> = self
            .clip_selection()
            .into_iter()
            .filter_map(|c| match c {
                crate::app::ClipRef::Arrangement { track, index } => self.project.tracks.get(track)?.arrangement.get(index).map(|p| (p.start, p.end())),
                _ => None,
            })
            .collect();
        if spans.is_empty() {
            return (self.project.loop_start, self.project.loop_end);
        }
        (spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min), spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max))
    }

    /// The tracks the selected Arrangement clips are on.
    pub fn selected_clip_tracks(&self) -> Vec<usize> {
        let mut tracks: Vec<usize> = self.clip_selection().into_iter().filter_map(|c| matches!(c, crate::app::ClipRef::Arrangement { .. }).then(|| c.track())).collect();
        tracks.sort_unstable();
        tracks.dedup();
        tracks
    }

    /// The stretch said for the menus: «compases 5–9».
    pub fn range_label(&self, (from, to): (f64, f64)) -> String {
        let bar = |b: f64| self.project.meter.bar_at(b + 1e-9).0 + 1;
        let last = self.project.meter.bar_at((to - 1e-6).max(from)).0 + 1;
        if last == bar(from) { format!("compás {}", bar(from)) } else { format!("compases {}–{last}", bar(from)) }
    }

    /// Starts rendering `sources` over `range` (see the module's notes).
    pub fn bounce(&mut self, sources: Vec<Source>, range: (f64, f64), resample: bool) {
        if self.bounce_job.is_some() {
            self.status = Some("Ya se está convirtiendo un tramo en audio".into());
            return;
        }
        if range.1 - range.0 < 1e-3 {
            self.status = Some("Elige un tramo: selecciona clips en el Arrangement o pon el loop donde quieras".into());
            return;
        }
        // A track inside a group also chosen goes with the group.
        let chosen: Vec<usize> = sources.iter().filter_map(|s| if let Source::Track(t) = s { Some(*t) } else { None }).collect();
        let mut sources: Vec<Source> = sources
            .into_iter()
            .filter(|s| match *s {
                Source::Track(t) => t < self.project.tracks.len() && !chosen.iter().any(|&g| g != t && crate::groups::block(&self.project.tracks, g).contains(&t)),
                Source::Master => true,
            })
            .collect();
        sources.dedup();
        let new_tracks = sources.len();
        if new_tracks == 0 {
            return;
        }
        if self.project.tracks.len() + new_tracks > engine::MAX_TRACKS {
            self.status = Some(format!("Máximo de {} pistas", engine::MAX_TRACKS));
            return;
        }
        self.project.assign_ids();
        let rate = self.audio.sample_rate();
        // Plugins load here (with their state), the renders run in the background.
        self.capture_plugins();
        let live = std::mem::take(&mut self.samples.plugins);
        let mut renders = Vec::new();
        let mut parts = Vec::new();
        for &source in &sources {
            let render = match source {
                Source::Track(t) => bounce_project(&self.project, t, range),
                Source::Master => self.project.clone(),
            };
            let song = crate::export::Song::of(&render);
            let commands = crate::sync::project_commands(&render, &mut self.samples, rate, &HashSet::new());
            renders.push((commands, song));
            parts.push(match source {
                Source::Track(t) => Part {
                    id: Some(self.project.tracks[t].id),
                    lanes: crate::groups::block(&self.project.tracks, t).map(|i| (self.project.tracks[i].id, self.project.tracks[i].arrangement.clone())).collect(),
                },
                Source::Master => Part { id: None, lanes: Vec::new() },
            });
        }
        self.samples.plugins = live;
        let tempo_changes = self.project.master.automation.iter().any(|l| l.param == Param::Tempo && !l.points.is_empty());
        let result = Arc::new(Mutex::new(None));
        let shared = result.clone();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let done: Vec<Option<Rendered>> = renders
                .into_iter()
                .map(|(commands, song)| {
                    let mut map = Vec::new();
                    crate::export::render_mapped(commands, range, song, rate, &cancel, &|_| {}, &mut map).map(|audio| Rendered { audio, map })
                })
                .collect();
            if let Ok(mut r) = shared.lock() {
                *r = Some(done);
            }
        });
        self.status = Some(format!("Convirtiendo en audio {}…", self.range_label(range)));
        self.bounce_job = Some(BounceJob { parts, range, resample, tempo_changes, result });
    }

    /// Every frame: finished renders become their tracks.
    pub fn follow_bounce(&mut self) -> bool {
        let Some(job) = &self.bounce_job else { return false };
        let done = job.result.lock().ok().and_then(|mut r| r.take());
        let Some(rendered) = done else { return true };
        let job = self.bounce_job.take().expect("a job");
        match self.finish_bounce(&job, rendered) {
            Ok(text) => self.status = Some(text),
            Err(error) => self.status = Some(error),
        }
        false
    }

    fn finish_bounce(&mut self, job: &BounceJob, rendered: Vec<Option<Rendered>>) -> Result<String, String> {
        let index_of = |app: &PulseApp, id: u32| app.project.tracks.iter().position(|t| t.id == id);
        // Nothing of what was rendered may have changed meanwhile.
        for part in &job.parts {
            for (id, lane) in &part.lanes {
                let now = index_of(self, *id).map(|t| &self.project.tracks[t].arrangement);
                if now != Some(lane) {
                    return Err("Las pistas cambiaron mientras se convertían en audio: vuelve a hacerlo".into());
                }
            }
        }
        let (tempo, rate, range) = (self.project.tempo, self.audio.sample_rate(), job.range);
        let dir = self.media_dir(crate::library::BOUNCED);
        std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear {}: {e}", dir.display()))?;
        let ids = self.track_ids();
        let mut made = Vec::new();
        // From the last one up, so the tracks above stay where they are.
        let mut order: Vec<(usize, &Part, Rendered)> = Vec::new();
        for (part, r) in job.parts.iter().zip(rendered) {
            let r = r.ok_or("No se pudo convertir en audio")?;
            let at = part.id.and_then(|id| index_of(self, id)).unwrap_or(usize::MAX);
            order.push((at, part, r));
        }
        order.sort_by_key(|(at, ..)| std::cmp::Reverse(*at));
        for (at, part, r) in order {
            let source = (at != usize::MAX).then(|| self.project.tracks[at].clone());
            let name = source.as_ref().map_or("Master".to_string(), |t| t.name.clone());
            let color = source.as_ref().map_or([180, 180, 180], |t| t.color);
            let label = if job.resample { "remuestreo" } else { "audio" };
            let path = crate::library::claim(&dir.join(format!("{} ({label}).wav", crate::files::safe_name(&name).trim())));
            crate::export::write(&path, &r.audio.0, &r.audio.1, rate as u32, crate::export::BitDepth::Float32, 1.0)?;
            let seconds = r.audio.0.len() as f64 / rate as f64;
            let mut audio = AudioClip {
                hard_start: true,
                fades: Default::default(),
                name: format!("{name} ({label})"),
                color,
                sample: path.to_string_lossy().into_owned(),
                length: seconds * tempo / 60.0,
                offset: 0.0,
                // At its own tempo, Re-Pitch plays it sample for sample (Stretch's
                // grains would smear it a little).
                warp: WarpMode::Repitch,
                original_bpm: tempo,
                warp_markers: Vec::new(),
                takes: Vec::new(),
                key: None,
                gain_db: 0.0,
                transpose: 0.0,
                envelopes: Vec::new(),
                muted: false,
            };
            if job.tempo_changes {
                // Rendered following the tempo: each beat where it fell (as a freeze does).
                let last = self.project.master.automation.iter().find(|l| l.param == Param::Tempo).and_then(|l| l.value_at(range.1)).map_or(tempo, |t| t as f64);
                let markers = crate::freeze::warp_markers(&r.map, rate, seconds, last);
                audio.length = markers.last().map_or(audio.length, |m| m.beat - range.0);
                audio.warp_markers = markers.into_iter().map(|m| engine::model::WarpMarker { beat: m.beat - range.0, ..m }).collect();
            }
            let length = audio.length;
            let mut track = Track::new_audio(format!("{name} ({label})"), color);
            track.arrangement.push(ArrangementClip { start: range.0, length, clip: Clip::Audio(audio) });
            let at = match &source {
                Some(source) => {
                    // As loud, as far and sending as much as the source, inside its groups.
                    (track.mixer, track.sends, track.depth) = (source.mixer.clone(), source.sends, source.depth);
                    track.automation = source.automation.iter().filter(|l| crate::freeze::mixer_param(l.param)).cloned().collect();
                    if !job.resample {
                        // The source's clips in the stretch, cut at its edges and off.
                        for (id, _) in &part.lanes {
                            let Some(i) = index_of(self, *id) else { continue };
                            let lane = &mut self.project.tracks[i].arrangement;
                            cut(lane, range.0, tempo);
                            cut(lane, range.1, tempo);
                            for placed in lane.iter_mut().filter(|p| inside(p, range)) {
                                placed.clip.set_muted(true);
                            }
                        }
                    }
                    crate::groups::block(&self.project.tracks, at).end
                }
                None => self.project.tracks.len(),
            };
            if job.resample {
                track.mixer.active = false;
            }
            self.project.tracks.insert(at, track);
            made.push(name);
        }
        self.follow_tracks(&ids);
        self.project.assign_ids();
        self.sync_all();
        made.reverse();
        let what = self.range_label(range);
        Ok(match (job.resample, made.len()) {
            (true, _) => format!("Remuestreado {what} de «{}» en una pista nueva, silenciada: actívala para oírla", made.join("», «")),
            (false, 1) => format!("«{}» ({what}) es ahora audio en la pista de debajo; sus clips de ahí quedan desactivados", made[0]),
            (false, n) => format!("{n} pistas ({what}) convertidas en audio, cada una debajo de la suya; sus clips de ahí quedan desactivados"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{self, SampleCache};
    use engine::model::*;

    const SR: f32 = 48_000.0;

    fn note(start: f64, length: f64, pitch: u8) -> Note {
        Note { start, length, pitch, velocity: 0.8, chance: 1.0 }
    }

    fn mix(project: &Project, range: (f64, f64)) -> (Vec<f32>, Vec<f32>) {
        let list = sync::project_commands(project, &mut SampleCache::default(), SR, &HashSet::new());
        crate::export::render(list, range, crate::export::Song::of(project), SR, &AtomicBool::new(false), &|_| {}).unwrap()
    }

    fn max_diff(a: &(Vec<f32>, Vec<f32>), b: &(Vec<f32>, Vec<f32>)) -> f32 {
        let n = a.0.len().min(b.0.len());
        (0..n).fold(0f32, |m, i| m.max((a.0[i] - b.0[i]).abs()).max((a.1[i] - b.1[i]).abs()))
    }

    fn peak(a: &(Vec<f32>, Vec<f32>)) -> f32 {
        a.0.iter().chain(&a.1).fold(0f32, |m, s| m.max(s.abs()))
    }

    fn wait(app: &mut PulseApp) {
        let started = std::time::Instant::now();
        while app.follow_bounce() {
            std::thread::sleep(std::time::Duration::from_millis(20));
            assert!(started.elapsed().as_secs() < 300, "the bounce never finished");
        }
    }

    /// A lead with a delay and a reverb (in a group with a filter), a
    /// bass with a send to the reverb return; clips before, in and after bars 3–4.
    fn song() -> Project {
        let mut p = Project::empty();
        // Beats on whole samples (24 000 a beat): the audio starts on one, as
        // the track's notes did (at other tempos it is off by a fraction of a
        // sample, which only the highest frequencies hear).
        p.tempo = 120.0;
        let group = Track::new_group("Bus", [10, 10, 10]);
        let mut lead = Track::new("Lead", [200, 80, 0], InstrumentKind::Synth(Default::default()));
        lead.depth = 1;
        lead.mixer.gain = 0.7;
        lead.mixer.pan = -0.3;
        lead.devices.push(Device::new(Effect::Delay(Default::default())));
        let riff = |name: &str| Clip::Midi(MidiClip { notes: (0..4).map(|i| note(i as f64, 0.5, 60 + 3 * i as u8)).collect(), ..MidiClip::new(name, 4.0, [0; 3]) });
        lead.arrangement = vec![ArrangementClip { start: 0.0, length: 4.0, clip: riff("a") }, ArrangementClip { start: 8.0, length: 8.0, clip: riff("b") }, ArrangementClip { start: 20.0, length: 4.0, clip: riff("c") }];
        let mut bass = Track::new("Bass", [0, 80, 200], InstrumentKind::Synth(Default::default()));
        bass.sends[0] = 0.4;
        bass.arrangement = vec![ArrangementClip { start: 0.0, length: 24.0, clip: Clip::Midi(MidiClip { notes: vec![note(0.0, 2.0, 36), note(2.0, 2.0, 38)], ..MidiClip::new("bajo", 4.0, [0; 3]) }) }];
        p.tracks = vec![group, lead, bass];
        p.tracks[0].devices.push(Device::new(Effect::AutoFilter(Default::default())));
        p.assign_ids();
        p
    }

    /// The lead's clip at bars 3–4 to audio, then the bass's (cut in the
    /// middle of its clip, at bar lines): the song sounds as before.
    #[test]
    fn a_bounced_stretch_sounds_as_the_track_did() {
        let p = song();
        let whole = (0.0, 28.0);
        let before = mix(&p, whole);
        assert!(peak(&before) > 0.01);
        let mut app = PulseApp::headless(p);
        app.commit_history();
        app.bounce(vec![Source::Track(1)], (8.0, 16.0), false);
        wait(&mut app);
        let tracks = &app.project.tracks;
        assert_eq!(tracks.len(), 4, "{:?}", app.status);
        assert_eq!(tracks[2].name, "Lead (audio)");
        assert_eq!((tracks[2].depth, tracks[2].mixer.gain, tracks[2].mixer.pan), (1, 0.7, -0.3), "in the group, with the lead's fader and pan");
        assert!(tracks[1].arrangement.iter().filter(|c| c.start >= 8.0 && c.end() <= 16.0).all(|c| c.clip.muted()));
        assert!(tracks[1].arrangement.iter().filter(|c| c.start < 8.0 || c.start >= 16.0).all(|c| !c.clip.muted()));
        let after = mix(&app.project, whole);
        let diff = max_diff(&before, &after);
        assert!(diff < 1e-4, "the lead from audio: off by {diff}");
        app.commit_history();

        // The bass, from the middle of its clip to the middle of the next bar.
        app.bounce(vec![Source::Track(3)], (4.0, 12.0), false);
        wait(&mut app);
        assert_eq!(app.project.tracks[4].name, "Bass (audio)");
        assert_eq!(app.project.tracks[4].sends[0], 0.4, "it sends as the bass did");
        let again = mix(&app.project, whole);
        let diff = max_diff(&before, &again);
        assert!(diff < 1e-3, "the bass cut at bar lines: off by {diff}");
        app.commit_history();
        app.undo();
        assert_eq!(app.project.tracks.len(), 4, "⌘Z takes the bounce back");
    }

    /// A whole group to audio: its tracks' clips in the stretch go off, its
    /// effects are in the audio; resampling the master leaves everything on.
    #[test]
    fn groups_and_the_master_can_be_bounced() {
        let mut p = song();
        p.tracks[2].sends[0] = 0.0;
        // The bass in the group too.
        p.tracks[2].depth = 1;
        let whole = (0.0, 28.0);
        let before = mix(&p, whole);
        let mut app = PulseApp::headless(p);
        app.bounce(vec![Source::Track(0), Source::Track(1)], (0.0, 24.0), false);
        wait(&mut app);
        assert_eq!(app.project.tracks.len(), 4, "one track for the group (the lead goes with it)");
        assert_eq!((app.project.tracks[3].name.as_str(), app.project.tracks[3].depth), ("Bus (audio)", 0));
        let after = mix(&app.project, whole);
        // (The render's tail ends where it falls below -90 dB or so.)
        assert!(max_diff(&before, &after) < 1e-3, "off by {}", max_diff(&before, &after));

        app.bounce(vec![Source::Master], (0.0, 8.0), true);
        wait(&mut app);
        let last = app.project.tracks.last().unwrap();
        assert_eq!(last.name, "Master (remuestreo)");
        assert!(!last.mixer.active, "silenced: the song doesn't double");
        let resampled = mix(&app.project, whole);
        assert!(max_diff(&after, &resampled) < 1e-6);
        let Clip::Audio(audio) = &last.arrangement[0].clip else { panic!() };
        let sample = engine::Sample::load(std::path::Path::new(&audio.sample)).unwrap();
        // What was heard over its first two bars (and the tail after).
        let heard = mix(&app.project, (0.0, 8.0));
        let n = (8.0 * 60.0 / 120.0 * SR as f64) as usize;
        let off = (0..n).fold(0f32, |m, i| m.max((sample.left[i] - heard.0[i]).abs()));
        assert!(off < 1e-4, "the resample is the mix: off by {off}");
    }
}
