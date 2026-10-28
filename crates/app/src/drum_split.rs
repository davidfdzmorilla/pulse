//! «Sacar a su pista»: a Drum Machine sound on a track of its own. The new
//! track has no clips: it takes that sound from the drum machine (see
//! `Track::sounds_from`), before the drum track's effects, and the drum track
//! no longer plays it. So the kick of a kit can have its own EQ, saturation
//! and fader, and be what a sidechain listens to, while the pattern stays one
//! clip on the drum track.

use crate::app::PulseApp;
use crate::project::{voice_name, Monitor, Track};
use engine::model::{InstrumentKind, DRUM_SOUNDS};

/// The names of the sounds in `mask`, quoted: «Bombo», «Palmada».
pub fn sound_names(mask: u16) -> String {
    let names: Vec<String> = (0..DRUM_SOUNDS).filter(|v| mask >> v & 1 == 1).map(|v| format!("«{}»", voice_name(v))).collect();
    names.join(", ")
}

impl PulseApp {
    /// The tracks (indices) that take sounds from track `t`'s drum machine.
    fn takers(&self, t: usize) -> impl Iterator<Item = usize> + '_ {
        let id = self.project.tracks.get(t).map_or(0, |track| track.id);
        self.project.tracks.iter().enumerate().filter(move |(_, o)| id != 0 && o.audio_from == Some(id) && o.sounds_from != 0).map(|(i, _)| i)
    }

    /// Which of track `t`'s drum machine sounds play on other tracks (a bit each).
    pub fn sounds_taken(&self, t: usize) -> u16 {
        self.takers(t).fold(0, |mask, i| mask | self.project.tracks[i].sounds_from)
    }

    /// The track (name) that plays sound `voice` of track `t`'s drum machine.
    pub fn sound_track(&self, t: usize, voice: usize) -> Option<&str> {
        self.takers(t).find(|&i| self.project.tracks[i].sounds_from >> voice & 1 == 1).map(|i| self.project.tracks[i].name.as_str())
    }

    /// What a track that plays another's sound does, for its header's hint.
    pub fn follows_hint(&self, t: usize) -> Option<String> {
        let source = self.follows(t)?;
        let sounds = self.project.tracks[t].sounds_from;
        Some(if sounds == 0 {
            format!("Toca lo que suena en «{source}», después de sus efectos (un rumble: no tiene clips propios, sigue a esa pista y se silencia con ella)")
        } else {
            format!("Toca {} de la Drum Machine de «{source}», antes de sus efectos; «{source}» ya no lo toca. No tiene clips propios: sigue a esa pista y se silencia con ella", sound_names(sounds))
        })
    }

    /// Sound `voice` of track `t`'s Drum Machine on a new track, under it and
    /// the ones already taken from it.
    pub fn sound_apart(&mut self, t: usize, voice: usize) {
        let Some(drums) = self.project.tracks.get(t) else { return };
        if !matches!(drums.instrument, InstrumentKind::DrumMachine(_)) || drums.is_group() || voice >= DRUM_SOUNDS {
            return;
        }
        if drums.frozen.is_some() {
            self.status = Some(format!("«{}» está congelada: descongélala para sacar sus sonidos a otras pistas", drums.name));
            return;
        }
        if let Some(name) = self.sound_track(t, voice) {
            self.status = Some(format!("«{}» ya suena en «{name}»", voice_name(voice)));
            return;
        }
        if self.project.tracks.len() >= engine::MAX_TRACKS {
            self.status = Some(format!("Máximo de {} pistas", engine::MAX_TRACKS));
            return;
        }
        let before = self.project.clone();
        let drums = &self.project.tracks[t];
        let mut track = Track::new_audio(format!("{} {}", drums.name, voice_name(voice)), drums.color);
        (track.depth, track.monitor, track.audio_from, track.sounds_from) = (drums.depth, Monitor::Off, Some(drums.id), 1 << voice);
        let at = self.takers(t).max().map_or(t, |last| last.max(t)) + 1;
        let ids = self.track_ids();
        self.project.tracks.insert(at, track);
        self.follow_tracks(&ids);
        self.selected_target = engine::Target::Track(at);
        self.history.commit(&self.project, false);
        // The tracks after it moved: those go again; the rest play on.
        self.sync_from(&before);
        self.status = Some(format!("«{}» suena ahora en «{}», con sus propios efectos y fader; los clips siguen en «{}»", voice_name(voice), self.project.tracks[at].name, self.project.tracks[t].name));
    }
}

#[cfg(test)]
mod tests {
    use crate::app::PulseApp;
    use crate::project::{Project, Track};
    use engine::model::{ArrangementClip, Clip, DrumModel, DrumParams, InstrumentKind, MidiClip, Note};

    const SR: f32 = 48_000.0;

    fn render(project: &Project, beats: f64) -> Vec<f32> {
        let mut renderer = engine::OfflineRenderer::new(SR);
        for command in crate::sync::project_commands(project, &mut crate::sync::SampleCache::default(), SR, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::SetOutputLimit(false));
        renderer.send(engine::Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((beats * 60.0 / project.tempo * SR as f64) as usize, &mut l, &mut r);
        l
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    /// A 909 kit: kick on the beats, clap on 2 and 4, closed hats on the off-beats.
    fn kit() -> Project {
        let mut project = Project::empty();
        let mut drums = Track::new("Drums", [200, 120, 0], InstrumentKind::DrumMachine(DrumParams { model: DrumModel::Tr909, ..Default::default() }));
        let mut clip = MidiClip::new("beat", 4.0, [0; 3]);
        for b in 0..4 {
            clip.notes.push(Note::new(b as f64, 0.25, 36, 0.5));
            clip.notes.push(Note::new(b as f64 + 0.5, 0.1, 42, 0.5));
            if b % 2 == 1 {
                clip.notes.push(Note::new(b as f64, 0.25, 39, 0.5));
            }
        }
        drums.arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: Clip::Midi(clip) }];
        project.tracks = vec![drums];
        project
    }

    /// The kick out on its own track: the song sounds the same, the drum track
    /// no longer has it, the new one has only it (it goes through that track's
    /// fader), and it is saved, as a format an older Pulse refuses.
    #[test]
    fn the_kick_goes_out_on_its_own_track() {
        let mut app = PulseApp::headless(kit());
        let whole = render(&app.project, 8.0);
        app.sound_apart(0, 0);
        assert_eq!(app.project.tracks.len(), 2);
        let kick = &app.project.tracks[1];
        assert_eq!(kick.name, "Drums Bombo");
        assert_eq!((kick.audio_from, kick.sounds_from), (Some(app.project.tracks[0].id), 1));
        assert!(kick.arrangement.is_empty(), "no clips of its own");
        assert_eq!(app.sounds_taken(0), 1);
        assert_eq!(app.sound_track(0, 0), Some("Drums Bombo"));
        assert!(app.follows_hint(1).unwrap().contains("«Bombo»"));
        let split = render(&app.project, 8.0);
        let differ = split.iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(differ < 1e-4, "the same song: {differ}");

        // Its fader down: the kick's beats go quiet, the rest stays.
        let mut quiet = app.project.clone();
        quiet.tracks[1].mixer.gain = 0.0;
        let without = render(&quiet, 8.0);
        let mut no_kick = kit();
        let Clip::Midi(clip) = &mut no_kick.tracks[0].arrangement[0].clip else { unreachable!() };
        clip.notes.retain(|n| n.pitch != 36);
        let rest = render(&no_kick, 8.0);
        let differ = without.iter().zip(&rest).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(differ < 1e-4, "the rest as it was, without the kick: {differ}");
        assert!(rms(&without) < rms(&whole) * 0.8, "the kick was a good part of it");

        // Again: refused, it already plays there. Another sound goes under it.
        app.sound_apart(0, 0);
        assert_eq!(app.project.tracks.len(), 2);
        app.sound_apart(0, 3);
        assert_eq!(app.project.tracks[2].name, "Drums Palmada");
        assert_eq!(app.sounds_taken(0), 0b1001);
        // Frozen, it would play them all again: refused.
        app.freeze(0);
        assert!(app.status.as_deref().is_some_and(|s| s.contains("no se puede congelar")), "{:?}", app.status);
        assert!(app.freeze_job.is_none());

        let json = app.project.to_json().unwrap();
        assert!(json.contains("\"format\": 4"));
        let loaded = Project::from_json(&json).unwrap();
        assert_eq!(loaded.tracks[1].sounds_from, 1);
        // Undone: the drum track plays its kick again.
        app.undo();
        app.undo();
        assert_eq!(app.project.tracks.len(), 1);
        assert_eq!(app.sounds_taken(0), 0);
    }

    /// A stem of the drum track has no kick; the kick track's stem has only it.
    #[test]
    fn stems_split_as_the_tracks_do() {
        let mut app = PulseApp::headless(kit());
        let whole = render(&app.project, 8.0);
        app.sound_apart(0, 0);
        let stem = |t: usize| {
            let mut only = crate::export::only(&app.project, t);
            only.tracks[t].solo = true;
            render(&only, 8.0)
        };
        let (drums, kick) = (stem(0), stem(1));
        let sum: Vec<f32> = drums.iter().zip(&kick).map(|(a, b)| a + b).collect();
        let differ = sum.iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(differ < 1e-4, "the two stems add up to the song: {differ}");
        assert!(rms(&kick) > 0.01 && rms(&drums) > 0.01);
    }
}
