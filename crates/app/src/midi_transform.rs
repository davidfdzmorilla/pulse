//! «Transformar» in the MIDI editor, as Live 12's transformations: strum a
//! chord, humanize, shape the velocities, legato or shorter notes, grace
//! notes and flams, arpeggiate chords, connect notes with steps of the scale,
//! and turn the notes upside down or back to front.
//!
//! Each is a pure function of the chosen notes (all of them with none
//! selected). The panel shows the result as you change it (the clip plays
//! it), «Aplicar» keeps it as one undo step and «Cancelar» (or closing the
//! menu) leaves the notes as they were. Quantizing, the groove's humanize
//! (different on each lap), rolls and «A la escala» are elsewhere in the bar.

use crate::note_edit::{self, Selection, MIN_LENGTH};
use eframe::egui;
use engine::model::Note;
use stems::key::Key as Scale;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Strum,
    Humanize,
    Velocity,
    Length,
    Ornament,
    Arpeggiate,
    Connect,
    Flip,
}

impl Kind {
    pub const ALL: [Kind; 8] = [Kind::Strum, Kind::Humanize, Kind::Velocity, Kind::Length, Kind::Ornament, Kind::Arpeggiate, Kind::Connect, Kind::Flip];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Strum => "Rasgueo",
            Kind::Humanize => "Humanizar",
            Kind::Velocity => "Velocidad",
            Kind::Length => "Longitud",
            Kind::Ornament => "Ornamentos",
            Kind::Arpeggiate => "Arpegiar",
            Kind::Connect => "Conectar",
            Kind::Flip => "Invertir",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Kind::Strum => "Las notas de cada acorde entran una tras otra, como al rasguear una guitarra",
            Kind::Humanize => "Mueve un poco al azar el tiempo y la velocidad de cada nota, siempre igual con la misma semilla",
            Kind::Velocity => "Rampa de velocidad, comprimir o expandir las diferencias, o darles la vuelta",
            Kind::Length => "Legato (cada nota llega a la siguiente) o notas más cortas o más largas",
            Kind::Ornament => "Una nota de gracia antes de cada nota, o un flam (la misma nota, antes y más suave)",
            Kind::Arpeggiate => "Cada acorde, nota a nota, a la rejilla que elijas",
            Kind::Connect => "Rellena los huecos entre una nota y la siguiente con pasos de la escala",
            Kind::Flip => "Los tonos al revés (lo agudo abajo) o las notas del final al principio",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VelocityMode {
    Ramp,
    Scale,
    Invert,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthMode {
    Legato,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ornament {
    Grace,
    Flam,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Up,
    Down,
    UpDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flip {
    Pitches,
    Time,
}

/// Every transformation's settings: each keeps its own while you try others.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub kind: Kind,
    /// Rasgueo: low to high (up) or high to low; beats from the first note to
    /// the last; -1..1, how they bunch at the end (< 0) or the start (> 0).
    pub strum_up: bool,
    pub strum_time: f64,
    pub strum_curve: f32,
    /// Humanizar: at most this many beats either way; at most this much
    /// velocity (0..1) either way; the same seed, the same result.
    pub human_time: f64,
    pub human_velocity: f32,
    pub seed: u32,
    pub velocity: VelocityMode,
    /// Rampa: from and to (0..1), first note to last.
    pub ramp: (f32, f32),
    /// Comprimir/expandir: -1 (all as the average) .. 1 (twice as far from it).
    pub spread: f32,
    pub length: LengthMode,
    pub factor: f64,
    pub ornament: Ornament,
    /// Semitones (or, with a key, steps of its scale) the grace note is from its note.
    pub grace_step: i32,
    /// Beats the grace note (or the flam's) comes before its note.
    pub grace_time: f64,
    pub pattern: Pattern,
    /// Arpegiar and Conectar: the step (beats), and how long each note is of it.
    pub rate: f64,
    pub gate: f64,
    pub flip: Flip,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            kind: Kind::Strum,
            strum_up: true,
            strum_time: 0.25,
            strum_curve: 0.0,
            human_time: 0.03,
            human_velocity: 0.1,
            seed: 1,
            velocity: VelocityMode::Ramp,
            ramp: (0.4, 1.0),
            spread: -0.5,
            length: LengthMode::Legato,
            factor: 0.5,
            ornament: Ornament::Grace,
            grace_step: -1,
            grace_time: 0.125,
            pattern: Pattern::Up,
            rate: 0.25,
            gate: 0.9,
            flip: Flip::Pitches,
        }
    }
}

/// Notes starting this close (beats) are one chord.
const TOGETHER: f64 = 1.0 / 64.0;

/// The notes as chords: groups (indices into `notes`) that start together,
/// earliest first, each lowest first.
fn chords(notes: &[Note]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|&a, &b| notes[a].start.total_cmp(&notes[b].start).then(notes[a].pitch.cmp(&notes[b].pitch)));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for i in order {
        match groups.last_mut() {
            Some(g) if notes[i].start - notes[g[0]].start <= TOGETHER => g.push(i),
            _ => groups.push(vec![i]),
        }
    }
    for g in &mut groups {
        g.sort_by_key(|&i| notes[i].pitch);
    }
    groups
}

/// A number from the seed and a note, -1..1, the same each time.
fn noise(seed: u32, index: usize, salt: u32) -> f64 {
    let mut x = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (index as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9) ^ (salt as u64).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// `at` (0..1) shaped by `curve`: above 0 the steps grow (bunched at the start,
/// a strum that slows down), below 0 they shrink (one that speeds up).
fn shape(at: f64, curve: f32) -> f64 {
    let k = 1.0 + 3.0 * curve.abs() as f64;
    if curve >= 0.0 { at.powf(k) } else { 1.0 - (1.0 - at).powf(k) }
}

/// `notes` transformed (in a clip `clip_length` long, in `key` if the
/// project has one). The result may have more or fewer notes.
pub fn transform(notes: &[Note], s: &Settings, clip_length: f64, key: Option<Scale>) -> Vec<Note> {
    let mut out: Vec<Note> = notes.to_vec();
    let fit = |n: &mut Note| {
        n.start = n.start.clamp(0.0, (clip_length - MIN_LENGTH).max(0.0));
        n.length = n.length.clamp(MIN_LENGTH, (clip_length - n.start).max(MIN_LENGTH));
        n.velocity = n.velocity.clamp(0.01, 1.0);
    };
    match s.kind {
        Kind::Strum => {
            for chord in chords(notes) {
                let count = chord.len();
                if count < 2 {
                    continue;
                }
                for (k, &i) in chord.iter().enumerate() {
                    let order = if s.strum_up { k } else { count - 1 - k };
                    let delay = s.strum_time * shape(order as f64 / (count - 1) as f64, s.strum_curve);
                    // The end stays: the chord still lets go together.
                    let end = out[i].start + out[i].length;
                    out[i].start += delay;
                    out[i].length = (end - out[i].start).max(MIN_LENGTH);
                }
            }
        }
        Kind::Humanize => {
            for (i, n) in out.iter_mut().enumerate() {
                n.start += noise(s.seed, i, 1) * s.human_time;
                n.velocity += (noise(s.seed, i, 2) * s.human_velocity as f64) as f32;
            }
        }
        Kind::Velocity => match s.velocity {
            VelocityMode::Ramp => {
                let from = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
                let to = notes.iter().map(|n| n.start).fold(f64::NEG_INFINITY, f64::max);
                for n in &mut out {
                    let at = if to > from { ((n.start - from) / (to - from)) as f32 } else { 1.0 };
                    n.velocity = s.ramp.0 + (s.ramp.1 - s.ramp.0) * at;
                }
            }
            VelocityMode::Scale => {
                let mean = notes.iter().map(|n| n.velocity).sum::<f32>() / notes.len().max(1) as f32;
                for n in &mut out {
                    n.velocity = mean + (n.velocity - mean) * (1.0 + s.spread);
                }
            }
            VelocityMode::Invert => {
                let lo = notes.iter().map(|n| n.velocity).fold(f32::INFINITY, f32::min);
                let hi = notes.iter().map(|n| n.velocity).fold(f32::NEG_INFINITY, f32::max);
                for n in &mut out {
                    n.velocity = hi + lo - n.velocity;
                }
            }
        },
        Kind::Length => match s.length {
            LengthMode::Legato => {
                let groups = chords(notes);
                for (g, chord) in groups.iter().enumerate() {
                    let Some(next) = groups.get(g + 1) else { continue };
                    let until = notes[next[0]].start;
                    for &i in chord {
                        out[i].length = until - out[i].start;
                    }
                }
                // The last ones keep their length (there is nothing after them).
            }
            LengthMode::Scale => {
                for n in &mut out {
                    n.length *= s.factor;
                }
            }
        },
        Kind::Ornament => {
            for n in notes {
                if n.start - s.grace_time < -1e-9 {
                    continue;
                }
                let pitch = match s.ornament {
                    Ornament::Flam => Some(n.pitch),
                    Ornament::Grace => match key {
                        Some(key) => key.step(n.pitch, s.grace_step),
                        None => u8::try_from(n.pitch as i32 + s.grace_step).ok().filter(|p| *p <= 127),
                    },
                };
                let Some(pitch) = pitch else { continue };
                let velocity = n.velocity * if s.ornament == Ornament::Flam { 0.55 } else { 0.8 };
                let length = if s.ornament == Ornament::Flam { s.grace_time.min(n.length) } else { s.grace_time };
                out.push(Note { start: n.start - s.grace_time, length, pitch, velocity, chance: n.chance });
            }
        }
        Kind::Arpeggiate => {
            out.clear();
            for chord in chords(notes) {
                let first = &notes[chord[0]];
                let end = chord.iter().map(|&i| notes[i].start + notes[i].length).fold(first.start, f64::max);
                let order: Vec<usize> = match s.pattern {
                    Pattern::Up => chord.clone(),
                    Pattern::Down => chord.iter().rev().copied().collect(),
                    Pattern::UpDown if chord.len() > 2 => chord.iter().copied().chain(chord[1..chord.len() - 1].iter().rev().copied()).collect(),
                    Pattern::UpDown => chord.clone(),
                };
                let steps = (((end - first.start) / s.rate) + 1e-6).floor().max(1.0) as usize;
                for k in 0..steps {
                    let n = &notes[order[k % order.len()]];
                    out.push(Note { start: first.start + k as f64 * s.rate, length: s.rate * s.gate, ..n.clone() });
                }
            }
        }
        Kind::Connect => {
            let mut line: Vec<&Note> = notes.iter().collect();
            line.sort_by(|a, b| a.start.total_cmp(&b.start).then(b.pitch.cmp(&a.pitch)));
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let from = a.start + a.length;
                let gap = b.start - from;
                let steps = ((gap / s.rate) + 1e-6).floor() as usize;
                if steps == 0 {
                    continue;
                }
                for k in 0..steps {
                    let at = (k + 1) as f64 / (steps + 1) as f64;
                    let heard = a.pitch as f64 + (b.pitch as f64 - a.pitch as f64) * at;
                    let pitch = key.map_or(heard.round().clamp(0.0, 127.0) as u8, |key| key.snap(heard));
                    let velocity = a.velocity + (b.velocity - a.velocity) * at as f32;
                    out.push(Note { start: from + k as f64 * s.rate, length: s.rate * s.gate, pitch, velocity, chance: 1.0 });
                }
            }
        }
        Kind::Flip => match s.flip {
            Flip::Pitches => {
                let lo = notes.iter().map(|n| n.pitch).min().unwrap_or(0) as i32;
                let hi = notes.iter().map(|n| n.pitch).max().unwrap_or(0) as i32;
                for n in &mut out {
                    let mirrored = (hi + lo - n.pitch as i32).clamp(0, 127) as u8;
                    // In a key, still in it.
                    n.pitch = key.map_or(mirrored, |key| key.snap(mirrored as f64));
                }
            }
            Flip::Time => {
                let from = notes.iter().map(|n| n.start).fold(f64::INFINITY, f64::min);
                let to = notes.iter().map(|n| n.start + n.length).fold(f64::NEG_INFINITY, f64::max);
                for n in &mut out {
                    n.start = from + (to - (n.start + n.length));
                }
            }
        },
    }
    for n in &mut out {
        fit(n);
    }
    out.retain(|n| n.start < clip_length);
    out
}

/// Transforms the notes at `targets` in `notes` (all, if none are given):
/// they are taken out and what they become goes at the end. Returns the new
/// notes' indices, for the selection.
pub fn apply(notes: &mut Vec<Note>, targets: &[usize], s: &Settings, clip_length: f64, key: Option<Scale>) -> Selection {
    let chosen: Vec<Note> = targets.iter().filter_map(|&i| notes.get(i).cloned()).collect();
    if chosen.is_empty() {
        return Selection::new();
    }
    let result = transform(&chosen, s, clip_length, key);
    note_edit::delete(notes, &targets.iter().copied().collect());
    let first = notes.len();
    notes.extend(result);
    (first..notes.len()).collect()
}

/// A transformation being tried: the notes as they were, to go back to.
#[derive(Clone, Debug, PartialEq)]
struct Trying {
    clip: crate::app::ClipRef,
    notes: Vec<Note>,
    selection: Selection,
}

/// The panel's state, kept by the editor.
#[derive(Default)]
pub struct Panel {
    pub settings: Settings,
    trying: Option<Trying>,
    /// Whether the panel was drawn this frame (if not, its menu closed).
    shown: bool,
}

impl Panel {
    /// A transformation is being tried: the clip changes aren't undo steps yet.
    pub fn trying(&self) -> bool {
        self.trying.is_some()
    }

    /// Before the editor's bar is drawn.
    pub fn begin(&mut self) {
        self.shown = false;
    }

    /// After it: the menu closed without «Aplicar» puts the notes back.
    pub fn end(&mut self, clip_ref: crate::app::ClipRef, notes: &mut Vec<Note>, selection: &mut Selection) {
        if self.shown {
            return;
        }
        if let Some(trying) = self.trying.take() {
            if trying.clip == clip_ref {
                *notes = trying.notes;
                *selection = trying.selection;
            }
        }
    }

    /// The clip opened is another: what was tried there stays.
    pub fn switched(&mut self) {
        self.trying = None;
    }
}

fn label(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(crate::theme::TEXT_DIM).size(10.5));
}

/// A share (0..1, or -1..1) as a percentage field.
fn percent(ui: &mut egui::Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, hint: &str) {
    let mut p = *value * 100.0;
    if ui.add(egui::DragValue::new(&mut p).range(*range.start() * 100.0..=*range.end() * 100.0).speed(1.0).suffix(" %").max_decimals(0)).on_hover_text(hint).changed() {
        *value = p / 100.0;
    }
}

/// A length in beats from a few choices.
fn step(ui: &mut egui::Ui, value: &mut f64, options: &[(f64, &str)], hint: &str) {
    ui.horizontal(|ui| {
        for &(option, text) in options {
            ui.selectable_value(value, option, text).on_hover_text(hint);
        }
    });
}

const SHORT: [(f64, &str); 4] = [(1.0 / 32.0, "1/128"), (1.0 / 16.0, "1/64"), (0.125, "1/32"), (0.25, "1/16")];
const RATES: [(f64, &str); 5] = [(1.0, "1/4"), (0.5, "1/8"), (1.0 / 3.0, "1/8T"), (0.25, "1/16"), (0.125, "1/32")];

/// The settings of the chosen transformation.
fn settings_ui(ui: &mut egui::Ui, s: &mut Settings, key: Option<Scale>) {
    match s.kind {
        Kind::Strum => {
            ui.horizontal(|ui| {
                label(ui, "Dirección");
                ui.selectable_value(&mut s.strum_up, true, "Arriba").on_hover_text("De la nota más grave a la más aguda");
                ui.selectable_value(&mut s.strum_up, false, "Abajo").on_hover_text("De la más aguda a la más grave");
            });
            label(ui, "Tiempo entre la primera y la última");
            step(ui, &mut s.strum_time, &[(0.0625, "1/64"), (0.125, "1/32"), (0.25, "1/16"), (0.5, "1/8"), (1.0, "1/4")], "Lo que tarda el rasgueo de la primera nota del acorde a la última");
            ui.horizontal(|ui| {
                label(ui, "Curva");
                ui.add(egui::Slider::new(&mut s.strum_curve, -1.0..=1.0).show_value(false)).on_hover_text("A la izquierda las notas se juntan al final (un rasgueo que acelera); a la derecha, al principio (uno que frena)");
            });
        }
        Kind::Humanize => {
            ui.horizontal(|ui| {
                label(ui, "Tiempo");
                let mut ms = s.human_time * 1000.0;
                if ui.add(egui::DragValue::new(&mut ms).range(0.0..=250.0).speed(1.0).suffix(" milésimas de tiempo").max_decimals(0)).on_hover_text("Cuánto puede adelantarse o retrasarse cada nota, en milésimas de tiempo (125 = una fusa)").changed() {
                    s.human_time = ms / 1000.0;
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Velocidad");
                percent(ui, &mut s.human_velocity, 0.0..=0.5, "Cuánto puede subir o bajar la velocidad de cada nota");
            });
            ui.horizontal(|ui| {
                label(ui, "Semilla");
                ui.add(egui::DragValue::new(&mut s.seed).range(1..=9999)).on_hover_text("La misma semilla da siempre el mismo resultado; cámbiala para otro");
                if ui.small_button("Otra").on_hover_text("Otra semilla: otro resultado").clicked() {
                    s.seed = s.seed % 9999 + 1;
                }
            });
        }
        Kind::Velocity => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.velocity, VelocityMode::Ramp, "Rampa").on_hover_text("De una velocidad a otra, de la primera nota a la última");
                ui.selectable_value(&mut s.velocity, VelocityMode::Scale, "Comprimir / expandir").on_hover_text("Acerca las velocidades a su media o las aleja");
                ui.selectable_value(&mut s.velocity, VelocityMode::Invert, "Dar la vuelta").on_hover_text("Lo fuerte pasa a suave y lo suave a fuerte");
            });
            match s.velocity {
                VelocityMode::Ramp => {
                    ui.horizontal(|ui| {
                        label(ui, "De");
                        percent(ui, &mut s.ramp.0, 0.01..=1.0, "La velocidad de la primera nota");
                        label(ui, "a");
                        percent(ui, &mut s.ramp.1, 0.01..=1.0, "La velocidad de la última");
                    });
                }
                VelocityMode::Scale => {
                    ui.horizontal(|ui| {
                        label(ui, "Diferencias");
                        percent(ui, &mut s.spread, -1.0..=1.0, "-100 %: todas como la media; 0: igual; +100 %: el doble de lejos de la media");
                    });
                }
                VelocityMode::Invert => {}
            }
        }
        Kind::Length => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.length, LengthMode::Legato, "Legato").on_hover_text("Cada nota dura hasta que empieza la siguiente");
                ui.selectable_value(&mut s.length, LengthMode::Scale, "Multiplicar").on_hover_text("Más cortas (staccato) o más largas");
            });
            if s.length == LengthMode::Scale {
                ui.horizontal(|ui| {
                    for (factor, text) in [(0.25, "×¼"), (0.5, "×½"), (0.75, "×¾"), (1.5, "×1½"), (2.0, "×2")] {
                        ui.selectable_value(&mut s.factor, factor, text).on_hover_text("Multiplica la duración de cada nota");
                    }
                });
            }
        }
        Kind::Ornament => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.ornament, Ornament::Grace, "Nota de gracia").on_hover_text("Una nota corta justo antes, un poco más arriba o más abajo");
                ui.selectable_value(&mut s.ornament, Ornament::Flam, "Flam").on_hover_text("La misma nota justo antes y más suave, como el flam de una caja");
            });
            if s.ornament == Ornament::Grace {
                ui.horizontal(|ui| {
                    label(ui, if key.is_some() { "Pasos de la escala" } else { "Semitonos" });
                    ui.add(egui::DragValue::new(&mut s.grace_step).range(-12..=12)).on_hover_text("Lo lejos que está la nota de gracia: negativo, por debajo");
                });
            }
            label(ui, "Antes de la nota");
            step(ui, &mut s.grace_time, &SHORT, "Cuánto antes de su nota entra");
        }
        Kind::Arpeggiate => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.pattern, Pattern::Up, "Sube");
                ui.selectable_value(&mut s.pattern, Pattern::Down, "Baja");
                ui.selectable_value(&mut s.pattern, Pattern::UpDown, "Sube y baja");
            });
            label(ui, "Cada");
            step(ui, &mut s.rate, &RATES, "Cada cuánto suena la siguiente nota del acorde");
            gate(ui, s);
        }
        Kind::Connect => {
            label(ui, "Cada");
            step(ui, &mut s.rate, &RATES, "Cada cuánto suena un paso en los huecos");
            gate(ui, s);
            if key.is_none() {
                label(ui, "Sin tonalidad en el proyecto, los pasos son cromáticos");
            }
        }
        Kind::Flip => {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.flip, Flip::Pitches, "Tonos").on_hover_text("Lo más agudo pasa a lo más grave y al revés (en la escala, si hay tonalidad)");
                ui.selectable_value(&mut s.flip, Flip::Time, "Retrogradar").on_hover_text("Del final al principio: la última nota es la primera");
            });
        }
    }
}

fn gate(ui: &mut egui::Ui, s: &mut Settings) {
    ui.horizontal(|ui| {
        label(ui, "Duración");
        let mut g = s.gate as f32;
        percent(ui, &mut g, 0.1..=1.0, "Cuánto del paso dura cada nota");
        s.gate = g as f64;
    });
}

/// The panel (in its menu): what to do, its settings, «Aplicar» and «Cancelar».
/// `notes` and `selection` are the clip's: they show the result as you go.
pub fn panel(ui: &mut egui::Ui, panel: &mut Panel, clip_ref: crate::app::ClipRef, notes: &mut Vec<Note>, selection: &mut Selection, clip_length: f64, key: Option<Scale>) {
    panel.shown = true;
    ui.set_min_width(300.0);
    if panel.trying.as_ref().is_some_and(|t| t.clip != clip_ref) {
        panel.trying = None;
    }
    let scope = if panel.trying.as_ref().map_or(selection.is_empty(), |t| t.selection.is_empty()) { "todas las notas" } else { "las notas seleccionadas" };
    label(ui, &format!("Transforma {scope}"));
    ui.horizontal_wrapped(|ui| {
        for kind in Kind::ALL {
            let chosen = panel.trying.is_some() && panel.settings.kind == kind;
            if ui.selectable_label(chosen, kind.label()).on_hover_text(kind.hint()).clicked() {
                panel.settings.kind = kind;
                if panel.trying.is_none() {
                    panel.trying = Some(Trying { clip: clip_ref, notes: notes.clone(), selection: selection.clone() });
                }
            }
        }
    });
    let Some(trying) = panel.trying.clone() else {
        label(ui, "Elige una para verla (y oírla) antes de aplicarla");
        return;
    };
    ui.separator();
    settings_ui(ui, &mut panel.settings, key);
    // The result, from the notes as they were.
    let mut result = trying.notes.clone();
    let targets = note_edit::targets(&trying.notes, &trying.selection);
    let added = apply(&mut result, &targets, &panel.settings, clip_length, key);
    if *notes != result {
        *notes = result;
    }
    // The transformed notes stay selected (all of them if none were).
    *selection = if trying.selection.is_empty() { Selection::new() } else { added };
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Aplicar").on_hover_text("Se queda así (⌘Z lo deshace)").clicked() {
            panel.trying = None;
            ui.close();
        }
        if ui.button("Cancelar").on_hover_text("Las notas vuelven a como estaban").clicked() {
            *notes = trying.notes.clone();
            *selection = trying.selection.clone();
            panel.trying = None;
            ui.close();
        }
    });
}

/// The right-click menu's «Transformar ▸»: each one at once, with its last settings.
pub fn quick_menu(ui: &mut egui::Ui, panel: &mut Panel, notes: &mut Vec<Note>, selection: &mut Selection, clip_length: f64, key: Option<Scale>) {
    for kind in Kind::ALL {
        if ui.button(kind.label()).on_hover_text(format!("{} (con lo último que elegiste en «Transformar»)", kind.hint())).clicked() {
            panel.settings.kind = kind;
            let targets = note_edit::targets(notes, selection);
            let had = !selection.is_empty();
            let added = apply(notes, &targets, &panel.settings, clip_length, key);
            *selection = if had { added } else { Selection::new() };
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, pitch: u8, length: f64, velocity: f32) -> Note {
        Note { start, length, pitch, velocity, chance: 1.0 }
    }

    fn with(kind: Kind) -> Settings {
        Settings { kind, ..Default::default() }
    }

    fn chord(at: f64) -> Vec<Note> {
        vec![note(at, 60, 1.0, 0.8), note(at, 64, 1.0, 0.8), note(at, 67, 1.0, 0.8)]
    }

    #[test]
    fn a_strum_spreads_each_chord_and_keeps_its_end() {
        let mut notes = chord(0.0);
        notes.extend(chord(2.0));
        let up = transform(&notes, &Settings { strum_time: 0.25, ..with(Kind::Strum) }, 4.0, None);
        let starts = |pitch: u8, from: f64| up.iter().find(|n| n.pitch == pitch && (n.start - from).abs() < 0.5).unwrap().start;
        assert_eq!((starts(60, 0.0), starts(64, 0.0), starts(67, 0.0)), (0.0, 0.125, 0.25));
        assert_eq!(starts(67, 2.0), 2.25);
        assert!(up.iter().all(|n| (n.start + n.length - if n.start < 1.0 { 1.0 } else { 3.0 }).abs() < 1e-9));
        let down = transform(&notes, &Settings { strum_up: false, ..with(Kind::Strum) }, 4.0, None);
        assert_eq!(down[2].start, 0.0, "high first");
        assert_eq!(down[0].start, 0.25);
        // Bunched at the start: the middle note comes before half way.
        let late = transform(&notes, &Settings { strum_curve: 1.0, ..with(Kind::Strum) }, 4.0, None);
        assert!(late[1].start < 0.125 && late[1].start > 0.0);
    }

    #[test]
    fn humanize_is_the_same_with_the_same_seed() {
        let notes: Vec<Note> = (0..16).map(|i| note(i as f64 * 0.25 + 0.25, 60, 0.2, 0.7)).collect();
        let s = with(Kind::Humanize);
        let a = transform(&notes, &s, 8.0, None);
        assert_eq!(a, transform(&notes, &s, 8.0, None));
        assert_ne!(a, transform(&notes, &Settings { seed: 2, ..s }, 8.0, None));
        for (n, h) in notes.iter().zip(&a) {
            assert!((h.start - n.start).abs() <= s.human_time + 1e-9);
            assert!((h.velocity - n.velocity).abs() <= s.human_velocity + 1e-6);
            assert_eq!(h.length, n.length);
        }
        assert!(a.iter().any(|h| h.start != notes[0].start));
    }

    #[test]
    fn velocities_ramp_compress_and_turn_round() {
        let notes = vec![note(0.0, 60, 0.5, 0.2), note(1.0, 60, 0.5, 0.6), note(2.0, 60, 0.5, 1.0)];
        let ramp = transform(&notes, &Settings { ramp: (1.0, 0.5), ..with(Kind::Velocity) }, 4.0, None);
        assert_eq!(ramp.iter().map(|n| n.velocity).collect::<Vec<_>>(), [1.0, 0.75, 0.5]);
        let squeeze = transform(&notes, &Settings { velocity: VelocityMode::Scale, spread: -0.5, ..with(Kind::Velocity) }, 4.0, None);
        assert!((squeeze[0].velocity - 0.4).abs() < 1e-6 && (squeeze[2].velocity - 0.8).abs() < 1e-6);
        let round = transform(&notes, &Settings { velocity: VelocityMode::Invert, ..with(Kind::Velocity) }, 4.0, None);
        assert!((round[0].velocity - 1.0).abs() < 1e-6 && (round[2].velocity - 0.2).abs() < 1e-6);
    }

    #[test]
    fn legato_reaches_the_next_note_and_scaling_shortens() {
        let notes = vec![note(0.0, 60, 0.25, 0.8), note(0.0, 64, 0.25, 0.8), note(1.0, 62, 0.25, 0.8), note(3.0, 65, 0.25, 0.8)];
        let legato = transform(&notes, &with(Kind::Length), 4.0, None);
        assert_eq!(legato.iter().map(|n| n.length).collect::<Vec<_>>(), [1.0, 1.0, 2.0, 0.25]);
        let short = transform(&notes, &Settings { length: LengthMode::Scale, factor: 0.5, ..with(Kind::Length) }, 4.0, None);
        assert!(short.iter().all(|n| n.length == 0.125));
    }

    #[test]
    fn grace_notes_and_flams_come_before() {
        let notes = vec![note(0.0, 60, 1.0, 0.8), note(1.0, 64, 1.0, 1.0)];
        let grace = transform(&notes, &with(Kind::Ornament), 4.0, None);
        // None before the first (it would start before the clip).
        assert_eq!(grace.len(), 3);
        assert_eq!((grace[2].start, grace[2].pitch, grace[2].length), (0.875, 63, 0.125));
        let key = Scale { tonic: 0, minor: false };
        let in_key = transform(&notes, &with(Kind::Ornament), 4.0, Some(key));
        assert_eq!(in_key[2].pitch, 62, "a step of C major down from E");
        let flam = transform(&notes, &Settings { ornament: Ornament::Flam, grace_time: 0.0625, ..with(Kind::Ornament) }, 4.0, None);
        assert_eq!((flam[2].start, flam[2].pitch), (0.9375, 64));
        assert!(flam[2].velocity < notes[1].velocity);
    }

    #[test]
    fn chords_become_arpeggios() {
        let notes = chord(0.0);
        let up = transform(&notes, &Settings { rate: 0.25, gate: 1.0, ..with(Kind::Arpeggiate) }, 4.0, None);
        assert_eq!(up.iter().map(|n| n.pitch).collect::<Vec<_>>(), [60, 64, 67, 60]);
        assert_eq!(up.iter().map(|n| n.start).collect::<Vec<_>>(), [0.0, 0.25, 0.5, 0.75]);
        let both = transform(&notes, &Settings { pattern: Pattern::UpDown, rate: 0.25, ..with(Kind::Arpeggiate) }, 4.0, None);
        assert_eq!(both.iter().map(|n| n.pitch).collect::<Vec<_>>(), [60, 64, 67, 64]);
        assert!((both[0].length - 0.225).abs() < 1e-9);
    }

    #[test]
    fn connecting_walks_the_scale_in_the_gaps() {
        let notes = vec![note(0.0, 60, 0.25, 0.8), note(1.0, 67, 0.25, 0.8)];
        let key = Scale { tonic: 0, minor: false };
        let joined = transform(&notes, &Settings { rate: 0.25, ..with(Kind::Connect) }, 4.0, Some(key));
        let added: Vec<u8> = joined[2..].iter().map(|n| n.pitch).collect();
        assert_eq!(added, [62, 64, 65], "D, E, F between C and G");
        assert_eq!(joined[2].start, 0.25);
        assert!(joined[2..].iter().all(|n| key.scale()[n.pitch as usize % 12]));
        // Notes that touch: nothing to fill.
        let tight = vec![note(0.0, 60, 1.0, 0.8), note(1.0, 67, 0.25, 0.8)];
        assert_eq!(transform(&tight, &with(Kind::Connect), 4.0, None).len(), 2);
    }

    #[test]
    fn flipping_pitches_and_time() {
        let notes = vec![note(0.0, 60, 0.5, 0.8), note(1.0, 64, 1.0, 0.8), note(2.0, 72, 0.5, 0.8)];
        let upside = transform(&notes, &with(Kind::Flip), 4.0, None);
        assert_eq!(upside.iter().map(|n| n.pitch).collect::<Vec<_>>(), [72, 68, 60]);
        let back = transform(&notes, &Settings { flip: Flip::Time, ..with(Kind::Flip) }, 4.0, None);
        assert_eq!(back.iter().map(|n| n.start).collect::<Vec<_>>(), [2.0, 0.5, 0.0]);
        // Twice is as it was.
        let twice = transform(&back, &Settings { flip: Flip::Time, ..with(Kind::Flip) }, 4.0, None);
        assert_eq!(twice, notes);
    }

    /// The clip editor drawn with a clip of two chords, its menu at hand.
    fn studio() -> crate::produccion::Studio {
        use engine::model::{Clip, MidiClip};
        let mut app = crate::app::PulseApp::headless(crate::project::Project::empty());
        let mut clip = MidiClip::new("Acordes", 4.0, [90, 160, 220]);
        clip.notes = chord(0.0);
        clip.notes.extend(chord(2.0));
        app.set_clip(0, 0, Some(Clip::Midi(clip)));
        app.select_clip(crate::app::ClipRef::Session { track: 0, slot: 0 });
        app.detail = crate::app::Detail::Clip;
        let mut s = crate::produccion::Studio::new(app, std::env::temp_dir());
        s.size = eframe::egui::vec2(1440.0, 900.0);
        s.frames(8);
        s
    }

    fn notes(s: &crate::produccion::Studio) -> Vec<Note> {
        match s.app.clip(crate::app::ClipRef::Session { track: 0, slot: 0 }) {
            Some(engine::model::Clip::Midi(m)) => m.notes.clone(),
            _ => panic!("no clip"),
        }
    }

    /// «Transformar»: choosing one shows it on the clip (not an undo step yet),
    /// «Cancelar» puts the notes back, «Aplicar» keeps them as one step.
    /// `PULSE_SHOTS=dir` leaves pictures of the panel.
    #[test]
    fn the_panel_shows_it_first_and_applies_it_as_one_step() {
        let mut s = studio();
        let before = notes(&s);
        let undoable = s.app.can_undo();
        s.click("Transformar");
        s.click("Arpegiar");
        assert_eq!(notes(&s).len(), 8, "two chords of a half note, in 1/16: eight notes");
        assert!(s.app.piano_roll.transform.trying());
        s.frames(4);
        assert_eq!(s.app.can_undo(), undoable, "not a step while it is tried");
        if let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) {
            std::fs::create_dir_all(&dir).unwrap();
            s.out = dir;
            s.shot("transformar-arpegiar");
        }
        s.click("Cancelar");
        assert_eq!(notes(&s), before);
        assert!(!s.app.piano_roll.transform.trying());
        s.frames(4);
        assert_eq!(s.app.can_undo(), undoable, "cancelled: nothing to undo");

        s.click("Transformar");
        s.click("Rasgueo");
        s.click("Aplicar");
        let strummed = notes(&s);
        assert_ne!(strummed, before);
        assert_eq!(strummed.len(), 6);
        s.frames(4);
        assert!(s.app.can_undo());
        s.app.undo();
        assert_eq!(notes(&s), before, "one ⌘Z: as it was");

        // Closed by clicking elsewhere: as it was too.
        s.click("Transformar");
        s.click("Invertir");
        assert_ne!(notes(&s), before);
        s.click_at(eframe::egui::pos2(700.0, 120.0));
        s.frames(3);
        assert_eq!(notes(&s), before);
    }

    #[test]
    fn apply_takes_only_the_targets_and_returns_the_new_ones() {
        let mut notes = vec![note(0.0, 48, 1.0, 0.8)];
        notes.extend(chord(1.0));
        let added = apply(&mut notes, &[1, 2, 3], &Settings { rate: 0.5, ..with(Kind::Arpeggiate) }, 4.0, None);
        assert_eq!(notes[0].pitch, 48);
        assert_eq!(added, (1..3).collect());
        assert_eq!(notes.len(), 3);
    }
}
