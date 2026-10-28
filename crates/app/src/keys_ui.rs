//! Pulse Keys' device: the model, its controls and the organ's drawbars.

use crate::theme::{self, ACCENT, TEXT_DIM};
use eframe::egui::{self, pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use engine::model::*;

/// The controls each model shows, by rows (indices into [`KEYS_PARAMS`]).
pub(crate) fn rows(model: KeysModel) -> &'static [&'static [usize]] {
    match model {
        KeysModel::Piano => &[&[KEYS_TONE, KEYS_LENGTH, KEYS_RELEASE, KEYS_DYNAMICS], &[KEYS_HAMMER, KEYS_STRINGS, KEYS_BODY, KEYS_WIDTH, KEYS_DRIVE, KEYS_GAIN]],
        KeysModel::Tines | KeysModel::Reed => &[
            &[KEYS_TONE, KEYS_BELL, KEYS_BARK, KEYS_LENGTH, KEYS_RELEASE, KEYS_DYNAMICS],
            &[KEYS_HAMMER, KEYS_TREMOLO_RATE, KEYS_TREMOLO, KEYS_DRIVE, KEYS_WIDTH, KEYS_GAIN],
        ],
        KeysModel::Organ => &[&[KEYS_PERCUSSION, KEYS_PERC_DECAY, KEYS_CLICK, KEYS_TONE], &[KEYS_LESLIE, KEYS_DRIVE, KEYS_WIDTH, KEYS_GAIN]],
    }
}

/// What each knob does, for its tooltip.
pub(crate) fn hint(index: usize, model: KeysModel) -> &'static str {
    match index {
        KEYS_TONE if model == KeysModel::Piano => "Lo duros que son los martillos: más brillo cuanto más fuerte se toca",
        KEYS_TONE if model == KeysModel::Organ => "Filtro de agudos del órgano",
        KEYS_TONE => "Lo abierto que suena la pastilla",
        KEYS_LENGTH => "Cuánto tardan las notas en apagarse solas mientras se mantienen",
        KEYS_RELEASE => "Cuánto suena una nota al soltarla (los apagadores)",
        KEYS_DYNAMICS => "Cuánto cambia el volumen con la fuerza de la nota (0: todas igual)",
        KEYS_HAMMER => "El golpe del martillo al empezar la nota",
        KEYS_STRINGS => "Desafinación entre las cuerdas de cada nota: más, más «de bar»",
        KEYS_BODY => "La resonancia de la tabla armónica y la caja",
        KEYS_BELL => "El brillo de campana al atacar la nota",
        KEYS_BARK => "Lo que gruñe la pastilla en las notas fuertes",
        KEYS_TREMOLO_RATE => "Velocidad del trémolo",
        KEYS_TREMOLO if model == KeysModel::Tines => "Trémolo estéreo: el sonido va de un lado a otro",
        KEYS_TREMOLO => "Trémolo: el volumen sube y baja",
        KEYS_PERCUSSION => "Golpe de percusión al empezar: solo en la primera nota de una ligadura",
        KEYS_PERC_DECAY => "Cuánto dura la percusión",
        KEYS_CLICK => "El chasquido de los contactos al pulsar y soltar",
        KEYS_LESLIE => "Cuánto suena por el altavoz giratorio",
        KEYS_DRIVE => "Saturación del amplificador",
        KEYS_WIDTH => "Anchura estéreo",
        KEYS_GAIN => "Volumen",
        _ => "",
    }
}

const FOOTAGES: [&str; 9] = ["16'", "5⅓'", "8'", "4'", "2⅔'", "2'", "1⅗'", "1⅓'", "1'"];

/// The colour of drawbar `i` on the real organ: brown for the sub-octaves, white for
/// the octaves, black for the fifths and thirds.
fn drawbar_color(i: usize) -> Color32 {
    match i {
        0 | 1 => Color32::from_rgb(0x8a, 0x52, 0x2e),
        2 | 3 | 5 | 8 => Color32::from_rgb(0xe8, 0xe4, 0xda),
        _ => Color32::from_rgb(0x22, 0x22, 0x22),
    }
}

/// One drawbar, pulled down from 0 to 8 in whole steps. Returns a new position.
pub(crate) fn drawbar(ui: &mut egui::Ui, i: usize, value: f32) -> Option<f32> {
    let (rect, response) = ui.allocate_exact_size(vec2(20.0, 104.0), Sense::click_and_drag());
    let slot = Rect::from_min_max(rect.min + vec2(5.0, 0.0), pos2(rect.right() - 5.0, rect.bottom() - 16.0));
    let painter = ui.painter();
    painter.rect_filled(slot, 2.0, theme::BG);
    let step = slot.height() / 8.0;
    for k in 1..8 {
        let y = slot.top() + k as f32 * step;
        painter.line_segment([pos2(slot.left() - 3.0, y), pos2(slot.left() - 1.0, y)], Stroke::new(1.0, Color32::from_gray(70)));
    }
    let color = drawbar_color(i);
    let out = value.clamp(0.0, 8.0);
    // The bar shows as far as it is pulled out, with its knob at the end.
    let bar = Rect::from_min_max(slot.min + vec2(2.0, 0.0), pos2(slot.right() - 2.0, slot.top() + out * step));
    painter.rect_filled(bar, 1.0, color.gamma_multiply(0.75));
    let knob = Rect::from_center_size(pos2(slot.center().x, slot.top() + out * step), vec2(slot.width() + 2.0, 8.0));
    painter.rect_filled(knob, 2.0, color);
    painter.rect_stroke(knob, 2.0, Stroke::new(1.0, if response.hovered() { ACCENT } else { Color32::from_gray(90) }), egui::StrokeKind::Inside);
    let digit = if i >= 2 && i != 4 && i != 6 && i != 7 { Color32::from_gray(40) } else { Color32::from_gray(220) };
    painter.text(knob.center(), Align2::CENTER_CENTER, format!("{}", out.round() as u8), egui::FontId::proportional(7.5), digit);
    painter.text(pos2(rect.center().x, rect.bottom() - 7.0), Align2::CENTER_CENTER, FOOTAGES[i], theme::small(), TEXT_DIM);
    let response = response.on_hover_text(format!("Registro {} · {} de 8 · arrastra hacia abajo para sacarlo", FOOTAGES[i], out.round()));
    if response.double_clicked() {
        return Some(if out > 0.0 { 0.0 } else { 8.0 });
    }
    let pointer = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked())?;
    let to = ((pointer.y - slot.top()) / step).round().clamp(0.0, 8.0);
    (to != out.round()).then_some(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, Track};

    #[test]
    fn every_model_shows_only_its_own_controls() {
        for model in KeysModel::ALL {
            for row in rows(model) {
                for &i in *row {
                    assert!(model.uses(i), "{model:?} shows {}", KEYS_PARAMS[i].name);
                    assert!(!hint(i, model).is_empty(), "{model:?}: {} has a tooltip", KEYS_PARAMS[i].name);
                }
            }
        }
        let shown = |m: KeysModel| rows(m).iter().flat_map(|r| r.iter().copied()).collect::<Vec<_>>();
        // Everything a model uses is on its device (the organ's drawbars and speed apart).
        for model in KeysModel::ALL {
            for i in 0..KEYS_PARAMS.len() {
                let elsewhere = model == KeysModel::Organ && ((KEYS_DRAWBAR..KEYS_DRAWBAR + 9).contains(&i) || i == KEYS_LESLIE_FAST);
                if model.uses(i) && !elsewhere {
                    assert!(shown(model).contains(&i), "{model:?} hides {}", KEYS_PARAMS[i].name);
                }
            }
        }
    }

    /// Renders the device for every model to PNG files, to look at:
    /// `PULSE_SHOTS=dir cargo test --release -p app keys_shots -- --ignored`.
    #[test]
    #[ignore]
    fn keys_shots() {
        let Some(dir) = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for model in KeysModel::ALL {
            let ctx = egui::Context::default();
            theme::apply(&ctx);
            let mut project = Project::empty();
            project.tracks.clear();
            project.tracks.push(Track::new("Teclas", [200, 120, 60], InstrumentKind::Keys(KeysParams::presets().into_iter().find(|(_, p)| p.model == model).unwrap().1)));
            let mut app = crate::app::PulseApp::headless(project);
            app.selected_target = engine::Target::Track(0);
            let mut canvas = crate::offscreen::Canvas::default();
            let size = vec2(1000.0, 200.0);
            let input = || egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
            let mut output = None;
            for frame in 0..4 {
                ctx.set_pixels_per_point(2.0);
                output = Some(ctx.run_ui(input(), |ui| {
                    egui::CentralPanel::default().show(ui, |ui| crate::devices::show(&mut app, ui));
                }));
                if frame < 3 {
                    canvas.draw(&ctx, output.take().unwrap(), size, 2.0);
                }
            }
            let image = canvas.draw(&ctx, output.unwrap(), size, 2.0);
            image.save_png(&dir.join(format!("keys-{}.png", model.label()))).unwrap();
        }
    }
}
