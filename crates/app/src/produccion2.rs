//! The second production audit: a melodic techno track with a spoken voice,
//! made through the interface without a window, using what came after the
//! first audit (factory kits, bus and multiband compression, the convolution
//! reverb, melodies over the chords, recorded expression, voice to MIDI,
//! tuning and lyrics). An audit, not a check:
//! `PULSE_PRUEBA=dir cargo test --release -p app produccion_melodic -- --ignored --nocapture`.
//! `PULSE_HASTA=n` stops after step `n`, to find one's way.

use crate::app::{ClipRef, PulseApp};
use crate::produccion::{listen, summary, Studio};
use crate::project::Project;
use eframe::egui::{pos2, vec2, Key, Modifiers, Pos2, Rect};
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

/// The widget showing exactly `text` below `y` (the editor or the devices).
fn below(s: &Studio, text: &str, y: f32) -> Option<Rect> {
    s.seen.iter().filter(|w| w.text == text && w.rect.top() > y).map(|w| w.rect).min_by(|a, b| a.top().total_cmp(&b.top()))
}

#[test]
#[ignore]
fn produccion_melodic() {
    let Some(out) = std::env::var_os("PULSE_PRUEBA").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let home = out.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // The lyrics model already downloaded, read where it is (never written).
    let real_home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let model = real_home.join("Library/Application Support/Pulse/Models/whisper-small");
    unsafe {
        std::env::set_var("PULSE_LYRICS_MODEL", &model);
        std::env::set_var("HOME", &home);
    }

    let app = PulseApp::headless(Project::empty());
    let mut s = Studio::new(app, out.clone());
    s.app.templates = crate::templates::all();
    s.shot("vacio");
    s.dump("vacio");
    s.note(format!("pistas al empezar: {:?}", names(&s)));

    // ---- 1. Tempo 122 and D minor.
    let tempo = s.seen.iter().find(|w| w.text.ends_with(".00") && w.rect.top() < 60.0).map(|w| w.rect).unwrap();
    s.dclick_at(tempo.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text("122");
    s.key(Key::Enter, Modifiers::NONE);
    s.note(format!("  tempo: {}", s.app.project.tempo));
    s.click("Escala");
    s.shot("menu-escala");
    s.dump("menu-escala");
    let re = s.seen.iter().filter(|w| w.text == "Re" && w.rect.left() > 1000.0).max_by(|a, b| a.rect.top().total_cmp(&b.rect.top())).map(|w| w.rect).unwrap();
    s.click_at(re.center());
    s.note(format!("  escala: {:?}", s.app.project.key.map(|k| k.name())));
    s.frames(3);
    if stop(&mut s, 1) {
        return;
    }

    // ---- 2. Drums: two Drum Pads with factory kits, as anyone would.
    for (header, kit, name) in [("Pista 1 MIDI", "Techno", "Bombo y charles"), ("Pista 2 MIDI", "Minimal", "Percusión")] {
        let h = s.find(header).unwrap();
        s.click_at(h.center());
        s.click("Drum Pads");
        s.frames(3);
        if !s.has("Kits") {
            s.click("Dispositivos");
        }
        s.shot(&format!("drum-pads-{kit}"));
        s.dump(&format!("drum-pads-{kit}"));
        s.click("Kits");
        s.shot(&format!("menu-kits-{kit}"));
        s.dump("menu-kits");
        s.click(kit);
        s.wait_for(&format!("el kit {kit}"), 120.0, |app| app.status.as_deref().is_some_and(|t| t.starts_with("Kit ")));
        s.shot(&format!("kit-{kit}"));
        // Its name, in the header's menu.
        let h = s.find(header).unwrap();
        s.rclick_at(h.center());
        let field = s.seen.iter().filter(|w| w.role == "TextInput" && w.rect.top() < 200.0).map(|w| w.rect).next().unwrap();
        s.click_at(field.center());
        s.key(Key::A, Modifiers::COMMAND);
        s.type_text(name);
        s.key(Key::Enter, Modifiers::NONE);
        s.key(Key::Escape, Modifiers::NONE);
    }
    s.note(format!("  pistas: {:?}", names(&s)));
    s.shot("kits-cargados");
    if stop(&mut s, 2) {
        return;
    }

    // ---- 3. Beats: techno with the AI on the kick and hats, hardgroove on the percussion.
    for (t, style, level, ai) in [(0usize, "Techno", "Medio", true), (1, "Hardgroove", "Básico", false)] {
        let at = s.find(&format!("Slot {}:1", t + 1)).unwrap();
        s.dclick_at(at.center());
        s.click("Generar ritmo");
        s.shot(&format!("generar-{style}"));
        s.dump(&format!("generar-{style}"));
        s.click(style);
        s.click(level);
        s.click("4 c.");
        if ai {
            s.click("Que lo toque la IA");
        } else {
            // The kick and the hats are the other track's: percussion only.
            for sound in ["Bombo", "Palmada", "Charles", "Charles abierto"] {
                s.click(sound);
            }
            s.click("Que lo toque la IA");
            s.shot("percusion-sin-bombo");
        }
        s.click("Generar");
        s.wait_for(&format!("el ritmo {style}"), if ai { 90.0 } else { 3.0 }, move |app| crate::produccion::notes(app, t, 0) > 8);
        s.note(format!("  notas: {}", crate::produccion::notes(&s.app, t, 0)));
        if !ai {
            // Almost nothing: more level, and the cowbell.
            s.shot("percusion-casi-vacia");
            s.click("Intenso");
            s.click("Cencerro");
            s.click("Generar");
            s.frames(5);
            s.note(format!("  notas en Intenso: {}", crate::produccion::notes(&s.app, t, 0)));
        }
        s.shot(&format!("ritmo-{style}"));
        s.key(Key::Escape, Modifiers::NONE);
    }
    if stop(&mut s, 3) {
        return;
    }

    // ---- 4. The drum bus: both in a group (⌘G), with the bus compressor.
    let h = s.find("Pista Bombo y charles").unwrap();
    s.click_at(h.center());
    let h = s.find("Pista Percusión").unwrap();
    s.press_at(h.center(), eframe::egui::PointerButton::Primary, Modifiers::SHIFT, 1);
    s.key(Key::G, Modifiers::COMMAND);
    s.note(format!("  pistas: {:?}", names(&s)));
    s.shot("grupo");
    rename(&mut s, "Pista ▾ Grupo 1", "Batería");
    let h = s.find("Pista ▾ Batería").unwrap();
    s.click_at(h.center());
    s.click("Compresor de bus");
    if !s.has("Presets") {
        s.click("Dispositivos");
    }
    s.shot("glue-en-el-bus");
    s.dump("glue-en-el-bus");
    effect_preset(&mut s, "Bus de batería");
    s.shot("glue-bus-de-bateria");
    summary(&mut s);
    if stop(&mut s, 4) {
        return;
    }

    // ---- 5. Chords: a new MIDI track, a minor progression from the menu.
    s.key(Key::T, Modifiers::COMMAND);
    let chords = s.app.project.tracks.len() - 1;
    let header = format!("Pista {}", s.app.project.tracks[chords].name);
    rename(&mut s, &header, "Acordes");
    let slot = s.find(&format!("Slot {}:1", chords + 1)).unwrap();
    s.dclick_at(slot.center());
    // The editor taller, as anyone would.
    s.drag(pos2(800.0, 585.0), pos2(800.0, 330.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 800.0), Modifiers::NONE);
    s.frames(10);
    let menu = s.seen.iter().filter(|w| w.text == "Acordes" && w.role == "Button" && w.rect.top() > 250.0).map(|w| w.rect).last().unwrap();
    s.click_at(menu.center());
    s.shot("menu-acordes");
    s.dump("menu-acordes");
    let pick = s.seen.iter().find(|w| w.role == "Button" && w.text.starts_with("Dramática")).map(|w| w.text.clone()).unwrap();
    s.click(&pick);
    s.key(Key::Escape, Modifiers::NONE);
    {
        let Some(Clip::Midi(c)) = &s.app.project.tracks[chords].clips[0] else { panic!() };
        s.note(format!("  acordes «{}»: {} notas, {} tiempos", c.name, c.notes.len(), c.length));
    }
    s.shot("progresion");
    if stop(&mut s, 5) {
        return;
    }

    // ---- 6. Bass, pad and a melody from the chords (Acompañar).
    for part in ["Bajo: contratiempo", "Pad", "Melodía: gancho"] {
        let clip_at = s.find("Clip 4 · Rem Sib Do Rem").unwrap();
        s.rclick_at(clip_at.center());
        s.click("Acompañar ⏵");
        if part == "Melodía: gancho" {
            s.move_to(s.find(part).unwrap().center(), Modifiers::NONE);
            s.shot("menu-acompanar");
        }
        s.submenu(part);
        s.frames(5);
    }
    s.note(format!("  pistas: {:?}", names(&s)));
    s.shot("melodia-generada");
    s.dump("melodia-generada");
    if stop(&mut s, 6) {
        return;
    }

    // ---- 7. The melody retouched: higher, fewer notes, another take, and by hand.
    let lead = track(&s, "Melodía");
    let pitches = |s: &Studio| match &s.app.project.tracks[lead].clips[0] {
        Some(Clip::Midi(c)) => c.notes.iter().map(|n| (n.start, n.pitch, n.length)).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    s.note(format!("  melodía generada: {:?}", pitches(&s)));
    let register = below(&s, "Medio", 330.0).unwrap();
    s.click_at(register.center());
    s.click("Agudo");
    let density = below(&s, "Normal", 330.0).unwrap();
    s.click_at(density.center());
    s.click("Pocas notas");
    s.click("Otra variación");
    s.shot("melodia-aguda-pocas");
    s.note(format!("  melodía aguda, pocas notas, otra variación: {:?}", pitches(&s)));
    // By hand: the last note an octave up (⇧↑), as an answer, and longer.
    let (start, pitch, length) = *pitches(&s).last().unwrap();
    let at = note_at(&s, pitch, start + length * 0.5, 16.0);
    s.click_at(at);
    s.key(Key::ArrowUp, Modifiers::SHIFT);
    let moved = pitches(&s).iter().any(|n| (n.0 - start).abs() < 1e-6 && n.1 == pitch + 12);
    s.note(format!("  última nota {} → una octava arriba: {}", crate::project::note_name(pitch), if moved { "sí" } else { "NO" }));
    s.shot("nota-fuera-de-vista");
    // The moved note went off the top of the view; a visible one made longer.
    let (start, pitch, length) = pitches(&s).into_iter().find(|n| n.0 > 2.0 && n.0 < 14.0).unwrap();
    let end = note_at(&s, pitch, start + length, 16.0) - vec2(2.0, 0.0);
    s.drag(end, end + vec2(72.0, 0.0), Modifiers::NONE);
    s.note(format!("  melodía retocada: {:?}", pitches(&s)));
    s.shot("melodia-retocada");
    if stop(&mut s, 7) {
        return;
    }

    // ---- 8. Sounds: Wavetable for chords and bass, factory presets, OTT on pad and lead.
    for (header, instrument, category) in [("Pista Acordes", Some("Wavetable Synth"), "Keys"), ("Pista Bajo", Some("Wavetable Synth"), "Bass"), ("Pista Pad", None, "Pad")] {
        let h = s.find(header).unwrap();
        s.click_at(h.center());
        if let Some(i) = instrument {
            s.click(i);
        }
        crate::produccion::preset(&mut s, header, category, "");
    }
    s.shot("presets");
    for (header, preset) in [("Pista Pad", "Brillo en pads"), ("Pista Melodía", "Lead que corta")] {
        let h = s.find(header).unwrap();
        s.click_at(h.center());
        s.click("Multibanda (OTT)");
        if !s.has("Presets") {
            s.click("Dispositivos");
        }
        effect_preset(&mut s, preset);
    }
    s.shot("ott-en-el-lead");
    summary(&mut s);
    if stop(&mut s, 8) {
        return;
    }

    // ---- 9. Expression: a lead played on a keyboard with the bend and the wheel,
    // recorded into a slot. The keyboard is the direct part (LiveNote); the
    // engine runs by hand, as the audio card would.
    s.key(Key::T, Modifiers::COMMAND);
    let lead2 = s.app.project.tracks.len() - 1;
    let header = format!("Pista {}", s.app.project.tracks[lead2].name);
    rename(&mut s, &header, "Lead");
    let h = s.find("Pista Lead").unwrap();
    s.click_at(h.center());
    s.click("Wavetable Synth");
    crate::produccion::preset(&mut s, "Pista Lead", "Lead", "Trance brillante");
    // The editor down again, to see the mixer.
    s.drag(pos2(800.0, 333.0), pos2(800.0, 585.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 850.0), Modifiers::NONE);
    s.frames(10);
    s.shot("botones-de-armar-tapados");
    // The arm buttons are under the panel: the panel lower still.
    s.drag(pos2(800.0, 586.0), pos2(800.0, 660.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 850.0), Modifiers::NONE);
    s.frames(10);
    s.dump("antes-de-armar");
    let h = s.find("Pista Lead").unwrap();
    let arm = s.seen.iter().filter(|w| w.text == "●" && w.role == "CheckBox").min_by(|a, b| (a.rect.center().x - h.center().x - 36.6).abs().total_cmp(&(b.rect.center().x - h.center().x - 36.6).abs())).map(|w| w.rect).unwrap();
    s.click_at(arm.center());
    s.note(format!("  Lead armada: {}", s.app.project.tracks[lead2].armed));
    assert!(s.app.project.tracks[lead2].armed, "no se armó la pista Lead");
    let (detached, mut engine) = crate::app::Audio::detached();
    let headless = std::mem::replace(&mut s.app.audio, detached);
    s.app.sync_all();
    s.shot("lead-armada");
    let slot = s.find(&format!("Slot {}:1", lead2 + 1)).unwrap();
    s.dump("slot-armado");
    // The red dot, on the slot's left.
    s.click_at(pos2(slot.left() + 8.0, slot.center().y));
    s.note(format!("  grabando en el slot: {:?}", s.app.recorder.session.iter().map(|t| (t.track, t.slot)).collect::<Vec<_>>()));
    let tempo = s.app.project.tempo;
    let mut run = |s: &mut Studio, beats: f64| {
        let mut buffer = vec![0.0; 1024];
        let blocks = ((beats * 60.0 / tempo * 48_000.0) / 512.0).round() as usize;
        for i in 0..blocks {
            engine.process(&mut buffer, 2);
            s.app.update_recording();
            if i % 8 == 0 {
                s.frames(1);
            }
        }
    };
    run(&mut s, 0.5);
    s.shot("grabando");
    let live = s.app.audio.live().unwrap();
    use engine::Controller;
    let send = |c: Controller, v: f32| live.send(engine::LiveNote::control(c, 0, v));
    // Waiting for the take to start at the bar.
    let state = s.app.audio.state().unwrap();
    let mut tries = 0;
    while !s.app.recorder.session.iter().any(|t| t.start.is_some()) {
        tries += 1;
        assert!(tries < 100, "la toma no empieza: {:?}", s.app.status);
        run(&mut s, 0.125);
    }
    s.note(format!("  la toma empieza en el tiempo {:.2}", state.position()));
    // Four bars over Dm Bb C Dm: A scooped up into D, held with the wheel, a
    // fall to C, a bend down to Bb, and D again with vibrato and a dive.
    let phrase: [(u8, f64); 4] = [(69, 4.0), (72, 4.0), (70, 4.0), (74, 4.0)];
    for (bar, (pitch, beats)) in phrase.into_iter().enumerate() {
        live.send(engine::LiveNote::note(pitch, 0.85));
        match bar {
            0 => {
                // Scoop: from a tone below up to the note.
                send(Controller::PitchBend, -1.0);
                for i in 1..=12 {
                    run(&mut s, 1.0 / 24.0);
                    send(Controller::PitchBend, -1.0 + i as f32 / 12.0);
                }
                run(&mut s, 1.5);
                for i in 0..=24 {
                    send(Controller::Cc(1), i as f32 / 24.0 * 0.7);
                    run(&mut s, 1.0 / 16.0);
                }
                run(&mut s, beats - 0.5 - 1.5 - 1.5);
                send(Controller::Cc(1), 0.0);
            }
            1 => {
                run(&mut s, 3.0);
                // Bend down a tone at the end of the bar (C → Bb), released.
                for i in 0..=16 {
                    send(Controller::PitchBend, -(i as f32) / 16.0);
                    run(&mut s, 1.0 / 32.0);
                }
                run(&mut s, beats - 3.0 - 0.53);
                send(Controller::PitchBend, 0.0);
            }
            2 => run(&mut s, beats),
            _ => {
                for i in 0..=32 {
                    send(Controller::Cc(1), (i as f32 / 32.0).min(1.0));
                    run(&mut s, 1.0 / 16.0);
                }
                run(&mut s, 0.5);
                // The dive at the end.
                for i in 0..=24 {
                    send(Controller::PitchBend, -(i as f32) / 24.0);
                    run(&mut s, 1.0 / 32.0);
                }
                run(&mut s, beats - 2.0 - 0.5 - 0.78);
            }
        }
        live.send(engine::LiveNote::note(pitch, 0.0));
    }
    send(Controller::PitchBend, 0.0);
    send(Controller::Cc(1), 0.0);
    // Clicked again to stop it, as in the guide.
    let slot = s.seen.iter().filter(|w| (w.rect.center() - slot.center()).length() < 4.0 && w.role == "Button").map(|w| w.rect).next().unwrap_or(slot);
    s.click_at(slot.center());
    let mut waited = 0;
    while !matches!(&s.app.project.tracks[lead2].clips[0], Some(Clip::Midi(_))) && waited < 400 {
        run(&mut s, 0.125);
        waited += 1;
    }
    s.app.audio.send(engine::Command::Stop);
    run(&mut s, 0.25);
    match &s.app.project.tracks[lead2].clips[0] {
        Some(Clip::Midi(c)) => {
            let lanes: Vec<String> = c.controls.iter().map(|l| format!("{:?}: {} puntos", l.control, l.points.len())).collect();
            s.note(format!("  toma «{}»: {} notas {:?}, {} tiempos, carriles {lanes:?}", c.name, c.notes.len(), c.notes.iter().map(|n| (n.pitch, (n.start * 100.0).round() / 100.0, (n.length * 100.0).round() / 100.0)).collect::<Vec<_>>(), c.length));
        }
        other => s.note(format!("  ¡no hay toma! {:?} · estado {:?}", other.is_some(), s.app.status)),
    }
    let _ = std::mem::replace(&mut s.app.audio, headless);
    s.app.sync_all();
    s.frames(5);
    s.shot("toma-con-expresion");
    s.dump("toma-con-expresion");
    // Played a little long: the take is five bars, the loop back to four.
    let combo = s.seen.iter().find(|w| w.role == "ComboBox" && w.text.ends_with("comp.")).map(|w| w.rect).unwrap();
    s.click_at(combo.center());
    s.click("4 comp.");
    if let Some(Clip::Midi(c)) = &s.app.project.tracks[lead2].clips[0] {
        s.note(format!("  toma con loop de {} tiempos", c.length));
    }
    if stop(&mut s, 9) {
        return;
    }

    // ---- 10. A «Break» scene (⌥-drag copies chords, pad and lead down a row),
    // scene 1 is the «Drop», and Crear › Arreglo automático… makes the song.
    for t in [track(&s, "Acordes"), track(&s, "Pad"), lead2] {
        let from = s.find(&format!("Clip {} ·", t + 1)).unwrap();
        let to = s.find(&format!("Slot {}:2", t + 1)).unwrap();
        s.drag(from.center(), to.center(), Modifiers::ALT);
    }
    s.note(format!("  escena 2: {:?}", s.app.project.tracks.iter().map(|t| t.clips[1].as_ref().map(|c| c.name().to_string())).collect::<Vec<_>>()));
    for (scene, name) in [("Escena 1", "Drop"), ("Escena 2", "Break")] {
        let at = s.find(scene).unwrap();
        s.rclick_at(at.center());
        let field = s.seen.iter().filter(|w| w.role == "TextInput" && (w.rect.center().y - at.center().y).abs() < 120.0).map(|w| w.rect).next().unwrap();
        s.click_at(field.center());
        s.key(Key::A, Modifiers::COMMAND);
        s.type_text(name);
        s.key(Key::Enter, Modifiers::NONE);
        s.key(Key::Escape, Modifiers::NONE);
    }
    s.note(format!("  escenas: {:?}", &s.app.project.scene_names[..2]));
    s.shot("escenas");
    s.click("Crear");
    s.click("Arreglo automático…");
    s.click("Electrónica (intro, subida, drop, break, drop, outro)");
    s.shot("arreglo-electronica");
    s.dump("arreglo-electronica");
    // The intro and the outro need the drums, for the DJ: kick and hats in, and
    // the percussion in the outro.
    for (x, y) in [(499.0, 409.0), (798.5, 409.0), (798.5, 433.0)] {
        s.click_at(pos2(x, y));
    }
    s.shot("arreglo-retocado");
    s.click("Crear en el Arrangement");
    s.frames(10);
    let markers: Vec<String> = s.app.project.markers.iter().map(|m| format!("{} @{}", m.name, m.beat / 4.0 + 1.0)).collect();
    s.note(format!("  marcadores: {markers:?}"));
    // Less editor, more song.
    let zoom = s.seen.iter().filter(|w| w.role == "Slider" && w.rect.top() < 90.0).map(|w| w.rect).next();
    if let Some(zoom) = zoom {
        s.drag(zoom.center(), pos2(zoom.left() + 2.0, zoom.center().y), Modifiers::NONE);
    }
    s.shot("arrangement");
    s.dump("arrangement");
    listen(&mut s, "1-arreglo-sin-mezcla");
    if stop(&mut s, 10) {
        return;
    }

    // ---- 11. A spoken voice, made with macOS's `say`, dropped on the break.
    let voice = s.out.join("Voz Monica.wav");
    let text = "Cuando la ciudad se apaga, seguimos bailando. [[slnc 900]] Cierra los ojos y escucha la noche. [[slnc 900]] \
        Somos luz en la oscuridad. [[slnc 900]] No mires atrás, el amanecer nos espera.";
    let ok = std::process::Command::new("say").args(["-v", "Mónica", "-r", "150", "-o"]).arg(&voice).args(["--data-format=LEI16@44100", text]).status().unwrap();
    assert!(ok.success());
    s.key(Key::T, Modifiers::COMMAND | Modifiers::SHIFT);
    let vt = s.app.project.tracks.len() - 1;
    // The new track is below the view: scrolled down to it.
    s.move_to(pos2(900.0, 450.0), Modifiers::NONE);
    s.frame_with(vec![eframe::egui::Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: vec2(0.0, -400.0), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move }], Modifiers::NONE);
    s.frames(40);
    s.shot("arrangement-sin-scroll-vertical");
    // The wheel doesn't scroll the tracks: the drum group folded to make room.
    let fold = s.seen.iter().find(|w| w.text.starts_with("▾ Batería") && w.rect.left() < 340.0).map(|w| w.rect).unwrap();
    s.click_at(pos2(fold.left() + 3.0, fold.center().y));
    s.frames(5);
    s.shot("bateria-plegada");
    let name = s.app.project.tracks[vt].name.clone();
    let h = s.seen.iter().find(|w| w.text == name && w.rect.left() < 340.0).map(|w| w.rect).unwrap_or_else(|| {
        s.dump("pista-voz");
        panic!("no veo la cabecera «{name}»")
    });
    s.rclick_at(h.center());
    let field = s.seen.iter().filter(|w| w.role == "TextInput").min_by(|a, b| (a.rect.center() - h.center()).length().total_cmp(&(b.rect.center() - h.center()).length())).map(|w| w.rect).unwrap();
    s.click_at(field.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text("Voz");
    s.key(Key::Enter, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
    s.frames(5);
    s.dump("pista-voz");
    let lane = s.seen.iter().find(|w| w.text == "Voz" && w.rect.left() < 340.0).map(|w| w.rect).unwrap();
    let break_at = s.app.project.markers.iter().find(|m| m.name == "Break").map(|m| m.beat).unwrap();
    let mark = s.seen.iter().find(|w| w.text == "Break" && w.rect.top() < 120.0).map(|w| w.rect).unwrap();
    let at = pos2(mark.left() - 6.0, lane.center().y);
    s.note(format!("soltar «{}» en el Break (tiempo {break_at}) en {at:?}", voice.display()));
    let placed = s.app.drop_files(vec![voice.clone()], at);
    s.frames(10);
    s.wait_for("el tempo de la voz", 20.0, |app| app.status.is_some());
    let clip = s.app.project.tracks[vt].arrangement.iter().map(|c| (c.start, c.length, c.clip.name().to_string())).collect::<Vec<_>>();
    s.note(format!("  soltado: {placed} · clips en Voz: {clip:?}"));
    s.shot("voz-soltada");
    let wait = std::time::Instant::now();
    while wait.elapsed().as_secs() < 30 && !s.app.status.as_deref().is_some_and(|t| !t.contains("analizando")) {
        std::thread::sleep(std::time::Duration::from_millis(50));
        s.frames(1);
    }
    let voice_clip = |s: &Studio| match &s.app.project.tracks[vt].arrangement.first().map(|c| (c.start, c.length, c.clip.clone())) {
        Some((start, length, Clip::Audio(a))) => format!("en el tiempo {start} ({length} tiempos), warp {:?} a {:.1} BPM, tono {:?}", a.warp, a.original_bpm, a.key),
        _ => "nada".into(),
    };
    s.note(format!("  la voz: {}", voice_clip(&s)));
    if stop(&mut s, 11) {
        return;
    }

    // ---- 12. The voice to MIDI, tuned, and its lyrics.
    // Spoken, not a loop: its warp Off by hand, in the editor that opened.
    s.shot("voz-en-beats");
    let off = s.seen.iter().find(|w| w.text == "Off" && w.rect.top() > 300.0).map(|w| w.rect).unwrap();
    s.click_at(off.center());
    s.note(format!("  la voz con warp Off: {}", voice_clip(&s)));
    // The panel down, to see the voice's lane.
    // The audio editor won't let the panel shrink: to the devices first.
    s.shot("panel-no-baja");
    s.click("Dispositivos");
    s.frames(5);
    let tabs = s.seen.iter().find(|w| w.text == "Dispositivos" && w.role == "Button").map(|w| w.rect).unwrap();
    s.drag(pos2(900.0, tabs.top() - 6.0), pos2(900.0, 660.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 870.0), Modifiers::NONE);
    s.frames(10);
    s.shot("panel-abajo");
    let clip_title = |s: &Studio| s.seen.iter().filter(|w| w.text == "Voz Monica" && w.role == "Texto" && w.rect.left() > 340.0).map(|w| w.rect).next().unwrap_or_else(|| panic!("no veo el clip de voz"));
    let title = clip_title(&s);
    s.rclick_at(title.center() + vec2(30.0, 10.0));
    s.shot("menu-clip-voz");
    s.dump("menu-clip-voz");
    s.click("Convertir a MIDI ⏵");
    s.shot("menu-convertir");
    s.submenu("Melodía");
    let started = std::time::Instant::now();
    s.wait_for("audio a MIDI", 120.0, |app| app.to_midi.is_none());
    s.frames(10);
    s.note(format!("  audio a MIDI en {:.1} s · pistas {:?}", started.elapsed().as_secs_f64(), names(&s)));
    if let Some(t) = s.app.project.tracks.iter().position(|t| t.name.contains("Voz") && !t.is_audio()) {
        let notes: Vec<(u8, f64, f64)> = s.app.project.tracks[t].arrangement.iter().flat_map(|c| match &c.clip {
            Clip::Midi(m) => m.notes.iter().map(|n| (n.pitch, (n.start * 100.0).round() / 100.0, (n.length * 100.0).round() / 100.0)).collect::<Vec<_>>(),
            _ => Vec::new(),
        }).collect();
        s.note(format!("  «{}»: {} notas {:?}", s.app.project.tracks[t].name, notes.len(), notes));
    }
    s.shot("voz-en-midi");

    // Tuned: the clip, «Editar notas…», «Afinar».
    let title = clip_title(&s);
    s.click_at(title.center() + vec2(30.0, 10.0));
    s.frames(5);
    if !s.has("Editar notas…") {
        s.click("Clip");
    }
    s.shot("editor-audio-voz");
    s.dump("editor-audio-voz");
    s.click("Editar notas…");
    let started = std::time::Instant::now();
    let source = s.app.tuning_source(ClipRef::Arrangement { track: vt, index: 0 }).unwrap_or_default();
    s.wait_for("análisis de la voz", 120.0, move |app| app.tuner.analysis(&source).is_some());
    s.note(format!("  análisis de afinación en {:.1} s", started.elapsed().as_secs_f64()));
    s.frames(10);
    s.shot("afinacion-notas");
    let before = match &s.app.project.tracks[vt].arrangement[0].clip { Clip::Audio(a) => a.sample.clone(), _ => String::new() };
    s.click("Afinar");
    s.frames(10);
    // Spoken, it glides: the vibrato off too, for the robotic tuned voice.
    s.click("Sin vibrato");
    s.frames(30);
    let after = match &s.app.project.tracks[vt].arrangement[0].clip { Clip::Audio(a) => a.sample.clone(), _ => String::new() };
    s.note(format!("  archivo de la voz: {before} → {after}"));
    s.app.finish_tuning_writes();
    s.frames(10);
    s.shot("afinacion-afinada");
    s.note(format!("  la voz tras afinar: {} · {:?}", voice_clip(&s), s.app.status));
    s.click("← Forma de onda");

    // The lyrics.
    // The clip's lane is under the editor again: «Transcribir…» in the editor.
    s.frames(5);
    s.click("Transcribir…");
    s.shot("letra-pregunta");
    s.dump("letra-pregunta");
    s.click("Español");
    s.click("Transcribir");
    let started = std::time::Instant::now();
    s.wait_for("la letra", 300.0, |app| app.lyrics_ui.job.is_none());
    let clip = ClipRef::Arrangement { track: vt, index: 0 };
    s.note(format!("  letra en {:.1} s: «{}»", started.elapsed().as_secs_f64(), s.app.clip_lyrics_text(clip)));
    if let Some(l) = s.app.clip_lyrics(clip).cloned() {
        for line in &l.lines {
            s.note(format!("    [{}–{}] {}", crate::lyrics::lrc_time(line.start), crate::lyrics::lrc_time(line.end), line.text()));
        }
    }
    s.frames(5);
    s.shot("letra-en-el-clip");
    s.app.select_clip(clip);
    s.frames(5);
    s.shot("letra-en-el-editor");
    if stop(&mut s, 12) {
        return;
    }

    // ---- 13. A return with Espacio (⌥⌘T), and the sends to it in the mixer.
    s.key(Key::Tab, Modifiers::NONE);
    s.key(Key::T, Modifiers::COMMAND | Modifiers::ALT);
    s.note(format!("  retornos: {:?}", s.app.project.returns.iter().map(|r| r.name.clone()).collect::<Vec<_>>()));
    let ret = s.app.project.returns.last().unwrap().name.clone();
    s.shot("session-con-retorno");
    s.dump("session-con-retorno");
    // ⌥⌘T also made a MIDI track (⌘T's shortcut doesn't look at ⌥): away with it.
    if let Some(extra) = s.app.project.tracks.iter().position(|t| t.name.ends_with(" MIDI")) {
        s.note(format!("  ¡⌥⌘T creó también la pista «{}»!", s.app.project.tracks[extra].name));
        let h = s.find(&format!("Pista {}", s.app.project.tracks[extra].name)).unwrap();
        s.rclick_at(h.center());
        s.click("Eliminar pista");
    }
    // The return is off the right edge: the Session scrolled sideways.
    s.move_to(pos2(700.0, 200.0), Modifiers::NONE);
    s.frame_with(vec![eframe::egui::Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: vec2(-600.0, 0.0), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move }], Modifiers::NONE);
    s.frames(40);
    // Returns have no name to change (their menu only deletes them).
    let h = s.find(&format!("Pista {ret}")).unwrap();
    s.rclick_at(h.center());
    s.shot("menu-retorno");
    s.key(Key::Escape, Modifiers::NONE);
    s.click_at(h.center());
    s.click("Dispositivos");
    browser_click(&mut s, "Espacio");
    s.shot("espacio");
    effect_preset_in(&mut s, "Retorno", "Sala");
    s.shot("espacio-retorno");
    s.dump("mezclador-con-c");
    let ret = s.app.project.returns.len() - 1;
    s.note(format!("  {}: {:?}", s.app.project.returns[ret].name, s.app.project.returns[ret].devices.iter().map(|d| format!("{:?}", d.effect)).collect::<Vec<_>>()));
    // The mixer is under the panel: the panel down, as far as it goes.
    let tabs = s.seen.iter().find(|w| w.text == "Dispositivos" && w.role == "Button").map(|w| w.rect).unwrap();
    s.drag(pos2(900.0, tabs.top() - 6.0), pos2(900.0, 760.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 880.0), Modifiers::NONE);
    s.frames(10);
    // Back to the left of the Session.
    s.move_to(pos2(700.0, 200.0), Modifiers::NONE);
    s.frame_with(vec![eframe::egui::Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: vec2(900.0, 0.0), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move }], Modifiers::NONE);
    s.frames(40);
    s.shot("mezclador");
    s.dump("mezclador");
    // The Lead is still armed from the take: disarmed.
    let h = s.find("Pista Lead").unwrap();
    let arm = s.seen.iter().filter(|w| w.text == "●" && w.role == "CheckBox").min_by(|a, b| (a.rect.center().x - h.center().x - 36.6).abs().total_cmp(&(b.rect.center().x - h.center().x - 36.6).abs())).map(|w| w.rect).unwrap();
    s.click_at(arm.center());
    s.note(format!("  Lead armada: {}", s.app.project.tracks[lead2].armed));
    // Sends to C: the knob under each track's «C» label, dragged up.
    for (name, pixels) in [("Pad", 50.0), ("Lead", 40.0), ("Voz", 60.0), ("Melodía", 30.0)] {
        let h = s.find(&format!("Pista {name}")).unwrap();
        let label = s.seen.iter().filter(|w| w.role == "Texto" && w.text == "C" && w.rect.top() > 300.0 && w.rect.top() < 420.0).min_by(|a, b| (a.rect.center().x - h.left() - 23.0).abs().total_cmp(&(b.rect.center().x - h.left() - 23.0).abs())).map(|w| w.rect).unwrap();
        let knob = label.center() + vec2(0.0, 17.0);
        s.drag(knob, knob - vec2(0.0, pixels), Modifiers::NONE);
        let t = track(&s, name);
        s.note(format!("  envío C de {name}: {:.0} %", s.app.project.tracks[t].sends[ret] * 100.0));
    }
    s.shot("envios-a-espacio");
    if stop(&mut s, 13) {
        return;
    }

    // ---- 14. Transitions into the drops, and the bus compressor on the master.
    s.key(Key::Tab, Modifiers::NONE);
    s.frames(5);
    for n in [1, 2] {
        for path in [&["Subida ⏵", "8 compases"][..], &["Impacto"][..]] {
            let drop = s.seen.iter().filter(|w| w.text == "Drop" && w.rect.top() < 130.0).map(|w| w.rect).nth(n - 1).unwrap();
            s.rclick_at(drop.center());
            s.click("Transición aquí ⏵");
            for item in path {
                s.submenu(item);
            }
        }
    }
    let t = s.app.project.tracks.iter().find(|t| t.name == "Transiciones").unwrap();
    let clips: Vec<String> = t.arrangement.iter().map(|c| format!("{} @{}–{}", c.clip.name(), c.start / 4.0 + 1.0, c.end() / 4.0 + 1.0)).collect();
    s.note(format!("  transiciones: {clips:?}"));
    s.shot("transiciones");
    s.key(Key::Tab, Modifiers::NONE);
    s.frames(5);
    // The Master is off the right edge of the Session: scrolled to it.
    s.move_to(pos2(700.0, 200.0), Modifiers::NONE);
    s.frame_with(vec![eframe::egui::Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: vec2(-900.0, 0.0), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move }], Modifiers::NONE);
    s.frames(40);
    let h = s.find("Pista Master").unwrap();
    assert!(h.right() < s.size.x, "el Master sigue fuera de la ventana: {h:?}");
    s.click_at(h.center());
    s.note(format!("  seleccionado: {:?}", s.app.selected_target));
    s.click("Dispositivos");
    browser_click(&mut s, "Compresor de bus");
    effect_preset(&mut s, "Pegar la mezcla");
    s.note(format!("  master: {:?}", s.app.project.master.devices.iter().map(|d| format!("{:?}", d.effect)).collect::<Vec<_>>()));
    s.shot("glue-en-el-master");
    summary(&mut s);
    listen(&mut s, "2-mezcla-a-mano");
    if stop(&mut s, 14) {
        return;
    }

    // ---- 15. The mix assistant, for the club, all at once.
    s.menu(crate::commands::Command::MixAssistant);
    s.frames(5);
    s.dump("asistente");
    let destination = s.seen.iter().find(|w| w.role == "ComboBox" && w.text.contains("LUFS")).map(|w| w.text.clone()).unwrap();
    s.click(&destination);
    let club = s.seen.iter().find(|w| w.text.starts_with("Club")).map(|w| w.text.clone()).unwrap();
    s.click(&club);
    s.click("Analizar la mezcla");
    s.wait_for("el análisis", 180.0, |app| app.mix.report.is_some());
    s.frames(5);
    s.shot("analisis");
    s.dump("analisis");
    s.click("Aplicar todo");
    let started = std::time::Instant::now();
    s.wait_for("aplicar todo", 600.0, |app| !app.mix.busy() && app.mix.report.is_some());
    s.note(format!("  aplicar todo: {:.0} s", started.elapsed().as_secs_f64()));
    s.frames(5);
    s.shot("aplicado");
    s.dump("aplicado");
    summary(&mut s);
    s.note(format!("  master: {:?}", s.app.project.master.devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ']).next().unwrap_or("").to_string()).collect::<Vec<_>>()));
    s.note(format!("  retornos: {:?}", s.app.project.returns.iter().map(|r| (r.name.clone(), r.devices.iter().map(|d| format!("{:?}", d.effect).split(['(', ' ']).next().unwrap_or("").to_string()).collect::<Vec<_>>())).collect::<Vec<_>>()));
    listen(&mut s, "3-asistente");
    s.click("Close window");
    if stop(&mut s, 15) {
        return;
    }

    // ---- 16. Saved (⇧⌘S), exported (⇧⌘R), measured, and opened again.
    let project = s.out.join("Noche en la ciudad").join("Noche en la ciudad.pulse");
    crate::script::answer_dialog(vec![project.clone()]);
    s.key(Key::S, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(5);
    s.note(format!("  guardado: {} ({:?})", project.display(), s.app.file));
    s.key(Key::R, Modifiers::COMMAND | Modifiers::SHIFT);
    s.shot("exportar");
    let wav = s.out.join("Noche en la ciudad.wav");
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
    s.note(format!("  {}: {:.1} s a {rate} Hz · {:?} LUFS · pico real {:.2} dBTP · pico {:.2} dBFS · rango {:?} LU · correlación {:.2}", wav.display(), l.len() as f32 / rate, m.lufs, m.true_peak, m.peak, m.range, m.correlation));
    s.note(format!("  octavas (31 Hz…16 kHz, dB): {:?}", m.bands.iter().map(|b| (b * 10.0).round() / 10.0).collect::<Vec<_>>()));
    let reopened = Project::load(&project).unwrap();
    let again = PulseApp::headless(reopened);
    let mut s2 = Studio::new(again, s.out.clone());
    let m2 = listen(&mut s2, "4-reabierto");
    s.log.extend(s2.log.drain(..));
    s.note(format!("  reabierto: {:?} LUFS frente a {:?} exportado", m2.lufs, m.lufs));
    stop(&mut s, 16);
}

/// Where the editor draws `pitch` at `beat` of a clip `length` beats long
/// (from its key labels, C3 being 60, and the grid's width at this zoom).
fn note_at(s: &Studio, pitch: u8, beat: f64, length: f64) -> Pos2 {
    let label = |name: &str| s.seen.iter().find(|w| w.role == "Texto" && w.text == name && w.rect.left() < 230.0).map(|w| w.rect.center().y).unwrap();
    let (c3, c4) = (label("C3"), label("C4"));
    let row = (c3 - c4) / 12.0;
    let (left, right) = (255.6, 1405.0);
    pos2(left + (right - left) * (beat / length) as f32, c3 - (pitch as f32 - 60.0) * row)
}

/// A track renamed in its header's menu.
fn rename(s: &mut Studio, header: &str, name: &str) {
    let h = s.find(header).unwrap_or_else(|| panic!("no veo «{header}»"));
    s.rclick_at(h.center());
    let field = s.seen.iter().filter(|w| w.role == "TextInput").min_by(|a, b| (a.rect.center() - h.center()).length().total_cmp(&(b.rect.center() - h.center()).length())).map(|w| w.rect).unwrap();
    s.click_at(field.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text(name);
    s.key(Key::Enter, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
}

/// The last effect's «Presets», and the factory one called `name` (in the
/// category `category`, if the menu has them).
fn effect_preset(s: &mut Studio, name: &str) {
    effect_preset_in(s, "", name)
}

fn effect_preset_in(s: &mut Studio, category: &str, name: &str) {
    let menu = s.seen.iter().filter(|w| w.text == "Presets" && w.rect.top() > 300.0).max_by(|a, b| a.rect.left().total_cmp(&b.rect.left())).map(|w| w.rect).unwrap();
    s.click_at(menu.center());
    s.dump(&format!("presets-{name}"));
    s.shot(&format!("presets-{name}"));
    let item = |s: &Studio| s.seen.iter().filter(|w| w.role == "Button" && w.text.contains(name) && w.rect.top() > menu.bottom() - 2.0 && !w.text.ends_with('⏵')).map(|w| w.text.clone()).next();
    let exact = s.seen.iter().filter(|w| w.role == "Button" && w.text == name && w.rect.top() > menu.bottom() - 2.0).map(|w| (w.text.clone(), w.rect)).next();
    let loose = s.seen.iter().filter(|w| w.role == "Button" && w.text.contains(name) && w.rect.top() > menu.bottom() - 2.0 && !w.text.ends_with('⏵')).map(|w| (w.text.clone(), w.rect)).next();
    let flat = !s.seen.iter().any(|w| w.role == "Button" && w.text.ends_with('⏵') && w.rect.top() > menu.bottom() - 2.0);
    if let Some((text, rect)) = exact.filter(|_| flat).or_else(|| loose.filter(|_| category.is_empty() || flat)) {
        s.note(format!("clic preset «{text}»"));
        s.click_at(rect.center());
    } else {
        let categories: Vec<String> = s.seen.iter().filter(|w| w.role == "Button" && w.text.ends_with('⏵') && w.rect.top() > menu.bottom() - 2.0).map(|w| w.text.clone()).collect();
        s.note(format!("  categorías: {categories:?}"));
        let chosen = categories.iter().find(|c| c.starts_with(category)).unwrap_or_else(|| panic!("no hay categoría «{category}»")).clone();
        s.hover(&chosen);
        let text = item(s).unwrap_or_else(|| panic!("no encuentro el preset «{name}» en «{chosen}»"));
        s.submenu(&text);
    }
    s.key(Key::Escape, Modifiers::NONE);
}

/// An item of the browser, scrolled into view first if it is below the window.
fn browser_click(s: &mut Studio, name: &str) {
    let find = |s: &Studio| s.seen.iter().find(|w| w.role == "Button" && w.text == name && w.rect.left() < 180.0).map(|w| w.rect);
    let mut tries = 0;
    while find(s).is_none_or(|r| r.bottom() > s.size.y - 30.0) && tries < 6 {
        if tries == 0 {
            s.note(format!("  «{name}» está debajo del navegador: lo desplazo"));
        }
        s.move_to(pos2(90.0, 600.0), Modifiers::NONE);
        s.frame_with(vec![eframe::egui::Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: vec2(0.0, -300.0), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move }], Modifiers::NONE);
        s.frames(30);
        tries += 1;
    }
    let r = find(s).unwrap();
    s.note(format!("clic «{name}» (navegador)"));
    s.click_at(r.center());
}

fn stop(s: &mut Studio, step: u32) -> bool {
    std::fs::write(s.out.join("diario.txt"), s.log.join("\n")).unwrap();
    until(step)
}
