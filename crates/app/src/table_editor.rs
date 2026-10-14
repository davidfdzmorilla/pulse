//! Wavetable editor: draw a few keyframes by hand; the table morphs between them.
//! The result is saved as a standard wavetable WAV (2048-sample frames with a
//! `clm ` chunk), so it also opens in other wavetable synths, and is loaded into
//! the oscillator it was opened from.

use crate::app::PulseApp;
use crate::theme::{self, ACCENT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};
use engine::model::{InstrumentKind, OscKind, WAVE_USER_TABLE};
use engine::wavetable::{FRAME, PREVIEW_POINTS};
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

/// Points drawn per keyframe.
pub const POINTS: usize = PREVIEW_POINTS;
pub const MAX_KEYS: usize = 16;
/// Frames in a saved table.
pub const TABLE_FRAMES: usize = 64;

pub struct TableEditor {
    pub track: usize,
    pub osc: usize,
    pub name: String,
    pub keys: Vec<Vec<f32>>,
    pub selected: usize,
    /// Last point drawn while dragging, to join strokes without gaps.
    last: Option<(usize, f32)>,
}

pub fn shape(kind: &str) -> Vec<f32> {
    (0..POINTS)
        .map(|i| {
            let p = i as f32 / POINTS as f32;
            match kind {
                "Seno" => (p * TAU).sin(),
                "Triángulo" => 1.0 - 4.0 * (((p + 0.25) % 1.0) - 0.5).abs(),
                "Sierra" => 2.0 * ((p + 0.5) % 1.0) - 1.0,
                "Cuadrada" => if p < 0.5 { 1.0 } else { -1.0 },
                "Ruido" => ((i as f32 * 12.9898).sin() * 43_758.547).fract() * 2.0 - 1.0,
                _ => 0.0,
            }
        })
        .collect()
}

/// Opens the editor with the oscillator's current table as keyframes.
pub fn open(app: &mut PulseApp, track: usize, osc: usize) {
    let keys = match app.cached_wavetable(track, osc) {
        Some(table) => {
            let frames = table.preview.len().max(1);
            let count = frames.min(4);
            (0..count).map(|k| table.preview[k * (frames - 1) / (count - 1).max(1)].clone()).collect()
        }
        None => vec![shape("Seno"), shape("Sierra")],
    };
    let name = format!("{} {}", app.project.tracks[track].name, if osc == 0 { "A" } else { "B" });
    app.table_editor = Some(TableEditor { track, osc, name, keys, selected: 0, last: None });
}

/// The table's frames: keyframes spread evenly, each step a linear morph,
/// every frame resampled to [`FRAME`] samples.
pub fn frames(keys: &[Vec<f32>]) -> Vec<Vec<f32>> {
    let keys: Vec<&Vec<f32>> = keys.iter().filter(|k| !k.is_empty()).collect();
    if keys.is_empty() {
        return vec![vec![0.0; FRAME]];
    }
    let count = if keys.len() == 1 { 1 } else { TABLE_FRAMES };
    (0..count)
        .map(|f| {
            let place = if count == 1 { 0.0 } else { f as f32 / (count - 1) as f32 * (keys.len() - 1) as f32 };
            let (a, b) = (place.floor() as usize, (place.floor() as usize + 1).min(keys.len() - 1));
            let t = place - place.floor();
            (0..FRAME)
                .map(|i| {
                    let x = i as f32 / FRAME as f32 * POINTS as f32;
                    let (j, k) = (x as usize % POINTS, (x as usize + 1) % POINTS);
                    let u = x - x.floor();
                    let at = |key: &Vec<f32>| key[j] + (key[k] - key[j]) * u;
                    at(keys[a]) + (at(keys[b]) - at(keys[a])) * t
                })
                .collect()
        })
        .collect()
}

/// Writes frames as a wavetable WAV: mono 32-bit float with a `clm ` chunk.
pub fn write_table(path: &Path, frames: &[Vec<f32>]) -> Result<(), String> {
    let data: Vec<f32> = frames.iter().flatten().copied().collect();
    crate::capture::write_wav(path, &data, &data, 48_000.0)?;
    let mut bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = format!("<!>{FRAME} 00000000 wavetable (Pulse)");
    bytes.extend_from_slice(b"clm ");
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    if text.len() % 2 == 1 {
        bytes.push(0);
    }
    let riff = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff.to_le_bytes());
    std::fs::write(path, bytes).map_err(|e| format!("No se pudo guardar {}: {e}", path.display()))
}

fn tables_dir() -> PathBuf {
    crate::kits::kits_dir().with_file_name("Wavetables")
}

fn save_and_use(app: &mut PulseApp) {
    let Some(editor) = &app.table_editor else { return };
    let (track, osc) = (editor.track, editor.osc);
    let clean: String = editor.name.chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    let dir = tables_dir();
    let path = dir.join(format!("{}.wav", clean.trim()));
    let result = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|_| write_table(&path, &frames(&editor.keys)));
    if let Err(error) = result {
        app.status = Some(error);
        return;
    }
    let path = path.to_string_lossy().into_owned();
    app.samples.forget(&path);
    let Some(InstrumentKind::Wavetable(mut params)) = app.project.tracks.get(track).map(|t| t.instrument) else { return };
    params.osc[osc].kind = OscKind::Wavetable;
    params.osc[osc].table = WAVE_USER_TABLE;
    app.project.tracks[track].set_sample(osc, Some(path.clone()));
    app.set_instrument(track, InstrumentKind::Wavetable(params));
    app.sync_samples(track);
    app.status = Some(format!("Wavetable guardada y cargada: {path}"));
}

fn thumbnail(ui: &mut egui::Ui, key: &[f32], selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(84.0, 40.0), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, if selected { theme::CONTROL_HOVER } else { theme::BG });
    let points: Vec<_> = key.iter().enumerate().map(|(i, v)| pos2(rect.left() + i as f32 / POINTS as f32 * rect.width(), rect.center().y - v * rect.height() * 0.42)).collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.0, if selected { ACCENT } else { Color32::from_gray(150) })));
    response
}

/// The drawing area: drag to draw the selected keyframe.
fn canvas(ui: &mut egui::Ui, editor: &mut TableEditor) {
    let (rect, response) = ui.allocate_exact_size(vec2(560.0, 240.0), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    painter.line_segment([pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)], Stroke::new(1.0, Color32::from_gray(45)));
    for q in 1..4 {
        let x = rect.left() + q as f32 / 4.0 * rect.width();
        painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, Color32::from_gray(34)));
    }
    // Neighbouring keyframes faintly behind, for reference.
    for (_, key) in editor.keys.iter().enumerate().filter(|(k, _)| k.abs_diff(editor.selected) == 1) {
        let points: Vec<_> = key.iter().enumerate().map(|(i, v)| pos2(rect.left() + i as f32 / POINTS as f32 * rect.width(), rect.center().y - v * rect.height() * 0.45)).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, Color32::from_gray(70))));
    }
    let Some(key) = editor.keys.get_mut(editor.selected) else { return };
    if let Some(pos) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked() || response.drag_started()) {
        let index = (((pos.x - rect.left()) / rect.width() * POINTS as f32) as isize).clamp(0, POINTS as isize - 1) as usize;
        let value = ((rect.center().y - pos.y) / (rect.height() * 0.45)).clamp(-1.0, 1.0);
        // Fill every point between the last one and this, so fast strokes leave no gaps.
        let (from, from_value) = editor.last.filter(|_| !response.drag_started()).unwrap_or((index, value));
        let (a, b) = (from.min(index), from.max(index));
        for i in a..=b {
            let t = if b == a { 1.0 } else { (i - a) as f32 / (b - a) as f32 };
            let t = if from <= index { t } else { 1.0 - t };
            key[i] = from_value + (value - from_value) * t;
        }
        editor.last = Some((index, value));
    }
    if response.drag_stopped() || !response.dragged() {
        editor.last = None;
    }
    let points: Vec<_> = key.iter().enumerate().map(|(i, v)| pos2(rect.left() + (i as f32 + 0.5) / POINTS as f32 * rect.width(), rect.center().y - v * rect.height() * 0.45)).collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, ACCENT)));
    response.on_hover_text("Arrastra para dibujar el fotograma");
}

/// The morph between keyframes as stacked frames.
fn preview(ui: &mut egui::Ui, keys: &[Vec<f32>]) {
    let (rect, _) = ui.allocate_exact_size(vec2(300.0, 150.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, theme::BG);
    let frames = frames(keys);
    let shown = frames.len().min(16);
    let depth = vec2(rect.width() * 0.25, -rect.height() * 0.4);
    let front = Rect::from_min_max(rect.min + vec2(4.0, -depth.y + 4.0), rect.max - vec2(depth.x + 4.0, 4.0));
    for k in (0..shown).rev() {
        let f = k * (frames.len() - 1) / (shown - 1).max(1);
        let along = k as f32 / (shown - 1).max(1) as f32;
        let points: Vec<_> = (0..64)
            .map(|i| {
                let v = frames[f][i * FRAME / 64];
                pos2(front.left() + i as f32 / 63.0 * front.width(), front.center().y - v * front.height() * 0.45) + depth * along
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.0, if k == 0 { ACCENT } else { Color32::from_gray(60 + (k * 6) as u8) })));
    }
}

pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if app.table_editor.is_none() {
        return;
    }
    let mut open = true;
    let mut save = false;
    egui::Window::new("Editor de wavetables").id(egui::Id::new("table_editor")).open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
        let Some(editor) = app.table_editor.as_mut() else { return };
        ui.horizontal(|ui| {
            ui.label("Nombre");
            ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(200.0));
            ui.label(egui::RichText::new(format!("Oscilador {}", if editor.osc == 0 { "A" } else { "B" })).color(TEXT_DIM));
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("FOTOGRAMAS CLAVE").color(TEXT_DIM).size(10.0).strong());
                egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                    let mut select = None;
                    for (k, key) in editor.keys.iter().enumerate() {
                        if thumbnail(ui, key, k == editor.selected).clicked() {
                            select = Some(k);
                        }
                    }
                    if let Some(k) = select {
                        editor.selected = k;
                    }
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(editor.keys.len() < MAX_KEYS, egui::Button::new("+")).on_hover_text("Duplica el fotograma seleccionado").clicked() {
                        let copy = editor.keys[editor.selected].clone();
                        editor.keys.insert(editor.selected + 1, copy);
                        editor.selected += 1;
                    }
                    if ui.add_enabled(editor.keys.len() > 1, egui::Button::new("−")).on_hover_text("Borra el fotograma seleccionado").clicked() {
                        editor.keys.remove(editor.selected);
                        editor.selected = editor.selected.min(editor.keys.len() - 1);
                    }
                });
            });
            ui.vertical(|ui| {
                canvas(ui, editor);
                ui.horizontal(|ui| {
                    for name in ["Seno", "Triángulo", "Sierra", "Cuadrada", "Ruido", "Silencio"] {
                        if ui.small_button(name).clicked() {
                            editor.keys[editor.selected] = shape(name);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let key = &mut editor.keys[editor.selected];
                    if ui.small_button("Normalizar").clicked() {
                        let peak = key.iter().fold(0f32, |m, v| m.max(v.abs()));
                        if peak > 1e-6 {
                            key.iter_mut().for_each(|v| *v /= peak);
                        }
                    }
                    if ui.small_button("Suavizar").clicked() {
                        let copy = key.clone();
                        for i in 0..POINTS {
                            key[i] = (copy[(i + POINTS - 1) % POINTS] + 2.0 * copy[i] + copy[(i + 1) % POINTS]) / 4.0;
                        }
                    }
                    if ui.small_button("Invertir").clicked() {
                        key.iter_mut().for_each(|v| *v = -*v);
                    }
                    if ui.small_button("Espejo").on_hover_text("La segunda mitad repite la primera al revés").clicked() {
                        for i in POINTS / 2..POINTS {
                            key[i] = -key[POINTS - 1 - i];
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("RESULTADO").color(TEXT_DIM).size(10.0).strong());
                preview(ui, &editor.keys);
                ui.label(egui::RichText::new(format!("{} fotogramas, morph entre las claves", if editor.keys.len() == 1 { 1 } else { TABLE_FRAMES })).color(TEXT_DIM).size(10.5));
                ui.add_space(8.0);
                save = ui.button("Guardar y usar en el oscilador").on_hover_text("Se guarda como WAV de wavetable en Application Support/Pulse/Wavetables").clicked();
            });
        });
    });
    if save {
        save_and_use(app);
    }
    if !open {
        app.table_editor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframes_morph_into_a_table_that_loads_back() {
        let keys = vec![shape("Seno"), shape("Cuadrada")];
        let frames = frames(&keys);
        assert_eq!(frames.len(), TABLE_FRAMES);
        assert!((frames[0][FRAME / 4] - 1.0).abs() < 0.01, "starts as the first key");
        assert!((frames[TABLE_FRAMES - 1][FRAME / 8] - 1.0).abs() < 0.01, "ends as the last key");
        let middle = frames[TABLE_FRAMES / 2][FRAME / 8];
        assert!(middle > (TAU / 8.0).sin() && middle < 1.0, "in between: {middle}");

        let dir = std::env::temp_dir().join(format!("pulse-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drawn.wav");
        write_table(&path, &frames).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(engine::wavetable::declared_frame_size(&bytes), Some(FRAME));
        let mut cache = crate::sync::SampleCache::default();
        let table = cache.table(&path.to_string_lossy());
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(table.frames(), TABLE_FRAMES);
        assert!((table.read(0, 0.0, 0.25) - 1.0).abs() < 0.05);
    }

    #[test]
    fn a_single_key_is_a_single_cycle() {
        assert_eq!(frames(&[shape("Sierra")]).len(), 1);
    }
}
