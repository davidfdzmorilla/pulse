//! The third production audit: a trap track at 140 BPM in F minor, made
//! through the interface without a window: a trap kit, triplet hat rolls by
//! hand, an 808 that glides, a piano resampled and chopped into pads, a voice
//! pitched down and vocoded, factory chains, the browser's search, an
//! arrangement of many tracks, automation, the assistant and the export. An
//! audit, not a check:
//! `PULSE_PRUEBA=dir cargo test --release -p app produccion_trap -- --ignored --nocapture`.
//! `PULSE_HASTA=n` stops after step `n`, to find one's way.

use crate::app::PulseApp;
use crate::produccion::{listen, summary, Studio};
use crate::project::Project;
use eframe::egui::{pos2, vec2, Event, Key, Modifiers, MouseWheelUnit, Pos2, Rect, TouchPhase};
use engine::model::Clip;
use std::path::PathBuf;

fn until(step: u32) -> bool {
    std::env::var("PULSE_HASTA").ok().and_then(|v| v.parse::<u32>().ok()).is_some_and(|n| step >= n)
}

fn track(s: &Studio, name: &str) -> usize {
    s.app.project.tracks.iter().position(|t| t.name == name).unwrap_or_else(|| panic!("no hay pista «{name}»: {:?}", names(s)))
}

fn names(s: &Studio) -> Vec<String> {
    s.app.project.tracks.iter().map(|t| t.name.clone()).collect()
}

fn stop(s: &mut Studio, step: u32) -> bool {
    std::fs::write(s.out.join("diario.txt"), s.log.join("\n")).unwrap();
    until(step)
}

fn wheel(s: &mut Studio, at: Pos2, delta: eframe::egui::Vec2, mods: Modifiers) {
    s.move_to(at, mods);
    s.frame_with(vec![Event::MouseWheel { unit: MouseWheelUnit::Point, delta, modifiers: mods, phase: TouchPhase::Move }], mods);
    s.frames(30);
}

/// The notes of a session clip: (start, pitch, length, velocity).
fn clip_notes(s: &Studio, t: usize, slot: usize) -> Vec<(f64, u8, f64, f32)> {
    match &s.app.project.tracks[t].clips[slot] {
        Some(Clip::Midi(c)) => c.notes.iter().map(|n| (n.start, n.pitch, n.length, n.velocity)).collect(),
        _ => Vec::new(),
    }
}

/// How the hats of a beat sit: on the sixteenths, on triplets, on thirty-seconds.
fn grid_of(starts: &[f64]) -> String {
    let on = |step: f64| starts.iter().filter(|b| ((*b / step) - (*b / step).round()).abs() < 1e-3).count();
    let sixteenths = on(0.25);
    let triplets = starts.iter().filter(|b| ((*b * 6.0) - (*b * 6.0).round()).abs() < 1e-3 && ((*b * 4.0) - (*b * 4.0).round()).abs() > 1e-3).count()
        + starts.iter().filter(|b| ((*b * 3.0) - (*b * 3.0).round()).abs() < 1e-3 && ((*b * 4.0) - (*b * 4.0).round()).abs() > 1e-3 && ((*b * 6.0) - (*b * 6.0).round()).abs() > 1e-3).count();
    let thirty_seconds = on(0.125) - sixteenths;
    format!("{} notas: {sixteenths} en semicorcheas, {thirty_seconds} en fusas (1/32), {triplets} en tresillos", starts.len())
}

#[test]
#[ignore]
fn produccion_trap() {
    let Some(out) = std::env::var_os("PULSE_PRUEBA").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let home = out.join("home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe {
        std::env::set_var("HOME", &home);
    }

    let app = PulseApp::headless(Project::empty());
    let mut s = Studio::new(app, out.clone());
    s.app.templates = crate::templates::all();
    s.shot("vacio");
    s.dump("vacio");
    s.note(format!("pistas al empezar: {:?}", names(&s)));

    // ---- 1. Tempo 140 and F minor.
    let tempo = s.seen.iter().find(|w| w.text.ends_with(".00") && w.rect.top() < 60.0).map(|w| w.rect).unwrap();
    s.dclick_at(tempo.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text("140");
    s.key(Key::Enter, Modifiers::NONE);
    s.note(format!("  tempo: {}", s.app.project.tempo));
    s.click("Escala");
    s.shot("menu-escala");
    s.dump("menu-escala");
    // The minor row is the lower one.
    let fa = s.seen.iter().filter(|w| w.text == "Fa" && w.role == "Button" && w.rect.left() > 1000.0).max_by(|a, b| a.rect.top().total_cmp(&b.rect.top())).map(|w| w.rect).unwrap();
    s.click_at(fa.center());
    s.note(format!("  escala: {:?}", s.app.project.key.map(|k| k.name())));
    s.frames(3);
    if stop(&mut s, 1) {
        return;
    }

    // ---- 2. Two Drum Pads with the Trap kit: the beat, and the hats apart (for the rolls).
    for (header, name) in [("Pista 1 MIDI", "Batería"), ("Pista 2 MIDI", "Charles")] {
        let h = s.find(header).unwrap();
        s.click_at(h.center());
        s.click("Drum Pads");
        s.frames(3);
        if !s.has("Kits") {
            s.click("Dispositivos");
        }
        s.click("Kits");
        if name == "Batería" {
            s.shot("menu-kits");
            s.dump("menu-kits");
        }
        s.click("Trap");
        s.wait_for("el kit Trap", 120.0, |app| app.status.as_deref().is_some_and(|t| t.starts_with("Kit ")));
        let header = format!("Pista {}", s.app.project.tracks[if name == "Batería" { 0 } else { 1 }].name);
        rename(&mut s, &header, name);
    }
    s.note(format!("  pistas: {:?}", names(&s)));
    s.shot("kit-trap");
    s.dump("kit-trap");
    if stop(&mut s, 2) {
        return;
    }

    // ---- 3. Generar ritmo: Trap. The beat without hats, the hats alone.
    for (t, level, hats_only) in [(0usize, "Intenso", false), (1, "Intenso", true)] {
        let at = s.find(&format!("Slot {}:1", t + 1)).unwrap();
        s.dclick_at(at.center());
        s.click("Generar ritmo");
        if t == 0 {
            s.shot("generar-trap");
            s.dump("generar-trap");
        }
        s.click("Trap");
        s.click(level);
        s.click("4 c.");
        let marked = |s: &Studio, sound: &str| s.seen.iter().any(|w| w.text == sound && w.role == "CheckBox");
        let _ = marked;
        if hats_only {
            // The menu keeps the last clip's sounds: the beat off, the hats on.
            for sound in ["Bombo", "Caja", "Palmada", "Charles"] {
                sound_toggle(&mut s, sound);
            }
        } else {
            for sound in ["Charles", "Charles abierto"] {
                sound_toggle(&mut s, sound);
            }
        }
        s.shot(&format!("generar-trap-{t}"));
        s.click("Generar");
        s.wait_for("el ritmo", 5.0, move |app| crate::produccion::notes(app, t, 0) > 4);
        let notes = clip_notes(&s, t, 0);
        let starts: Vec<f64> = notes.iter().map(|n| n.0).collect();
        s.note(format!("  pista {t}: {}", grid_of(&starts)));
        s.note(format!("  notas (tiempo, nota, largo, vel.) del primer compás: {:?}", notes.iter().filter(|n| n.0 < 4.0).map(|n| (n.0, n.1, (n.2 * 1000.0).round() / 1000.0, (n.3 * 100.0).round() / 100.0)).collect::<Vec<_>>()));
        s.shot(&format!("ritmo-trap-{t}"));
        s.dump(&format!("ritmo-trap-{t}"));
        s.key(Key::Escape, Modifiers::NONE);
    }
    if stop(&mut s, 3) {
        return;
    }

    // ---- 4. Triplet hat rolls by hand: the grid and the note at 1/16T, the
    // straight sixteenths of a beat selected and deleted, six triplets drawn.
    // The hats' clip is still open in the editor.
    s.dump("editor-charles");
    let combos: Vec<Rect> = {
        let mut c: Vec<Rect> = s.seen.iter().filter(|w| w.role == "ComboBox" && w.text == "1/16" && w.rect.top() > 300.0).map(|w| w.rect).collect();
        c.sort_by(|a, b| a.left().total_cmp(&b.left()));
        c
    };
    for combo in combos {
        s.click_at(combo.center());
        let item = s.seen.iter().filter(|w| w.text == "1/16T" && w.rect.top() > combo.bottom()).map(|w| w.rect).next().unwrap();
        s.click_at(item.center());
    }
    s.shot("rejilla-tresillos");
    let bar1 = s.seen.iter().filter(|w| w.role == "Texto" && w.text == "1" && w.rect.top() > 400.0 && w.rect.left() > 240.0).map(|w| w.rect).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    let bar2 = s.seen.iter().filter(|w| w.role == "Texto" && w.text == "2" && (w.rect.center().y - bar1.center().y).abs() < 2.0).map(|w| w.rect).next().unwrap();
    let beat_w = (bar2.left() - bar1.left()) / 4.0;
    let x0 = bar1.left() - 2.0;
    let hats_y = s.seen.iter().find(|w| w.role == "Texto" && w.text == "Charles cerrado trap").map(|w| w.rect.center().y).unwrap();
    let x = move |beat: f64| x0 + beat as f32 * beat_w;
    let hats = 1usize;
    for roll in [7.0, 11.0, 15.0] {
        s.drag(pos2(x(roll + 0.04), hats_y - 22.0), pos2(x(roll + 0.96), hats_y + 8.0), Modifiers::NONE);
        s.key(Key::Backspace, Modifiers::NONE);
        for k in 0..6 {
            s.dclick_at(pos2(x(roll + k as f64 / 6.0 + 0.05), hats_y));
        }
    }
    let notes = clip_notes(&s, hats, 0);
    let starts: Vec<f64> = notes.iter().map(|n| n.0).collect();
    s.note(format!("  charles a mano: {}", grid_of(&starts)));
    s.note(format!("  tramos de los redobles: {:?}", notes.iter().filter(|n| [7.0, 11.0, 15.0].iter().any(|r| n.0 >= *r && n.0 < r + 1.0)).map(|n| ((n.0 * 1000.0).round() / 1000.0, (n.2 * 1000.0).round() / 1000.0)).collect::<Vec<_>>()));
    s.shot("redobles-tresillo");
    if stop(&mut s, 4) {
        return;
    }

    // ---- 5. The 808: a new track, and the browser's search (⌘F) for one.
    s.key(Key::T, Modifiers::COMMAND);
    let bass = s.app.project.tracks.len() - 1;
    let header = format!("Pista {}", s.app.project.tracks[bass].name);
    rename(&mut s, &header, "808");
    let h = s.find("Pista 808").unwrap();
    s.click_at(h.center());
    for query in ["808", "glide", "sub"] {
        s.key(Key::F, Modifiers::COMMAND);
        s.key(Key::A, Modifiers::COMMAND);
        s.type_text(query);
        s.frames(10);
        s.shot(&format!("buscar-{query}"));
        s.dump(&format!("buscar-{query}"));
        let found: Vec<String> = s.seen.iter().filter(|w| w.role == "Button" && w.rect.left() < 240.0 && w.rect.top() > 150.0 && w.rect.top() < 880.0).map(|w| w.text.clone()).take(14).collect();
        s.note(format!("  buscar «{query}»: {found:?}"));
    }
    let pick = s.seen.iter().find(|w| w.role == "Button" && w.text.contains("808 con glide") && w.rect.left() < 240.0).map(|w| w.rect);
    match pick {
        Some(r) => {
            s.note("doble clic «808 con glide»".to_string());
            s.dclick_at(r.center());
        }
        None => s.note("  ¡la búsqueda no da «808 con glide»!"),
    }
    s.key(Key::Escape, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
    s.frames(5);
    s.note(format!("  instrumento del 808: {}", s.app.project.tracks[bass].instrument.label()));
    s.shot("808-instrumento");
    if stop(&mut s, 5) {
        return;
    }

    // ---- 6. The chords: a dark piano (Pulse Keys) with an Andalusian cadence.
    s.key(Key::T, Modifiers::COMMAND);
    let piano = s.app.project.tracks.len() - 1;
    let header = format!("Pista {}", s.app.project.tracks[piano].name);
    rename(&mut s, &header, "Piano");
    let h = s.find("Pista Piano").unwrap();
    s.click_at(h.center());
    s.click("Pulse Keys");
    crate::produccion::preset(&mut s, "Pista Piano", "Piano", "Cine oscuro");
    s.note(format!("  piano: {}", s.app.project.tracks[piano].instrument.label()));
    let slot = s.find(&format!("Slot {}:1", piano + 1)).unwrap();
    s.dclick_at(slot.center());
    s.frames(5);
    let menu = s.seen.iter().filter(|w| w.text == "Acordes" && w.role == "Button" && w.rect.top() > 300.0).map(|w| w.rect).last().unwrap();
    s.click_at(menu.center());
    s.shot("menu-acordes");
    s.dump("menu-acordes");
    let pick = s.seen.iter().find(|w| w.role == "Button" && w.text.starts_with("Andaluza")).map(|w| w.text.clone()).unwrap();
    s.click(&pick);
    s.key(Key::Escape, Modifiers::NONE);
    {
        let Some(Clip::Midi(c)) = &s.app.project.tracks[piano].clips[0] else { panic!() };
        s.note(format!("  acordes «{}»: {} notas, {} tiempos: {:?}", c.name, c.notes.len(), c.length, c.notes.iter().map(|n| (n.start, n.pitch, n.length)).collect::<Vec<_>>()));
    }
    s.shot("progresion");
    if stop(&mut s, 6) {
        return;
    }

    // ---- 7. The 808 by hand, in draw mode, following the kick and the roots
    // (F Eb Db C), with two slides: a note held into the next one an octave up.
    let slot = s.find(&format!("Slot {}:1", bass + 1)).unwrap();
    s.dclick_at(slot.center());
    s.frames(5);
    s.dump("editor-808");
    s.shot("editor-808");
    // The grid stayed at the hats' 1/16T: back to sixteenths.
    for _ in 0..2 {
        let combo = s.seen.iter().filter(|w| w.role == "ComboBox" && w.text == "1/16T" && w.rect.top() > 300.0).map(|w| w.rect).min_by(|a, b| a.left().total_cmp(&b.left())).unwrap();
        s.click_at(combo.center());
        let item = s.seen.iter().filter(|w| w.text == "1/16" && w.rect.top() > combo.bottom()).map(|w| w.rect).next().unwrap();
        s.click_at(item.center());
    }
    s.click("Dibujar");
    let label = |s: &Studio, name: &str| s.seen.iter().find(|w| w.role == "Texto" && w.text == name && w.rect.left() < 230.0).map(|w| w.rect.center().y).unwrap();
    let (c3, c2) = (label(&s, "C3"), label(&s, "C2"));
    let row = (c2 - c3) / 12.0;
    let y = move |pitch: u8| c3 - (pitch as f32 - 60.0) * row;
    let x = |beat: f64| 255.6 + (1405.0 - 255.6) * (beat / 16.0) as f32;
    // Two octaves up, to see them: transposed down after.
    let mut drawn = Vec::new();
    for (bar, root) in [53u8, 51, 49, 48].into_iter().enumerate() {
        let b = bar as f64 * 4.0;
        let slide = bar % 2 == 1;
        let notes: Vec<(f64, f64, u8)> = vec![
            (0.0, 0.75, root),
            (0.75, 0.75, root),
            (1.5, 1.0, root),
            (2.5, if slide { 1.0 } else { 0.75 }, root),
            (3.25, 0.75, if slide { root + 12 } else { root }),
        ];
        for (start, length, pitch) in notes {
            let from = pos2(x(b + start + 0.05), y(pitch));
            let to = pos2(x(b + start + length) - 3.0, y(pitch));
            s.drag(from, to, Modifiers::NONE);
            drawn.push((b + start, pitch, length));
        }
    }
    s.click("Dibujar");
    let notes = clip_notes(&s, bass, 0);
    s.note(format!("  808 dibujado: {} notas de {} · {:?}", notes.len(), drawn.len(), notes.iter().map(|n| ((n.0 * 100.0).round() / 100.0, n.1, (n.2 * 100.0).round() / 100.0)).collect::<Vec<_>>()));
    s.shot("808-dibujado");
    s.key(Key::A, Modifiers::COMMAND);
    s.click("-12");
    s.click("-12");
    s.key(Key::Escape, Modifiers::NONE);
    let notes = clip_notes(&s, bass, 0);
    s.note(format!("  808 dos octavas abajo: {:?}", notes.iter().map(|n| n.1).collect::<Vec<_>>()));
    s.shot("808-abajo");
    // Heard alone: where the pitch is 15 ms and 120 ms into each note.
    let Some(clip) = s.app.project.tracks[bass].clips[0].clone() else { panic!() };
    let solo = solo_render(&s.app.project, bass, &clip, 16.0);
    let beat = 60.0 / s.app.project.tempo;
    let mut glides = Vec::new();
    for n in clip_notes(&s, bass, 0) {
        let t = n.0 * beat;
        let early = pitch_at(&solo, t + 0.015);
        let later = pitch_at(&solo, t + 0.12);
        glides.push(format!("{:.2}:{}→{:.1}/{:.1}", n.0, n.1, early, later));
    }
    s.note(format!("  808 a solas, nota escrita → tono a 15 ms / 120 ms (MIDI): {glides:?}"));
    crate::export::write(&s.out.join("808-solo.wav"), &solo, &solo, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
    if stop(&mut s, 7) {
        return;
    }

    // ---- 8. The 808 ducks under the kick, in one click.
    let h = s.find("Pista 808").unwrap();
    s.rclick_at(h.center());
    s.shot("menu-pista-808");
    s.dump("menu-pista-808");
    s.click("Sidechain desde el bombo");
    s.frames(5);
    summary(&mut s);
    if stop(&mut s, 8) {
        return;
    }

    // ---- 9. The sample: the piano duplicated, resampled to audio (freeze and
    // convert), and chopped into pads by its transients.
    let h = s.find("Pista Piano").unwrap();
    s.rclick_at(h.center());
    s.click("Duplicar pista");
    s.frames(5);
    s.note(format!("  pistas: {:?}", names(&s)));
    let copy = track(&s, "Piano copia");
    rename(&mut s, "Pista Piano copia", "Sample");
    let h = s.find("Pista Sample").unwrap();
    s.rclick_at(h.center());
    s.click("Congelar pista");
    s.wait_for("congelar", 120.0, move |app| app.project.tracks[copy].frozen.is_some());
    s.frames(5);
    let h = s.find("Pista Sample").unwrap();
    s.rclick_at(h.center());
    s.shot("menu-congelada");
    s.click("Convertir en audio");
    s.frames(10);
    s.note(format!("  «Sample» es de audio: {} · clips: {:?}", s.app.project.tracks[copy].is_audio(), s.app.project.tracks[copy].clips.iter().flatten().map(|c| c.name().to_string()).collect::<Vec<_>>()));
    s.shot("sample-en-audio");
    let clip_at = s.find(&format!("Slot {}:1", copy + 1)).or_else(|| s.find(&format!("Clip {} ·", copy + 1))).unwrap();
    s.click_at(clip_at.center());
    s.frames(5);
    if !s.has("A Drum Pads") {
        s.click("Clip");
    }
    s.shot("editor-sample");
    s.dump("editor-sample");
    let mode = s.seen.iter().find(|w| w.role == "ComboBox" && w.rect.top() > 300.0 && (w.text == "Transitorios" || w.text.starts_with("1/"))).map(|w| (w.text.clone(), w.rect)).unwrap();
    s.note(format!("  modo de troceado: {}", mode.0));
    let tracks_before = s.app.project.tracks.len();
    s.click("A Drum Pads");
    s.wait_for("trocear", 60.0, move |app| app.project.tracks.len() > tracks_before);
    s.frames(10);
    let pads = s.app.project.tracks.len() - 1;
    s.note(format!("  troceado por transitorios: pista «{}» con {} muestras · clip {:?}", s.app.project.tracks[pads].name, s.app.project.tracks[pads].samples.iter().filter(|p| !p.is_empty()).count(), clip_notes(&s, pads, 0).iter().map(|n| (n.0, n.1)).collect::<Vec<_>>()));
    s.shot("troceado-transitorios");
    // Four chords, four chops: not enough to flip. Undone, and by quarters.
    s.key(Key::Z, Modifiers::COMMAND);
    s.frames(5);
    s.note(format!("  deshecho: pistas {:?}", names(&s)));
    let clip_at = s.find(&format!("Slot {}:1", copy + 1)).or_else(|| s.find(&format!("Clip {} ·", copy + 1))).unwrap();
    s.click_at(clip_at.center());
    s.frames(5);
    let mode = s.seen.iter().find(|w| w.role == "ComboBox" && w.rect.top() > 300.0 && (w.text == "Transitorios" || w.text.starts_with("1/"))).map(|w| w.rect).unwrap();
    s.click_at(mode.center());
    let quarter = s.seen.iter().filter(|w| w.text == "1/4" && w.rect.top() > mode.bottom() - 2.0).map(|w| w.rect).next().unwrap();
    s.click_at(quarter.center());
    s.click("Editar cortes");
    s.shot("cortes-negras");
    s.click("Editar cortes");
    let tracks_before = s.app.project.tracks.len();
    s.click("A Drum Pads");
    s.wait_for("trocear", 60.0, move |app| app.project.tracks.len() > tracks_before);
    s.frames(10);
    let pads = s.app.project.tracks.len() - 1;
    s.note(format!("  troceado por negras: pista «{}» con {} muestras · clip {:?}", s.app.project.tracks[pads].name, s.app.project.tracks[pads].samples.iter().filter(|p| !p.is_empty()).count(), clip_notes(&s, pads, 0).iter().map(|n| (n.0, n.1, n.2)).collect::<Vec<_>>()));
    s.shot("troceado");
    s.dump("troceado");
    // The flip: in each bar, the second chop repeats the first and the last two swap.
    let pad_y = |s: &Studio, pitch: u8| {
        let last = s.seen.iter().filter(|w| w.role == "Texto" && w.text.ends_with(" 16") && w.rect.left() < 240.0).map(|w| w.rect.center().y).next().unwrap();
        last + (51 - pitch as i32) as f32 * 20.0
    };
    let x = |beat: f64| 255.6 + (1405.0 - 255.6) * (beat / 16.0) as f32;
    for bar in 0..4u8 {
        for (k, up) in [(1u8, false), (2, true), (3, false)] {
            let pitch = 36 + bar * 4 + k;
            let at = pos2(x((bar * 4 + k) as f64 + 0.5), pad_y(&s, pitch));
            s.click_at(at);
            s.key(if up { Key::ArrowUp } else { Key::ArrowDown }, Modifiers::NONE);
        }
    }
    s.key(Key::Escape, Modifiers::NONE);
    s.note(format!("  la vuelta al sample (tiempo, pad): {:?}", clip_notes(&s, pads, 0).iter().map(|n| (n.0.round(), n.1 - 35)).collect::<Vec<_>>()));
    s.shot("sample-flip");
    rename(&mut s, "Pista Fam Mib Do# Do Slices", "Sample troceado");
    summary(&mut s);
    if stop(&mut s, 9) {
        return;
    }

    // ---- 10. The voice: a hook said by macOS's `say`, dropped in a slot of a new audio track.
    let voice = s.out.join("Voz Rocko.wav");
    let text = "Sigo en la calle, [[slnc 300]] no paro, no paro. [[slnc 600]] Dinero en la mente, [[slnc 300]] subiendo despacio. [[slnc 600]] \
        Noche en la ciudad, [[slnc 300]] yeah, yeah. [[slnc 600]] Nadie me para, [[slnc 300]] vamos arriba.";
    let ok = std::process::Command::new("say").args(["-v", "Rocko (Español (España))", "-r", "170", "-o"]).arg(&voice).args(["--data-format=LEI16@44100", text]).status().unwrap();
    assert!(ok.success());
    s.key(Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
    let vt = s.app.project.tracks.len() - 1;
    let header = format!("Pista {}", s.app.project.tracks[vt].name);
    rename(&mut s, &header, "Voz");
    let slot = s.find(&format!("Slot {}:1", vt + 1)).unwrap();
    let placed = s.app.drop_files(vec![voice.clone()], slot.center());
    s.frames(10);
    let wait = std::time::Instant::now();
    while wait.elapsed().as_secs() < 30 && !s.app.status.as_deref().is_some_and(|t| !t.contains("analizando") && !t.contains("Analizando")) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        s.frames(1);
    }
    s.frames(20);
    let voice_clip = |s: &Studio| match &s.app.project.tracks[vt].clips[0] {
        Some(Clip::Audio(a)) => format!("warp {:?} a {:.1} BPM, {} tiempos", a.warp, a.original_bpm, a.length),
        _ => "nada".into(),
    };
    s.note(format!("  voz soltada ({placed}): {}", voice_clip(&s)));
    s.shot("voz-soltada");
    if stop(&mut s, 10) {
        return;
    }

    // ---- 11. The voice's sound, all from the search: the hard tune of trap,
    // a pitched-down double for the ad-libs, and a vocoder on the piano's chords.
    let h = s.find("Pista Voz").unwrap();
    s.click_at(h.center());
    search_apply(&mut s, "autotune", None);
    s.click("Dispositivos");
    s.frames(5);
    // The trap hard tune: Speed to 0 ms (the Tuner has no presets).
    let speed = s.seen.iter().find(|w| w.role == "Slider" && w.text.starts_with("Speed")).map(|w| w.rect).unwrap();
    s.drag(speed.center(), speed.center() + vec2(0.0, 150.0), Modifiers::NONE);
    let tuner_speed = |s: &Studio| s.app.project.tracks[vt].devices.iter().find_map(|d| match d.effect { engine::model::Effect::Tuner(p) => Some(p), _ => None });
    s.note(format!("  Tuner: {:?}", tuner_speed(&s)));
    s.shot("tuner-a-cero");
    for (name, query, pick) in [("Voz grave", "voz grave", Some("Voz grave")), ("Vocoder", "acordes que tocas", Some("Acordes que tocas"))] {
        let h = s.find("Pista Voz").unwrap();
        s.rclick_at(h.center());
        s.click("Duplicar pista");
        s.frames(5);
        let copy = s.app.project.tracks.iter().rposition(|t| t.name == "Voz copia").unwrap();
        rename(&mut s, "Pista Voz copia", name);
        let h = s.find(&format!("Pista {name}")).unwrap();
        s.click_at(h.center());
        // Only the voice's own sound: the copy's effects off with ⌥ (replace) by loading the effect alone.
        search_apply(&mut s, query, pick);
        s.note(format!("  {name}: {:?}", fx_names(&s, copy)));
        if name == "Voz grave" {
            search_apply(&mut s, "voz radio", Some("Voz · Radio"));
            s.note(format!("  {name}: {:?}", fx_names(&s, copy)));
        }
    }
    let h = s.find("Pista Voz").unwrap();
    s.click_at(h.center());
    search_apply(&mut s, "voz pop", Some("Voz · Pop"));
    s.note(format!("  Voz: {:?}", fx_names(&s, vt)));
    // The vocoder's notes: the piano's chords.
    let h = s.find("Pista Vocoder").unwrap();
    s.click_at(h.center());
    s.frames(5);
    s.dump("vocoder");
    let from = s.seen.iter().find(|w| w.role == "ComboBox" && w.text == "Esta pista").map(|w| w.rect).unwrap();
    s.click_at(from.center());
    s.shot("vocoder-de-donde");
    let piano_item = s.seen.iter().filter(|w| w.text == "Piano" && w.rect.top() > from.bottom() - 2.0).map(|w| w.rect).next().unwrap();
    s.click_at(piano_item.center());
    let vocoder = track(&s, "Vocoder");
    let vp = s.app.project.tracks[vocoder].devices.iter().find_map(|d| match d.effect { engine::model::Effect::Vocoder(p) => Some(p), _ => None }).unwrap();
    s.note(format!("  Vocoder: notas MIDI {} desde {:?} (Piano es {:?})", vp.midi, vp.sidechain, s.app.project.tracks[piano].id));
    s.shot("vocoder");
    // Ad-libs only: the pitched-down double in its own clip region later; the voice copies quieter.
    s.shot("voces");
    if stop(&mut s, 11) {
        return;
    }

    // ---- 12. A bell melody over the chords (Acompañar), the drums in a bus
    // with a factory chain, and the piano silenced (it only gives the vocoder its notes).
    let clip_at = s.find(&format!("Clip {} ·", piano + 1)).unwrap();
    s.rclick_at(clip_at.center());
    s.click("Acompañar ⏵");
    s.shot("menu-acompanar");
    s.submenu("Melodía: pregunta y respuesta");
    s.frames(5);
    let melody = track(&s, "Melodía");
    s.note(format!("  melodía: {} con {} notas", s.app.project.tracks[melody].instrument.label(), clip_notes(&s, melody, 0).len()));
    let h = s.find("Pista Melodía").unwrap();
    s.click_at(h.center());
    search_apply(&mut s, "caja de musica", Some("Caja de música"));
    s.note(format!("  melodía: {}", s.app.project.tracks[melody].instrument.label()));
    let h = s.find("Pista Batería").unwrap();
    s.click_at(h.center());
    let h = s.find("Pista Charles").unwrap();
    s.press_at(h.center(), eframe::egui::PointerButton::Primary, Modifiers::SHIFT, 1);
    s.key(Key::G, Modifiers::COMMAND);
    s.frames(5);
    s.note(format!("  pistas: {:?}", names(&s)));
    let group = s.app.project.tracks.iter().position(|t| t.name.contains("Grupo")).unwrap();
    let header = format!("Pista ▾ {}", s.app.project.tracks[group].name);
    rename(&mut s, &header, "Drums");
    s.note(format!("  pistas: {:?}", names(&s)));
    s.dump("grupo");
    let h = s.find("Pista ▾ Drums").or_else(|| s.find("Drums")).unwrap();
    s.click_at(h.center());
    search_apply(&mut s, "bus bateria", Some("Bus de batería · Pegada"));
    s.note(format!("  Drums: {:?}", fx_names(&s, group)));
    s.dump("session-grupo");
    s.shot("session-grupo");
    summary(&mut s);
    if stop(&mut s, 12) {
        return;
    }

    // ---- 13. Scenes: «Estribillo» (row 1), «Estrofa» (row 2) and «Intro» (row 3),
    // dragging clips (⌥ copies), and the song with Crear › Arreglo automático › Pop.
    let drag_clip = |s: &mut Studio, name: &str, from: usize, to: usize, copy: bool| {
        let t = track(s, name);
        let a = s.find(&format!("Slot {}:{}", t + 1, from)).or_else(|| s.seen.iter().find(|w| w.role == "Button" && w.text.starts_with(&format!("Clip {} ·", t + 1)) && (w.rect.center().y - (99.3 + 23.0 * (from - 1) as f32)).abs() < 6.0).map(|w| w.rect)).unwrap_or_else(|| panic!("no veo el clip de {name} en la fila {from}"));
        let b = s.find(&format!("Slot {}:{}", t + 1, to)).unwrap_or_else(|| panic!("no veo el slot {to} de {name}"));
        s.drag(a.center(), b.center(), if copy { Modifiers::ALT } else { Modifiers::NONE });
    };
    s.dump("session-antes-escenas");
    drag_clip(&mut s, "Sample", 1, 3, false);
    drag_clip(&mut s, "Sample", 3, 2, true);
    s.shot("session-tras-arrastrar");
    // The Session scrolled down while dragging, headers and all: back up.
    wheel(&mut s, pos2(700.0, 150.0), vec2(0.0, 600.0), Modifiers::NONE);
    s.shot("session-arriba");
    for name in ["Batería", "Charles", "808", "Piano"] {
        drag_clip(&mut s, name, 1, 2, true);
        wheel(&mut s, pos2(700.0, 150.0), vec2(0.0, 600.0), Modifiers::NONE);
    }
    drag_clip(&mut s, "Piano", 1, 3, true);
    wheel(&mut s, pos2(700.0, 150.0), vec2(0.0, 600.0), Modifiers::NONE);
    // Voz grave and Vocoder are under the returns, off to the right: sideways first.
    wheel(&mut s, pos2(700.0, 150.0), vec2(-600.0, 0.0), Modifiers::NONE);
    s.shot("session-a-la-derecha");
    drag_clip(&mut s, "Voz grave", 1, 2, true);
    wheel(&mut s, pos2(700.0, 150.0), vec2(0.0, 600.0), Modifiers::NONE);
    drag_clip(&mut s, "Vocoder", 1, 3, true);
    wheel(&mut s, pos2(700.0, 150.0), vec2(600.0, 600.0), Modifiers::NONE);
    let rows: Vec<Vec<String>> = (0..3).map(|r| s.app.project.tracks.iter().filter(|t| t.clips.get(r).is_some_and(|c| c.is_some())).map(|t| t.name.clone()).collect()).collect();
    s.note(format!("  escenas: {rows:?}"));
    for (scene, name) in [("Escena 1", "Estribillo"), ("Escena 2", "Estrofa"), ("Escena 3", "Intro")] {
        let at = s.find(scene).unwrap_or_else(|| panic!("no veo «{scene}»"));
        s.rclick_at(at.center());
        let field = s.seen.iter().filter(|w| w.role == "TextInput" && !w.text.starts_with("Buscar") && (w.rect.center().y - at.center().y).abs() < 120.0).map(|w| w.rect).next().unwrap();
        s.click_at(field.center());
        s.key(Key::A, Modifiers::COMMAND);
        s.type_text(name);
        s.key(Key::Enter, Modifiers::NONE);
        s.key(Key::Escape, Modifiers::NONE);
    }
    s.note(format!("  nombres: {:?}", &s.app.project.scene_names[..3]));
    s.shot("escenas");
    s.dump("escenas");
    if stop(&mut s, 13) {
        return;
    }

    // ---- 14. The song: Crear › Arreglo automático… › Pop (intro, verse, hook…).
    s.click("Crear");
    s.click("Arreglo automático…");
    s.shot("arreglo-menu");
    s.dump("arreglo-menu");
    let pop = s.seen.iter().find(|w| w.role == "Button" && w.text.starts_with("Pop")).map(|w| w.text.clone()).unwrap();
    s.click(&pop);
    s.shot("arreglo-pop");
    s.dump("arreglo-pop");
    // The chops are taken for drums and go into the verses: out of them, and no
    // EDM snare roll at the end of each verse.
    // The whole sample doubles the chops in the hooks: out of them.
    for (x, y) in [(549.4, 525.0), (649.4, 525.0), (549.4, 620.6), (649.4, 620.6), (598.3, 501.0), (698.4, 501.0), (798.5, 501.0)] {
        s.click_at(pos2(x, y));
    }
    s.shot("arreglo-retocado");
    s.click("Crear en el Arrangement");
    s.frames(10);
    let markers: Vec<String> = s.app.project.markers.iter().map(|m| format!("{} @{}", m.name, m.beat / 4.0 + 1.0)).collect();
    s.note(format!("  marcadores: {markers:?}"));
    for t in &s.app.project.tracks {
        let spans: Vec<String> = t.arrangement.iter().map(|c| format!("{}–{}", c.start / 4.0 + 1.0, c.end() / 4.0 + 1.0)).collect();
        eprintln!("[tema]   {}: {}", t.name, spans.join(" "));
    }
    let lines: Vec<String> = s.app.project.tracks.iter().map(|t| format!("    {}: {} clips", t.name, t.arrangement.len())).collect();
    for l in lines {
        s.note(l);
    }
    // The piano only gives the vocoder its notes: its activator off (number in the header).
    s.shot("arrangement");
    // An empty editor takes most of the window: the panel down, the zoom out.
    let tabs = s.seen.iter().find(|w| w.text == "Dispositivos" && w.role == "Button").map(|w| w.rect).unwrap();
    s.drag(pos2(900.0, tabs.top() - 6.0), pos2(900.0, 820.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 880.0), Modifiers::NONE);
    s.frames(10);
    let zoom = s.seen.iter().filter(|w| w.role == "Slider" && w.rect.top() < 90.0).map(|w| w.rect).next().unwrap();
    s.drag(zoom.center(), pos2(zoom.left() + 2.0, zoom.center().y), Modifiers::NONE);
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, 2000.0), Modifiers::NONE);
    s.shot("arrangement-entero");
    s.dump("arrangement");
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, -2000.0), Modifiers::NONE);
    s.shot("arrangement-abajo");
    let rows: Vec<String> = s.seen.iter().filter(|w| w.role == "Texto" && w.rect.left() < 300.0 && w.rect.left() > 180.0 && w.rect.top() > 130.0 && s.app.project.tracks.iter().any(|t| t.name == w.text || w.text == "Master")).map(|w| w.text.clone()).collect();
    s.note(format!("  cabeceras a la vista abajo del todo: {rows:?}"));
    wheel(&mut s, pos2(900.0, 300.0), vec2(0.0, 2000.0), Modifiers::NONE);
    let on = s.seen.iter().find(|w| w.role == "CheckBox" && w.text == "5" && w.rect.left() < 300.0).map(|w| w.rect).unwrap();
    s.click_at(on.center());
    let piano = track(&s, "Piano");
    s.note(format!("  Piano activo: {}", s.app.project.tracks[piano].mixer.active));
    listen(&mut s, "1-arreglo-sin-mezcla");
    let vocoder = track(&s, "Vocoder");
    {
        // The vocoder heard alone in the intro: does it sing with the piano silenced?
        let mut p = s.app.project.clone();
        for (i, t) in p.tracks.iter_mut().enumerate() {
            if i != vocoder && i != piano {
                t.mixer.active = false;
            }
        }
        let mut samples = crate::sync::SampleCache::default();
        let commands = crate::sync::project_commands(&p, &mut samples, 48_000.0, &Default::default());
        let (l, r) = crate::export::render(commands, (0.0, 32.0), crate::export::Song::of(&p), 48_000.0, &Default::default(), &|_| {}).unwrap();
        let m = crate::mix_assist::Measures::of(&l, &r, 48_000.0);
        s.note(format!("  vocoder en la intro, piano silenciado: {:?} LUFS, pico {:.1} dBFS", m.lufs, m.peak));
        crate::export::write(&s.out.join("vocoder-intro.wav"), &l, &r, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
    }
    if stop(&mut s, 14) {
        return;
    }

    // ---- 15. Automation: the sample through a low-pass that opens over the
    // intro, drawn in the Sample's lane.
    let header = |s: &Studio, name: &str| s.seen.iter().find(|w| w.role == "Texto" && w.text == name && w.rect.left() < 300.0 && w.rect.left() > 180.0 && w.rect.top() > 130.0).map(|w| w.rect).unwrap_or_else(|| panic!("no veo la cabecera «{name}» en el Arrangement"));
    let h = header(&s, "Sample");
    s.click_at(h.center());
    search_apply(&mut s, "auto filter", None);
    let sample = track(&s, "Sample");
    s.note(format!("  Sample: {:?}", fx_names(&s, sample)));
    let a = s.seen.iter().filter(|w| w.text == "A" && w.role == "CheckBox" && w.rect.left() < 340.0 && (w.rect.center().y - h.center().y).abs() < 40.0).map(|w| w.rect).next().unwrap();
    s.click_at(a.center());
    s.frames(5);
    s.shot("carril-sample");
    let combo = s.seen.iter().find(|w| w.role == "ComboBox" && w.text == "Volumen" && w.rect.left() < 340.0).map(|w| w.rect).unwrap();
    s.click_at(combo.center());
    s.shot("parametros-sample");
    s.dump("parametros-sample");
    let params: Vec<String> = s.seen.iter().filter(|w| w.role == "Button" && w.rect.top() > combo.bottom() - 2.0 && w.rect.left() < 400.0).map(|w| w.text.clone()).collect();
    s.note(format!("  parámetros: {params:?}"));
    let cutoff = s.seen.iter().find(|w| w.rect.top() > combo.bottom() - 2.0 && w.text.contains("Auto Filter") && (w.text.contains("Freq") || w.text.contains("Cutoff") || w.text.contains("Frec"))).map(|w| w.rect).unwrap();
    s.click_at(cutoff.center());
    s.frames(5);
    s.dump("carril-sample");
    let hint = s.seen.iter().find(|w| w.text.starts_with("Clic para añadir puntos")).map(|w| w.rect).unwrap();
    let bar = |s: &Studio, n: &str| s.seen.iter().find(|w| w.role == "Texto" && w.text == n && w.rect.top() > 110.0 && w.rect.top() < 135.0 && w.rect.left() > 330.0).map(|w| w.rect.left() - 2.0).unwrap();
    let (b1, b9) = (bar(&s, "1"), bar(&s, "9"));
    let bx = move |b: f32| b1 + (b - 1.0) * (b9 - b1) / 8.0;
    let (low, high) = (hint.top() + 44.0, hint.top() + 1.0);
    for (b, y) in [(1.05, low), (5.0, low - 8.0), (8.9, high)] {
        s.click_at(pos2(bx(b), y));
    }
    let lane = s.app.project.tracks[sample].automation.iter().map(|l| format!("{:?}: {:?}", l.param, l.points.iter().map(|p| ((p.beat * 10.0).round() / 10.0, (p.value * 100.0).round() / 100.0)).collect::<Vec<_>>())).collect::<Vec<_>>();
    s.note(format!("  automatización de Sample: {lane:?}"));
    s.shot("carril-cutoff");
    if stop(&mut s, 15) {
        return;
    }

    // ---- 16. A reverse cymbal into each hook, and the factory master chain.
    for n in 1..=3 {
        let mark = s.seen.iter().filter(|w| w.text == "Estribillo" && w.rect.top() < 120.0).map(|w| w.rect).nth(n - 1);
        let Some(mark) = mark else {
            s.note(format!("  no veo el estribillo {n} en la franja (fuera de la vista)"));
            continue;
        };
        s.rclick_at(mark.center());
        s.click("Transición aquí ⏵");
        if n == 1 {
            s.shot("menu-transicion");
        }
        s.submenu("Platillo invertido ⏵");
        s.submenu("2 compases");
    }
    if let Some(t) = s.app.project.tracks.iter().find(|t| t.name == "Transiciones") {
        let clips: Vec<String> = t.arrangement.iter().map(|c| format!("{} @{}–{}", c.clip.name(), c.start / 4.0 + 1.0, c.end() / 4.0 + 1.0)).collect();
        s.note(format!("  transiciones: {clips:?}"));
    }
    let m = s.seen.iter().find(|w| w.role == "Texto" && w.text == "Master" && w.rect.left() < 300.0).map(|w| w.rect).unwrap();
    s.click_at(m.center());
    search_apply(&mut s, "master club", Some("Máster · Club"));
    s.note(format!("  master: {:?}", s.app.project.master.devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ', '{']).next().unwrap_or("").to_string()).collect::<Vec<_>>()));
    s.shot("master-club");
    summary(&mut s);
    listen(&mut s, "2-mezcla-a-mano");
    if stop(&mut s, 16) {
        return;
    }

    // ---- 17. The mix assistant for SoundCloud and the club (-9 LUFS), all at once.
    s.menu(crate::commands::Command::MixAssistant);
    s.frames(5);
    let destination = s.seen.iter().find(|w| w.role == "ComboBox" && w.text.contains("LUFS")).map(|w| w.text.clone()).unwrap();
    s.click(&destination);
    s.shot("destinos");
    let club = s.seen.iter().find(|w| w.text.starts_with("Club")).map(|w| w.text.clone()).unwrap();
    s.click(&club);
    s.click("Analizar la mezcla");
    s.wait_for("el análisis", 240.0, |app| app.mix.report.is_some());
    s.frames(5);
    s.shot("analisis");
    s.dump("analisis");
    s.click("Aplicar todo");
    let started = std::time::Instant::now();
    s.wait_for("aplicar todo", 900.0, |app| !app.mix.busy() && app.mix.report.is_some());
    s.note(format!("  aplicar todo: {:.0} s", started.elapsed().as_secs_f64()));
    s.frames(5);
    s.shot("aplicado");
    s.dump("aplicado");
    let said: Vec<String> = s.seen.iter().filter(|w| w.role == "Texto" && w.rect.left() > 300.0 && w.text.len() > 40).map(|w| w.text.clone()).collect();
    for line in said.iter().take(40) {
        s.note(format!("    · {line}"));
    }
    summary(&mut s);
    s.note(format!("  master: {:?}", s.app.project.master.devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ', '{']).next().unwrap_or("").to_string()).collect::<Vec<_>>()));
    s.note(format!("  retornos: {:?}", s.app.project.returns.iter().map(|r| (r.name.clone(), r.devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ', '{']).next().unwrap_or("").to_string()).collect::<Vec<_>>())).collect::<Vec<_>>()));
    listen(&mut s, "3-asistente");
    s.click("Close window");
    if stop(&mut s, 17) {
        return;
    }

    // ---- 18. Saved (⇧⌘S), exported (⇧⌘R), measured, and opened again.
    let project = s.out.join("Noche en la calle").join("Noche en la calle.pulse");
    crate::script::answer_dialog(vec![project.clone()]);
    s.key(Key::S, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(5);
    s.note(format!("  guardado: {} ({:?})", project.display(), s.app.file));
    s.key(Key::R, Modifiers::COMMAND | Modifiers::SHIFT);
    s.shot("exportar");
    let wav = s.out.join("Noche en la calle.wav");
    let _ = std::fs::remove_file(&wav);
    crate::script::answer_dialog(vec![wav.clone()]);
    s.click("Exportar…");
    let w = wav.clone();
    s.wait_for("la exportación", 300.0, move |_| w.exists());
    s.frames(20);
    s.shot("exportado");
    let (l, r, rate) = {
        let sample = engine::Sample::load(&wav).unwrap();
        let n = sample.frames();
        let (l, r): (Vec<f32>, Vec<f32>) = (0..n).map(|i| sample.read(i as f64)).unzip();
        (l, r, sample.sample_rate)
    };
    let m = crate::mix_assist::Measures::of(&l, &r, rate);
    s.note(format!("  {}: {:.1} s a {rate} Hz · {:?} LUFS · pico real {:.2} dBTP · pico {:.2} dBFS · rango {:?} LU · máx. corto {:?} · correlación {:.2}", wav.display(), l.len() as f32 / rate, m.lufs, m.true_peak, m.peak, m.range, m.short_max, m.correlation));
    s.note(format!("  octavas (31 Hz…16 kHz, dB): {:?}", m.bands.iter().map(|b| (b * 10.0).round() / 10.0).collect::<Vec<_>>()));
    let reopened = Project::load(&project).unwrap();
    let again = PulseApp::headless(reopened);
    let mut s2 = Studio::new(again, s.out.clone());
    let m2 = listen(&mut s2, "4-reabierto");
    s.log.extend(s2.log.drain(..));
    s.note(format!("  reabierto: {:?} LUFS frente a {:?} exportado", m2.lufs, m.lufs));
    stop(&mut s, 18);
}

fn fx_names(s: &Studio, t: usize) -> Vec<String> {
    s.app.project.tracks[t].devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ', '{']).next().unwrap_or("").to_string()).collect()
}

/// ⌘F, `query`, and the result called `pick` double-clicked (or the first with Enter).
fn search_apply(s: &mut Studio, query: &str, pick: Option<&str>) {
    s.key(Key::F, Modifiers::COMMAND);
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text(query);
    s.frames(10);
    let found: Vec<String> = s.seen.iter().filter(|w| w.role == "Button" && w.rect.left() < 240.0 && w.rect.top() > 110.0 && w.rect.top() < 880.0).map(|w| w.text.clone()).take(8).collect();
    s.note(format!("  buscar «{query}»: {found:?}"));
    s.shot(&format!("buscar-{}", query.replace(' ', "-")));
    match pick {
        Some(name) => {
            let r = s.seen.iter().find(|w| w.role == "Button" && w.text.contains(name) && w.rect.left() < 240.0 && w.rect.top() > 110.0).map(|w| w.rect).unwrap_or_else(|| panic!("la búsqueda «{query}» no da «{name}»"));
            s.dclick_at(r.center());
        }
        None => s.key(Key::Enter, Modifiers::NONE),
    }
    s.frames(5);
    s.key(Key::Escape, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
    s.frames(3);
}

/// One track playing `clip` from the start, the rest silent: the left channel.
fn solo_render(project: &Project, track: usize, clip: &Clip, beats: f64) -> Vec<f32> {
    let mut p = project.clone();
    for (i, t) in p.tracks.iter_mut().enumerate() {
        t.arrangement.clear();
        t.automation.clear();
        if i != track {
            t.mixer.active = false;
        }
    }
    p.tracks[track].arrangement.push(engine::model::ArrangementClip { start: 0.0, length: beats, clip: clip.clone() });
    let mut samples = crate::sync::SampleCache::default();
    let commands = crate::sync::project_commands(&p, &mut samples, 48_000.0, &Default::default());
    let (l, _) = crate::export::render(commands, (0.0, beats), crate::export::Song::of(&p), 48_000.0, &Default::default(), &|_| {}).unwrap();
    l
}

/// The pitch (fractional MIDI note) of a low sound at `t` seconds, by autocorrelation (20–200 Hz).
fn pitch_at(x: &[f32], t: f64) -> f64 {
    let rate = 48_000.0;
    let start = (t * rate) as usize;
    let n = 2400;
    if start + n + 2400 > x.len() {
        return f64::NAN;
    }
    let w = &x[start..start + n + 2400];
    let (lo, hi) = ((rate / 200.0) as usize, (rate / 20.0) as usize);
    let corr = |lag: usize| -> f64 { (0..n).map(|i| w[i] as f64 * w[i + lag] as f64).sum() };
    let energy: f64 = (0..n).map(|i| (w[i] as f64).powi(2)).sum();
    if energy < 1e-6 {
        return f64::NAN;
    }
    let values: Vec<f64> = (lo..hi).map(corr).collect();
    // The first strong peak, not the largest (which may be an octave down).
    let max = values.iter().cloned().fold(f64::MIN, f64::max);
    let mut best = 0;
    for i in 1..values.len() - 1 {
        if values[i] > values[i - 1] && values[i] >= values[i + 1] && values[i] > 0.85 * max {
            best = i;
            break;
        }
    }
    let lag = (lo + best) as f64;
    let f = rate / lag;
    69.0 + 12.0 * (f / 440.0).log2()
}

/// A sound of «Generar ritmo» marked or unmarked (the button, not the clip
/// called the same).
fn sound_toggle(s: &mut Studio, sound: &str) {
    let r = s.seen.iter().filter(|w| w.text == sound && w.role == "Button" && w.rect.top() > 450.0).map(|w| w.rect).next().unwrap_or_else(|| panic!("no veo el sonido «{sound}»"));
    s.note(format!("clic sonido «{sound}»"));
    s.click_at(r.center());
}

/// A track renamed in its header's menu.
fn rename(s: &mut Studio, header: &str, name: &str) {
    s.frames(3);
    let h = s.find(header).unwrap_or_else(|| {
        s.dump("perdido");
        panic!("no veo «{header}»")
    });
    s.rclick_at(h.center());
    // (Not the browser's search field.)
    let field = s.seen.iter().filter(|w| w.role == "TextInput" && !w.text.starts_with("Buscar")).min_by(|a, b| (a.rect.center() - h.center()).length().total_cmp(&(b.rect.center() - h.center()).length())).map(|w| w.rect).unwrap();
    s.click_at(field.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text(name);
    s.key(Key::Enter, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
}
