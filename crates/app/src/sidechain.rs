//! «Sidechain desde el bombo»: a Ducker on a track that pumps with the
//! song's kick, in one click, as a club mix does with the bass and the pads.
//! The kick is found by its role (a Kick Designer, a track named «Kick» or
//! «Bombo»); in a Drum Machine, its kick goes to a track of its own first (see
//! `drum_split`), so the hats don't pump it. Also: which sources a sidechain
//! would close a loop with (the engine hears those a fixed 64 frames late).

use crate::app::PulseApp;
use crate::mix_assist::Role;
use crate::project::Track;
use engine::model::{Device, DuckerParams, Effect, InstrumentKind};

/// The club pump: down at once on each kick (the Ducker's attack is 5 ms at
/// most), back up over an eighth of the tempo, deep but not to silence;
/// listening only to the lows (the kick, not the hats).
pub fn club_pump(source: u32) -> DuckerParams {
    DuckerParams { depth: 0.8, release: 1.0, rate_beats: 0.5, sidechain: Some(source), lows: true, ..Default::default() }
}

/// Whether track `listener` listening to the track whose id is `source`
/// closes a loop: the source sounds with the listener in it (its group, at
/// any depth) or listens to it in turn (through sidechains, followed tracks or
/// groups). The engine can't hear such a source in the same block: it hears
/// it 64 frames late.
pub fn closes_loop(tracks: &[Track], listener: usize, source: u32) -> bool {
    let Some(start) = tracks.iter().position(|t| t.id == source && source != 0) else { return false };
    let by_id = |id: u32| tracks.iter().position(|t| t.id == id && id != 0);
    let mut seen = vec![false; tracks.len()];
    let mut stack = vec![start];
    while let Some(t) = stack.pop() {
        if t == listener {
            return true;
        }
        if std::mem::replace(&mut seen[t], true) {
            continue;
        }
        // What its sound is made of: its tracks (a group), what its effects
        // listen to and the track it plays.
        let track = &tracks[t];
        stack.extend(crate::groups::children(tracks, t));
        stack.extend(track.devices.iter().filter_map(|d| d.effect.sidechain()).chain(track.audio_from).filter_map(by_id));
    }
    false
}

/// Where a track's kick comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kick {
    /// A track that is the kick: a Kick Designer, a «Kick», a kick taken out of a kit.
    Track(usize),
    /// A Drum Machine whose kick isn't on a track of its own yet.
    Machine(usize),
    /// A whole drum track (Drum Pads, a loop): its lows.
    Drums(usize),
}

/// Has clips (a template's empty «Kick» is passed over for one that plays).
fn plays(track: &Track) -> bool {
    !track.arrangement.is_empty() || track.clips.iter().any(Option::is_some) || track.audio_from.is_some()
}

impl PulseApp {
    /// The kick track `t` would pump with: a kick of its own first, then a
    /// Drum Machine (its kick), then any drum track; those that play first.
    /// Never `t` itself nor one that would close a loop.
    pub fn kick_for(&self, t: usize) -> Option<Kick> {
        let tracks = &self.project.tracks;
        let fits = |i: usize| i != t && !tracks[i].is_group() && !closes_loop(tracks, t, tracks[i].id);
        let kick = |i: usize| crate::mix_assist::track_role(&tracks[i]) == Some(Role::Kick) || tracks[i].sounds_from & 1 == 1;
        let machine = |i: usize| matches!(tracks[i].instrument, InstrumentKind::DrumMachine(_)) && !tracks[i].is_audio();
        let drums = |i: usize| crate::mix_assist::is_drums(&tracks[i]) || crate::arrange::role(&tracks[i]) == crate::arrange::Role::Drums;
        for playing in [true, false] {
            let candidates = || (0..tracks.len()).filter(|&i| fits(i) && plays(&tracks[i]) == playing);
            if let Some(i) = candidates().find(|&i| kick(i)) {
                return Some(Kick::Track(i));
            }
            if let Some(i) = candidates().find(|&i| machine(i)) {
                // Its kick on a track already: that one (if it can listen to it).
                return Some(match tracks.iter().position(|o| o.audio_from == Some(tracks[i].id) && o.sounds_from & 1 == 1) {
                    Some(k) if fits(k) => Kick::Track(k),
                    _ => Kick::Machine(i),
                });
            }
            if let Some(i) = candidates().find(|&i| drums(i)) {
                return Some(Kick::Drums(i));
            }
        }
        None
    }

    /// «Sidechain desde el bombo» on track `t`: a Ducker with the club pump
    /// listening to the kick, at the end of its effects (before a limiter).
    /// One step to undo, with the kick taken out of a Drum Machine if need be.
    pub fn sidechain_from_kick(&mut self, t: usize) {
        let Some(track) = self.project.tracks.get(t) else { return };
        let name = track.name.clone();
        let Some(kick) = self.kick_for(t) else {
            self.status = Some(format!("No hay bombo para «{name}»: añade un Kick Designer o una batería (o llama «Kick» o «Bombo» a su pista)"));
            return;
        };
        let tracks = &self.project.tracks;
        // Already listening to that kick (or to the drums it comes from).
        let (source, drums) = match kick {
            Kick::Track(k) => (tracks[k].id, tracks[k].audio_from),
            Kick::Machine(d) | Kick::Drums(d) => (tracks[d].id, None),
        };
        if let Some(already) = tracks[t].devices.iter().find(|d| d.effect.sidechain().is_some_and(|s| s == source || Some(s) == drums)) {
            let from = tracks.iter().find(|o| Some(o.id) == already.effect.sidechain()).map_or(String::new(), |o| o.name.clone());
            self.status = Some(format!("«{name}» ya se aparta con «{from}» ({})", already.effect.label()));
            return;
        }
        let before = self.project.clone();
        let mut t = t;
        let (source, warning) = match kick {
            Kick::Track(k) => (self.project.tracks[k].id, None),
            Kick::Drums(d) => {
                let drums = &self.project.tracks[d];
                (drums.id, Some(format!(" Escucha toda «{}», solo sus graves (por debajo de 600 Hz): casi solo el bombo. Para que sea solo el bombo, ponlo en su propia pista", drums.name)))
            }
            Kick::Machine(d) => match self.take_sound(d, 0) {
                Ok(Some(k)) => {
                    // The new track went in above `t`: `t` moved down one.
                    if k <= t {
                        t += 1;
                    }
                    (self.project.tracks[k].id, Some(format!(" El bombo de «{}» suena ahora en su propia pista, «{}», para que solo él lo mueva", self.project.tracks[d].name, self.project.tracks[k].name)))
                }
                // Can't take it out: the whole kit's lows.
                Ok(None) | Err(_) => (self.project.tracks[d].id, Some(format!(" Escucha toda «{}», solo sus graves: casi solo el bombo", self.project.tracks[d].name))),
            },
        };
        let devices = &mut self.project.tracks[t].devices;
        let at = devices.iter().position(|d| matches!(d.effect, Effect::Limiter(_))).unwrap_or(devices.len());
        devices.insert(at, Device::new(Effect::Ducker(club_pump(source))));
        self.selected_target = engine::Target::Track(t);
        self.history.commit(&self.project, false);
        self.sync_from(&before);
        let kick_name = self.project.tracks.iter().find(|o| o.id == source).map_or("", |o| o.name.as_str());
        self.status = Some(format!("«{name}» se aparta con cada golpe de «{kick_name}» y vuelve en una corchea (Ducker).{}", warning.unwrap_or_default()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Track};
    use engine::model::{ArrangementClip, Clip, DrumParams, KickParams, MidiClip, Note, RackParams};

    const SR: f32 = 48_000.0;

    fn four_on_the_floor(bars: usize) -> Clip {
        let mut clip = MidiClip::new("4/4", 4.0 * bars as f64, [0; 3]);
        for b in 0..4 * bars {
            clip.notes.push(Note { start: b as f64, length: 0.25, pitch: 36, velocity: 1.0, chance: 1.0 });
        }
        Clip::Midi(clip)
    }

    /// Hats on every off-beat, for the kits.
    fn with_hats(clip: Clip) -> Clip {
        let Clip::Midi(mut m) = clip else { unreachable!() };
        let beats = m.length as usize;
        for b in 0..beats {
            m.notes.push(Note { start: b as f64 + 0.5, length: 0.1, pitch: 42, velocity: 0.8, chance: 1.0 });
        }
        Clip::Midi(m)
    }

    /// A kick and a rolling bass over Am, `bars` long.
    fn kick_and_bass(bars: usize) -> Project {
        let mut project = Project::empty();
        project.tempo = 128.0;
        let mut kick = Track::new("Kick", [200, 0, 0], InstrumentKind::Kick(KickParams::default()));
        kick.arrangement = vec![ArrangementClip { start: 0.0, length: 4.0 * bars as f64, clip: four_on_the_floor(bars) }];
        let am = stems::chord::Chord::new(9, stems::chord::Quality::Minor);
        let length = 4.0 * bars as f64;
        let notes = crate::accompany::generate(crate::accompany::Part::BassRolling, &[(0.0, length, am)], length, 3, engine::meter::TimeSig::COMMON);
        let mut bass = Track::new("Bajo", [0, 0, 200], crate::accompany::Part::BassRolling.instrument());
        bass.arrangement = vec![ArrangementClip { start: 0.0, length, clip: Clip::Midi(MidiClip { notes, ..MidiClip::new("Bajo: rodante", length, [0; 3]) }) }];
        project.tracks = vec![kick, bass];
        project
    }

    fn render(project: &Project, beats: f64) -> (Vec<f32>, Vec<f32>) {
        let mut renderer = engine::OfflineRenderer::new(SR);
        for command in crate::sync::project_commands(project, &mut crate::sync::SampleCache::default(), SR, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::SetOutputLimit(false));
        renderer.send(engine::Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        renderer.render((beats * 60.0 / project.tempo * SR as f64) as usize, &mut l, &mut r);
        (l, r)
    }

    fn ducker(track: &Track) -> Option<DuckerParams> {
        track.devices.iter().find_map(|d| if let Effect::Ducker(p) = d.effect { Some(p) } else { None })
    }

    /// The bass pumps with the kick: a Ducker listening to it, the club pump;
    /// heard alone, its level is down on the sixteenth after each kick and
    /// back by the last one before the next. Once only; and ⌘Z takes it away.
    #[test]
    fn the_bass_pumps_with_the_kick_in_one_click() {
        let mut app = PulseApp::headless(kick_and_bass(4));
        app.sidechain_from_kick(1);
        let kick = app.project.tracks[0].id;
        assert_eq!(ducker(&app.project.tracks[1]), Some(club_pump(kick)), "{:?}", app.status);
        assert!(app.status.as_deref().unwrap().contains("«Kick»"), "{:?}", app.status);
        // Again: nothing more.
        app.sidechain_from_kick(1);
        assert_eq!(app.project.tracks[1].devices.len(), 1, "{:?}", app.status);
        assert!(app.status.as_deref().unwrap().contains("ya se aparta"));
        // Measured: the bass alone (the kick muted: the sidechain hears it still).
        let beats = 16.0;
        let alone = |project: &Project| {
            let mut p = project.clone();
            p.tracks[0].mixer.active = false;
            render(&p, beats).0
        };
        let (pumped, free) = (alone(&app.project), alone(&kick_and_bass(4)));
        let beat = 60.0 / 128.0 * SR as f64;
        let rms = |x: &[f32], at: f64| {
            let from = (at * beat) as usize;
            let part = &x[from..from + 480];
            (part.iter().map(|s| s * s).sum::<f32>() / part.len() as f32).sqrt()
        };
        // From the second bar (the Ducker has heard a kick), on every beat.
        for b in 4..16 {
            let b = b as f64;
            let early = rms(&pumped, b + 0.25 + 0.02) / rms(&free, b + 0.25 + 0.02);
            let late = rms(&pumped, b + 0.75 + 0.02) / rms(&free, b + 0.75 + 0.02);
            assert!(early < 0.7, "beat {b}: down after the kick, {early:.2}");
            assert!(late > 0.95, "beat {b}: back up, {late:.2} after {early:.2}");
        }
        // Undone in one step.
        app.undo();
        assert!(ducker(&app.project.tracks[1]).is_none());
        assert_eq!(app.project.tracks.len(), 2);
    }

    /// A Drum Machine: its kick goes to a track of its own, which the bass
    /// listens to; ⌘Z puts both back. Drum Pads: the whole kit, warned.
    #[test]
    fn from_a_kit_only_the_kick_if_it_can() {
        let mut project = kick_and_bass(2);
        project.tracks[0] = Track::new("Batería", [200, 0, 0], InstrumentKind::DrumMachine(DrumParams::default()));
        project.tracks[0].arrangement = vec![ArrangementClip { start: 0.0, length: 8.0, clip: with_hats(four_on_the_floor(2)) }];
        let mut app = PulseApp::headless(project.clone());
        app.sidechain_from_kick(1);
        assert_eq!(app.project.tracks.len(), 3, "{:?}", app.status);
        let kick = &app.project.tracks[1];
        assert_eq!((kick.audio_from, kick.sounds_from), (Some(app.project.tracks[0].id), 1));
        assert_eq!(ducker(&app.project.tracks[2]).and_then(|d| d.sidechain), Some(kick.id));
        assert!(app.status.as_deref().unwrap().contains("su propia pista"), "{:?}", app.status);
        // Asked again: it is that kick it pumps with already.
        app.sidechain_from_kick(2);
        assert_eq!(app.project.tracks[2].devices.len(), 1);
        app.undo();
        assert_eq!(app.project, PulseApp::headless(project.clone()).project);

        project.tracks[0].instrument = InstrumentKind::DrumRack(RackParams::default());
        let mut app = PulseApp::headless(project);
        app.sidechain_from_kick(1);
        assert_eq!(app.project.tracks.len(), 2);
        assert_eq!(ducker(&app.project.tracks[1]).and_then(|d| d.sidechain), Some(app.project.tracks[0].id));
        assert!(app.status.as_deref().unwrap().contains("solo sus graves"), "{:?}", app.status);
    }

    /// No kick: nothing, and says why. The kick itself, or a group holding it,
    /// don't pump with it.
    #[test]
    fn without_a_kick_or_around_it_nothing() {
        let mut project = kick_and_bass(1);
        project.tracks.remove(0);
        let mut app = PulseApp::headless(project);
        app.sidechain_from_kick(0);
        assert!(app.project.tracks[0].devices.is_empty());
        assert!(app.status.as_deref().unwrap().starts_with("No hay bombo"));

        let mut app = PulseApp::headless(kick_and_bass(1));
        let lone = PulseApp::headless(kick_and_bass(1)).project;
        app.sidechain_from_kick(0);
        assert_eq!(app.project, lone, "the kick doesn't pump with itself");
    }

    /// Loops: a track listening to its own group, to a track that listens to
    /// it, or to one inside a group that listens to it; not to a sibling.
    #[test]
    fn loops_are_found() {
        let mut project = kick_and_bass(1);
        let mut group = Track::new_group("Grupo", [0; 3]);
        group.depth = 0;
        project.tracks.insert(0, group);
        for t in &mut project.tracks[1..] {
            t.depth = 1;
        }
        project.assign_ids();
        let ids: Vec<u32> = project.tracks.iter().map(|t| t.id).collect();
        let tracks = &project.tracks;
        assert!(closes_loop(tracks, 2, ids[0]), "its own group");
        assert!(!closes_loop(tracks, 2, ids[1]), "a sibling");
        assert!(!closes_loop(tracks, 0, ids[1]), "a group listening to one of its tracks");
        let mut project = project.clone();
        project.tracks[1].devices.push(Device::new(Effect::Ducker(club_pump(ids[2]))));
        assert!(closes_loop(&project.tracks, 2, ids[1]), "each other");
        assert!(!closes_loop(&project.tracks, 2, 999), "a track that isn't there");
    }

    /// The example: a kick and a rolling bass pumping with it, as WAV
    /// (`PULSE_RODANTE=carpeta`).
    #[test]
    #[ignore]
    fn render_the_rolling_bass_example() {
        let Ok(dir) = std::env::var("PULSE_RODANTE") else { return };
        let mut app = PulseApp::headless(kick_and_bass(8));
        app.sidechain_from_kick(1);
        let (l, r) = render(&app.project, 32.0);
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        crate::capture::write_wav(&dir.join("bombo_bajo_rodante_sidechain.wav"), &l, &r, SR).unwrap();
        let (l, r) = render(&kick_and_bass(8), 32.0);
        crate::capture::write_wav(&dir.join("bombo_bajo_rodante_sin_sidechain.wav"), &l, &r, SR).unwrap();
        let peak = stems::loudness::sample_peak(&l, &r);
        println!("pico {peak:+.1} dBFS");
    }
}
