//! «Sonidos de Pulse» measured: every file clean, even within its kind and in
//! tune, every kit full and playing in the Drum Pads.

use crate::factory_recipes::{recipes, Role, RATE};
use crate::factory_sounds::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// What is measured of a file.
pub struct Measure {
    pub peak_db: f32,
    pub loudness_db: f32,
    pub seconds: f32,
    /// Its mean against its peak.
    pub dc: f32,
    /// The first sample and the last millisecond, dBFS.
    pub start_db: f32,
    pub end_db: f32,
    /// How long it goes on under -60 dB of its peak at the end, seconds.
    pub quiet_tail: f32,
    pub finite: bool,
    pub bits: u16,
}

pub fn measure(path: &Path) -> Measure {
    let bits = hound::WavReader::open(path).map(|r| r.spec().bits_per_sample).unwrap_or(0);
    let sample = engine::Sample::load(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (l, r) = (&sample.left, &sample.right);
    let top = peak(l, r);
    let mean = l.iter().chain(r.iter()).map(|&s| s as f64).sum::<f64>() / (2 * l.len()).max(1) as f64;
    let end = l.len().saturating_sub((0.001 * RATE) as usize);
    let floor = top * 10f32.powf(TAIL_DB / 20.0);
    let last = l.iter().zip(r.iter()).rposition(|(a, b)| a.abs().max(b.abs()) > floor).map_or(0, |i| i + 1);
    Measure {
        peak_db: db(top),
        loudness_db: loudness(l, r),
        seconds: l.len() as f32 / RATE,
        dc: (mean.abs() as f32) / top.max(1e-9),
        start_db: db(l[0].abs().max(r[0].abs())),
        end_db: db(peak(&l[end..], &r[end..])),
        quiet_tail: (l.len() - last) as f32 / RATE,
        finite: l.iter().chain(r.iter()).all(|s| s.is_finite()),
        bits,
    }
}

/// How far from `note` (cents) the sound's pitch is: where the energy of the
/// strongest of its first harmonics is centred, within a third of a semitone
/// of where `note` would put it (a stack of detuned voices is heard at its
/// centre), over a steady stretch.
pub fn cents_off(path: &Path, note: u8, role: Role) -> f32 {
    let sample = engine::Sample::load(path).unwrap();
    let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(a, b)| (a + b) * 0.5).collect();
    // After the attack (and a 808's pitch drop).
    let from = ((if role == Role::Sub { 0.25 } else { 0.06 }) * RATE) as usize;
    let length = (mono.len().saturating_sub(from)).min(RATE as usize);
    let size = 1 << 17;
    let (mut re, mut im) = (vec![0f32; size], vec![0f32; size]);
    for i in 0..length {
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / length as f32).cos();
        re[i] = mono[from + i] * w;
    }
    engine::fft(&mut re, &mut im);
    let magnitude = |bin: usize| (re[bin] * re[bin] + im[bin] * im[bin]).sqrt();
    let hz_per_bin = RATE / size as f32;
    let f0 = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
    let harmonics: &[f32] = if role == Role::Stab { &[1.0, 2.0] } else { &[1.0, 2.0, 3.0, 4.0] };
    let mut best = (0f32, f0);
    for &h in harmonics {
        let (low, high) = ((f0 * h * 2f32.powf(-0.35 / 12.0) / hz_per_bin) as usize, (f0 * h * 2f32.powf(0.35 / 12.0) / hz_per_bin) as usize);
        let energy: f32 = (low..=high).map(|b| magnitude(b).powi(2)).sum();
        let centre = (low..=high).map(|b| b as f32 * magnitude(b).powi(2)).sum::<f32>() / energy.max(1e-12);
        if energy > best.0 {
            best = (energy, centre * hz_per_bin / h);
        }
    }
    1200.0 * (best.1 / f0).log2()
}

/// Brightness and length of a sound: the spectral centroid of its first
/// 200 ms (Hz), and how long it takes to fall 20 dB from its loudest 10 ms.
fn character(path: &Path) -> (f32, f32) {
    let sample = engine::Sample::load(path).unwrap();
    let mono: Vec<f32> = sample.left.iter().zip(&sample.right).map(|(a, b)| (a + b) * 0.5).collect();
    let size = 1 << 14;
    let (mut re, mut im) = (vec![0f32; size], vec![0f32; size]);
    let n = mono.len().min((0.2 * RATE) as usize).min(size);
    re[..n].copy_from_slice(&mono[..n]);
    engine::fft(&mut re, &mut im);
    let (mut sum, mut weighted) = (0f32, 0f32);
    for bin in 1..size / 2 {
        let m = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt();
        sum += m;
        weighted += m * bin as f32 * RATE / size as f32;
    }
    let block = (0.01 * RATE) as usize;
    let levels: Vec<f32> = mono.chunks(block).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).collect();
    let loudest = levels.iter().cloned().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)).map_or(0, |m| m.0);
    let fall = levels[loudest..].iter().position(|&v| v < levels[loudest] * 0.1).unwrap_or(levels.len() - loudest);
    (weighted / sum.max(1e-9), fall as f32 * 0.01)
}

/// Everything made into a fresh folder, timed: (folder, seconds).
fn make_library(dir: &Path) -> f32 {
    let _ = std::fs::remove_dir_all(dir);
    let started = std::time::Instant::now();
    make_all(dir, &AtomicUsize::new(0), &AtomicBool::new(false)).unwrap();
    started.elapsed().as_secs_f32()
}

/// Every check, as a list of problems (empty: all fine).
fn problems(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for recipe in recipes() {
        let path = file(dir, &recipe);
        let name = format!("{}/{}", recipe.folder, recipe.name);
        if !path.exists() {
            problems.push(format!("{name}: missing"));
            continue;
        }
        let m = measure(&path);
        let mut fail = |what: String| problems.push(format!("{name}: {what}"));
        if !m.finite {
            fail("not finite".into());
        }
        if m.bits != 24 {
            fail(format!("{} bits", m.bits));
        }
        if m.peak_db > CEILING_DB + 0.01 {
            fail(format!("peak {:.2} dBFS", m.peak_db));
        }
        if m.dc > 0.01 {
            fail(format!("DC {:.4} of its peak", m.dc));
        }
        if m.start_db > -60.0 {
            fail(format!("starts at {:.1} dBFS (a click)", m.start_db));
        }
        if m.end_db > -30.0 {
            fail(format!("ends at {:.1} dBFS (a click)", m.end_db));
        }
        if recipe.length.is_none() && m.quiet_tail > 0.03 {
            fail(format!("{:.2} s of tail under -60 dB", m.quiet_tail));
        }
        if let Some(seconds) = recipe.length {
            if (m.seconds - seconds).abs() > 0.001 {
                fail(format!("{:.3} s long, not {seconds}", m.seconds));
            }
        }
        let spread = if recipe.role == Role::Fx { 3.0 } else { 2.0 };
        if (m.loudness_db - target(recipe.role)).abs() > spread {
            fail(format!("loudness {:.1} dB, its kind's is {:.1}", m.loudness_db, target(recipe.role)));
        }
        if let Some(note) = recipe.note {
            let cents = cents_off(&path, note, recipe.role);
            if cents.abs() > 10.0 {
                fail(format!("{cents:+.1} cents off its note"));
            }
        }
    }
    // Kits: every pad filled with a sound that is made, the loop sounding.
    let all = recipes();
    for kit in kits() {
        for &(folder, name, level) in &kit.pads {
            if !all.iter().any(|r| r.folder == folder && r.name == name) {
                problems.push(format!("Kit {}: no sound {folder}/{name}", kit.name));
            }
            if !(0.0..=1.0).contains(&level) {
                problems.push(format!("Kit {}: level {level}", kit.name));
            }
        }
        for &(pitch, steps) in kit.pattern {
            if engine::model::rack_pad(pitch).is_none() || steps.chars().filter(|c| !c.is_whitespace()).count() != 32 {
                problems.push(format!("Kit {}: pattern of {pitch}", kit.name));
            }
        }
        let path = loop_file(dir, &kit);
        let sample = engine::Sample::load(&path).unwrap();
        let bars = sample.duration() * kit.tempo / 240.0;
        let rms = db((sample.left.iter().map(|s| s * s).sum::<f32>() / sample.frames() as f32).sqrt());
        if (bars - LOOP_BARS).abs() > 1e-3 || crate::tempo_ui::name_bpm(&path.file_stem().unwrap().to_string_lossy(), sample.duration()) != Some(kit.tempo) {
            problems.push(format!("Loop {}: {bars:.3} bars, its tempo not in its name", kit.name));
        }
        if !(-24.0..=-6.0).contains(&rms) {
            problems.push(format!("Loop {}: {rms:.1} dB RMS", kit.name));
        }
        let preset: crate::presets::Preset = serde_json::from_str(&std::fs::read_to_string(dir.join(KITS).join(format!("{}.json", kit.name))).unwrap()).unwrap();
        if preset.samples.iter().filter(|f| Path::new(f).exists()).count() != engine::model::RACK_PADS {
            problems.push(format!("Kit {}: its preset misses pads", kit.name));
        }
    }
    problems
}

fn size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| if e.path().is_dir() { size(&e.path()) } else { e.metadata().map_or(0, |m| m.len()) }).sum()
}

#[test]
fn the_factory_library_is_clean_even_and_in_tune() {
    let dir = std::env::temp_dir().join(format!("pulse-sonidos-{}", std::process::id()));
    let seconds = make_library(&dir);
    let problems = problems(&dir);
    // Made again, nothing changes: what is there stays.
    let before = std::fs::metadata(file(&dir, &recipes()[0])).unwrap().modified().unwrap();
    make_all(&dir, &AtomicUsize::new(0), &AtomicBool::new(false)).unwrap();
    let after = std::fs::metadata(file(&dir, &recipes()[0])).unwrap().modified().unwrap();
    let bytes = size(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    assert_eq!(before, after, "made files are not made again");
    assert!(bytes < 150 << 20, "{} MB", bytes >> 20);
    eprintln!("Sonidos de Pulse: {seconds:.1} s, {:.1} MB", bytes as f64 / 1048576.0);
}

#[test]
fn every_sound_is_the_same_each_time_it_is_made() {
    // A few of each source: the drum machine, the kick designer, the engine.
    let all = recipes();
    for name in ["Caja trap", "808 largo (Do)", "Stab de house (Do menor 7)", "Ruido de vinilo"] {
        let recipe = all.iter().find(|r| r.name == name).unwrap();
        let (a, b) = ((recipe.make)(), (recipe.make)());
        assert!(a == b, "{name} changes between renders");
    }
}

#[test]
fn a_factory_kit_loads_into_the_drum_pads_and_plays() {
    let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
    let kit = kits().iter().position(|k| k.name == "Techno").unwrap();
    // Made in the background, then loaded.
    app.load_factory_kit(0, kit);
    let started = std::time::Instant::now();
    let ctx = eframe::egui::Context::default();
    while app.factory.busy() && started.elapsed().as_secs() < 60 {
        app.poll_factory_sounds(&ctx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let track = &app.project.tracks[0];
    assert!(matches!(track.instrument, engine::model::InstrumentKind::DrumRack(_)));
    assert_eq!(track.samples.iter().filter(|f| Path::new(f).exists()).count(), engine::model::RACK_PADS);
    // Its pattern through the engine: it sounds, and not too loud.
    let (l, r) = render_loop(&root(), &kits()[kit]);
    let rms = db((l.iter().chain(&r).map(|s| s * s).sum::<f32>() / (2 * l.len()) as f32).sqrt());
    assert!((-24.0..=-6.0).contains(&rms), "{rms} dB RMS");
    std::fs::remove_dir_all(root().parent().unwrap()).ok();
}

#[test]
fn a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there() {
    use eframe::egui::{pos2, Rect};
    let dir = std::env::temp_dir().join(format!("pulse-kit-drop-{}", std::process::id()));
    let kit = &kits()[0];
    let preset = dir.join(format!("{}.json", kit.name));
    std::fs::create_dir_all(&dir).unwrap();
    crate::store::write_json(&preset, &kit_preset(&dir, kit)).unwrap();
    let mut app = crate::app::PulseApp::headless(crate::project::Project::demo());
    let midi = (0..app.project.tracks.len()).find(|&t| !app.project.tracks[t].is_audio() && !app.project.tracks[t].is_group()).unwrap();
    let lane = Rect::from_min_max(pos2(300.0, 300.0), pos2(900.0, 350.0));
    app.drop_targets = crate::browser::DropTargets { slots: Vec::new(), lanes: vec![(midi, lane, 300.0, 14.0)], view: None };
    app.drop_from_browser(crate::browser::Dragged(preset), lane.center());
    std::fs::remove_dir_all(&dir).ok();
    let track = &app.project.tracks[midi];
    assert!(matches!(track.instrument, engine::model::InstrumentKind::DrumRack(_)), "{:?}", app.status);
    assert_eq!(track.samples[0], dir.join("Bombos/Bombo house.wav").to_string_lossy());
}

/// The whole library and a table of what was measured, to listen to:
/// `PULSE_SONIDOS=dir cargo test --release -p app factory_listening_set -- --ignored --nocapture`
#[test]
#[ignore]
fn factory_listening_set() {
    let Ok(dir) = std::env::var("PULSE_SONIDOS") else { return };
    let dir = PathBuf::from(dir);
    let seconds = make_library(&dir);
    for recipe in recipes() {
        let path = file(&dir, &recipe);
        let m = measure(&path);
        let cents = recipe.note.map(|n| format!("{:+5.1} c", cents_off(&path, n, recipe.role))).unwrap_or_default();
        println!(
            "{:18} {:40} pico {:6.2}  sonoridad {:6.1} ({:+4.1})  {:5.2} s  centroide {:6.0} Hz  caída {:4.2} s  {cents}",
            recipe.folder,
            recipe.name,
            m.peak_db,
            m.loudness_db,
            m.loudness_db - target(recipe.role),
            m.seconds,
            character(&path).0,
            character(&path).1,
        );
    }
    let all = recipes();
    for recipe in &all {
        let ((l, r), report) = finish((recipe.make)(), recipe.role, recipe.length);
        let _ = (l, r);
        if report.limited_db > 0.5 {
            println!("limitado {:5.1} dB  {}", report.limited_db, recipe.name);
        }
    }
    let problems = problems(&dir);
    println!("{}", problems.join("\n"));
    println!("{seconds:.1} s, {:.1} MB", size(&dir) as f64 / 1048576.0);
}
