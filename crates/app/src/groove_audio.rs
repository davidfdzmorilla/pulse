//! A groove from a recording: where a drum loop's (or a beatbox's) hits fall
//! against the grid and how hard, step by step, as a groove MIDI clips can
//! play with (see `engine::model::GrooveTemplate`).

use crate::app::{ClipRef, PulseApp};
use crate::project::SavedGroove;
use engine::model::{Clip, Groove, GrooveTemplate, GROOVE_STEPS};

/// Where (beats after an eighth) a hit falls to be an in-between sixteenth,
/// straight (0.25) to a triplet's swing (0.33) and past it: a loop with
/// enough of them swings in sixteenths, otherwise in eighths (whose shuffle
/// comes at most a sixth of a beat after the eighth).
const SIXTEENTH: std::ops::Range<f64> = 0.2..0.42;
/// Hits quieter than this (of the loudest) are left out: noise, a tail.
const QUIET: f32 = 0.05;

/// The groove of `hits` (beat in the clip, how loud). None with too few.
pub fn extract(hits: &[(f64, f32)]) -> Option<Groove> {
    if hits.len() < 4 {
        return None;
    }
    // Eighths or sixteenths: whether hits fall between the eighths (the
    // recording's lateness as a whole aside: how late those near them are).
    let mut near: Vec<f64> = hits.iter().map(|(beat, _)| beat - (beat * 2.0).round() / 2.0).filter(|d| d.abs() < 0.06).collect();
    near.sort_by(f64::total_cmp);
    let late = near.get(near.len() / 2).copied().unwrap_or(0.0);
    let between = hits.iter().filter(|(beat, _)| SIXTEENTH.contains(&(beat - late).rem_euclid(0.5))).count();
    let grid = if between * 100 >= 15 * hits.len() { 0.25 } else { 0.5 };
    let mut shift = [0f64; GROOVE_STEPS];
    let mut loud = [0f64; GROOVE_STEPS];
    let mut count = [0usize; GROOVE_STEPS];
    for &(beat, level) in hits {
        let step = (beat / grid).round();
        let k = (step as i64).rem_euclid(GROOVE_STEPS as i64) as usize;
        shift[k] += beat / grid - step;
        loud[k] += level as f64;
        count[k] += 1;
    }
    let mean = |sum: &[f64; GROOVE_STEPS], k: usize| (count[k] > 0).then(|| sum[k] / count[k] as f64);
    // On the beat is on time: a recording late (or early) as a whole doesn't
    // move the clips, only how its steps differ from its beats.
    let per_beat = (1.0 / grid) as usize;
    let mut on_beat: Vec<f64> = (0..GROOVE_STEPS).step_by(per_beat).filter_map(|k| mean(&shift, k)).collect();
    on_beat.sort_by(f64::total_cmp);
    let late = on_beat.get(on_beat.len() / 2).copied().unwrap_or(0.0);
    let loudest = (0..GROOVE_STEPS).filter_map(|k| mean(&loud, k)).fold(0.0, f64::max).max(1e-9);
    let heard: Vec<f64> = (0..GROOVE_STEPS).filter_map(|k| mean(&loud, k)).collect();
    // A step never played: on time, as hard as the rest on average.
    let usual = heard.iter().sum::<f64>() / heard.len().max(1) as f64 / loudest;
    let template = GrooveTemplate {
        shift: std::array::from_fn(|k| mean(&shift, k).map_or(0.0, |s| (s - late).clamp(-0.5, 0.5)) as f32),
        velocity: std::array::from_fn(|k| mean(&loud, k).map_or(usual, |l| l / loudest) as f32),
    };
    Some(Groove { grid, swing: 1.0, accent: 1.0, humanize: 0.0, template: Some(template) })
}

/// How a groove sounds, in a few words: its grid and how late its in-between
/// steps come (% of a step).
pub fn describe(groove: &Groove) -> String {
    let grid = if groove.grid < 0.375 { "semicorcheas" } else { "corcheas" };
    let Some(t) = groove.template else { return grid.to_string() };
    let off: Vec<f32> = (1..GROOVE_STEPS).step_by(2).map(|k| t.shift[k]).collect();
    let late = off.iter().sum::<f32>() / off.len() as f32;
    match (late * 100.0).round() as i32 {
        0 => format!("recto, en {grid}"),
        l if l > 0 => format!("en {grid}, los contratiempos un {l} % de paso tarde"),
        l => format!("en {grid}, los contratiempos un {} % de paso pronto", -l),
    }
}

impl PulseApp {
    /// Takes the groove of audio clip `clip` and keeps it in the project, for
    /// MIDI clips to play with.
    pub fn take_groove(&mut self, clip: ClipRef) {
        let Some(Clip::Audio(audio)) = self.clip(clip).cloned() else { return };
        let sample = self.sample(&audio.sample);
        if sample.frames() == 0 {
            self.status = Some("No se encuentra el audio del clip".into());
            return;
        }
        let tempo = self.project.tempo;
        let rate = sample.sample_rate;
        let from = audio.offset.max(0.0);
        let to = audio.source_seconds(audio.length, tempo).min(sample.duration());
        let (a, b) = ((from * rate as f64) as usize, ((to * rate as f64) as usize).min(sample.frames()));
        if b <= a + 64 {
            self.status = Some("El clip es demasiado corto para sacar su groove".into());
            return;
        }
        let (left, right) = (&sample.left[a..b], &sample.right[a..b]);
        // Each hit and its peak (in the 30 ms after it starts).
        let after = (0.03 * rate) as usize;
        let peaks: Vec<(usize, f32)> = crate::slicing::transients(left, right, rate, usize::MAX)
            .into_iter()
            .map(|f| (f, left[f..(f + after).min(left.len())].iter().chain(&right[f..(f + after).min(right.len())]).fold(0f32, |m, x| m.max(x.abs()))))
            .collect();
        let loudest = peaks.iter().map(|p| p.1).fold(0.0, f32::max);
        let hits: Vec<(f64, f32)> = peaks
            .into_iter()
            .filter(|&(_, peak)| peak >= QUIET * loudest && peak > 0.0)
            .map(|(f, peak)| (audio.beats_until(from + f as f64 / rate as f64, tempo), peak))
            .filter(|&(beat, _)| beat > -0.25 && beat < audio.length)
            .collect();
        let Some(groove) = extract(&hits) else {
            self.status = Some(format!("En «{}» no hay golpes suficientes para sacar un groove", audio.name));
            return;
        };
        // Its name: the clip's, numbered if there is one already.
        let mut name = audio.name.clone();
        let mut n = 2;
        while self.project.grooves.iter().any(|g| g.name == name) {
            name = format!("{} {n}", audio.name);
            n += 1;
        }
        self.status = Some(format!("Groove «{name}» guardado ({}): elígelo en Groove de cualquier clip MIDI", describe(&groove)));
        self.project.grooves.push(SavedGroove { name, groove });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two bars of sixteenths played MPC-style: the in-between ones 30 % of a
    /// step late and softer, all of it 10 ms late (the clip's start).
    fn mpc() -> Vec<(f64, f32)> {
        (0..32)
            .map(|k| {
                let late = if k % 2 == 1 { 0.3 * 0.25 } else { 0.0 };
                (k as f64 * 0.25 + late + 0.02, if k % 2 == 1 { 0.5 } else { 1.0 })
            })
            .collect()
    }

    #[test]
    fn a_swung_loop_gives_its_groove_step_by_step() {
        let groove = extract(&mpc()).unwrap();
        assert_eq!(groove.grid, 0.25);
        let t = groove.template.unwrap();
        for k in 0..GROOVE_STEPS {
            let (shift, velocity) = if k % 2 == 1 { (0.3, 0.5) } else { (0.0, 1.0) };
            assert!((t.shift[k] - shift).abs() < 1e-4, "step {k}: {}", t.shift[k]);
            assert!((t.velocity[k] - velocity).abs() < 1e-4, "step {k}: {}", t.velocity[k]);
        }
        assert_eq!(describe(&groove), "en semicorcheas, los contratiempos un 30 % de paso tarde");
        // A note on the 2nd sixteenth plays 30 % of a step late and at half
        // the velocity; one on the beat as written.
        let (shift, velocity) = groove.apply(0, 0, 0.25, 0.8);
        assert!((shift - 0.075).abs() < 1e-6 && (velocity - 0.4).abs() < 1e-6, "{shift} {velocity}");
        assert_eq!(groove.apply(0, 0, 1.0, 0.8), (0.0, 0.8));
        // Half of it: half as late, three quarters as loud.
        let half = Groove { swing: 0.5, accent: 0.5, ..groove };
        let (shift, velocity) = half.apply(0, 0, 0.25, 0.8);
        assert!((shift - 0.0375).abs() < 1e-6 && (velocity - 0.6).abs() < 1e-6, "{shift} {velocity}");
    }

    /// A shuffle in eighths (a triplet's last third): eighths, not sixteenths.
    #[test]
    fn a_shuffle_is_in_eighths() {
        let hits: Vec<(f64, f32)> = (0..16).map(|k| (k as f64 * 0.5 + if k % 2 == 1 { 1.0 / 6.0 } else { 0.0 }, 1.0)).collect();
        let groove = extract(&hits).unwrap();
        assert_eq!(groove.grid, 0.5);
        let t = groove.template.unwrap();
        assert!((t.shift[1] - 1.0 / 3.0).abs() < 1e-4, "{}", t.shift[1]);
        assert_eq!(t.shift[0], 0.0);
        assert!(extract(&hits[..3]).is_none(), "three hits are no groove");
    }

    /// A recorded loop (clicks, two bars at 120, the in-between sixteenths
    /// 30 % of a step late and half as loud) from the clip's menu: its groove
    /// kept in the project, saved and opened again as it was.
    #[test]
    fn a_recorded_loop_gives_its_groove_to_the_project() {
        use engine::model::{AudioClip, WarpMode};
        let rate = 48_000.0f32;
        let mut audio = vec![0f32; (4.2 * rate) as usize];
        let mut seed = 7u32;
        for k in 0..32 {
            let beat = k as f64 * 0.25 + if k % 2 == 1 { 0.3 * 0.25 } else { 0.0 };
            let (at, gain) = ((beat * 0.5 * rate as f64) as usize, if k % 2 == 1 { 0.25 } else { 0.5 });
            for i in 0..(0.02 * rate) as usize {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let noise = (seed >> 8) as f32 / (1 << 23) as f32 - 1.0;
                audio[at + i] += gain * noise * (-(i as f32) / (0.004 * rate)).exp();
            }
        }
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-media");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("groove-loop-{}.wav", std::process::id()));
        crate::export::write(&path, &audio, &audio, rate as u32, crate::export::BitDepth::Float32, 1.0).unwrap();
        let mut project = crate::project::Project::empty();
        project.tempo = 120.0;
        let mut track = crate::project::Track::new_audio("Loop", [0; 3]);
        track.clips[0] = Some(Clip::Audio(AudioClip { hard_start: false, fades: Default::default(), name: "Break".into(), color: [0; 3], sample: path.to_string_lossy().into_owned(), length: 8.0, offset: 0.0, warp: WarpMode::Off, original_bpm: 120.0, gain_db: 0.0, transpose: 0.0, envelopes: Vec::new(), warp_markers: Vec::new(), takes: Vec::new(), key: None, muted: false }));
        project.tracks = vec![track];
        let mut app = PulseApp::headless(project);
        app.take_groove(ClipRef::Session { track: 0, slot: 0 });
        println!("{:?}", app.status);
        let saved = app.project.grooves.first().cloned().unwrap_or_else(|| panic!("{:?}", app.status));
        assert_eq!(saved.name, "Break");
        assert_eq!(saved.groove.grid, 0.25);
        let t = saved.groove.template.unwrap();
        println!("{:?}\n{:?}", t.shift, t.velocity);
        for k in 0..GROOVE_STEPS {
            let (shift, velocity) = if k % 2 == 1 { (0.3, 0.5) } else { (0.0, 1.0) };
            assert!((t.shift[k] - shift).abs() < 0.03, "step {k}: {}", t.shift[k]);
            // (Each click is noise: its peak varies a little from hit to hit.)
            assert!((t.velocity[k] - velocity).abs() < 0.1, "step {k}: {}", t.velocity[k]);
        }
        assert!(app.status.as_deref().unwrap_or("").contains("un 30 % de paso tarde"), "{:?}", app.status);
        // A second one is numbered.
        app.take_groove(ClipRef::Session { track: 0, slot: 0 });
        assert_eq!(app.project.grooves[1].name, "Break 2");
        // Saved: a Pulse that reads format 10 would lose it, so it is 11.
        let text = app.project.to_json().unwrap();
        assert!(text.contains("\"format\": 11"));
        let again = crate::project::Project::from_json(&text).unwrap();
        assert_eq!(again.grooves, app.project.grooves);
        // Without one, the format it was.
        app.project.grooves.clear();
        assert!(app.project.to_json().unwrap().contains("\"format\": 2"));
    }
}
