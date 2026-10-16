//! Test aid: `PULSE_SCRIPT=<file>` drives the app as a user would, with pointer
//! and keyboard input fed to egui (the real mouse stays put), and writes what
//! happened to `PULSE_SCRIPT_OUT` (default: next to the script): screenshots, the
//! widgets on screen, the project, and a log with every step and status message.
//! Settings and recent projects are left alone, and file dialogs don't open: the
//! script answers them.
//!
//! One step per line (`#` starts a comment). A target is `"text"` (a widget
//! showing it: exact, else containing it; `"text"#2` the second one, top to
//! bottom; `"text"~x,y` the one nearest that point) or `x,y` in points, and may be
//! followed by ` +dx,dy`. Modifiers go first: `cmd`, `shift`, `alt`, `ctrl`, joined
//! with `+`.
//!
//! - `click|rclick|dclick|hover [mods] <target>`
//! - `drag [mods] <target> -> <target>`; `scroll <target> dx,dy`
//! - `key [mods+]<Key>` (egui key names: Space, Enter, Escape, Delete, A, …);
//!   `down <Key>` / `up <Key>` (held: a note on the computer keyboard); `type <text>`
//! - `wait <frames>`; `sleep <ms>` (wall time: playback, analyses)
//! - `shot <name>` (a screenshot), `widgets <name>` (what's on screen, with where),
//!   `state <name>` (the project as JSON), `expect "text"` / `absent "text"`
//! - `scene <steps>` (as `PULSE_SCENE`), `run <Command>` (a menu command, by name)
//! - `pick <path>[|<path>…]` / `pick` (cancel): the next file dialog's answer;
//!   `answer save|discard|cancel`: the next unsaved-changes question's
//! - `quit`

use crate::app::PulseApp;
use crate::commands::Command;
use eframe::egui::{self, pos2, vec2, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Where the running script writes (also the slices and recordings it makes, so
/// they stay out of the user's folders).
static OUT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn out_dir() -> Option<PathBuf> {
    OUT.get().cloned()
}

/// Answers for the next file dialogs; `Some` while a script runs.
static DIALOGS: Mutex<Option<VecDeque<Vec<PathBuf>>>> = Mutex::new(None);

/// While a script runs, the answer to a file dialog (empty: cancelled), else None.
pub fn dialog_answer() -> Option<Vec<PathBuf>> {
    let mut answers = DIALOGS.lock().ok()?;
    let queue = answers.as_mut()?;
    Some(queue.pop_front().unwrap_or_else(|| {
        log_line("  (un diálogo de archivo sin respuesta en el guion: cancelado)");
        Vec::new()
    }))
}

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn log_line(line: &str) {
    if let Ok(mut log) = LOG.lock() {
        if let Some(file) = log.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }
    eprintln!("[guion] {line}");
}

/// A widget on screen: what it shows and where.
#[derive(Clone, Debug)]
struct Widget {
    text: String,
    role: String,
    rect: Rect,
    disabled: bool,
}

pub struct Script {
    steps: Vec<(usize, String)>,
    next: usize,
    out: PathBuf,
    /// Input for the coming frames, one batch a frame.
    frames: VecDeque<(Vec<Event>, Modifiers)>,
    wait: u32,
    sleep_until: Option<Instant>,
    /// A screenshot asked for and not yet written.
    shot: Option<String>,
    shot_asked: Option<Instant>,
    widgets: Vec<Widget>,
    pointer: Pos2,
    last_click: Option<Instant>,
    /// Modifiers held down by the last input.
    held: Modifiers,
    status: Option<String>,
    started: Instant,
    pub done: bool,
}

impl Script {
    pub fn from_env() -> Option<Script> {
        let path = PathBuf::from(std::env::var_os("PULSE_SCRIPT")?);
        let text = std::fs::read_to_string(&path).map_err(|e| eprintln!("PULSE_SCRIPT: {e}")).ok()?;
        let out = std::env::var_os("PULSE_SCRIPT_OUT").map(PathBuf::from).unwrap_or_else(|| path.with_extension("out"));
        let _ = std::fs::create_dir_all(&out);
        let _ = OUT.set(out.clone());
        if let Ok(mut log) = LOG.lock() {
            *log = std::fs::File::create(out.join("log.txt")).ok();
        }
        if let Ok(mut d) = DIALOGS.lock() {
            *d = Some(VecDeque::new());
        }
        let steps = text
            .lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.trim().to_string()))
            .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
            .collect();
        Some(Script {
            steps,
            next: 0,
            out,
            frames: VecDeque::new(),
            wait: 30,
            sleep_until: None,
            shot: None,
            shot_asked: None,
            widgets: Vec::new(),
            pointer: pos2(-10.0, -10.0),
            held: Modifiers::NONE,
            last_click: None,
            status: None,
            started: Instant::now(),
            done: false,
        })
    }
}

impl PulseApp {
    /// Before each frame: the script's input for it, or its next step.
    pub fn script_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let Some(mut script) = self.script.take() else { return };
        ctx.enable_accesskit();
        ctx.request_repaint();
        script.feed(self, ctx, raw);
        self.script = Some(script);
    }

    /// After each frame: what's on screen, screenshots, status messages.
    pub fn script_output(&mut self, ctx: &egui::Context) {
        let Some(script) = self.script.as_mut() else { return };
        script.widgets = widgets(ctx);
        if std::env::var_os("PULSE_SCRIPT_DEBUG").is_some() {
            ctx.input(|i| {
                for e in &i.raw.events {
                    if matches!(e, Event::PointerButton { .. }) {
                        log_line(&format!("    {e:?} t={:.3} doble={}", i.time, i.pointer.button_double_clicked(PointerButton::Primary)));
                    }
                }
            });
        }
        if self.status != script.status {
            if let Some(status) = &self.status {
                log_line(&format!("  estado: {status}"));
            }
            script.status = self.status.clone();
        }
        if let Some(name) = script.shot.clone() {
            let image = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let [w, h] = image.size;
                let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
                for p in &image.pixels {
                    ppm.extend_from_slice(&[p.r(), p.g(), p.b()]);
                }
                let path = script.out.join(format!("{name}.ppm"));
                if let Err(e) = std::fs::write(&path, ppm) {
                    log_line(&format!("  no se pudo guardar la captura: {e}"));
                }
                script.shot = None;
            }
        }
    }
}

impl Script {
    fn feed(&mut self, app: &mut PulseApp, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // The window has the keyboard, as if the user had clicked on it.
        raw.events.push(Event::WindowFocused(true));
        if let Some((events, modifiers)) = self.frames.pop_front() {
            raw.events.push(Event::ModifiersChanged(modifiers));
            raw.events.extend(events);
            self.held = modifiers;
            return;
        }
        if self.held != Modifiers::NONE {
            raw.events.push(Event::ModifiersChanged(Modifiers::NONE));
            self.held = Modifiers::NONE;
        }
        if self.shot.is_some() {
            // Not drawn (a covered window gets no frames): on without it.
            if self.shot_asked.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                log_line("  (la captura no llegó en 5 s)");
                self.shot = None;
            }
            return;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        if let Some(until) = self.sleep_until {
            if Instant::now() < until {
                return;
            }
            self.sleep_until = None;
        }
        let Some((line, step)) = self.steps.get(self.next).cloned() else {
            if !self.done {
                log_line(&format!("fin del guion ({:.1} s)", self.started.elapsed().as_secs_f32()));
                self.done = true;
                app.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        };
        // Clicks apart: egui takes a double click less than 0.6 s after another click
        // (anywhere) for a triple one.
        let clicks = ["click", "rclick", "dclick", "drag"].iter().any(|v| step.starts_with(&format!("{v} ")));
        if clicks {
            if let Some(last) = self.last_click {
                if last.elapsed() < Duration::from_millis(700) {
                    self.sleep_until = Some(last + Duration::from_millis(700));
                    return;
                }
            }
            self.last_click = Some(Instant::now());
        }
        self.next += 1;
        log_line(&format!("{line}: {step}"));
        if let Err(e) = self.step(app, ctx, &step) {
            log_line(&format!("  ERROR: {e}"));
        }
        // A frame between steps, for the app to answer.
        self.wait = self.wait.max(2);
    }

    fn step(&mut self, app: &mut PulseApp, ctx: &egui::Context, step: &str) -> Result<(), String> {
        let (verb, rest) = step.split_once(' ').unwrap_or((step, ""));
        let rest = rest.trim();
        match verb {
            "click" | "rclick" | "dclick" | "hover" => {
                let (mods, target) = modifiers(rest);
                let at = self.target(target)?;
                self.move_to(at, mods);
                match verb {
                    "click" => self.press(at, PointerButton::Primary, mods, 1),
                    "rclick" => self.press(at, PointerButton::Secondary, mods, 1),
                    "dclick" => self.press(at, PointerButton::Primary, mods, 2),
                    _ => self.wait = 40,
                }
            }
            "drag" => {
                let (mods, targets) = modifiers(rest);
                let (from, to) = targets.split_once("->").ok_or("drag A -> B")?;
                let from = self.target(from.trim())?;
                let to = match to.trim().strip_prefix('+') {
                    Some(delta) => from + parse_vec(delta)?,
                    None => self.target(to.trim())?,
                };
                self.move_to(from, mods);
                self.frames.push_back((vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: mods }], mods));
                for i in 1..=12 {
                    let p = from + (to - from) * (i as f32 / 12.0);
                    self.frames.push_back((vec![Event::PointerMoved(p)], mods));
                }
                self.frames.push_back((vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: mods }], mods));
                self.pointer = to;
            }
            "scroll" => {
                let (target, delta) = rest.rsplit_once(' ').ok_or("scroll <target> dx,dy")?;
                let at = self.target(target.trim())?;
                self.move_to(at, Modifiers::NONE);
                let (mods, delta) = modifiers(delta);
                let delta = parse_vec(delta)?;
                self.frames.push_back((vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase: egui::TouchPhase::Move, modifiers: mods }], mods));
            }
            "key" => {
                let (mods, name) = match rest.rsplit_once('+') {
                    Some((m, k)) if !k.is_empty() => (modifiers(&format!("{m} ")).0, k),
                    _ => (Modifiers::NONE, rest),
                };
                let key = Key::from_name(name).ok_or(format!("tecla desconocida: {name}"))?;
                // ⌘X ⌘C ⌘V are the menu bar's (key equivalents), which runs them before
                // egui sees the keys.
                let menu = match key {
                    Key::X => Some(Command::Cut),
                    Key::C => Some(Command::Copy),
                    Key::V => Some(Command::Paste),
                    _ => None,
                };
                if let Some(command) = menu.filter(|_| mods.command && !mods.shift) {
                    app.run(command, ctx);
                    return Ok(());
                }
                for pressed in [true, false] {
                    self.frames.push_back((vec![Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers: mods }], mods));
                }
            }
            "down" | "up" => {
                let key = Key::from_name(rest).ok_or(format!("tecla desconocida: {rest}"))?;
                self.frames.push_back((vec![Event::Key { key, physical_key: Some(key), pressed: verb == "down", repeat: false, modifiers: Modifiers::NONE }], Modifiers::NONE));
            }
            "type" => self.frames.push_back((vec![Event::Text(rest.to_string())], Modifiers::NONE)),
            "wait" => self.wait = rest.parse().map_err(|_| "wait <frames>")?,
            "sleep" => self.sleep_until = Some(Instant::now() + Duration::from_millis(rest.parse().map_err(|_| "sleep <ms>")?)),
            "shot" => {
                self.shot = Some(rest.to_string());
                self.shot_asked = Some(Instant::now());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            "widgets" => {
                let mut text = String::new();
                for w in &self.widgets {
                    text += &format!(
                        "{:7.1},{:6.1} {:5.0}×{:<4.0} {:<12} {}{}\n",
                        w.rect.center().x,
                        w.rect.center().y,
                        w.rect.width(),
                        w.rect.height(),
                        w.role,
                        w.text.replace('\n', "⏎"),
                        if w.disabled { "  (desactivado)" } else { "" }
                    );
                }
                std::fs::write(self.out.join(format!("{rest}.txt")), text).map_err(|e| e.to_string())?;
            }
            "state" => {
                let json = serde_json::to_string_pretty(&app.project).map_err(|e| e.to_string())?;
                std::fs::write(self.out.join(format!("{rest}.json")), json).map_err(|e| e.to_string())?;
            }
            "expect" | "absent" => {
                let found = self.find(rest).is_ok();
                if found != (verb == "expect") {
                    return Err(format!("{} {rest}", if found { "está y no debería:" } else { "no está:" }));
                }
                log_line("  ok");
            }
            "scene" => app.apply_scene(rest),
            "run" => {
                let command = COMMANDS.iter().find(|c| format!("{c:?}").eq_ignore_ascii_case(rest)).ok_or(format!("comando desconocido: {rest}"))?;
                app.run(*command, ctx);
            }
            "pick" => {
                let paths = rest.split('|').map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from).collect();
                if let Ok(mut d) = DIALOGS.lock() {
                    d.get_or_insert_with(VecDeque::new).push_back(paths);
                }
            }
            "answer" => app.answers.push_back(match rest {
                "save" => crate::commands::Answer::Save,
                "discard" => crate::commands::Answer::Discard,
                _ => crate::commands::Answer::Cancel,
            }),
            "quit" => self.next = self.steps.len(),
            _ => return Err(format!("paso desconocido: {verb}")),
        }
        Ok(())
    }

    fn move_to(&mut self, at: Pos2, mods: Modifiers) {
        // In a few steps, so hover effects see the pointer arrive.
        let from = self.pointer;
        for i in 1..=3 {
            let p = if from.x < 0.0 { at } else { from + (at - from) * (i as f32 / 3.0) };
            self.frames.push_back((vec![Event::PointerMoved(p)], mods));
        }
        self.pointer = at;
    }

    fn press(&mut self, at: Pos2, button: PointerButton, mods: Modifiers, times: usize) {
        for _ in 0..times {
            for pressed in [true, false] {
                self.frames.push_back((vec![Event::PointerButton { pos: at, button, pressed, modifiers: mods }], mods));
            }
        }
    }

    /// A target's point: `"text"[#n|~x,y]` or `x,y`, then ` +dx,dy`.
    fn target(&self, target: &str) -> Result<Pos2, String> {
        let (base, offset) = match target.rsplit_once(" +") {
            Some((base, delta)) => (base.trim(), parse_vec(delta)?),
            None => (target, vec2(0.0, 0.0)),
        };
        let at = if base.starts_with('"') { self.find(base)?.center() } else { parse_vec(base)?.to_pos2() };
        Ok(at + offset)
    }

    fn find(&self, query: &str) -> Result<Rect, String> {
        let end = query[1..].find('"').ok_or("falta la comilla de cierre")? + 1;
        let text = &query[1..end];
        let tail = &query[end + 1..];
        let exact: Vec<&Widget> = self.widgets.iter().filter(|w| w.text == text).collect();
        let mut matches = if exact.is_empty() { self.widgets.iter().filter(|w| w.text.contains(text)).collect() } else { exact };
        if matches.is_empty() {
            return Err(format!("no hay ningún widget con «{text}»"));
        }
        matches.sort_by(|a, b| (a.rect.top(), a.rect.left()).partial_cmp(&(b.rect.top(), b.rect.left())).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(n) = tail.strip_prefix('#') {
            let n: usize = n.parse().map_err(|_| "#n")?;
            return matches.get(n.saturating_sub(1)).map(|w| w.rect).ok_or(format!("solo hay {} con «{text}»", matches.len()));
        }
        if let Some(point) = tail.strip_prefix('~') {
            let p = parse_vec(point)?.to_pos2();
            return Ok(matches.into_iter().min_by(|a, b| a.rect.center().distance(p).total_cmp(&b.rect.center().distance(p))).map(|w| w.rect).unwrap_or(Rect::NOTHING));
        }
        Ok(matches[0].rect)
    }
}

/// Leading modifiers (`cmd+shift `) and the rest.
fn modifiers(text: &str) -> (Modifiers, &str) {
    let (first, rest) = text.split_once(' ').unwrap_or((text, ""));
    let mut mods = Modifiers::NONE;
    for part in first.split('+') {
        match part {
            "cmd" => mods |= Modifiers::COMMAND | Modifiers::MAC_CMD,
            "shift" => mods |= Modifiers::SHIFT,
            "alt" => mods |= Modifiers::ALT,
            "ctrl" => mods |= Modifiers::CTRL,
            _ => return (Modifiers::NONE, text),
        }
    }
    (mods, rest.trim())
}

fn parse_vec(text: &str) -> Result<egui::Vec2, String> {
    let (x, y) = text.trim().split_once(',').ok_or(format!("se esperaba x,y: {text}"))?;
    Ok(vec2(x.trim().parse().map_err(|_| format!("x: {x}"))?, y.trim().parse().map_err(|_| format!("y: {y}"))?))
}

/// The widgets of the last frame, from egui's accessibility tree.
fn widgets(ctx: &egui::Context) -> Vec<Widget> {
    ctx.viewport(|vp| {
        let Some(state) = vp.this_pass.accesskit_state.as_ref() else { return Vec::new() };
        state
            .nodes
            .values()
            .filter_map(|node| {
                let b = node.bounds()?;
                let rect = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
                let text = [node.label(), node.value(), node.description()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                Some(Widget { text, role: format!("{:?}", node.role()), rect, disabled: node.is_disabled() })
            })
            .filter(|w| w.rect.is_positive())
            .collect()
    })
}

const COMMANDS: [Command; 42] = [
    Command::New,
    Command::Open,
    Command::ClearRecent,
    Command::Revert,
    Command::Save,
    Command::SaveAs,
    Command::ImportLive,
    Command::ImportAudio,
    Command::Export,
    Command::ShowFolder,
    Command::Quit,
    Command::Undo,
    Command::Redo,
    Command::Cut,
    Command::Copy,
    Command::Paste,
    Command::Duplicate,
    Command::Split,
    Command::Join,
    Command::Delete,
    Command::SelectAll,
    Command::NewMidiTrack,
    Command::NewAudioTrack,
    Command::DuplicateTrack,
    Command::DeleteTrack,
    Command::ShowSession,
    Command::ShowArrangement,
    Command::ToggleDetail,
    Command::ToggleBrowser,
    Command::FullScreen,
    Command::PlayStop,
    Command::Record,
    Command::Loop,
    Command::Metronome,
    Command::AutomationArm,
    Command::ComputerKeyboard,
    Command::MidiAssign,
    Command::Shortcuts,
    Command::About,
    Command::Settings,
    Command::CheckUpdates,
    Command::OpenRecent(0),
];
