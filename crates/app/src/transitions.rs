//! Transitions for the changes of section, made to measure: a riser (noise
//! and a tone that climb into the drop), a downlifter (they fall after it), an
//! impact (a deep hit and its tail) and a reverse cymbal (swelling into it).
//! Each is rendered to an audio file at the project's tempo and placed on the
//! Arrangement, on a track "Transiciones", ending (or starting) at the bar
//! chosen. The tone is in the project's key, if it has one.

use crate::app::{ClipRef, PulseApp};
use engine::model::{ArrangementClip, AudioClip, Clip, WarpMode};
use std::f32::consts::PI;

/// The name of the track they go on.
pub const TRACK: &str = "Transiciones";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Riser,
    Downlifter,
    Impact,
    ReverseCymbal,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Riser, Kind::Downlifter, Kind::Impact, Kind::ReverseCymbal];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Riser => "Subida",
            Kind::Downlifter => "Bajada",
            Kind::Impact => "Impacto",
            Kind::ReverseCymbal => "Platillo invertido",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Kind::Riser => "Ruido y un tono que suben hasta aquí (riser)",
            Kind::Downlifter => "Ruido y un tono que caen desde aquí (downlifter)",
            Kind::Impact => "Un golpe grave con su cola, aquí",
            Kind::ReverseCymbal => "Un platillo al revés que crece hasta aquí",
        }
    }

    /// Lengths offered, in bars (an impact has its own: two).
    fn bars(self) -> &'static [u32] {
        match self {
            Kind::Riser => &[1, 2, 4, 8],
            Kind::Downlifter | Kind::ReverseCymbal => &[1, 2, 4],
            Kind::Impact => &[2],
        }
    }

    /// Whether it ends at the point (else it starts there).
    fn leads_in(self) -> bool {
        matches!(self, Kind::Riser | Kind::ReverseCymbal)
    }
}

/// A small, repeatable noise source (the same transition sounds the same).
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// A state-variable filter (Simper's): low-, band- and high-pass at once.
#[derive(Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// (low, band, high) of `x` with cutoff `hz` and resonance `q`.
    fn run(&mut self, x: f32, hz: f32, q: f32, rate: f32) -> (f32, f32, f32) {
        let g = (PI * hz.clamp(10.0, rate * 0.45) / rate).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + g * a1 * v3;
        let v2 = self.ic2 + g * v1;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - k * v1 - v2)
    }
}

/// A naive sawtooth's next sample at `phase` (0..1), moved on by `step`.
fn saw(phase: &mut f32, step: f32) -> f32 {
    *phase = (*phase + step).fract();
    *phase * 2.0 - 1.0
}

/// `kind`, `seconds` long, at `rate`: left and right, peaking at -6 dBFS.
/// `tonic`: the key's (0 = C), for the tone.
pub fn render(kind: Kind, seconds: f64, rate: f32, tonic: u8) -> (Vec<f32>, Vec<f32>) {
    let n = ((seconds * rate as f64) as usize).max(1);
    let (mut left, mut right) = (vec![0.0f32; n], vec![0.0f32; n]);
    let (mut noise_l, mut noise_r) = (Noise(0x9E37_79B9), Noise(0x85EB_CA6B));
    let (mut filter_l, mut filter_r) = (Svf::default(), Svf::default());
    // The key's tonic around 110-208 Hz (A2 to G#3).
    let root = 110.0 * 2f32.powf(((tonic as i32 - 9).rem_euclid(12)) as f32 / 12.0);
    match kind {
        Kind::Riser | Kind::Downlifter => {
            let rising = kind == Kind::Riser;
            let mut tone = Svf::default();
            let mut phases = [0.0f32, 0.31, 0.67];
            for i in 0..n {
                let x = i as f32 / n as f32;
                // Where it is in its sweep: 0 low and quiet, 1 bright and loud.
                let up = if rising { x } else { 1.0 - x };
                let cutoff = 300.0 * (14_000.0f32 / 300.0).powf(up.powf(1.6));
                let level = 0.03 + 0.97 * up * up;
                let (_, band_l, _) = filter_l.run(noise_l.next(), cutoff, 2.5, rate);
                let (_, band_r, _) = filter_r.run(noise_r.next(), cutoff, 2.5, rate);
                // A detuned saw two octaves up (or down) over it.
                let hz = root * 2f32.powf(2.0 * up);
                let mut saws = 0.0;
                for (k, phase) in phases.iter_mut().enumerate() {
                    let detune = [1.0, 1.006, 0.994][k];
                    saws += saw(phase, hz * detune / rate);
                }
                let (low, _, _) = tone.run(saws / 3.0, cutoff * 0.6, 0.9, rate);
                let tone = low * 0.5 * up;
                left[i] = level * (band_l * 0.8 + tone);
                right[i] = level * (band_r * 0.8 + tone);
            }
        }
        Kind::Impact => {
            let mut phase = 0.0f32;
            let mut room = Svf::default();
            for i in 0..n {
                let t = i as f32 / rate;
                // A sub that drops from 95 to 35 Hz, ringing on.
                let hz = 35.0 + 60.0 * (-t * 14.0).exp();
                phase = (phase + hz / rate).fract();
                let boom = (phase * 2.0 * PI).sin() * (-t * 1.4).exp();
                // A burst of noise, its brightness falling as it fades.
                let cutoff = 200.0 + 6_000.0 * (-t * 3.0).exp();
                let (low_l, _, _) = filter_l.run(noise_l.next(), cutoff, 0.7, rate);
                let (low_r, _, _) = filter_r.run(noise_r.next(), cutoff, 0.7, rate);
                let burst = (-t * 2.2).exp() * 0.6;
                let (body, _, _) = room.run(boom, 120.0, 0.7, rate);
                left[i] = boom * 0.9 + body * 0.2 + low_l * burst;
                right[i] = boom * 0.9 + body * 0.2 + low_r * burst;
            }
        }
        Kind::ReverseCymbal => {
            // An 808-like cymbal: six square waves out of tune with each
            // other, and noise, high-passed; it rings out, then is turned round.
            let partials = [205.3f32, 304.4, 369.6, 522.7, 540.0, 800.0].map(|f| f * 4.0);
            let mut phases = [0.0f32; 6];
            let decay = 4.5 / seconds as f32;
            for i in 0..n {
                let t = i as f32 / rate;
                let mut metal = 0.0;
                for (phase, hz) in phases.iter_mut().zip(partials) {
                    *phase = (*phase + hz / rate).fract();
                    metal += if *phase < 0.5 { 1.0 } else { -1.0 };
                }
                let env = (-t * decay).exp();
                let (_, _, high_l) = filter_l.run(metal / 6.0 * 0.5 + noise_l.next(), 7_000.0, 0.7, rate);
                let (_, _, high_r) = filter_r.run(metal / 6.0 * 0.5 + noise_r.next(), 7_200.0, 0.7, rate);
                left[i] = high_l * env;
                right[i] = high_r * env;
            }
            left.reverse();
            right.reverse();
        }
    }
    // No click where it starts or ends (a riser still ends right on the beat).
    let fade = ((0.004 * rate) as usize).min(n / 2);
    for i in 0..fade {
        let g = i as f32 / fade as f32;
        for side in [&mut left, &mut right] {
            side[i] *= g;
            side[n - 1 - i] *= g;
        }
    }
    let peak = left.iter().chain(&right).fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let gain = 0.5 / peak;
        for s in left.iter_mut().chain(right.iter_mut()) {
            *s *= gain;
        }
    }
    (left, right)
}

impl PulseApp {
    /// The track transitions go on: the one called so, or a new one.
    fn transitions_track(&mut self) -> Option<usize> {
        if let Some(t) = self.project.tracks.iter().position(|t| t.is_audio() && t.name == TRACK) {
            return Some(t);
        }
        let t = self.add_audio_track()?;
        self.project.tracks[t].name = TRACK.into();
        Some(t)
    }

    /// Makes a `kind` of `bars` bars at beat `at` (ending there, or starting
    /// there) on the Arrangement.
    pub fn add_transition(&mut self, kind: Kind, bars: u32, at: f64) {
        let tempo = self.project.tempo;
        let mut beats = bars as f64 * 4.0;
        let start = if kind.leads_in() {
            // From the song's start at the earliest: shorter, then.
            beats = beats.min(at);
            at - beats
        } else {
            at
        };
        if beats <= 0.0 {
            self.status = Some(format!("No cabe «{}» antes del principio de la canción", kind.label()));
            return;
        }
        let rate = self.audio.sample_rate();
        let tonic = self.project.key.map_or(9, |k| k.tonic);
        let (left, right) = render(kind, beats * 60.0 / tempo, rate, tonic);
        let dir = self.media_dir(crate::library::TRANSITIONS);
        let name = format!("{} {bars} compás{}", kind.label(), if bars == 1 { "" } else { "es" });
        let written = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| {
            let path = crate::library::claim(&dir.join(format!("{name}.wav")));
            crate::export::write(&path, &left, &right, rate as u32, crate::export::BitDepth::Float32, 1.0).map(|_| path)
        });
        let path = match written {
            Ok(path) => path,
            Err(error) => {
                self.status = Some(format!("No se pudo crear la transición: {error}"));
                return;
            }
        };
        let Some(track) = self.transitions_track() else { return };
        let clip = AudioClip {
            hard_start: false,
            fades: Default::default(),
            envelopes: Vec::new(),
            name,
            color: self.project.tracks[track].color,
            sample: path.to_string_lossy().into_owned(),
            length: beats,
            offset: 0.0,
            // Made at this tempo, and following it if it changes.
            warp: WarpMode::Stretch,
            original_bpm: tempo,
            warp_markers: Vec::new(),
            takes: Vec::new(),
            key: None,
            gain_db: 0.0,
            transpose: 0.0,
            muted: false,
        };
        let lane = &mut self.project.tracks[track].arrangement;
        lane.push(ArrangementClip { start, length: beats, clip: Clip::Audio(clip) });
        let index = lane.len() - 1;
        self.sync_arrangement(track);
        self.select_clip(ClipRef::Arrangement { track, index });
        self.status = Some(format!("{} en la pista «{TRACK}»", kind.label()));
    }
}

/// The «Transición» submenu for beat `at` (a bar, a marker).
pub fn menu(app: &mut PulseApp, ui: &mut eframe::egui::Ui, at: f64) {
    ui.menu_button("Transición aquí", |ui| {
        for kind in Kind::ALL {
            let bars = kind.bars();
            if let [only] = bars {
                if ui.button(kind.label()).on_hover_text(kind.hint()).clicked() {
                    app.add_transition(kind, *only, at);
                    ui.close();
                }
                continue;
            }
            ui.menu_button(kind.label(), |ui| {
                for &n in bars {
                    if ui.button(format!("{n} compás{}", if n == 1 { "" } else { "es" })).on_hover_text(kind.hint()).clicked() {
                        app.add_transition(kind, n, at);
                        ui.close();
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    const RATE: f32 = 48_000.0;

    /// RMS of `samples` in `parts` equal parts.
    fn loudness(samples: &[f32], parts: usize) -> Vec<f32> {
        samples.chunks(samples.len() / parts).take(parts).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).collect()
    }

    /// How bright `samples` are: the share of their energy in the change from
    /// one sample to the next (high frequencies).
    fn brightness(samples: &[f32]) -> f32 {
        let total: f32 = samples.iter().map(|s| s * s).sum();
        let edges: f32 = samples.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        edges / total.max(1e-9)
    }

    #[test]
    fn each_transition_goes_the_way_it_should() {
        for kind in Kind::ALL {
            let (left, right) = render(kind, 2.0, RATE, 9);
            assert_eq!(left.len(), 96_000);
            let peak = left.iter().chain(&right).fold(0.0f32, |m, s| m.max(s.abs()));
            assert!((peak - 0.5).abs() < 1e-3, "{kind:?}: peak {peak}");
            assert!(left.iter().chain(&right).all(|s| s.is_finite()));
            assert_ne!(left, right, "{kind:?}: wide, not mono");
            let parts = loudness(&left, 8);
            let (first, last) = (parts[0], parts[7]);
            match kind {
                Kind::Riser | Kind::ReverseCymbal => assert!(last > first * 4.0, "{kind:?} grows: {parts:?}"),
                Kind::Downlifter | Kind::Impact => assert!(first > last * 4.0, "{kind:?} fades: {parts:?}"),
            }
        }
        // A riser brightens as it climbs; a downlifter darkens.
        let (riser, _) = render(Kind::Riser, 2.0, RATE, 9);
        assert!(brightness(&riser[86_000..95_000]) > brightness(&riser[10_000..19_000]) * 3.0);
        let (down, _) = render(Kind::Downlifter, 2.0, RATE, 9);
        assert!(brightness(&down[1_000..10_000]) > brightness(&down[76_000..85_000]) * 3.0);
        // The same each time.
        assert_eq!(render(Kind::Impact, 0.5, RATE, 0), render(Kind::Impact, 0.5, RATE, 0));
    }

    #[test]
    fn a_riser_ends_on_the_bar_on_a_track_of_its_own() {
        let mut app = PulseApp::headless(Project::demo());
        let tracks = app.project.tracks.len();
        app.add_transition(Kind::Riser, 4, 32.0);
        assert_eq!(app.project.tracks.len(), tracks + 1);
        let t = app.project.tracks.len() - 1;
        assert_eq!(app.project.tracks[t].name, TRACK);
        let placed = app.project.tracks[t].arrangement[0].clone();
        assert_eq!((placed.start, placed.length), (16.0, 16.0));
        let Clip::Audio(audio) = &placed.clip else { panic!() };
        let sample = app.sample(&audio.sample);
        let seconds = 16.0 * 60.0 / app.project.tempo;
        assert!((sample.duration() - seconds).abs() < 0.01, "{} s, not {seconds}", sample.duration());

        // The next ones go on the same track; an impact starts at its point;
        // a riser near the start is cut to fit.
        app.add_transition(Kind::Impact, 2, 32.0);
        app.add_transition(Kind::Riser, 8, 8.0);
        assert_eq!(app.project.tracks.len(), tracks + 1);
        let lane = &app.project.tracks[t].arrangement;
        assert_eq!((lane[1].start, lane[1].length), (32.0, 8.0));
        assert_eq!((lane[2].start, lane[2].length), (0.0, 8.0));
        app.add_transition(Kind::ReverseCymbal, 1, 0.0);
        assert_eq!(app.project.tracks[t].arrangement.len(), 3, "nothing fits before the start");
        let _ = std::fs::remove_dir_all(app.media_dir(crate::library::TRANSITIONS));
    }
}
