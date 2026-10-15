//! Live note input: hardware MIDI ports and the computer keyboard.

use eframe::egui::{self, Key};
use engine::{Controller, LiveInput, LiveNote};
use midir::{MidiInput, MidiInputConnection};
use std::collections::HashMap;

/// Open connections to every MIDI input port; notes go straight to the audio thread.
#[derive(Default)]
pub struct MidiInputs {
    connections: Vec<MidiInputConnection<MidiParser>>,
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
            // PULSE_MIDI_LOG=1 prints every packet and what it was read as.
            let log = std::env::var_os("PULSE_MIDI_LOG").is_some();
            let port_name = name.clone();
            let read = move |_: u64, bytes: &[u8], parser: &mut MidiParser| {
                if log {
                    eprintln!("[midi] {port_name}: paquete {}", hex(bytes));
                }
                parser.feed(bytes, |message| {
                    if log {
                        eprintln!("[midi]   → {}", describe(message));
                    }
                    forward(message, &live)
                })
            };
            match input.connect(&port, "Pulse input", read, MidiParser::default()) {
                Ok(connection) => {
                    inputs.connections.push(connection);
                    inputs.names.push(name);
                }
                Err(e) => inputs.error = Some(format!("{name}: {e}")),
            }
        }
        if std::env::var_os("PULSE_MIDI_LOG").is_some() {
            eprintln!("[midi] puertos conectados: {:?}; error: {:?}", inputs.names, inputs.error);
        }
        inputs
    }
}

/// Splits raw MIDI bytes into messages. CoreMIDI can hand over several messages in
/// one packet (chords, bursts of pitch bend or modulation), keyboards may leave out
/// repeated status bytes (running status), and clock or active-sensing bytes may come
/// in between; system exclusive data is skipped.
#[derive(Default)]
pub struct MidiParser {
    status: u8,
    data: [u8; 2],
    len: usize,
    sysex: bool,
}

impl MidiParser {
    pub fn feed(&mut self, bytes: &[u8], mut message: impl FnMut(&[u8])) {
        for &b in bytes {
            match b {
                // Real-time bytes may come anywhere, even inside other messages.
                0xF8..=0xFF => {}
                0xF0 => {
                    self.sysex = true;
                    self.status = 0;
                }
                0xF7 => self.sysex = false,
                0x80..=0xEF => {
                    self.status = b;
                    self.len = 0;
                    self.sysex = false;
                }
                // Other system common messages: their data is not for us.
                0xF1..=0xF6 => {
                    self.status = 0;
                    self.sysex = false;
                }
                _ => {
                    if self.sysex || self.status == 0 {
                        continue;
                    }
                    self.data[self.len] = b;
                    self.len += 1;
                    let needed = if matches!(self.status & 0xF0, 0xC0 | 0xD0) { 1 } else { 2 };
                    if self.len == needed {
                        let full = [self.status, self.data[0], self.data[1]];
                        message(&full[..1 + needed]);
                        // The status stays for running status.
                        self.len = 0;
                    }
                }
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

/// A MIDI message in words, for the log.
fn describe(message: &[u8]) -> String {
    let channel = message[0] & 0x0F;
    let what = match (message[0] & 0xF0, message.get(1).copied().unwrap_or(0), message.get(2).copied().unwrap_or(0)) {
        (0x90, n, 0) => format!("note-off {n} ({})", crate::project::note_name(n)),
        (0x90, n, v) => format!("note-on {n} ({}) vel {v}", crate::project::note_name(n)),
        (0x80, n, _) => format!("note-off {n} ({})", crate::project::note_name(n)),
        (0xB0, c, v) => format!("CC {c} = {v}"),
        (0xE0, lo, hi) => format!("pitch bend {}", ((hi as i32) << 7 | lo as i32) - 8192),
        (0xD0, v, _) => format!("aftertouch {v}"),
        (0xA0, n, v) => format!("presión polifónica {n} = {v}"),
        (0xC0, p, _) => format!("program change {p}"),
        _ => hex(message),
    };
    format!("canal {} · {what}", channel + 1)
}

/// Forwards notes, controllers, pitch bend and pressure on any channel.
fn forward(message: &[u8], live: &LiveInput) {
    match *message {
        [status, pressure] if status & 0xF0 == 0xD0 => live.send(LiveNote::control(Controller::Pressure, 0, pressure as f32 / 127.0)),
        [status, a, b] => match status & 0xF0 {
            // A note-on with velocity 0 is a note-off.
            0x90 => live.send(LiveNote::note(a, b as f32 / 127.0)),
            0x80 => live.send(LiveNote::note(a, 0.0)),
            0xA0 => live.send(LiveNote::control(Controller::PolyPressure, a, b as f32 / 127.0)),
            0xB0 => live.send(LiveNote::control(Controller::Cc(a), 0, b as f32 / 127.0)),
            0xE0 => {
                let bend = ((b as i32) << 7 | a as i32) - 8192;
                live.send(LiveNote::control(Controller::PitchBend, 0, bend as f32 / 8192.0));
            }
            _ => {}
        },
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
                live.send(LiveNote::note(pitch, 0.0));
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
                    live.send(LiveNote::note(pitch, self.velocity as f32 / 127.0));
                    self.held.insert(key, pitch);
                } else if !pressed {
                    if let Some(pitch) = self.held.remove(&key) {
                        live.send(LiveNote::note(pitch, 0.0));
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

#[cfg(test)]
mod midi_tests {
    use super::MidiParser;

    fn parse(chunks: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut parser = MidiParser::default();
        let mut out = Vec::new();
        for chunk in chunks {
            parser.feed(chunk, |m| out.push(m.to_vec()));
        }
        out
    }

    #[test]
    fn packets_with_several_messages_are_split() {
        // A chord in one packet, then pitch bend and the modulation wheel together.
        let out = parse(&[&[0x90, 60, 100, 0x90, 64, 90, 0x90, 67, 80], &[0xE0, 0, 80, 0xB0, 1, 64]]);
        assert_eq!(out, [vec![0x90, 60, 100], vec![0x90, 64, 90], vec![0x90, 67, 80], vec![0xE0, 0, 80], vec![0xB0, 1, 64]]);
    }

    #[test]
    fn running_status_clock_and_sysex() {
        let out = parse(&[
            &[0x90, 60, 100, 62, 100],  // running status: a second note-on
            &[64, 0],                    // and a third (a note-off), in the next packet
            &[0xE0, 0xF8, 0x10, 0x40],  // clock inside a pitch bend
            &[0xF0, 1, 2, 3, 0xF7],     // system exclusive
            &[0xD0, 70, 0x80, 60, 0],   // channel pressure, then a note-off
            &[0xB0, 1],                 // a controller split across packets
            &[127],
        ]);
        assert_eq!(out, [vec![0x90, 60, 100], vec![0x90, 62, 100], vec![0x90, 64, 0], vec![0xE0, 0x10, 0x40], vec![0xD0, 70], vec![0x80, 60, 0], vec![0xB0, 1, 127]]);
    }
}
