//! Live note input: hardware MIDI ports and the computer keyboard.

use eframe::egui::{self, Key};
use engine::{LiveInput, LiveNote};
use midir::{MidiInput, MidiInputConnection};
use std::collections::HashMap;

/// Open connections to every MIDI input port; notes go straight to the audio thread.
#[derive(Default)]
pub struct MidiInputs {
    connections: Vec<MidiInputConnection<()>>,
    pub names: Vec<String>,
    pub error: Option<String>,
}

impl MidiInputs {
    pub fn connect(live: Option<LiveInput>) -> Self {
        let mut inputs = Self::default();
        let Some(live) = live else { return inputs };
        let count = match MidiInput::new("Pulse") {
            Ok(probe) => probe.ports().len(),
            Err(e) => {
                inputs.error = Some(e.to_string());
                return inputs;
            }
        };
        // A connection consumes its MidiInput, so each port gets its own client.
        for index in 0..count {
            let Ok(input) = MidiInput::new("Pulse") else { continue };
            let Some(port) = input.ports().get(index).cloned() else { continue };
            let name = input.port_name(&port).unwrap_or_else(|_| format!("Puerto {}", index + 1));
            let live = live.clone();
            match input.connect(&port, "Pulse input", move |_, message, _| forward(message, &live), ()) {
                Ok(connection) => {
                    inputs.connections.push(connection);
                    inputs.names.push(name);
                }
                Err(e) => inputs.error = Some(format!("{name}: {e}")),
            }
        }
        inputs
    }
}

/// Forwards note-on/off messages on any channel.
fn forward(message: &[u8], live: &LiveInput) {
    let &[status, pitch, velocity] = message else { return };
    match status & 0xF0 {
        0x90 => live.send(LiveNote { pitch, velocity: velocity as f32 / 127.0 }),
        0x80 => live.send(LiveNote { pitch, velocity: 0.0 }),
        _ => {}
    }
}

/// Plays notes from the letter keys, laid out like a piano: A S D F G H J K L are
/// the white keys from C, W E T Y U O P the black ones. Z/X change octave, C/V velocity.
pub struct ComputerKeyboard {
    pub enabled: bool,
    pub octave: i32,
    pub velocity: u8,
    held: HashMap<Key, u8>,
}

impl Default for ComputerKeyboard {
    fn default() -> Self {
        Self {
            enabled: false,
            octave: 0,
            velocity: 100,
            held: HashMap::new(),
        }
    }
}

const PIANO: [(Key, u8); 17] = [
    (Key::A, 0),
    (Key::W, 1),
    (Key::S, 2),
    (Key::E, 3),
    (Key::D, 4),
    (Key::F, 5),
    (Key::T, 6),
    (Key::G, 7),
    (Key::Y, 8),
    (Key::H, 9),
    (Key::U, 10),
    (Key::J, 11),
    (Key::K, 12),
    (Key::O, 13),
    (Key::L, 14),
    (Key::P, 15),
    (Key::Semicolon, 16),
];

impl ComputerKeyboard {
    pub fn base_note(&self) -> u8 {
        (60 + self.octave * 12).clamp(0, 120) as u8
    }

    pub fn release_all(&mut self, live: Option<&LiveInput>) {
        for (_, pitch) in self.held.drain() {
            if let Some(live) = live {
                live.send(LiveNote { pitch, velocity: 0.0 });
            }
        }
    }

    pub fn toggle(&mut self, live: Option<&LiveInput>) {
        self.enabled = !self.enabled;
        if !self.enabled {
            self.release_all(live);
        }
    }

    /// Turns this frame's key presses into notes. Returns true if a key was used.
    pub fn handle(&mut self, ctx: &egui::Context, live: Option<&LiveInput>) {
        if !self.enabled {
            return;
        }
        let focused = ctx.input(|i| i.focused);
        if ctx.egui_wants_keyboard_input() || !focused {
            // Typing in a text field or switching apps must not leave notes hanging.
            self.release_all(live);
            return;
        }
        let Some(live) = live else { return };
        let events = ctx.input(|i| i.events.clone());
        for event in events {
            let egui::Event::Key { key, pressed, repeat, modifiers, .. } = event else { continue };
            if repeat || modifiers.command || modifiers.ctrl || modifiers.alt {
                continue;
            }
            if let Some(&(_, semitone)) = PIANO.iter().find(|(k, _)| *k == key) {
                if pressed && !self.held.contains_key(&key) {
                    let pitch = (self.base_note() + semitone).min(127);
                    live.send(LiveNote { pitch, velocity: self.velocity as f32 / 127.0 });
                    self.held.insert(key, pitch);
                } else if !pressed {
                    if let Some(pitch) = self.held.remove(&key) {
                        live.send(LiveNote { pitch, velocity: 0.0 });
                    }
                }
                continue;
            }
            if pressed {
                match key {
                    Key::Z => self.octave = (self.octave - 1).max(-5),
                    Key::X => self.octave = (self.octave + 1).min(5),
                    Key::C => self.velocity = self.velocity.saturating_sub(20).max(1),
                    Key::V => self.velocity = self.velocity.saturating_add(20).min(127),
                    _ => {}
                }
            }
        }
    }
}
