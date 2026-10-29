//! A whole house/techno track made the way a producer in a hurry would make it,
//! through the interface without a window: clicks, menus and drags fed to egui,
//! screenshots drawn in memory, and the song rendered offline. An audit, not a
//! check: `PULSE_PRUEBA=dir cargo test --release -p app produccion -- --ignored --nocapture`.

use crate::app::PulseApp;
use crate::project::Project;
use eframe::App as _;
use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SIZE: egui::Vec2 = egui::Vec2 { x: 1440.0, y: 900.0 };
const SCALE: f32 = 1.5;

/// Something on screen: a widget (from the accessibility tree) or a painted text.
#[derive(Clone, Debug)]
pub struct Seen {
    pub text: String,
    pub role: String,
    pub rect: Rect,
    pub disabled: bool,
}

/// The app on a screen nobody sees, driven like a user would.
pub struct Studio {
    pub app: PulseApp,
    ctx: egui::Context,
    canvas: crate::offscreen::Canvas,
    frame: eframe::Frame,
    time: f64,
    last_click: f64,
    pointer: Pos2,
    pub seen: Vec<Seen>,
    pub out: PathBuf,
    pub log: Vec<String>,
    status: Option<String>,
    shots: usize,
    last: Option<egui::FullOutput>,
    /// When the last status came (to time each step).
    since: Instant,
    /// The screen, in points.
    pub size: egui::Vec2,
}

impl Studio {
    pub fn new(app: PulseApp, out: PathBuf) -> Self {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        let mut s = Self {
            app,
            ctx,
            canvas: Default::default(),
            frame: eframe::Frame::_new_kittest(),
            time: 1.0,
            last_click: -10.0,
            pointer: pos2(700.0, 450.0),
            seen: Vec::new(),
            out,
            log: Vec::new(),
            status: None,
            shots: 0,
            last: None,
            since: Instant::now(),
            size: SIZE,
        };
        s.frames(4);
        s
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn note(&mut self, line: impl Into<String>) {
        let line = line.into();
        eprintln!("[tema] {line}");
        self.log.push(line);
    }

    /// One frame with `events`.
    pub fn frame_with(&mut self, events: Vec<Event>, modifiers: Modifiers) {
        self.time += 1.0 / 60.0;
        let mut raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            focused: true,
            ..Default::default()
        };
        raw.events.push(Event::WindowFocused(true));
        raw.events.push(Event::ModifiersChanged(modifiers));
        raw.events.extend(events);
        self.app.raw_input_hook(&self.ctx, &mut raw);
        self.ctx.set_pixels_per_point(SCALE);
        let app = &mut self.app;
        let frame = &mut self.frame;
        let mut seen = Vec::new();
        let output = self.ctx.run_ui(raw, |ui| {
            app.ui(ui, frame);
            seen = widgets(ui.ctx());
        });
        let mut texts = Vec::new();
        fn collect(shape: &egui::Shape, texts: &mut Vec<Seen>) {
            match shape {
                egui::Shape::Text(t) => texts.push(Seen { text: t.galley.text().to_string(), role: "Texto".into(), rect: Rect::from_min_size(t.pos, t.galley.size()), disabled: false }),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                _ => {}
            }
        }
        output.shapes.iter().for_each(|s| collect(&s.shape, &mut texts));
        seen.extend(texts);
        self.seen = seen;
        if let Some(previous) = self.last.take() {
            self.canvas.skip(&previous);
        }
        self.last = Some(output);
        if self.app.status != self.status {
            if let Some(status) = &self.app.status {
                let line = format!("  estado (+{:.1} s): {status}", self.since.elapsed().as_secs_f64());
                self.since = Instant::now();
                eprintln!("[tema] {line}");
                self.log.push(line);
            }
            self.status = self.app.status.clone();
        }
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame_with(Vec::new(), Modifiers::NONE);
        }
    }

    /// Frames until `done` (real time: background jobs), at most `seconds`.
    pub fn wait_for(&mut self, what: &str, seconds: f64, done: impl Fn(&PulseApp) -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < seconds {
            if done(&self.app) {
                self.note(format!("  ({what}: {:.1} s)", start.elapsed().as_secs_f64()));
                return true;
            }
            std::thread::sleep(Duration::from_millis(30));
            self.frames(1);
        }
        self.note(format!("  ¡{what}: no llegó en {seconds} s!"));
        false
    }

    pub fn shot(&mut self, name: &str) -> PathBuf {
        // Popups fade in.
        self.frames(20);
        self.shots += 1;
        let path = self.out.join(format!("{:02}-{name}.png", self.shots));
        let output = self.last.take().unwrap();
        let image = self.canvas.draw(&self.ctx, output, self.size, SCALE);
        image.save_png(&path).unwrap();
        self.note(format!("  captura {}", path.display()));
        path
    }

    /// Only `rect` (points) of the screen, to PNG.
    pub fn shot_of(&mut self, name: &str, rect: Rect) -> PathBuf {
        self.frames(20);
        self.shots += 1;
        let path = self.out.join(format!("{:02}-{name}.png", self.shots));
        let output = self.last.take().unwrap();
        let image = self.canvas.draw(&self.ctx, output, SIZE, SCALE);
        let px = |v: f32| (v * SCALE).max(0.0) as usize;
        image.crop(px(rect.left()), px(rect.top()), px(rect.width()), px(rect.height())).save_png(&path).unwrap();
        self.note(format!("  captura {}", path.display()));
        path
    }

    /// What's on screen, to a text file (for finding one's way).
    pub fn dump(&mut self, name: &str) {
        let mut text = String::new();
        let mut seen = self.seen.clone();
        seen.sort_by(|a, b| (a.rect.top(), a.rect.left()).partial_cmp(&(b.rect.top(), b.rect.left())).unwrap());
        for w in &seen {
            text += &format!("{:7.1},{:6.1} {:5.0}×{:<4.0} {:<12} {}{}\n", w.rect.center().x, w.rect.center().y, w.rect.width(), w.rect.height(), w.role, w.text.replace('\n', "⏎"), if w.disabled { "  (desactivado)" } else { "" });
        }
        std::fs::write(self.out.join(format!("widgets-{name}.txt")), text).unwrap();
    }

    /// The thing showing `text`: a widget first (exact, else containing), then a painted text.
    pub fn find(&self, text: &str) -> Option<Rect> {
        self.find_nth(text, 1)
    }

    pub fn find_nth(&self, text: &str, n: usize) -> Option<Rect> {
        let pick = |exact: bool, widget: bool| {
            let mut m: Vec<&Seen> = self.seen.iter().filter(|w| (w.role != "Texto") == widget && if exact { w.text == text } else { w.text.contains(text) }).collect();
            m.sort_by(|a, b| (a.rect.top(), a.rect.left()).partial_cmp(&(b.rect.top(), b.rect.left())).unwrap());
            m.get(n - 1).map(|w| w.rect)
        };
        pick(true, true).or_else(|| pick(true, false)).or_else(|| pick(false, true)).or_else(|| pick(false, false))
    }

    pub fn find_near(&self, text: &str, near: Pos2) -> Option<Rect> {
        self.seen.iter().filter(|w| w.text == text).min_by(|a, b| a.rect.center().distance(near).total_cmp(&b.rect.center().distance(near))).map(|w| w.rect)
    }

    pub fn has(&self, text: &str) -> bool {
        self.find(text).is_some()
    }

    pub fn move_to(&mut self, at: Pos2, mods: Modifiers) {
        let from = self.pointer;
        for i in 1..=3 {
            self.frame_with(vec![Event::PointerMoved(from + (at - from) * (i as f32 / 3.0))], mods);
        }
        self.pointer = at;
    }

    /// Clicks apart, so egui never takes two for a double click.
    fn space_clicks(&mut self) {
        if self.time - self.last_click < 0.8 {
            self.time = self.last_click + 0.8;
        }
    }

    pub fn press_at(&mut self, at: Pos2, button: PointerButton, mods: Modifiers, times: usize) {
        self.space_clicks();
        self.move_to(at, mods);
        for _ in 0..times {
            for pressed in [true, false] {
                self.frame_with(vec![Event::PointerButton { pos: at, button, pressed, modifiers: mods }], mods);
            }
        }
        self.last_click = self.time;
        self.frames(3);
    }

    pub fn click_at(&mut self, at: Pos2) {
        self.press_at(at, PointerButton::Primary, Modifiers::NONE, 1);
    }

    pub fn try_click(&mut self, text: &str) -> bool {
        match self.find(text) {
            Some(r) => {
                self.note(format!("clic «{text}»"));
                self.click_at(r.center());
                true
            }
            None => {
                self.note(format!("  ¡no encuentro «{text}» para hacer clic!"));
                false
            }
        }
    }

    pub fn click(&mut self, text: &str) {
        if !self.try_click(text) {
            self.dump("perdido");
            panic!("no encuentro «{text}» (ver widgets-perdido.txt)");
        }
    }

    /// A submenu's item: across first, then down, as one learns to.
    pub fn submenu(&mut self, text: &str) {
        let r = self.find(text).unwrap_or_else(|| panic!("no encuentro «{text}» en el submenú"));
        self.note(format!("clic «{text}» (submenú)"));
        let across = pos2(r.left() + 6.0, self.pointer.y);
        self.move_to(across, Modifiers::NONE);
        self.frames(2);
        let r = self.find(text).unwrap();
        self.click_at(r.center());
    }

    pub fn rclick_at(&mut self, at: Pos2) {
        self.press_at(at, PointerButton::Secondary, Modifiers::NONE, 1);
    }

    pub fn dclick_at(&mut self, at: Pos2) {
        self.press_at(at, PointerButton::Primary, Modifiers::NONE, 2);
    }

    pub fn hover(&mut self, text: &str) {
        let r = self.find(text).unwrap_or_else(|| panic!("no encuentro «{text}» para pasar por encima"));
        self.move_to(r.center(), Modifiers::NONE);
        self.frames(20);
    }

    pub fn drag(&mut self, from: Pos2, to: Pos2, mods: Modifiers) {
        self.space_clicks();
        self.move_to(from, mods);
        self.frame_with(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: mods }], mods);
        for i in 1..=12 {
            self.frame_with(vec![Event::PointerMoved(from + (to - from) * (i as f32 / 12.0))], mods);
        }
        self.frame_with(vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: mods }], mods);
        self.pointer = to;
        self.last_click = self.time;
        self.frames(3);
    }

    pub fn key(&mut self, key: Key, mods: Modifiers) {
        for pressed in [true, false] {
            self.frame_with(vec![Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: mods }], mods);
        }
        self.frames(2);
    }

    pub fn type_text(&mut self, text: &str) {
        self.frame_with(vec![Event::Text(text.to_string())], Modifiers::NONE);
        self.frames(2);
    }

    /// A menu command, as chosen in the menu bar.
    pub fn menu(&mut self, command: crate::commands::Command) {
        self.note(format!("menú {command:?}"));
        let ctx = self.ctx.clone();
        self.app.run(command, &ctx);
        self.frames(3);
    }
}

/// The widgets of this frame, from egui's accessibility tree.
fn widgets(ctx: &egui::Context) -> Vec<Seen> {
    ctx.viewport(|vp| {
        let Some(state) = vp.this_pass.accesskit_state.as_ref() else { return Vec::new() };
        state
            .nodes
            .values()
            .filter_map(|node| {
                let b = node.bounds()?;
                let rect = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
                let text = [node.label(), node.value(), node.description()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                Some(Seen { text, role: format!("{:?}", node.role()), rect, disabled: node.is_disabled() })
            })
            .filter(|w| w.rect.is_positive())
            .collect()
    })
}

#[test]
#[ignore]
fn produccion() {
    let Some(out) = std::env::var_os("PULSE_PRUEBA").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    // Settings and recent projects stay out of the user's own.
    let home = out.join("home");
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("HOME", &home) };

    let mut app = PulseApp::headless(Project::empty());
    app.templates = crate::templates::all();
    let mut s = Studio::new(app, out.clone());
    s.shot("vacio");
    s.dump("vacio");
    let beat = s.app.templates.iter().position(|t| t.name == "Beat").unwrap();
    s.menu(crate::commands::Command::NewFromTemplate(beat));
    s.shot("plantilla-beat");
    s.dump("plantilla-beat");

    // Tempo 126: double click on the field, type, Enter.
    let tempo = s.find("124.00").unwrap();
    s.dclick_at(tempo.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text("126");
    s.key(Key::Enter, Modifiers::NONE);
    s.note(format!("  tempo: {}", s.app.project.tempo));
    // Key: A minor.
    s.click("Escala");
    s.shot("menu-escala");
    let la = s.find_near("La", pos2(1233.0, 235.0)).unwrap();
    s.click_at(la.center());
    s.note(format!("  escala: {:?}", s.app.project.key));
    s.frames(3);

    // Drums: a clip in the first slot, and a techno beat.
    let slot = s.find("Slot 1:1").unwrap();
    s.dclick_at(slot.center());
    s.shot("clip-bateria");
    s.click("Generar ritmo");
    s.click("Techno");
    s.click("8 c.");
    s.click("Que lo toque la IA");
    s.shot("generar-ritmo");
    s.dump("generar-ritmo");
    s.click("Generar");
    s.wait_for("la IA toca la batería", 60.0, |app| notes(app, 0, 0) > 20);
    s.note(format!("  notas en la batería: {}", notes(&s.app, 0, 0)));
    s.shot("ritmo-generado");
    s.key(Key::Escape, Modifiers::NONE);

    // Chords: an epic minor progression, a bar each.
    let slot = s.find("Slot 3:1").unwrap();
    s.dclick_at(slot.center());
    // The editor shows five rows: the panel is dragged up, as anyone would.
    s.drag(pos2(800.0, 585.0), pos2(800.0, 330.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 800.0), Modifiers::NONE);
    s.frames(30);
    s.shot("editor-mas-alto");
    s.dump("editor-mas-alto");
    let menu = s.seen.iter().filter(|w| w.text == "Acordes" && w.role == "Button" && w.rect.top() > 250.0).map(|w| w.rect).last().unwrap();
    s.note(format!("  botón Acordes en {menu:?}"));
    s.click_at(menu.center());
    s.shot("menu-acordes");
    s.click("Épica (i–VI–III–VII)");
    s.key(Key::Escape, Modifiers::NONE);
    {
        let Some(engine::model::Clip::Midi(c)) = &s.app.project.tracks[2].clips[0] else { panic!() };
        s.note(format!("  acordes: {} notas, loop {} tiempos, tonos {:?}", c.notes.len(), c.length, c.notes.iter().map(|n| n.pitch).collect::<Vec<_>>()));
    }
    s.shot("progresion");

    // Bass and pad from the chords.
    s.rclick_at(pos2(458.0, 99.0));
    s.click("Acompañar ⏵");
    s.shot("menu-acompanar");
    // Straight to «Bajo: contratiempo», diagonally, across «De tarareo a canción ⏵»:
    // the bass, on the template's empty «Bajo».
    s.click("Bajo: contratiempo");
    s.rclick_at(pos2(458.0, 99.0));
    s.click("Acompañar ⏵");
    s.submenu("Pad");
    s.frames(5);
    let names: Vec<String> = s.app.project.tracks.iter().map(|t| format!("{} ({:?})", t.name, std::mem::discriminant(&t.instrument))).collect();
    s.note(format!("  pistas: {names:?}"));
    s.shot("bajo-y-pad");
    // The lead: an arpeggio over the chords.
    s.rclick_at(pos2(458.0, 99.0));
    s.click("Acompañar ⏵");
    s.submenu("Arpegio arriba y abajo");
    s.frames(5);
    let names: Vec<&str> = s.app.project.tracks.iter().map(|t| t.name.as_str()).collect();
    s.note(format!("  pistas: {names:?}"));

    // Sounds: the bass's factory presets.
    let bass = s.find("Pista Bajo").unwrap();
    s.click_at(bass.center());
    s.click("Dispositivos");
    s.shot("bajo-dispositivos");
    preset(&mut s, "Pista Bajo", "Bajo", "Techno rodante");
    s.shot("presets-bajo");
    preset(&mut s, "Pista Pad", "Pad", "");
    preset(&mut s, "Pista Arpegio", "Pluck", "");
    preset(&mut s, "Pista Acordes", "Acordes", "");
    s.shot("presets-acordes");

    // The template's empty Melodía goes: the arpeggio is the lead.
    for name in ["Pista Melodía"] {
        let header = s.find(name).unwrap();
        s.rclick_at(header.center());
        if name == "Pista Melodía" {
            s.shot("menu-pista");
            s.dump("menu-pista");
        }
        s.click("Eliminar pista");
    }
    let names: Vec<&str> = s.app.project.tracks.iter().map(|t| t.name.as_str()).collect();
    s.note(format!("  pistas: {names:?}"));

    // The kick to its own track, for the sidechain.
    let drums = s.find("Pista Batería").unwrap();
    s.rclick_at(drums.center());
    s.click("Sacar a su pista ⏵");
    s.shot("sacar-a-su-pista");
    s.submenu("Bombo");
    let names: Vec<&str> = s.app.project.tracks.iter().map(|t| t.name.as_str()).collect();
    s.note(format!("  pistas: {names:?}"));

    // Bass and pad duck under the kick.
    for track in ["Pista Bajo", "Pista Pad"] {
        let header = s.find(track).unwrap();
        s.click_at(header.center());
        s.click("Ducker");
        s.click("Ninguno");
        let kick = s.seen.iter().filter(|w| w.text == "Batería Bombo" && w.rect.top() > 380.0).map(|w| w.rect).next();
        match kick {
            Some(k) => s.click_at(k.center()),
            None => {
                s.dump("sidechain");
                panic!("no está «Batería Bombo» en el menú Sidechain");
            }
        }
    }
    s.shot("ducker-sidechain");
    summary(&mut s);

    // The scene is the drop.
    let scene = s.find("Escena 1").unwrap();
    s.rclick_at(scene.center());
    let field = s.seen.iter().filter(|w| w.role == "TextInput" && w.rect.top() < 200.0 && w.rect.left() > 800.0).map(|w| w.rect).next().unwrap();
    s.click_at(field.center());
    s.key(Key::A, Modifiers::COMMAND);
    s.type_text("Drop");
    s.key(Key::Enter, Modifiers::NONE);
    s.key(Key::Escape, Modifiers::NONE);
    s.note(format!("  escenas: {:?}", s.app.project.scene_names));

    // The whole song from the scene: Crear › Arreglo automático…
    s.click("Crear");
    s.click("Arreglo automático…");
    s.click("Electrónica (intro, subida, drop, break, drop, outro)");
    s.shot("arreglo-electronica");
    // Intro and outro: drums only, as a DJ wants them.
    for (x, y) in [(500.0, 457.0), (500.0, 505.0), (799.0, 457.0), (799.0, 505.0)] {
        s.click_at(pos2(x, y));
    }
    s.shot("arreglo-retocado");
    s.click("Crear en el Arrangement");
    s.frames(10);
    let markers: Vec<String> = s.app.project.markers.iter().map(|m| format!("{} @{}", m.name, m.beat / 4.0 + 1.0)).collect();
    s.note(format!("  marcadores: {markers:?}"));
    s.shot("arrangement");
    // Less editor, more song: the panel down and the zoom out.
    s.drag(pos2(800.0, 333.0), pos2(800.0, 640.0), Modifiers::NONE);
    s.move_to(pos2(1000.0, 850.0), Modifiers::NONE);
    s.frames(10);
    let zoom = s.seen.iter().filter(|w| w.role == "Slider" && w.rect.top() < 90.0).map(|w| w.rect).next().unwrap();
    s.drag(zoom.center(), pos2(zoom.left() + 2.0, zoom.center().y), Modifiers::NONE);
    s.shot("arrangement-entero");
    s.dump("arrangement-entero");
    listen(&mut s, "1-sin-mezclar");

    // Transitions: a riser into each drop, and an impact on it.
    for n in [1, 2] {
        for path in [&["Subida ⏵", "8 compases"][..], &["Impacto"][..]] {
            let drop = s.find_nth("Drop", n).unwrap();
            s.rclick_at(drop.center());
            s.click("Transición aquí ⏵");
            for item in path {
                s.submenu(item);
            }
            if n == 1 && path.len() == 2 {
                s.shot("subida-puesta");
            }
        }
    }
    let t = s.app.project.tracks.iter().find(|t| t.name == "Transiciones").unwrap();
    let clips: Vec<String> = t.arrangement.iter().map(|c| format!("{} @{}–{}", c.clip.name(), c.start / 4.0 + 1.0, c.end() / 4.0 + 1.0)).collect();
    s.note(format!("  transiciones: {clips:?}"));
    s.shot("transiciones");
    listen(&mut s, "2-con-transiciones");

    // A little delay on the arpeggio, by hand, in the Session mixer.
    s.key(Key::Tab, Modifiers::NONE);
    s.drag(pos2(582.7, 343.3), pos2(582.7, 303.3), Modifiers::NONE);
    s.note(format!("  envío B del arpegio: {:.2}", s.app.project.tracks[3].sends[1]));
    s.shot("mezclador");

    // The mix assistant, for the club.
    s.menu(crate::commands::Command::MixAssistant);
    s.click("Spotify, YouTube, Tidal (−14 LUFS)");
    s.shot("destinos");
    s.dump("destinos");
    let club = s.seen.iter().find(|w| w.text.starts_with("Club")).map(|w| w.text.clone()).unwrap();
    s.click(&club);
    s.click("Analizar la mezcla");
    s.wait_for("el análisis", 120.0, |app| app.mix.report.is_some());
    s.frames(5);
    s.shot("analisis");
    s.click("Aplicar todo");
    s.wait_for("aplicar todo", 600.0, |app| !app.mix.busy() && app.mix.report.is_some());
    s.frames(5);
    s.shot("aplicado");
    s.dump("aplicado");
    summary(&mut s);
    let master: Vec<String> = s.app.project.master.devices.iter().map(|d| format!("{:?}", d.effect)).collect();
    s.note(format!("  master: {master:?}"));
    listen(&mut s, "3-asistente");

    // By ear: the limiter flattens the song. Close the assistant, back the
    // limiter's gain off on the master.
    s.click("Close window");
    let master = s.find("Pista Master").unwrap();
    s.click_at(master.center());
    s.shot("master-en-clip");
    s.click("Dispositivos");
    let limiter_gain = |app: &PulseApp| app.project.master.devices.iter().find_map(|d| match d.effect { engine::model::Effect::Limiter(p) => Some(p.gain_db), _ => None }).unwrap_or(0.0);
    let knob = s.find("Gain").unwrap();
    let mut pulls = 0;
    while limiter_gain(&s.app) > 6.0 && pulls < 20 {
        s.drag(knob.center(), knob.center() + vec2(0.0, 12.0), Modifiers::NONE);
        pulls += 1;
    }
    s.note(format!("  ganancia del limitador: {:.1} dB tras {pulls} arrastres", limiter_gain(&s.app)));
    s.shot("master");
    listen(&mut s, "4-limitador-a-mano");

    // Save it (⇧⌘S), then export it (⇧⌘R) as anyone would.
    let project = s.out.join("Tema techno").join("Tema techno.pulse");
    crate::script::answer_dialog(vec![project.clone()]);
    s.key(Key::S, Modifiers::COMMAND | Modifiers::SHIFT);
    s.frames(5);
    s.note(format!("  guardado: {} ({:?})", project.display(), s.app.file));
    s.key(Key::R, Modifiers::COMMAND | Modifiers::SHIFT);
    s.shot("exportar");
    let wav = s.out.join("Tema techno.wav");
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
    s.note(format!("  {}: {:.1} s a {rate} Hz · {:?} LUFS · pico real {:.1} dBTP", wav.display(), l.len() as f32 / rate, m.lufs, m.true_peak));
    // Opened again, it sounds as exported.
    let reopened = Project::load(&project).unwrap();
    let again = PulseApp::headless(reopened);
    let mut s2 = Studio { app: again, ..Studio::new(PulseApp::headless(Project::empty()), s.out.clone()) };
    let m2 = listen(&mut s2, "5-reabierto");
    s.log.extend(s2.log.drain(..));
    s.note(format!("  reabierto: {:?} LUFS frente a {:?} exportado", m2.lufs, m.lufs));
    std::fs::write(s.out.join("diario.txt"), s.log.join("\n")).unwrap();

    let _ = (vec2(0.0, 0.0), Key::A);
}

/// A factory preset on the track whose header shows `track`: the category whose
/// name starts with `category` (else the first), the preset `name` (else the first).
fn preset(s: &mut Studio, track: &str, category: &str, name: &str) {
    let header = s.find(track).unwrap();
    s.click_at(header.center());
    if !s.has("Presets") {
        s.click("Dispositivos");
    }
    s.click("Presets");
    let menus: Vec<Seen> = s.seen.iter().filter(|w| w.role == "Button" && w.text.ends_with("⏵") && w.rect.left() > 600.0).cloned().collect();
    s.note(format!("  categorías: {:?}", menus.iter().map(|m| m.text.as_str()).collect::<Vec<_>>()));
    let Some(chosen) = menus.iter().find(|m| !category.is_empty() && m.text.starts_with(category)).or(menus.first()) else {
        // Few presets: one list.
        let items: Vec<Seen> = s.seen.iter().filter(|w| w.role == "Button" && w.rect.left() > 600.0 && w.rect.top() > 380.0).cloned().collect();
        s.note(format!("  presets: {:?}", items.iter().map(|m| m.text.as_str()).collect::<Vec<_>>()));
        return;
    };
    let before: Vec<String> = s.seen.iter().filter(|w| w.role == "Button").map(|w| format!("{}{:?}", w.text, w.rect)).collect();
    let text = chosen.text.clone();
    s.hover(&text);
    let parent = s.find(&text).unwrap();
    let items: Vec<Seen> = s.seen.iter().filter(|w| w.role == "Button" && w.rect.left() > parent.right() - 4.0 && !before.contains(&format!("{}{:?}", w.text, w.rect))).cloned().collect();
    s.note(format!("  en {text}: {:?}", items.iter().map(|m| m.text.as_str()).collect::<Vec<_>>()));
    let pick = items.iter().find(|m| m.text == name).or(items.first()).unwrap().text.clone();
    s.submenu(&pick);
    s.key(Key::Escape, Modifiers::NONE);
}

/// Each track: instrument, effects (sidechained ones marked), level, sends.
fn summary(s: &mut Studio) {
    let lines: Vec<String> = s
        .app
        .project
        .tracks
        .iter()
        .map(|t| {
            let fx: Vec<String> = t.devices.iter().map(|d| {
                let text = format!("{:?}", d.effect);
                let name = text.split(['(', ' ', '{']).next().unwrap_or("").to_string();
                if text.contains("sidechain: Some") { format!("{name}↤sc") } else { name }
            }).collect();
            format!("  {:<16} {:<12} {:+5.1} dB pan {:+.2} envíos {:?} fx {:?}", t.name, t.instrument.label(), 20.0 * t.mixer.gain.max(1e-6).log10(), t.mixer.pan, &t.sends[..s.app.project.returns.len()], fx)
        })
        .collect();
    for l in lines {
        s.note(l);
    }
}

/// The Arrangement rendered offline, written to `name`.wav, and measured: the
/// whole song, each section (by markers), and the octave balance.
fn listen(s: &mut Studio, name: &str) -> crate::mix_assist::Measures {
    let project = s.app.project.clone();
    let end = project.tracks.iter().map(|t| t.arrangement_end()).fold(0.0, f64::max);
    let mut samples = crate::sync::SampleCache::default();
    let commands = crate::sync::project_commands(&project, &mut samples, 48_000.0, &Default::default());
    let start = Instant::now();
    let (l, r) = crate::export::render(commands, (0.0, end), crate::export::Song::of(&project), 48_000.0, &Default::default(), &|_| {}).unwrap();
    let path = s.out.join(format!("{name}.wav"));
    crate::export::write(&path, &l, &r, 48_000, crate::export::BitDepth::Int24, 1.0).unwrap();
    let m = crate::mix_assist::Measures::of(&l, &r, 48_000.0);
    let f = |x: Option<f64>| x.map_or("—".into(), |x| format!("{x:.1}"));
    s.note(format!(
        "  {name}.wav: {:.1} s (render {:.1} s) · {} LUFS · pico real {:.1} dBTP · pico {:.1} dBFS · rango {} LU · correlación {:.2}",
        l.len() as f64 / 48_000.0,
        start.elapsed().as_secs_f64(),
        f(m.lufs),
        m.true_peak,
        m.peak,
        f(m.range),
        m.correlation
    ));
    s.note(format!("  octavas (31 Hz…16 kHz, dB): {:?}", m.bands.iter().map(|b| (b * 10.0).round() / 10.0).collect::<Vec<_>>()));
    let seconds = |beat: f64| beat * 60.0 / project.tempo;
    let mut marks: Vec<(String, f64)> = project.markers.iter().map(|m| (m.name.clone(), m.beat)).collect();
    marks.push(("fin".into(), end));
    for w in marks.windows(2) {
        let (a, b) = ((seconds(w[0].1) * 48_000.0) as usize, ((seconds(w[1].1) * 48_000.0) as usize).min(l.len()));
        if b <= a {
            continue;
        }
        let part = crate::mix_assist::Measures::of(&l[a..b], &r[a..b], 48_000.0);
        s.note(format!("    {:<8} {} LUFS, pico {:.1} dBFS", w[0].0, f(part.lufs), part.peak));
    }
    m
}

/// Notes in the session clip of `track` at `slot`.
fn notes(app: &PulseApp, track: usize, slot: usize) -> usize {
    match &app.project.tracks[track].clips[slot] {
        Some(engine::model::Clip::Midi(c)) => c.notes.len(),
        _ => 0,
    }
}
