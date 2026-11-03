//! Grid controllers connected: their ports, what they send turned into
//! launches, and their lights kept as the Session is (see `grid` for the
//! maps). A thin layer: the MIDI input threads hand the controllers'
//! messages to an inbox, the UI takes them each frame, and their lights go
//! out through an [`Output`] (a MIDI port, or a fake one in the tests).

use crate::app::PulseApp;
use crate::grid::{self, Action, Column, Layout, Light, Model, PortUse, Surface, View, Window, RED};
use engine::{Command, NONE, NUM_SCENES, QUEUED_STOP};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Where a controller's lights go.
pub trait Output {
    fn send(&mut self, message: &[u8]) -> bool;
}

struct Port(midir::MidiOutputConnection);

impl Output for Port {
    fn send(&mut self, message: &[u8]) -> bool {
        self.0.send(message).is_ok()
    }
}

/// What the MIDI input threads hand to the controllers.
#[derive(Clone, Default)]
pub struct Inbox {
    messages: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    /// Other ports whose messages are the grids' (learned, or being learned), by `port_key`.
    wanted: Arc<Mutex<Vec<String>>>,
}

impl Inbox {
    /// Whether this message is the grids' (and not the instruments'). Called
    /// on a MIDI input thread.
    pub fn take(&self, port: &str, message: &[u8]) -> bool {
        let ours = match grid::classify(port) {
            PortUse::Grid(_) => true,
            PortUse::Ignore => return true,
            PortUse::Other => self.wanted.lock().is_ok_and(|w| w.contains(&grid::port_key(port))),
        };
        if ours {
            if let Ok(mut messages) = self.messages.lock() {
                if messages.len() < 4096 {
                    messages.push((port.to_string(), message.to_vec()));
                }
            }
        }
        ours
    }

    fn drain(&self) -> Vec<(String, Vec<u8>)> {
        self.messages.lock().map(|mut m| std::mem::take(&mut *m)).unwrap_or_default()
    }

    fn want(&self, keys: Vec<String>) {
        if let Ok(mut wanted) = self.wanted.lock() {
            *wanted = keys;
        }
    }
}

/// A controller in use.
pub struct Controller {
    /// Its output port's name.
    pub port: String,
    pub surface: Surface,
    out: Box<dyn Output>,
}

impl Controller {
    /// Whether a message from input port `port` is this one's.
    fn hears(&self, port: &str) -> bool {
        match (self.surface.model, grid::classify(port)) {
            (Model::Generic, _) => grid::port_key(port) == grid::port_key(&self.port),
            (model, PortUse::Grid(from)) => model == from,
            _ => false,
        }
    }

    fn send_all(&mut self, messages: Vec<Vec<u8>>) {
        for m in messages {
            self.out.send(&m);
        }
    }
}

/// A generic grid being learned: the port, and the pads pressed so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Learning {
    pub port: String,
    pub notes: Vec<u8>,
}

impl Learning {
    /// What to press next.
    pub fn prompt(&self) -> &'static str {
        match self.notes.len() {
            0 => "Pulsa el pad de arriba a la izquierda",
            1 => "Ahora el que tiene a su derecha",
            _ => "Y el que tiene debajo",
        }
    }
}

/// The grid controllers.
#[derive(Default)]
pub struct Grids {
    pub controllers: Vec<Controller>,
    pub inbox: Inbox,
    pub learning: Option<Learning>,
    /// Generic grids learned, by `port_key`.
    pub layouts: Vec<(String, Layout)>,
    /// Where they are kept (none in tests and headless apps).
    path: Option<PathBuf>,
    pub error: Option<String>,
}

/// Of the output ports a model has, the one to use: on Windows a Launchpad's
/// second interface («MIDIOUT2 (LPX MIDI)») is its MIDI one, the first its DAW one.
pub fn pick_port(names: &[String], model: Model) -> Option<usize> {
    let mine: Vec<usize> = (0..names.len()).filter(|&i| grid::classify(&names[i]) == PortUse::Grid(model)).collect();
    mine.iter().copied().find(|&i| names[i].to_lowercase().contains("2 (")).or(mine.first().copied())
}

impl Grids {
    pub fn load() -> Grids {
        let path = paths::config_dir().join("grids.json");
        let layouts = crate::store::read_json(&path);
        let grids = Grids { layouts, path: Some(path), ..Default::default() };
        grids.update_wanted();
        grids
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Err(e) = crate::store::write_json(path, &self.layouts) {
            crate::logs::note(&e);
        }
    }

    fn update_wanted(&self) {
        let mut keys: Vec<String> = self.layouts.iter().map(|(k, _)| k.clone()).collect();
        keys.extend(self.learning.iter().map(|l| grid::port_key(&l.port)));
        self.inbox.want(keys);
    }

    /// Uses a controller (its lights out through `out`): it is put in the
    /// mode where Pulse lights it.
    pub fn add(&mut self, port: &str, model: Model, layout: Option<Layout>, out: Box<dyn Output>) {
        let mut controller = Controller { port: port.to_string(), surface: Surface::new(model, layout), out };
        let hello = controller.surface.hello();
        controller.send_all(hello);
        self.controllers.push(controller);
    }

    /// Opens the outputs of the controllers connected (the known ones, and the
    /// generic grids learned).
    pub fn connect(&mut self) {
        self.release();
        let names = match midir::MidiOutput::new("Pulse") {
            Ok(probe) => probe.ports().iter().map(|p| probe.port_name(p).unwrap_or_default()).collect::<Vec<_>>(),
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        let mut wanted: Vec<(usize, Model, Option<Layout>)> = Model::KNOWN.iter().filter_map(|&m| pick_port(&names, m).map(|i| (i, m, None))).collect();
        for (key, layout) in &self.layouts {
            if let Some(i) = names.iter().position(|n| grid::classify(n) == PortUse::Other && &grid::port_key(n) == key) {
                wanted.push((i, Model::Generic, Some(*layout)));
            }
        }
        for (index, model, layout) in wanted {
            let Ok(output) = midir::MidiOutput::new("Pulse") else { continue };
            let Some(port) = output.ports().get(index).cloned() else { continue };
            match output.connect(&port, "Pulse grid") {
                Ok(connection) => self.add(&names[index], model, layout, Box::new(Port(connection))),
                Err(e) => self.error = Some(format!("{}: {e}", names[index])),
            }
        }
    }

    /// Lets go of every controller: lights off, back to its own mode.
    pub fn release(&mut self) {
        for mut controller in self.controllers.drain(..) {
            let bye = controller.surface.goodbye();
            controller.send_all(bye);
        }
    }

    /// Starts learning a generic grid on input port `port`.
    pub fn start_learning(&mut self, port: &str) {
        self.learning = Some(Learning { port: port.to_string(), notes: Vec::new() });
        self.update_wanted();
    }

    pub fn stop_learning(&mut self) {
        self.learning = None;
        self.update_wanted();
    }

    /// Forgets a learned grid.
    pub fn forget(&mut self, port: &str) {
        let key = grid::port_key(port);
        self.layouts.retain(|(k, _)| *k != key);
        self.controllers.retain(|c| !(c.surface.model == Model::Generic && grid::port_key(&c.port) == key));
        self.save();
        self.update_wanted();
    }

    /// A message while learning: the layout, once three pads were pressed;
    /// `Err` if they don't make a grid (learning starts again).
    fn learn(&mut self, port: &str, message: &[u8]) -> Option<Result<Layout, ()>> {
        let learning = self.learning.as_mut().filter(|l| grid::port_key(&l.port) == grid::port_key(port))?;
        let (&status, &note, &velocity) = (message.first()?, message.get(1)?, message.get(2)?);
        if status & 0xF0 != 0x90 || velocity == 0 || learning.notes.last() == Some(&note) {
            return None;
        }
        learning.notes.push(note);
        if learning.notes.len() < 3 {
            return None;
        }
        let notes = [learning.notes[0], learning.notes[1], learning.notes[2]];
        learning.notes.clear();
        let Some(layout) = Layout::learn(notes) else { return Some(Err(())) };
        let key = grid::port_key(port);
        self.layouts.retain(|(k, _)| *k != key);
        self.layouts.push((key, layout));
        self.learning = None;
        self.save();
        self.update_wanted();
        Some(Ok(layout))
    }

    /// The windows shown on the Session: the controller's name, where and how big.
    pub fn windows(&self) -> Vec<(&'static str, Window, (usize, usize))> {
        self.controllers.iter().map(|c| (c.surface.model.label(), c.surface.window, c.surface.size())).collect()
    }
}

/// The settings' part on them: the ones in use, where their window is (and
/// arrows to move it), and the other MIDI ports to learn as a grid.
pub fn settings(app: &mut PulseApp, ui: &mut eframe::egui::Ui) {
    use crate::theme::TEXT_DIM;
    use eframe::egui::RichText;
    ui.separator();
    ui.heading("Controladores de rejilla");
    let columns = app.grid_columns().len();
    if app.grids.controllers.is_empty() {
        ui.label(RichText::new("Ninguno conectado. Pulse reconoce solo los Launchpad Mini MK3, X y Pro MK3, los APC mini (y mk2) y el APC40 mkII; otro con pads, apréndelo abajo.").color(TEXT_DIM).size(10.5));
    }
    for controller in &mut app.grids.controllers {
        let surface = &mut controller.surface;
        let (cols, rows) = surface.size();
        let w = surface.window;
        ui.horizontal(|ui| {
            ui.label(format!("● {}", surface.model.label()));
            ui.label(RichText::new(format!("pistas {}–{} · escenas {}–{}", w.column + 1, w.column + cols, w.scene + 1, w.scene + rows)).color(TEXT_DIM).size(10.5));
            for (text, dir) in [("←", grid::Dir::Left), ("→", grid::Dir::Right), ("↑", grid::Dir::Up), ("↓", grid::Dir::Down)] {
                if ui.small_button(text).on_hover_text("Mueve la zona que controla (el recuadro de la Session)").clicked() {
                    surface.window = w.moved(dir, false, columns, (cols, rows));
                }
            }
        });
    }
    if let Some(error) = &app.grids.error {
        ui.colored_label(eframe::egui::Color32::LIGHT_RED, error);
    }
    if let Some(learning) = app.grids.learning.clone() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}: {}", learning.port, learning.prompt())).strong());
            if ui.small_button("Cancelar").clicked() {
                app.grids.stop_learning();
            }
        });
        return;
    }
    let others: Vec<String> = app.midi.names.iter().filter(|n| grid::classify(n) == PortUse::Other).cloned().collect();
    for name in others {
        let learned = app.grids.layouts.iter().any(|(k, _)| *k == grid::port_key(&name));
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).size(10.5));
            if learned {
                if ui.small_button("Olvidar la rejilla").on_hover_text("Sus notas vuelven a tocar los instrumentos").clicked() {
                    app.grids.forget(&name);
                }
            } else if ui.small_button("Aprender como rejilla").on_hover_text("Pulsa tres pads: el de arriba a la izquierda, el de su derecha y el de debajo. Desde entonces sus pads lanzan clips y no tocan notas").clicked() {
                app.grids.start_learning(&name);
            }
        });
    }
}

impl PulseApp {
    /// The Session's columns: its tracks, but those hidden in a folded group.
    pub fn grid_columns(&self) -> Vec<usize> {
        let tracks = &self.project.tracks;
        (0..tracks.len()).filter(|&t| !crate::groups::hidden(tracks, t)).collect()
    }

    /// What the controllers show of the Session now.
    pub fn grid_view(&self, columns: &[usize]) -> View {
        let state = self.audio.state();
        let tracks = &self.project.tracks;
        let status = |t: usize| {
            state.as_ref().map_or((NONE, NONE, NONE, false), |s| {
                let st = &s.tracks[t];
                (st.playing.load(Ordering::Relaxed), st.queued.load(Ordering::Relaxed), st.recording.load(Ordering::Relaxed), st.queued_record.load(Ordering::Relaxed))
            })
        };
        let cell = |t: usize, slot: usize| -> Light {
            let track = &tracks[t];
            let (playing, queued, recording, queued_record) = status(t);
            let clip = track.clips[slot].as_ref();
            let color = clip.map_or(track.color, |c| if c.muted() { [90, 90, 90] } else { c.color() });
            if recording == slot as i32 {
                Light::Pulse(RED)
            } else if queued_record && queued == slot as i32 {
                Light::Blink(RED)
            } else if clip.is_none() {
                Light::Off
            } else if queued == slot as i32 {
                Light::Blink(color)
            } else if playing == slot as i32 {
                Light::Lit(color)
            } else {
                Light::Dim(color)
            }
        };
        let stop = |t: usize| -> Light {
            let (playing, queued, _, _) = status(t);
            if queued == QUEUED_STOP {
                Light::Blink(RED)
            } else if playing != NONE {
                Light::Lit(RED)
            } else if tracks[t].clips.iter().any(|c| c.is_some()) {
                Light::Dim(RED)
            } else {
                Light::Off
            }
        };
        let columns = columns
            .iter()
            .map(|&t| {
                let mut column = Column::empty();
                if tracks[t].is_group() {
                    // A group: the strongest of its tracks', in its colour.
                    let inside: Vec<usize> = crate::groups::children(tracks, t).filter(|&c| !tracks[c].is_group()).collect();
                    let rank = |l: Light| match l {
                        Light::Off => 0,
                        Light::Dim(_) => 1,
                        Light::Lit(_) => 2,
                        Light::Pulse(_) => 3,
                        Light::Blink(_) => 4,
                    };
                    let recolour = |l: Light| match l {
                        Light::Dim(_) => Light::Dim(tracks[t].color),
                        Light::Lit(_) => Light::Lit(tracks[t].color),
                        Light::Blink(_) => Light::Blink(tracks[t].color),
                        other => other,
                    };
                    for slot in 0..NUM_SCENES {
                        column.cells[slot] = recolour(inside.iter().map(|&c| cell(c, slot)).max_by_key(|&l| rank(l)).unwrap_or(Light::Off));
                    }
                    column.stop = inside.iter().map(|&c| stop(c)).max_by_key(|&l| rank(l)).unwrap_or(Light::Off);
                } else {
                    for slot in 0..NUM_SCENES {
                        column.cells[slot] = cell(t, slot);
                    }
                    column.stop = stop(t);
                }
                column
            })
            .collect();
        View { columns }
    }

    /// Does what a controller asked, on the Session's `columns`.
    pub fn grid_action(&mut self, action: Action, columns: &[usize]) {
        match action {
            Action::Launch { column, scene } => {
                let Some(&t) = columns.get(column) else { return };
                if scene >= NUM_SCENES {
                    return;
                }
                let track = &self.project.tracks[t];
                if track.is_group() {
                    self.launch_group(t, scene);
                } else if track.clips[scene].is_none() && track.armed {
                    self.record_slot(t, scene);
                } else {
                    self.audio.send(Command::LaunchClip { track: t, slot: scene });
                }
            }
            Action::LaunchScene(scene) if scene < NUM_SCENES => self.audio.send(Command::LaunchScene(scene)),
            Action::LaunchScene(_) => {}
            Action::StopTrack(column) => {
                let Some(&t) = columns.get(column) else { return };
                if self.project.tracks[t].is_group() {
                    for c in crate::groups::children(&self.project.tracks, t) {
                        self.audio.send(Command::StopTrack(c));
                    }
                } else {
                    self.audio.send(Command::StopTrack(t));
                }
            }
            Action::StopAll => self.audio.send(Command::StopAllClips),
            Action::Moved => {}
        }
    }

    /// Each frame: what the controllers sent, done; their lights brought up
    /// to date. Whether something came (to draw again).
    pub fn tick_grids(&mut self) -> bool {
        let messages = self.grids.inbox.drain();
        let came = !messages.is_empty();
        if self.grids.controllers.is_empty() && self.grids.learning.is_none() {
            return came;
        }
        let columns = self.grid_columns();
        for (port, message) in messages {
            match self.grids.learn(&port, &message) {
                Some(Ok(_)) => {
                    self.status = Some(format!("Rejilla aprendida en {port}"));
                    self.grids.connect();
                    continue;
                }
                Some(Err(())) => {
                    self.status = Some("Esos tres pads no forman una rejilla: vuelve a empezar por el de arriba a la izquierda".into());
                    continue;
                }
                None if self.grids.learning.is_some() => continue,
                None => {}
            }
            let action = self.grids.controllers.iter_mut().find(|c| c.hears(&port)).and_then(|c| c.surface.handle(&message, columns.len()));
            if let Some(action) = action {
                self.grid_action(action, &columns);
            }
        }
        if !self.grids.controllers.is_empty() {
            let view = self.grid_view(&columns);
            for controller in &mut self.grids.controllers {
                let messages = controller.surface.leds(&view);
                controller.send_all(messages);
            }
        }
        came
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use std::sync::atomic::AtomicBool;

    /// A port that keeps what it is sent.
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<Vec<u8>>>>);

    impl Output for Fake {
        fn send(&mut self, message: &[u8]) -> bool {
            self.0.lock().unwrap().push(message.to_vec());
            true
        }
    }

    impl Fake {
        fn take(&self) -> Vec<Vec<u8>> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    fn with_launchpad() -> (PulseApp, Fake) {
        let mut app = PulseApp::headless(Project::demo());
        let fake = Fake::default();
        app.grids.add("LPX MIDI", Model::LaunchpadX, None, Box::new(fake.clone()));
        (app, fake)
    }

    #[test]
    fn a_launchpad_is_put_in_programmer_mode_and_lit_with_the_session() {
        let (mut app, fake) = with_launchpad();
        assert_eq!(fake.take(), [vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7]]);
        app.tick_grids();
        let lit = fake.take();
        let t = (0..app.project.tracks.len()).find(|&t| app.project.tracks[t].clips[0].is_some()).unwrap();
        let color = app.project.tracks[t].clips[0].as_ref().unwrap().color();
        let pad = 81 + t as u8;
        assert!(lit.contains(&vec![0x90, pad, grid::dim_index(grid::palette_index(color))]), "the demo's first clip, dim in its colour: {lit:?}");
        app.tick_grids();
        assert!(fake.take().is_empty(), "nothing changed");

        // A clip deleted: its pad goes dark.
        app.set_clip(t, 0, None);
        app.tick_grids();
        assert!(fake.take().contains(&vec![0x90, pad, 0]));
        // Let go: its lights off and back to Live mode.
        app.grids.release();
        assert_eq!(fake.take().last().unwrap(), &vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7]);
    }

    /// Its pads launch through the inbox, as the MIDI thread hands them; the
    /// arrows move the window, and a pad then is another track's.
    #[test]
    fn pads_launch_through_the_inbox_and_the_window_moves() {
        let (mut app, _fake) = with_launchpad();
        for _ in 0..10 {
            app.add_track();
        }
        assert!(app.grids.inbox.take("LPX MIDI", &[0x90, 81, 127]), "the grid's, not the instruments'");
        assert!(app.grids.inbox.take("LPX DAW", &[0x90, 81, 127]), "its DAW port: swallowed");
        assert!(!app.grids.inbox.take("KeyStep", &[0x90, 60, 127]), "a keyboard's notes go on to the instruments");
        app.grids.inbox.take("LPX MIDI", &[0xB0, 94, 127]);
        app.grids.inbox.take("LPX MIDI", &[0xB0, 94, 127]);
        app.tick_grids();
        assert_eq!(app.grids.windows()[0].1, Window { column: 2, scene: 0 });
        // A grid action on the Session: a group's pad launches its tracks.
        let columns = app.grid_columns();
        assert_eq!(columns.len(), app.project.tracks.len());
    }

    /// What the controllers show: clips dim in their colour (grey switched
    /// off), empty slots dark, the stop button of a track with clips dim.
    #[test]
    fn the_view_shows_the_session() {
        let (mut app, _fake) = with_launchpad();
        let t = (0..app.project.tracks.len()).find(|&t| app.project.tracks[t].clips[0].is_some()).unwrap();
        let columns = app.grid_columns();
        let view = app.grid_view(&columns);
        assert!(matches!(view.columns[t].cells[0], Light::Dim(_)));
        assert!(matches!(view.columns[t].cells[NUM_SCENES - 1], Light::Off));
        assert_eq!(view.columns[t].stop, Light::Dim(RED));
        let muted = app.project.tracks[t].clips[0].as_mut().unwrap();
        muted.set_muted(true);
        assert_eq!(app.grid_view(&columns).columns[t].cells[0], Light::Dim([90, 90, 90]), "switched off: grey");
    }

    /// The Session with a controller's box and a clip with a follow action,
    /// and the follow action editor, to look at:
    /// `PULSE_SHOTS=dir cargo test --release -p app grid_shots -- --ignored`.
    #[test]
    #[ignore]
    fn grid_shots() {
        use eframe::egui::{self, vec2, Rect};
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        let shot = |name: &str, size: egui::Vec2, app: &mut PulseApp, draw: &dyn Fn(&mut PulseApp, &mut egui::Ui)| {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            let mut canvas = crate::offscreen::Canvas::default();
            let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            let mut output = None;
            for frame in 0..4 {
                ctx.set_pixels_per_point(2.0);
                output = Some(ctx.run_ui(input(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| draw(app, ui));
                }));
                if frame < 3 {
                    canvas.draw(&ctx, output.take().unwrap(), size, 2.0);
                }
            }
            canvas.draw(&ctx, output.unwrap(), size, 2.0).save_png(&dir.join(name)).unwrap();
        };
        let (mut app, _fake) = with_launchpad();
        for _ in 0..8 {
            app.add_track();
        }
        app.grids.controllers[0].surface.window = Window { column: 1, scene: 1 };
        let t = (0..app.project.tracks.len()).find(|&t| app.project.tracks[t].clips[0].is_some()).unwrap();
        app.set_follow(t, 0, engine::follow::Follow { a: engine::follow::FollowKind::Next, b: engine::follow::FollowKind::Stop, chance_a: 0.7, count: 2, unit: engine::follow::FollowUnit::Loops });
        app.set_scene_follow(0, engine::follow::Follow { a: engine::follow::FollowKind::Next, count: 8, unit: engine::follow::FollowUnit::Bars, ..Default::default() });
        shot("grid-session.png", vec2(1400.0, 520.0), &mut app, &|app, ui| crate::session::show(app, ui));
        shot("grid-session-narrow.png", vec2(1000.0, 520.0), &mut app, &|app, ui| crate::session::show(app, ui));
        shot("follow-editor.png", vec2(420.0, 220.0), &mut app, &|app, ui| {
            let mut follow = app.project.tracks[t].follow[0];
            crate::follow_ui::editor(ui, &mut follow, false);
        });
        shot("follow-editor-scene.png", vec2(420.0, 220.0), &mut app, &|app, ui| {
            let mut follow = app.project.scene_follow[0];
            crate::follow_ui::editor(ui, &mut follow, true);
        });
        app.selected_clip = Some(crate::app::ClipRef::Session { track: t, slot: 0 });
        app.detail = crate::app::Detail::Clip;
        shot("follow-bar.png", vec2(1000.0, 300.0), &mut app, &|app, ui| crate::detail::show(app, ui));
    }

    #[test]
    fn windows_ports_pick_the_midi_interface() {
        let names = ["LPX MIDI".to_string(), "MIDIOUT2 (LPX MIDI)".to_string(), "APC MINI".to_string()];
        assert_eq!(pick_port(&names, Model::LaunchpadX), Some(1));
        assert_eq!(pick_port(&names, Model::ApcMini), Some(2));
        assert_eq!(pick_port(&names, Model::Apc40Mk2), None);
        assert_eq!(pick_port(&["LPX MIDI".to_string()], Model::LaunchpadX), Some(0));
    }

    /// An unknown controller: three pads teach Pulse its grid, then it
    /// launches; its notes stop reaching the instruments.
    #[test]
    fn an_unknown_grid_is_learned() {
        let mut app = PulseApp::headless(Project::demo());
        let fake = Fake::default();
        assert!(!app.grids.inbox.take("Pads In", &[0x90, 36, 100]));
        app.grids.start_learning("Pads In");
        let learned = AtomicBool::new(false);
        for note in [56u8, 57, 48] {
            assert!(app.grids.inbox.take("Pads In", &[0x90, note, 100]));
            app.grids.inbox.take("Pads In", &[0x80, note, 0]);
        }
        // `connect` would open real ports: done by hand here.
        let messages = app.grids.inbox.drain();
        for (port, message) in messages {
            if let Some(Ok(layout)) = app.grids.learn(&port, &message) {
                learned.store(true, Ordering::Relaxed);
                app.grids.add("Pads Out", Model::Generic, Some(layout), Box::new(fake.clone()));
            }
        }
        assert!(learned.load(Ordering::Relaxed));
        assert!(app.grids.learning.is_none());
        assert!(app.grids.inbox.take("Pads In", &[0x90, 56, 100]), "its notes are the grid's now");
        app.tick_grids();
        assert!(!fake.take().is_empty(), "lit");
        app.grids.forget("Pads In");
        assert!(!app.grids.inbox.take("Pads In", &[0x90, 56, 100]));
        assert!(app.grids.controllers.is_empty());
    }
}
