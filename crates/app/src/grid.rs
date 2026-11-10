//! Grid controllers: a pad grid (Launchpad, APC) launches the Session's clips,
//! as in Live. Pulse knows them by their MIDI port's name and needs nothing set.
//!
//! This is the pure part: which message is which pad or button of each model,
//! the colours they light, the 8×8 window they control and the queue of LED
//! changes. `grid_io` connects it to the ports; the tests use fake ones.
//!
//! The maps, from each maker's public documents:
//!
//! - **Launchpad Mini [MK3]** — Novation, «Launchpad Mini [MK3] Programmer's
//!   reference manual»: SysEx header `F0 00 20 29 02 0D`, Programmer mode
//!   `F0 00 20 29 02 0D 0E 01 F7` (`… 0E 00 F7` back to Live), sent to its
//!   «LPMiniMK3 MIDI» port (the second one; the «DAW» one is for DAWs' own
//!   scripts). In Programmer mode the pads send and take notes 11–88 (the
//!   bottom left is 11, the top right 88: row × 10 + column), the buttons CCs:
//!   the top row 91–98 (↑ ↓ ← → Session …, as in Ardour's
//!   `libs/surfaces/launchpad_x/lpx.h`), the right column 89, 79 … 19. The
//!   MIDI channel says how it lights: 1 static, 2 flashing (between the static
//!   colour and this one), 3 pulsing; the velocity is a palette entry.
//! - **Launchpad X** — the same, «Launchpad X Programmer's reference manual»,
//!   device byte `0C`, port «LPX MIDI».
//! - **Launchpad Pro [MK3]** — Novation, «Launchpad Pro [MK3] Programmer's
//!   reference manual» (LPP3_prog_ref_guide_200415): header `F0 00 20 29 02
//!   0E`, Programmer mode `… 0E 01 F7`, port «LPProMK3 MIDI»; the same grid
//!   and channels; ← → are CC 91, 92 and ↑ ↓ CC 80, 70 (left column), the row
//!   under the grid CC 101–108 and «Stop Clip» CC 8 (the button ids in
//!   Ardour's `libs/surfaces/launchpad_pro/lppro.h` agree).
//! - **APC mini** (the first) — Akai, «APC mini Communications Protocol»:
//!   pads notes 0–63 (0 bottom left, 63 top right), the buttons under them
//!   64–71, the scene column 82–89 (top to bottom), Shift 98. A pad lights
//!   with a note on: velocity 0 off, 1 green, 2 green flashing, 3 red, 4 red
//!   flashing, 5 yellow, 6 yellow flashing; the buttons 0 off, 1 on, 2
//!   flashing.
//! - **APC mini mk2** — Akai, «APC mini mk2 Communication Protocol v1.0»:
//!   pads notes 0–63, the buttons under them 100–107 (0x64–0x6B), the scene
//!   column 112–119 (0x70–0x77), Shift 122 (0x7A); the open-source
//!   `apc-mini-mk2` driver (github.com/DJj123dj/apc-mini-mk2) uses the same.
//!   A pad lights with a note on whose velocity is a palette entry (the same
//!   128 colours as the Launchpads') and whose channel is how: 1–7
//!   brightness 10 % … 100 %, 8–11 pulsing, 12–16 flashing. The buttons as
//!   on the first one.
//! - **APC40 mkII** — Akai, «APC40 Mk2 Communications Protocol v1.2»: it is
//!   put in Ableton Live mode (the host lights it) with `F0 47 7F 29 60 00
//!   04 41 00 00 00 F7` (mode `41`; `40` is its own Generic mode). The clip
//!   grid is 8 × 5: notes 0–39 (0 bottom left); scene launch 82–86, clip
//!   stop note 52 on each track's channel (1–8), stop all 81, bank ↑ ↓ → ←
//!   94–97. Pads and scenes light with the palette (channel 1 static, 7–11
//!   pulsing, 12–16 flashing); clip stop with 0 off, 1 on, 2 flashing.
//!
//! What Pulse does with them (all the same, as far as they have the buttons):
//! a pad launches its clip (an empty one stops its track, as in the
//! Session), the scene column launches the scenes, the buttons under the
//! grid stop their tracks, and the arrows move the window over the Session.
//! A Launchpad Mini or X has no stop buttons: «Session» (CC 95) turns the
//! bottom row of pads into them, and the last scene button into «stop all».
//! An APC mini moves the window with Shift + the first four buttons under the
//! grid (↑ ↓ ← →), and Shift + the last scene button stops all.

use engine::NUM_SCENES;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Model {
    LaunchpadMini,
    LaunchpadX,
    LaunchpadPro,
    ApcMini,
    ApcMiniMk2,
    Apc40Mk2,
    /// Any other, with its grid learned (see [`Layout`]).
    Generic,
}

/// What Pulse does with a MIDI port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortUse {
    /// A grid controller's port to use.
    Grid(Model),
    /// Another port of a known controller (a Launchpad's DAW or DIN port):
    /// nothing it sends goes to the instruments either.
    Ignore,
    /// Not a grid controller's.
    Other,
}

/// What a port is, by its name (as macOS or Windows give it).
pub fn classify(port: &str) -> PortUse {
    let name = port.to_lowercase();
    let launchpad = |model| if name.contains("midi") && !name.contains("daw") && !name.contains("din") { PortUse::Grid(model) } else { PortUse::Ignore };
    if name.contains("lpminimk3") || (name.contains("launchpad mini") && name.contains("mk3")) {
        launchpad(Model::LaunchpadMini)
    } else if name.contains("lppromk3") || (name.contains("launchpad pro") && name.contains("mk3")) {
        launchpad(Model::LaunchpadPro)
    } else if name.contains("lpx") || name.contains("launchpad x") {
        launchpad(Model::LaunchpadX)
    } else if name.contains("apc mini") || name.contains("apc_mini") || name.contains("apcmini") {
        if name.contains("mk2") || name.contains("mkii") || name.contains("mk ii") {
            PortUse::Grid(Model::ApcMiniMk2)
        } else {
            PortUse::Grid(Model::ApcMini)
        }
    } else if name.contains("apc40") && (name.contains("mk2") || name.contains("mkii") || name.contains("mk ii")) {
        PortUse::Grid(Model::Apc40Mk2)
    } else {
        PortUse::Other
    }
}

/// A port's name without what tells its input from its output, to pair them
/// («MIDIIN2 (LPX MIDI)» and «MIDIOUT2 (LPX MIDI)», «X In» and «X Out»).
pub fn port_key(port: &str) -> String {
    let lower = port.to_lowercase();
    let mut words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.retain(|w| !matches!(*w, "in" | "out" | "input" | "output") && !w.starts_with("midiin") && !w.starts_with("midiout"));
    words.join(" ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// A pad or button, as Pulse sees it: pads by column and row (from the top
/// left), the rest by what they do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spot {
    Pad(usize, usize),
    /// A scene button, by row.
    Scene(usize),
    /// A track's stop button, by column.
    Stop(usize),
    StopAll,
    Arrow(Dir),
    /// The Launchpad Mini's or X's «Session»: the bottom row stops tracks.
    StopRow,
}

/// What a pad or button shows. On a Session cell: a clip stopped (dim, its
/// colour), playing (lit), queued (flashing), recording (pulsing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    Off,
    Dim(Rgb),
    Lit(Rgb),
    Blink(Rgb),
    Pulse(Rgb),
}

pub const RED: Rgb = [255, 0, 0];
pub const GREEN: Rgb = [0, 255, 0];
pub const WHITE: Rgb = [255, 255, 255];

/// The first 64 colours of the palette the Launchpads (and the APC mini mk2
/// and APC40 mkII) light with, as their manuals list them: off, greys, then
/// 14 hues of four shades (light, full, dark, darkest).
const PALETTE: [u32; 64] = [
    0x000000, 0x1E1E1E, 0x7F7F7F, 0xFFFFFF, 0xFF4C4C, 0xFF0000, 0x590000, 0x190000, 0xFFBD6C, 0xFF5400, 0x591D00, 0x271B00, 0xFFFF4C, 0xFFFF00, 0x595900, 0x191900,
    0x88FF4C, 0x54FF00, 0x1D5900, 0x142B00, 0x4CFF4C, 0x00FF00, 0x005900, 0x001900, 0x4CFF5E, 0x00FF19, 0x00590D, 0x001902, 0x4CFF88, 0x00FF55, 0x00591D, 0x001F12,
    0x4CFFB7, 0x00FF99, 0x005935, 0x001912, 0x4CC3FF, 0x00A9FF, 0x004152, 0x001019, 0x4C88FF, 0x0055FF, 0x001D59, 0x000819, 0x4C4CFF, 0x0000FF, 0x000059, 0x000019,
    0x874CFF, 0x5400FF, 0x190064, 0x0F0030, 0xFF4CFF, 0xFF00FF, 0x590059, 0x190019, 0xFF4C87, 0xFF0054, 0x59001D, 0x220013, 0xFF1500, 0x993500, 0x795100, 0x436400,
];

fn palette_rgb(index: usize) -> Rgb {
    let c = PALETTE[index];
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// The palette entry nearest `rgb`, lit: white or the light or full shade of
/// a hue. The colours are brightened first (a dark clip colour is still its hue).
pub fn palette_index(rgb: Rgb) -> u8 {
    let max = rgb.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bright = rgb.map(|c| (c as f32 * 255.0 / max) as i32);
    let candidates = std::iter::once(3).chain((1..15).flat_map(|k| [4 * k, 4 * k + 1]));
    let distance = |i: usize| {
        let p = palette_rgb(i);
        (0..3).map(|j| (bright[j] - p[j] as i32).pow(2)).sum::<i32>()
    };
    // A grey (no hue) is white.
    if rgb.iter().max().unwrap_or(&0) - rgb.iter().min().unwrap_or(&0) < 24 {
        return 3;
    }
    candidates.min_by_key(|&i| distance(i)).unwrap_or(3) as u8
}

/// The same hue's dark shade (white: dark grey).
pub fn dim_index(lit: u8) -> u8 {
    match lit {
        4..=59 => lit / 4 * 4 + 2,
        _ => 1,
    }
}

/// The APC mini's three colours: green, red or yellow, for `rgb`.
fn apc_mini_colour(rgb: Rgb) -> u8 {
    let [r, g, b] = rgb.map(|c| c as i32);
    if r > 2 * g && r > 2 * b {
        3
    } else if r > 160 && g > 160 && b < 120 {
        5
    } else {
        1
    }
}

/// A generic controller's grid, learned from three pads: the top left one,
/// the one to its right and the one under it. Its notes go by columns and rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub origin: u8,
    pub column_step: i16,
    pub row_step: i16,
}

impl Layout {
    /// From the three pads pressed (top left, right of it, under it).
    pub fn learn(notes: [u8; 3]) -> Option<Layout> {
        let column_step = notes[1] as i16 - notes[0] as i16;
        let row_step = notes[2] as i16 - notes[0] as i16;
        let layout = Layout { origin: notes[0], column_step, row_step };
        (column_step != 0 && row_step != 0 && column_step != row_step && layout.cells().iter().all(|n| n.is_some())).then_some(layout)
    }

    fn note(&self, column: usize, row: usize) -> Option<u8> {
        let n = self.origin as i16 + column as i16 * self.column_step + row as i16 * self.row_step;
        (0..128).contains(&n).then_some(n as u8)
    }

    fn cells(&self) -> Vec<Option<u8>> {
        (0..64).map(|i| self.note(i % 8, i / 8)).collect()
    }

    fn cell(&self, note: u8) -> Option<(usize, usize)> {
        (0..8).flat_map(|r| (0..8).map(move |c| (c, r))).find(|&(c, r)| self.note(c, r) == Some(note))
    }
}

/// A press on the controller, before the window places it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Pad(usize, usize),
    Scene(usize),
    Stop(usize),
    StopAll,
    Arrow(Dir),
    StopRow,
    Shift(bool),
}

impl Model {
    pub const KNOWN: [Model; 6] = [Model::LaunchpadMini, Model::LaunchpadX, Model::LaunchpadPro, Model::ApcMini, Model::ApcMiniMk2, Model::Apc40Mk2];

    pub fn label(self) -> &'static str {
        match self {
            Model::LaunchpadMini => "Launchpad Mini MK3",
            Model::LaunchpadX => "Launchpad X",
            Model::LaunchpadPro => "Launchpad Pro MK3",
            Model::ApcMini => "APC mini",
            Model::ApcMiniMk2 => "APC mini mk2",
            Model::Apc40Mk2 => "APC40 mkII",
            Model::Generic => crate::i18n::tr("Rejilla aprendida"),
        }
    }

    fn launchpad(self) -> bool {
        matches!(self, Model::LaunchpadMini | Model::LaunchpadX | Model::LaunchpadPro)
    }

    /// Its clip grid: columns and rows.
    pub fn grid(self) -> (usize, usize) {
        match self {
            Model::Apc40Mk2 => (8, 5),
            _ => (8, 8),
        }
    }

    /// What it is sent on connecting: the mode where Pulse lights it.
    pub fn hello(self) -> Vec<Vec<u8>> {
        match self {
            Model::LaunchpadMini => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0D, 0x0E, 0x01, 0xF7]],
            Model::LaunchpadX => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x01, 0xF7]],
            Model::LaunchpadPro => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0E, 0x0E, 0x01, 0xF7]],
            Model::Apc40Mk2 => vec![vec![0xF0, 0x47, 0x7F, 0x29, 0x60, 0x00, 0x04, 0x41, 0x00, 0x00, 0x00, 0xF7]],
            Model::ApcMini | Model::ApcMiniMk2 | Model::Generic => Vec::new(),
        }
    }

    /// What it is sent on letting it go (its LEDs already off): back to its own mode.
    pub fn goodbye(self) -> Vec<Vec<u8>> {
        match self {
            Model::LaunchpadMini => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0D, 0x0E, 0x00, 0xF7]],
            Model::LaunchpadX => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0C, 0x0E, 0x00, 0xF7]],
            Model::LaunchpadPro => vec![vec![0xF0, 0x00, 0x20, 0x29, 0x02, 0x0E, 0x0E, 0x00, 0xF7]],
            Model::Apc40Mk2 => vec![vec![0xF0, 0x47, 0x7F, 0x29, 0x60, 0x00, 0x04, 0x40, 0x00, 0x00, 0x00, 0xF7]],
            Model::ApcMini | Model::ApcMiniMk2 | Model::Generic => Vec::new(),
        }
    }

    /// What a message from it is (presses only: a release is nothing but Shift's).
    pub fn decode(self, message: &[u8], shift: bool, layout: Option<&Layout>) -> Option<Press> {
        let (&status, &a, &b) = (message.first()?, message.get(1)?, message.get(2)?);
        let kind = status & 0xF0;
        let channel = (status & 0x0F) as usize;
        let on = kind == 0x90 && b > 0;
        let off = kind == 0x80 || (kind == 0x90 && b == 0);
        let cc = kind == 0xB0 && b > 0;
        match self {
            Model::LaunchpadMini | Model::LaunchpadX | Model::LaunchpadPro => {
                let (row, column) = (a / 10, a % 10);
                if on && (1..=8).contains(&row) && (1..=8).contains(&column) {
                    return Some(Press::Pad(column as usize - 1, 8 - row as usize));
                }
                if !cc {
                    return None;
                }
                if column == 9 && (1..=8).contains(&row) {
                    return Some(Press::Scene(8 - row as usize));
                }
                match (self, a) {
                    (Model::LaunchpadPro, 80) => Some(Press::Arrow(Dir::Up)),
                    (Model::LaunchpadPro, 70) => Some(Press::Arrow(Dir::Down)),
                    (Model::LaunchpadPro, 91) => Some(Press::Arrow(Dir::Left)),
                    (Model::LaunchpadPro, 92) => Some(Press::Arrow(Dir::Right)),
                    (Model::LaunchpadPro, 101..=108) => Some(Press::Stop(a as usize - 101)),
                    (Model::LaunchpadPro, 8) => Some(Press::StopAll),
                    (Model::LaunchpadPro, _) => None,
                    (_, 91) => Some(Press::Arrow(Dir::Up)),
                    (_, 92) => Some(Press::Arrow(Dir::Down)),
                    (_, 93) => Some(Press::Arrow(Dir::Left)),
                    (_, 94) => Some(Press::Arrow(Dir::Right)),
                    (_, 95) => Some(Press::StopRow),
                    _ => None,
                }
            }
            Model::ApcMini | Model::ApcMiniMk2 => {
                let (tracks, scenes, shift_note) = if self == Model::ApcMini { (64, 82, 98) } else { (100, 112, 122) };
                if a == shift_note && (on || off) {
                    return Some(Press::Shift(on));
                }
                if !on {
                    return None;
                }
                match a {
                    0..=63 => Some(Press::Pad(a as usize % 8, 7 - a as usize / 8)),
                    n if (tracks..tracks + 8).contains(&n) => {
                        let i = (n - tracks) as usize;
                        match (shift, i) {
                            (true, 0) => Some(Press::Arrow(Dir::Up)),
                            (true, 1) => Some(Press::Arrow(Dir::Down)),
                            (true, 2) => Some(Press::Arrow(Dir::Left)),
                            (true, 3) => Some(Press::Arrow(Dir::Right)),
                            (true, _) => None,
                            (false, _) => Some(Press::Stop(i)),
                        }
                    }
                    n if (scenes..scenes + 8).contains(&n) => {
                        let i = (n - scenes) as usize;
                        if shift && i == 7 {
                            Some(Press::StopAll)
                        } else {
                            Some(Press::Scene(i))
                        }
                    }
                    _ => None,
                }
            }
            Model::Apc40Mk2 => {
                if !on {
                    return None;
                }
                match a {
                    0..=39 => Some(Press::Pad(a as usize % 8, 4 - a as usize / 8)),
                    0x52..=0x56 => Some(Press::Scene(a as usize - 0x52)),
                    0x34 if channel < 8 => Some(Press::Stop(channel)),
                    0x51 => Some(Press::StopAll),
                    0x5E => Some(Press::Arrow(Dir::Up)),
                    0x5F => Some(Press::Arrow(Dir::Down)),
                    0x60 => Some(Press::Arrow(Dir::Right)),
                    0x61 => Some(Press::Arrow(Dir::Left)),
                    _ => None,
                }
            }
            Model::Generic => {
                if !on {
                    return None;
                }
                layout?.cell(a).map(|(c, r)| Press::Pad(c, r))
            }
        }
    }

    /// The messages that light `spot` so (none: it has no such light).
    /// `stop_row`: a Launchpad Mini's or X's bottom row stops tracks.
    pub fn encode(self, spot: Spot, light: Light, layout: Option<&Layout>) -> Vec<Vec<u8>> {
        // A palette light: (status for static, flashing, pulsing) by kind.
        let palette = |status: u8, index: u8, light: Light, channels: (u8, u8, u8, u8)| -> Vec<Vec<u8>> {
            let (lit, dim, blink, pulse) = channels;
            match light {
                Light::Off => vec![vec![status | lit, index, 0]],
                Light::Dim(c) if dim == lit => vec![vec![status | lit, index, dim_index(palette_index(c))]],
                Light::Dim(c) => vec![vec![status | dim, index, palette_index(c)]],
                Light::Lit(c) => vec![vec![status | lit, index, palette_index(c)]],
                // A Launchpad flashes between the static colour and this one: off first.
                Light::Blink(c) if self.launchpad() => vec![vec![status | lit, index, 0], vec![status | blink, index, palette_index(c)]],
                Light::Blink(c) => vec![vec![status | blink, index, palette_index(c)]],
                Light::Pulse(c) => vec![vec![status | pulse, index, palette_index(c)]],
            }
        };
        // A one-colour button: off, on, flashing.
        let single = |status: u8, note: u8| -> Vec<Vec<u8>> {
            let v = match light {
                Light::Off | Light::Dim(_) => 0,
                Light::Lit(_) => 1,
                Light::Blink(_) | Light::Pulse(_) => 2,
            };
            vec![vec![status, note, v]]
        };
        match self {
            Model::LaunchpadMini | Model::LaunchpadX | Model::LaunchpadPro => {
                let channels = (0, 0, 1, 2);
                let pro = self == Model::LaunchpadPro;
                match spot {
                    Spot::Pad(c, r) if c < 8 && r < 8 => palette(0x90, ((8 - r) * 10 + c + 1) as u8, light, channels),
                    Spot::Scene(r) if r < 8 => palette(0xB0, ((8 - r) * 10 + 9) as u8, light, channels),
                    Spot::Stop(c) if c < 8 && pro => palette(0xB0, 101 + c as u8, light, channels),
                    // The bottom row of pads, while it stops tracks.
                    Spot::Stop(c) if c < 8 => palette(0x90, 11 + c as u8, light, channels),
                    Spot::StopAll if pro => palette(0xB0, 8, light, channels),
                    Spot::StopAll => palette(0xB0, 19, light, channels),
                    Spot::Arrow(d) => {
                        let cc = match (pro, d) {
                            (true, Dir::Up) => 80,
                            (true, Dir::Down) => 70,
                            (true, Dir::Left) => 91,
                            (true, Dir::Right) => 92,
                            (false, Dir::Up) => 91,
                            (false, Dir::Down) => 92,
                            (false, Dir::Left) => 93,
                            (false, Dir::Right) => 94,
                        };
                        palette(0xB0, cc, light, channels)
                    }
                    Spot::StopRow if !pro => palette(0xB0, 95, light, channels),
                    _ => Vec::new(),
                }
            }
            Model::ApcMini => match spot {
                Spot::Pad(c, r) if c < 8 && r < 8 => {
                    let note = ((7 - r) * 8 + c) as u8;
                    let v = match light {
                        Light::Off => 0,
                        Light::Dim(_) => 5,
                        Light::Lit(rgb) => apc_mini_colour(rgb),
                        Light::Blink(rgb) => apc_mini_colour(rgb) + 1,
                        Light::Pulse(_) => 4,
                    };
                    vec![vec![0x90, note, v]]
                }
                Spot::Scene(r) if r < 8 => single(0x90, 82 + r as u8),
                Spot::Stop(c) if c < 8 => single(0x90, 64 + c as u8),
                _ => Vec::new(),
            },
            Model::ApcMiniMk2 => match spot {
                // Channel 7: full brightness; 2: a quarter; 10: pulsing 1/4; 15: flashing 1/4.
                Spot::Pad(c, r) if c < 8 && r < 8 => palette(0x90, ((7 - r) * 8 + c) as u8, light, (6, 1, 14, 9)),
                Spot::Scene(r) if r < 8 => single(0x90, 112 + r as u8),
                Spot::Stop(c) if c < 8 => single(0x90, 100 + c as u8),
                _ => Vec::new(),
            },
            Model::Apc40Mk2 => match spot {
                // Channel 1: static; 10: pulsing 1/4; 15: flashing 1/4.
                Spot::Pad(c, r) if c < 8 && r < 5 => palette(0x90, ((4 - r) * 8 + c) as u8, light, (0, 0, 14, 9)),
                Spot::Scene(r) if r < 5 => palette(0x90, 0x52 + r as u8, light, (0, 0, 14, 9)),
                Spot::Stop(c) if c < 8 => single(0x90 | c as u8, 0x34),
                Spot::StopAll => single(0x90, 0x51),
                _ => Vec::new(),
            },
            Model::Generic => match (spot, layout.and_then(|l| if let Spot::Pad(c, r) = spot { l.note(c, r) } else { None })) {
                (Spot::Pad(..), Some(note)) => {
                    let v = match light {
                        Light::Off => 0,
                        Light::Dim(_) => 1,
                        _ => 127,
                    };
                    vec![vec![0x90, note, v]]
                }
                _ => Vec::new(),
            },
        }
    }
}

/// Where the window is: its top left cell (column, scene).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Window {
    pub column: usize,
    pub scene: usize,
}

impl Window {
    /// Moved one step `dir` (a whole window with `page`), kept on the Session:
    /// `columns` columns and `size` (columns, rows) wide.
    pub fn moved(self, dir: Dir, page: bool, columns: usize, size: (usize, usize)) -> Window {
        let step = |n: usize| if page { n } else { 1 };
        let mut w = self;
        match dir {
            Dir::Up => w.scene = w.scene.saturating_sub(step(size.1)),
            Dir::Down => w.scene += step(size.1),
            Dir::Left => w.column = w.column.saturating_sub(step(size.0)),
            Dir::Right => w.column += step(size.0),
        }
        w.clamped(columns, size)
    }

    pub fn clamped(self, columns: usize, size: (usize, usize)) -> Window {
        Window { column: self.column.min(columns.saturating_sub(size.0)), scene: self.scene.min(NUM_SCENES.saturating_sub(size.1)) }
    }
}

/// What the Session shows, for the lights: each column's cells and its stop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub columns: Vec<Column>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub cells: [Light; NUM_SCENES],
    pub stop: Light,
}

impl Column {
    pub fn empty() -> Self {
        Column { cells: [Light::Off; NUM_SCENES], stop: Light::Off }
    }
}

/// What the app does for a press, on the whole Session (the window already
/// placed it): launch a clip (an empty one stops its track), a scene, stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Launch { column: usize, scene: usize },
    LaunchScene(usize),
    StopTrack(usize),
    StopAll,
    /// The window moved (the Session shows it).
    Moved,
}

/// The most LED messages sent at once: the rest wait for the next round, so
/// a whole new page doesn't flood a slow port.
pub const LED_BUDGET: usize = 160;

/// One controller: its model, window and what its lights show now.
pub struct Surface {
    pub model: Model,
    pub window: Window,
    pub layout: Option<Layout>,
    shift: bool,
    /// A Launchpad Mini's or X's bottom row stops tracks.
    pub stop_row: bool,
    /// What each light was last sent.
    sent: HashMap<Spot, Light>,
}

impl Surface {
    pub fn new(model: Model, layout: Option<Layout>) -> Self {
        Surface { model, window: Window::default(), layout, shift: false, stop_row: false, sent: HashMap::new() }
    }

    fn stop_row_model(&self) -> bool {
        matches!(self.model, Model::LaunchpadMini | Model::LaunchpadX)
    }

    /// The clip cells it controls: columns and rows.
    pub fn size(&self) -> (usize, usize) {
        let (c, r) = self.model.grid();
        (c, if self.stop_row { r - 1 } else { r })
    }

    /// Its mode, and every light again from nothing.
    pub fn hello(&mut self) -> Vec<Vec<u8>> {
        self.sent.clear();
        self.model.hello()
    }

    /// Its lights off and back to its own mode.
    pub fn goodbye(&mut self) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = self.sent.iter().filter(|(_, l)| **l != Light::Off).flat_map(|(&spot, _)| self.model.encode(spot, Light::Off, self.layout.as_ref())).collect();
        out.extend(self.model.goodbye());
        self.sent.clear();
        out
    }

    /// What a message from it does, on a Session of `columns` columns.
    pub fn handle(&mut self, message: &[u8], columns: usize) -> Option<Action> {
        let press = self.model.decode(message, self.shift, self.layout.as_ref())?;
        let size = self.size();
        let w = self.window;
        match press {
            Press::Shift(down) => {
                self.shift = down;
                None
            }
            // The bottom row stopping tracks, or the last scene button stopping all.
            Press::Pad(c, r) if self.stop_row && r == size.1 => Some(Action::StopTrack(w.column + c)),
            Press::Scene(r) if self.stop_row && r == size.1 => Some(Action::StopAll),
            Press::Pad(c, r) if c < size.0 && r < size.1 => Some(Action::Launch { column: w.column + c, scene: w.scene + r }),
            Press::Pad(..) => None,
            Press::Scene(r) => (r < size.1).then_some(Action::LaunchScene(w.scene + r)),
            Press::Stop(c) => Some(Action::StopTrack(w.column + c)),
            Press::StopAll => Some(Action::StopAll),
            Press::Arrow(dir) => {
                self.window = w.moved(dir, self.shift && !matches!(self.model, Model::ApcMini | Model::ApcMiniMk2), columns, size);
                Some(Action::Moved)
            }
            Press::StopRow if self.stop_row_model() => {
                self.stop_row = !self.stop_row;
                self.window = self.window.clamped(columns, self.size());
                Some(Action::Moved)
            }
            Press::StopRow => None,
        }
    }

    /// Every light as the Session is now.
    pub fn frame(&self, view: &View) -> Vec<(Spot, Light)> {
        let (cols, rows) = self.size();
        let w = self.window.clamped(view.columns.len(), (cols, rows));
        let mut lights = Vec::new();
        let cell = |c: usize, s: usize| view.columns.get(c).map_or(Light::Off, |col| col.cells[s]);
        for r in 0..rows {
            for c in 0..cols {
                lights.push((Spot::Pad(c, r), cell(w.column + c, w.scene + r)));
            }
            // A scene: green while one of its clips plays, flashing while one is
            // queued, dim with clips, off without.
            let scene = w.scene + r;
            let all = || view.columns.iter().map(|col| col.cells[scene]);
            let light = if all().any(|l| matches!(l, Light::Lit(_))) {
                Light::Lit(GREEN)
            } else if all().any(|l| matches!(l, Light::Blink(_))) {
                Light::Blink(GREEN)
            } else if all().any(|l| l != Light::Off) {
                Light::Dim(WHITE)
            } else {
                Light::Off
            };
            lights.push((Spot::Scene(r), light));
        }
        let has_stops = !self.stop_row_model() || self.stop_row;
        if has_stops {
            for c in 0..cols {
                lights.push((Spot::Stop(c), view.columns.get(w.column + c).map_or(Light::Off, |col| col.stop)));
            }
            lights.push((Spot::StopAll, Light::Dim(RED)));
        }
        if self.stop_row_model() {
            lights.push((Spot::StopRow, if self.stop_row { Light::Lit(RED) } else { Light::Dim(WHITE) }));
        }
        let can = |dir| w.moved(dir, false, view.columns.len(), (cols, rows)) != w;
        for dir in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            lights.push((Spot::Arrow(dir), if can(dir) { Light::Lit(WHITE) } else { Light::Off }));
        }
        lights
    }

    /// The messages that bring its lights to the Session as it is now: only
    /// the changed ones, at most `LED_BUDGET` (the rest go next time).
    pub fn leds(&mut self, view: &View) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for (spot, light) in self.frame(view) {
            let before = self.sent.get(&spot).copied();
            // Never sent and off: it already is (the controller starts dark).
            if before == Some(light) || (before.is_none() && light == Light::Off) {
                continue;
            }
            let messages = self.model.encode(spot, light, self.layout.as_ref());
            if out.len() + messages.len() > LED_BUDGET {
                break;
            }
            out.extend(messages);
            self.sent.insert(spot, light);
        }
        // Spots no longer shown (the stop row turned off): their pads are
        // shown again by the frame; scene buttons and the rest stay as sent.
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_known_by_their_names() {
        assert_eq!(classify("LPMiniMK3 MIDI"), PortUse::Grid(Model::LaunchpadMini));
        assert_eq!(classify("Launchpad Mini MK3 LPMiniMK3 MIDI Out"), PortUse::Grid(Model::LaunchpadMini));
        assert_eq!(classify("LPMiniMK3 DAW"), PortUse::Ignore, "the DAW port is for DAW scripts");
        assert_eq!(classify("MIDIIN2 (LPX MIDI)"), PortUse::Grid(Model::LaunchpadX));
        assert_eq!(classify("LPX DAW"), PortUse::Ignore);
        assert_eq!(classify("LPProMK3 MIDI"), PortUse::Grid(Model::LaunchpadPro));
        assert_eq!(classify("LPProMK3 DIN"), PortUse::Ignore);
        assert_eq!(classify("APC MINI"), PortUse::Grid(Model::ApcMini));
        assert_eq!(classify("APC mini mk2 Control"), PortUse::Grid(Model::ApcMiniMk2));
        assert_eq!(classify("APC40 mkII"), PortUse::Grid(Model::Apc40Mk2));
        assert_eq!(classify("Arturia KeyStep 37"), PortUse::Other);
        assert_eq!(classify("IAC Driver Bus 1"), PortUse::Other);
        assert_eq!(port_key("MIDIIN2 (LPX MIDI)"), port_key("MIDIOUT2 (LPX MIDI)"));
        assert_eq!(port_key("Grid In"), port_key("Grid Out"));
    }

    /// Each model's pads and buttons: the top left pad, the bottom right, the
    /// first scene, a stop and the arrows, as their manuals number them.
    #[test]
    fn each_model_maps_its_pads_and_buttons() {
        use Press::*;
        let lp = Model::LaunchpadMini;
        assert_eq!(lp.decode(&[0x90, 81, 127], false, None), Some(Pad(0, 0)), "81: top left");
        assert_eq!(lp.decode(&[0x90, 18, 127], false, None), Some(Pad(7, 7)), "18: bottom right");
        assert_eq!(lp.decode(&[0x90, 18, 0], false, None), None, "a release");
        assert_eq!(lp.decode(&[0xB0, 89, 127], false, None), Some(Scene(0)));
        assert_eq!(lp.decode(&[0xB0, 19, 127], false, None), Some(Scene(7)));
        assert_eq!(lp.decode(&[0xB0, 91, 127], false, None), Some(Arrow(Dir::Up)));
        assert_eq!(lp.decode(&[0xB0, 94, 127], false, None), Some(Arrow(Dir::Right)));
        assert_eq!(lp.decode(&[0xB0, 95, 127], false, None), Some(StopRow));
        let pro = Model::LaunchpadPro;
        assert_eq!(pro.decode(&[0xB0, 80, 127], false, None), Some(Arrow(Dir::Up)));
        assert_eq!(pro.decode(&[0xB0, 70, 127], false, None), Some(Arrow(Dir::Down)));
        assert_eq!(pro.decode(&[0xB0, 91, 127], false, None), Some(Arrow(Dir::Left)));
        assert_eq!(pro.decode(&[0xB0, 103, 127], false, None), Some(Stop(2)));
        assert_eq!(pro.decode(&[0xB0, 8, 127], false, None), Some(StopAll));

        let apc = Model::ApcMini;
        assert_eq!(apc.decode(&[0x90, 56, 127], false, None), Some(Pad(0, 0)), "56: top left");
        assert_eq!(apc.decode(&[0x90, 7, 127], false, None), Some(Pad(7, 7)), "7: bottom right");
        assert_eq!(apc.decode(&[0x90, 82, 127], false, None), Some(Scene(0)));
        assert_eq!(apc.decode(&[0x90, 66, 127], false, None), Some(Stop(2)));
        assert_eq!(apc.decode(&[0x90, 98, 127], false, None), Some(Shift(true)));
        assert_eq!(apc.decode(&[0x80, 98, 0], false, None), Some(Shift(false)));
        assert_eq!(apc.decode(&[0x90, 66, 127], true, None), Some(Arrow(Dir::Left)), "Shift + the third: ←");
        assert_eq!(apc.decode(&[0x90, 89, 127], true, None), Some(StopAll));
        let mk2 = Model::ApcMiniMk2;
        assert_eq!(mk2.decode(&[0x90, 100, 127], false, None), Some(Stop(0)));
        assert_eq!(mk2.decode(&[0x90, 119, 127], false, None), Some(Scene(7)));
        assert_eq!(mk2.decode(&[0x90, 122, 127], false, None), Some(Shift(true)));
        assert_eq!(mk2.decode(&[0x90, 101, 127], true, None), Some(Arrow(Dir::Down)));

        let apc40 = Model::Apc40Mk2;
        assert_eq!(apc40.decode(&[0x90, 32, 127], false, None), Some(Pad(0, 0)), "32: top left of 8×5");
        assert_eq!(apc40.decode(&[0x90, 7, 127], false, None), Some(Pad(7, 4)));
        assert_eq!(apc40.decode(&[0x93, 0x34, 127], false, None), Some(Stop(3)), "clip stop on track 4's channel");
        assert_eq!(apc40.decode(&[0x90, 0x52, 127], false, None), Some(Scene(0)));
        assert_eq!(apc40.decode(&[0x90, 0x51, 127], false, None), Some(StopAll));
        assert_eq!(apc40.decode(&[0x90, 0x61, 127], false, None), Some(Arrow(Dir::Left)));
        assert_eq!(apc40.decode(&[0x80, 0x52, 0], false, None), None);
    }

    /// Lighting: the same spots back to their notes, with the channel saying
    /// how (static, flashing, pulsing) and the velocity the colour.
    #[test]
    fn lights_go_to_their_spots_with_the_model_s_colours() {
        let red = Light::Lit(RED);
        assert_eq!(Model::LaunchpadX.encode(Spot::Pad(0, 0), red, None), [vec![0x90, 81, 5]]);
        assert_eq!(Model::LaunchpadX.encode(Spot::Scene(7), Light::Dim(RED), None), [vec![0xB0, 19, 6]]);
        assert_eq!(Model::LaunchpadX.encode(Spot::Pad(7, 7), Light::Blink(GREEN), None), [vec![0x90, 18, 0], vec![0x91, 18, 21]], "flashing: off, then green on channel 2");
        assert_eq!(Model::LaunchpadX.encode(Spot::Pad(7, 7), Light::Pulse(GREEN), None), [vec![0x92, 18, 21]]);
        assert_eq!(Model::LaunchpadMini.encode(Spot::Stop(2), red, None), [vec![0x90, 13, 5]], "the bottom row stops");
        assert_eq!(Model::LaunchpadPro.encode(Spot::Stop(2), red, None), [vec![0xB0, 103, 5]]);
        assert_eq!(Model::ApcMini.encode(Spot::Pad(0, 0), Light::Lit(GREEN), None), [vec![0x90, 56, 1]]);
        assert_eq!(Model::ApcMini.encode(Spot::Pad(0, 0), Light::Blink(GREEN), None), [vec![0x90, 56, 2]]);
        assert_eq!(Model::ApcMini.encode(Spot::Pad(0, 0), Light::Lit(RED), None), [vec![0x90, 56, 3]]);
        assert_eq!(Model::ApcMini.encode(Spot::Pad(0, 0), Light::Dim([0, 0, 255]), None), [vec![0x90, 56, 5]], "a clip stopped: yellow");
        assert_eq!(Model::ApcMini.encode(Spot::Stop(1), Light::Blink(RED), None), [vec![0x90, 65, 2]]);
        assert_eq!(Model::ApcMiniMk2.encode(Spot::Pad(7, 7), Light::Lit(RED), None), [vec![0x96, 7, 5]], "channel 7: full brightness");
        assert_eq!(Model::ApcMiniMk2.encode(Spot::Pad(7, 7), Light::Blink(RED), None), [vec![0x9E, 7, 5]]);
        assert_eq!(Model::ApcMiniMk2.encode(Spot::Scene(0), Light::Lit(GREEN), None), [vec![0x90, 112, 1]]);
        assert_eq!(Model::Apc40Mk2.encode(Spot::Stop(3), Light::Lit(RED), None), [vec![0x93, 0x34, 1]]);
        assert_eq!(Model::Apc40Mk2.encode(Spot::Scene(4), Light::Lit(GREEN), None), [vec![0x90, 0x56, 21]]);
        assert!(Model::Apc40Mk2.encode(Spot::Pad(0, 5), red, None).is_empty(), "it has five rows");
    }

    #[test]
    fn clip_colours_go_to_the_nearest_of_the_palette() {
        assert_eq!(palette_index([255, 0, 0]), 5);
        assert_eq!(palette_index([0, 255, 0]), 21);
        assert_eq!(palette_index([0, 0, 255]), 45);
        assert_eq!(palette_index([255, 255, 0]), 13);
        assert_eq!(palette_index([128, 128, 128]), 3, "a grey: white");
        assert_eq!(palette_index([120, 0, 0]), 5, "a dark red is still red");
        assert_eq!(palette_index([255, 140, 0]), 9, "orange");
        assert_eq!(palette_index([255, 0, 255]), 53);
        assert_eq!(dim_index(5), 6);
        assert_eq!(dim_index(21), 22);
        assert_eq!(dim_index(3), 1);
    }

    #[test]
    fn the_window_stays_on_the_session() {
        let w = Window::default();
        assert_eq!(w.moved(Dir::Up, false, 20, (8, 8)), w);
        assert_eq!(w.moved(Dir::Right, false, 20, (8, 8)), Window { column: 1, scene: 0 });
        assert_eq!(w.moved(Dir::Right, true, 20, (8, 8)), Window { column: 8, scene: 0 });
        assert_eq!(Window { column: 10, scene: 0 }.moved(Dir::Right, true, 20, (8, 8)), Window { column: 12, scene: 0 }, "up to the last eight");
        assert_eq!(w.moved(Dir::Right, false, 5, (8, 8)), w, "fewer tracks than the grid: it doesn't move");
        assert_eq!(Window { column: 0, scene: 7 }.moved(Dir::Down, false, 4, (8, 8)), Window { column: 0, scene: 8 });
        assert_eq!(Window { column: 0, scene: 8 }.moved(Dir::Down, false, 4, (8, 8)), Window { column: 0, scene: 8 }, "16 scenes");
        assert_eq!(Window { column: 0, scene: 10 }.moved(Dir::Down, false, 4, (8, 5)), Window { column: 0, scene: 11 }, "an APC40's five rows");
    }

    fn session(columns: usize) -> View {
        View { columns: (0..columns).map(|_| Column::empty()).collect() }
    }

    /// Presses go to the cells of the window: moved, the same pad is another clip.
    #[test]
    fn presses_land_on_the_window() {
        let mut s = Surface::new(Model::LaunchpadX, None);
        assert_eq!(s.handle(&[0x90, 81, 127], 12), Some(Action::Launch { column: 0, scene: 0 }));
        assert_eq!(s.handle(&[0xB0, 94, 127], 12), Some(Action::Moved));
        assert_eq!(s.handle(&[0xB0, 92, 127], 12), Some(Action::Moved));
        assert_eq!(s.window, Window { column: 1, scene: 1 });
        assert_eq!(s.handle(&[0x90, 81, 127], 12), Some(Action::Launch { column: 1, scene: 1 }));
        assert_eq!(s.handle(&[0x90, 18, 127], 12), Some(Action::Launch { column: 8, scene: 8 }));
        assert_eq!(s.handle(&[0xB0, 79, 127], 12), Some(Action::LaunchScene(2)));
        // The Session button: the bottom row stops tracks, the last scene button all.
        assert_eq!(s.handle(&[0xB0, 95, 127], 12), Some(Action::Moved));
        assert_eq!(s.size(), (8, 7));
        assert_eq!(s.handle(&[0x90, 13, 127], 12), Some(Action::StopTrack(3)));
        assert_eq!(s.handle(&[0xB0, 19, 127], 12), Some(Action::StopAll));
        assert_eq!(s.handle(&[0x90, 21, 127], 12), Some(Action::Launch { column: 1, scene: 7 }));

        let mut apc = Surface::new(Model::ApcMini, None);
        apc.handle(&[0x90, 98, 127], 20);
        apc.handle(&[0x90, 67, 127], 20);
        apc.handle(&[0x80, 98, 0], 20);
        assert_eq!(apc.window, Window { column: 1, scene: 0 }, "Shift + → one track");
        assert_eq!(apc.handle(&[0x90, 64, 127], 20), Some(Action::StopTrack(1)));
    }

    /// The lights follow the Session: only what changed is sent again, a
    /// queued clip flashes, and a moved window shows other clips.
    #[test]
    fn lights_follow_the_session_and_only_changes_are_sent() {
        let mut s = Surface::new(Model::LaunchpadMini, None);
        let mut view = session(10);
        let blue = [0, 0, 255];
        view.columns[0].cells[0] = Light::Dim(blue);
        view.columns[9].cells[0] = Light::Dim(RED);
        let first = s.leds(&view);
        assert!(first.contains(&vec![0x90, 81, 46]), "a stopped clip, dim: {first:?}");
        assert!(first.contains(&vec![0xB0, 89, 1]), "its scene, dim white");
        assert!(!first.iter().any(|m| m[1] == 82), "an empty slot is never sent: it is dark already");
        assert!(s.leds(&view).is_empty(), "nothing changed, nothing sent");

        view.columns[0].cells[0] = Light::Blink(blue);
        assert_eq!(s.leds(&view), [vec![0x90, 81, 0], vec![0x91, 81, 45], vec![0xB0, 89, 0], vec![0xB1, 89, 21]], "queued: the clip and its scene flash");
        view.columns[0].cells[0] = Light::Lit(blue);
        let playing = s.leds(&view);
        assert!(playing.contains(&vec![0x90, 81, 45]) && playing.contains(&vec![0xB0, 89, 21]), "{playing:?}");
        view.columns[0].cells[0] = Light::Off;
        assert!(s.leds(&view).contains(&vec![0x90, 81, 0]), "deleted: dark");

        // Two to the right: column 9's clip shows on the eighth pad.
        s.window = Window { column: 2, scene: 0 };
        let moved = s.leds(&view);
        assert!(moved.contains(&vec![0x90, 88, 6]), "{moved:?}");
        // Back on: everything again.
        s.hello();
        assert!(!s.leds(&view).is_empty());
    }

    #[test]
    fn a_whole_page_goes_in_rounds() {
        let mut s = Surface::new(Model::LaunchpadPro, None);
        let mut view = session(8);
        for column in &mut view.columns {
            column.cells = [Light::Blink(GREEN); NUM_SCENES];
            column.stop = Light::Blink(RED);
        }
        // 64 flashing pads are 128 messages, the scenes and the stops 32 more…
        let first = s.leds(&view);
        assert!(first.len() <= LED_BUDGET);
        let second = s.leds(&view);
        assert!(!second.is_empty(), "the rest, next round");
        assert!(s.leds(&view).is_empty());
    }

    #[test]
    fn a_generic_grid_is_learned_from_three_pads() {
        // Rows of 8 going up from 0 (the top left is 56): like an APC mini.
        let layout = Layout::learn([56, 57, 48]).unwrap();
        let mut s = Surface::new(Model::Generic, Some(layout));
        assert_eq!(s.handle(&[0x90, 0, 100], 8), Some(Action::Launch { column: 0, scene: 7 }));
        assert_eq!(s.handle(&[0x90, 63, 100], 8), Some(Action::Launch { column: 7, scene: 0 }));
        assert_eq!(Model::Generic.encode(Spot::Pad(1, 0), Light::Lit(RED), Some(&layout)), [vec![0x90, 57, 127]]);
        assert_eq!(Layout::learn([60, 60, 52]), None, "the same pad twice");
        assert_eq!(Layout::learn([120, 121, 127]), None, "past note 127");
    }
}
