//! «Crear rumble»: the techno rumble from a kick track. A copy of the track (it
//! plays the same hits) through a long, all-wet reverb, a tube saturation and a
//! steep low-pass, ducking to the original: what is left is a low, distorted
//! tail that breathes between the kicks.

use crate::app::PulseApp;
use engine::model::{AutoFilterParams, Device, DuckerParams, Effect, FilterMode, KickDrive, ReverbParams, SaturatorParams};

impl PulseApp {
    /// A rumble track right under track `t`, playing what it plays.
    pub fn make_rumble(&mut self, t: usize) {
        if t >= self.project.tracks.len() || self.project.tracks[t].is_group() {
            return;
        }
        if self.project.tracks.len() >= engine::MAX_TRACKS {
            self.status = Some(format!("Máximo de {} pistas", engine::MAX_TRACKS));
            return;
        }
        let before = self.project.clone();
        self.capture_plugins();
        let source = self.project.tracks[t].id;
        let mut rumble = self.copy_track(t);
        rumble.name = format!("{} Rumble", self.project.tracks[t].name);
        (rumble.id, rumble.solo, rumble.armed, rumble.frozen) = (0, false, false, None);
        rumble.mixer.gain *= 0.8;
        rumble.sends = Default::default();
        rumble.devices.extend(chain(source));
        let ids = self.track_ids();
        self.project.tracks.insert(t + 1, rumble);
        self.follow_tracks(&ids);
        self.selected_target = engine::Target::Track(t + 1);
        self.history.commit(&self.project, false);
        // The tracks after it moved: those go again; the rest play on.
        self.sync_from(&before);
        self.status = Some(format!("«{}»: reverb, saturación Tube y filtro de 24 dB a 180 Hz, apartándose cuando suena «{}»", self.project.tracks[t + 1].name, self.project.tracks[t].name));
    }
}

/// The rumble's effects, after the copied track's own; ducking to track `source`.
pub fn chain(source: u32) -> [Device; 4] {
    [
        Device::new(Effect::Reverb(ReverbParams { size: 0.92, damping: 0.6, width: 0.4, mix: 1.0 })),
        Device::new(Effect::Saturator(SaturatorParams { drive_db: 18.0, output_db: -12.0, mix: 1.0, kind: KickDrive::Tube, oversample: true })),
        Device::new(Effect::AutoFilter(AutoFilterParams { mode: FilterMode::LowPass, cutoff: 180.0, resonance: 0.35, steep: true, drive_db: 3.0, ..Default::default() })),
        Device::new(Effect::Ducker(DuckerParams { depth: 0.9, release: 0.5, rate_beats: 1.0, sidechain: Some(source), ..Default::default() })),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Track};
    use engine::model::{ArrangementClip, Clip, InstrumentKind, MidiClip, Note};

    /// From a four-on-the-floor kick: a track under it with the rumble chain,
    /// playing the same, low (under 250 Hz) and louder between the kicks than
    /// on them.
    #[test]
    fn a_rumble_is_low_and_breathes_between_the_kicks() {
        let mut project = Project::empty();
        let mut kick = Track::new("Kick", [200, 0, 0], InstrumentKind::Kick(Default::default()));
        let mut clip = MidiClip::new("4/4", 4.0, [0; 3]);
        for b in 0..4 {
            clip.notes.push(Note { start: b as f64, length: 0.25, pitch: 36, velocity: 1.0, chance: 1.0 });
        }
        kick.arrangement = vec![ArrangementClip { start: 0.0, length: 16.0, clip: Clip::Midi(clip) }];
        project.tracks = vec![kick];
        project.tempo = 140.0;
        let mut app = PulseApp::headless(project);
        app.make_rumble(0);
        assert_eq!(app.project.tracks.len(), 2);
        let rumble = &app.project.tracks[1];
        assert_eq!(rumble.name, "Kick Rumble");
        assert!(matches!(rumble.devices.last().map(|d| d.effect), Some(Effect::Ducker(DuckerParams { sidechain: Some(id), .. })) if id == app.project.tracks[0].id));
        // Only the rumble heard.
        let mut solo = app.project.clone();
        solo.tracks[0].mixer.gain = 0.0;
        let mut renderer = engine::OfflineRenderer::new(48_000.0);
        for command in crate::sync::project_commands(&solo, &mut crate::sync::SampleCache::default(), 48_000.0, &Default::default()) {
            renderer.send(command);
        }
        renderer.send(engine::Command::SetOutputLimit(false));
        renderer.send(engine::Command::Play);
        let (mut l, mut r) = (Vec::new(), Vec::new());
        let beat = (60.0 / 140.0 * 48_000.0) as usize;
        renderer.render(16 * beat, &mut l, &mut r);
        let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt();
        // Bars 3 and 4: on each kick (its first 30 ms) and between them (the off-beat).
        let (mut on, mut between) = (0.0, 0.0);
        for b in 8..16 {
            on += rms(&l[b * beat..b * beat + 1440]);
            between += rms(&l[b * beat + beat / 2..b * beat + beat / 2 + 1440]);
        }
        assert!(between > 0.005, "it sounds: {between}");
        assert!(between > on * 1.5, "it ducks on the kicks: on {on:.4}, between {between:.4}");
        // Low: the high band (above ~1 kHz, its difference from a smoothed copy) is little.
        let smooth: Vec<f32> = l.windows(48).map(|w| w.iter().sum::<f32>() / 48.0).collect();
        let high: Vec<f32> = l[24..].iter().zip(&smooth).map(|(a, b)| a - b).collect();
        println!("graves frente a agudos: total {:.4}, por encima de ~1 kHz {:.4}", rms(&l[8 * beat..]), rms(&high[8 * beat..]));
        assert!(rms(&high[8 * beat..]) < rms(&l[8 * beat..]) * 0.1, "mostly lows");
    }
}
