//! The README's pictures: the example song (see `example`: factory kits, a
//! bass, chords, a pad and a lead from «Acompañar», a sung voice with its
//! lyrics, a drum group, two returns), with a longer arrangement with
//! sections and automation, drawn without a window and saved as PNG. Fixed
//! seeds and no clock: the same pictures every run.
//! `cargo test --release -p app readme_shots -- --ignored` writes them in
//! `docs/capturas/` (or in `PULSE_SHOTS`).

use crate::app::{ClipRef, Detail, PulseApp, View};
use crate::example::{BREAK, DROP};
use crate::produccion::Studio;
use crate::project::Project;
use eframe::egui::{pos2, Modifiers};
use engine::model::{ArrangementClip, AutomationLane, AutomationPoint, MidiControl, Param};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Where the pictures go.
fn out_dir() -> PathBuf {
    std::env::var_os("PULSE_SHOTS").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/capturas"))
}

/// The song: the example's tracks, scenes and mix, before its arrangement.
fn project(voice: &Path) -> (Project, (), f64) {
    let (project, beats) = crate::example::tracks(voice).unwrap();
    (project, (), beats)
}

/// The arrangement from the scenes, the voice on the breaks, risers and
/// impacts into the drops, and the pad's filter opening on the builds.
fn arrange(app: &mut PulseApp, voice_beats: f64) {
    let sections = crate::arrange::plan(&app.project.tracks, &app.project.scene_names, crate::arrange::Template::Electronic);
    app.lay_out(&sections);
    let markers = app.project.markers.clone();
    let at = |section: &str| markers.iter().filter(|m| m.name == crate::arrange::marker_name(section)).map(|m| m.beat).collect::<Vec<_>>();
    let vocal = app.project.tracks.iter().position(|t| crate::i18n::is_named(&t.name, "Voz")).unwrap();
    let clip = app.project.tracks[vocal].clips[BREAK].clone().unwrap();
    app.project.tracks[vocal].arrangement = at("Break").into_iter().chain(at("Drop").into_iter().skip(1)).map(|start| ArrangementClip { start, length: voice_beats, clip: clip.clone() }).collect();
    for drop in at("Drop") {
        app.add_transition(crate::transitions::Kind::Riser, 8, drop);
        app.add_transition(crate::transitions::Kind::Impact, 1, drop);
    }
    let pad = app.project.tracks.iter().position(|t| t.name == "Pad").unwrap();
    let mut lane = AutomationLane::new(Param::Device { device: 0, param: 0 });
    let point = |beat: f64, value: f32, curve: f32| AutomationPoint { beat, value, curve };
    for (name, from, to) in [("Subida", 500.0, 12_000.0), ("Break", 900.0, 4_000.0)] {
        for start in at(name) {
            lane.points.push(point(start, from, -0.5));
            lane.points.push(point(start + if name == "Break" { 60.0 } else { 31.0 }, to, 0.0));
        }
    }
    for start in at("Drop") {
        lane.points.push(point(start, 12_000.0, 0.0));
        lane.points.push(point(start + 63.0, 12_000.0, 0.0));
    }
    lane.points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    app.project.tracks[pad].automation.push(lane);
    app.sync_all();
}

#[test]
#[ignore]
fn readme_shots() {
    let out = out_dir();
    std::fs::create_dir_all(&out).unwrap();
    let work = std::env::temp_dir().join(format!("pulse-readme-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let (project, _, voice_beats) = project(&work.join("Voz principal.wav"));
    let mut app = PulseApp::headless(project);
    arrange(&mut app, voice_beats);
    let mut s = Studio::new(app, work.clone());
    let save = |s: &mut Studio, name: &str| {
        s.app.status = None;
        let shot = s.shot(name);
        let path = out.join(format!("{name}.png"));
        compact_png(&shot, &path);
        s.note(format!("{} · {} KB", path.display(), std::fs::metadata(&path).unwrap().len() / 1024));
    };

    // 1. The Arrangement: the whole song.
    s.app.view = View::Arrangement;
    s.app.selected_clip = None;
    let pad = s.app.project.tracks.iter().position(|t| t.name == "Pad").unwrap();
    s.app.arrangement.automation.insert(pad, Param::Device { device: 0, param: 0 });
    s.app.arrangement.pixels_per_beat = 2.38;
    s.app.arrangement.lane_h = crate::arrangement::LANE_MIN;
    s.app.selected_target = engine::Target::Track(pad);
    s.app.detail = Detail::Devices;
    // The browser away: the whole song fits.
    s.app.show_browser = false;
    s.frames(10);
    panel_to(&mut s, 650.0);
    s.dump("arrangement");
    save(&mut s, "arrangement");

    // 2. The Session, with the mixer and the returns.
    s.app.show_browser = true;
    s.app.view = View::Session;
    let vocal = s.app.project.tracks.iter().position(|t| crate::i18n::is_named(&t.name, "Voz")).unwrap();
    s.app.selected_target = engine::Target::Track(vocal);
    s.frames(10);
    panel_to(&mut s, 600.0);
    s.dump("session");
    save(&mut s, "session");

    // 3. The piano roll: the melody and its brightness.
    let lead = s.app.project.tracks.iter().position(|t| crate::i18n::is_named(&t.name, "Melodía")).unwrap();
    s.app.select_clip(ClipRef::Session { track: lead, slot: DROP });
    s.app.detail = Detail::Clip;
    s.app.piano_roll.lane.show_control(MidiControl::Cc(74));
    s.frames(10);
    panel_to(&mut s, 250.0);
    s.app.piano_roll.recenter();
    s.frames(10);
    s.dump("piano-roll");
    save(&mut s, "piano-roll");

    // 4. Devices: the drum bus's chain, the browser away to see all of it.
    s.app.selected_clip = None;
    s.app.selected_target = engine::Target::Track(0);
    s.app.detail = Detail::Devices;
    s.app.show_browser = false;
    s.frames(10);
    panel_to(&mut s, 590.0);
    s.dump("dispositivos");
    save(&mut s, "dispositivos");

    // 5. The voice's editor with its lyrics; the pad's automation folded, so
    // the voice's row shows above the editor.
    s.app.show_browser = true;
    s.app.arrangement.automation.clear();
    s.app.view = View::Arrangement;
    s.app.select_clip(ClipRef::Arrangement { track: vocal, index: 0 });
    s.app.detail = Detail::Clip;
    s.frames(10);
    panel_to(&mut s, 520.0);
    s.dump("letra");
    save(&mut s, "editor-de-audio-con-letra");

    // 6. The mix assistant and its report.
    s.app.selected_clip = None;
    s.app.detail = Detail::Devices;
    s.menu(crate::commands::Command::MixAssistant);
    s.click("Analizar la mezcla");
    s.wait_for("el análisis", 300.0, |app| app.mix.report.is_some());
    s.move_to(pos2(1430.0, 890.0), Modifiers::NONE);
    s.frames(5);
    s.dump("asistente");
    save(&mut s, "asistente-de-mezcla");
    std::fs::remove_dir_all(&work).ok();
}

/// The detail panel's top (above its tabs) dragged to `y`.
fn panel_to(s: &mut Studio, y: f32) {
    let tabs = s.seen.iter().filter(|w| w.text == "Dispositivos" && w.role == "Button").map(|w| w.rect).next().expect("las pestañas del panel");
    s.drag(pos2(900.0, tabs.top() - 6.0), pos2(900.0, y), Modifiers::NONE);
    s.move_to(pos2(1435.0, 895.0), Modifiers::NONE);
    s.frames(10);
}

/// The PNG the canvas wrote (unfiltered, fast) again, smaller: each row with
/// the filter that leaves the least to compress, at the best compression.
fn compact_png(from: &Path, to: &Path) {
    let bytes = std::fs::read(from).unwrap();
    let mut at = 8;
    let (mut width, mut height, mut data) = (0usize, 0usize, Vec::new());
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = &bytes[at + 8..at + 8 + len];
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(body[0..4].try_into().unwrap()) as usize;
                height = u32::from_be_bytes(body[4..8].try_into().unwrap()) as usize;
                assert_eq!(&body[8..10], &[8, 2], "RGB, 8 bits");
            }
            b"IDAT" => data.extend_from_slice(body),
            _ => {}
        }
        at += 12 + len;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut raw).unwrap();
    let stride = width * 3;
    let rows: Vec<&[u8]> = (0..height).map(|y| {
        let row = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        assert_eq!(row[0], 0, "unfiltered rows");
        &row[1..]
    }).collect();
    let zero = vec![0u8; stride];
    let mut filtered = Vec::with_capacity(raw.len());
    for y in 0..height {
        let (row, up) = (rows[y], if y > 0 { rows[y - 1] } else { &zero[..] });
        let mut best: Option<(u64, u8, Vec<u8>)> = None;
        for filter in 0..5u8 {
            let line: Vec<u8> = (0..stride).map(|i| {
                let a = if i >= 3 { row[i - 3] } else { 0 };
                let b = up[i];
                let c = if i >= 3 { up[i - 3] } else { 0 };
                let predicted = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    _ => {
                        let p = a as i16 + b as i16 - c as i16;
                        let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
                        if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                    }
                };
                row[i].wrapping_sub(predicted)
            }).collect();
            let cost = line.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
            if best.as_ref().is_none_or(|b| cost < b.0) {
                best = Some((cost, filter, line));
            }
        }
        let (_, filter, line) = best.unwrap();
        filtered.push(filter);
        filtered.extend_from_slice(&line);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&filtered).unwrap();
    let idat = z.finish().unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    for (kind, body) in [(&b"IHDR"[..], header), (b"IDAT", idat), (b"IEND", Vec::new())] {
        png.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend_from_slice(&body);
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&crc32(&chunk).to_be_bytes());
    }
    std::fs::write(to, png).unwrap();
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
