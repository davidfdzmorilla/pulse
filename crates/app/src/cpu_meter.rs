//! The audio's load and its dropouts in the transport bar («CPU 23 %», «2
//! cortes»), and on a click what each track, instrument, effect and plugin
//! takes. The engine measures (see `engine::cpu`); this reads its counters
//! twice a second and turns them into shares of real time.

use crate::app::PulseApp;
use crate::i18n::{fill, tr};
use crate::theme::{self, TEXT, TEXT_DIM};
use eframe::egui::{self, vec2, Align2, RichText, Sense};
use engine::cpu::Load;
use engine::{SharedState, Target};

/// How often the readout changes (seconds).
const EVERY: f64 = 0.5;

/// What the meter shows and remembers between reads.
#[derive(Default)]
pub struct CpuMeter {
    /// The load last read: its mean and the highest since the read before.
    pub load: Load,
    last: Option<f64>,
    /// Dropouts counted when they were last cleared.
    pub cleared: u64,
    pub dropouts: u64,
    /// The detail window is open (the engine times each part meanwhile).
    pub open: bool,
    snapshot: Option<Snapshot>,
    /// What each part took over the last read, as shares of real time.
    pub shares: Shares,
}

/// The engine's counters at one moment (nanoseconds spent so far).
#[derive(Clone, Default)]
struct Snapshot {
    at: f64,
    tracks: Vec<(u64, u64, Vec<u64>)>,
    returns: Vec<(u64, Vec<u64>)>,
    master: (u64, Vec<u64>),
}

/// Shares of real time (1: all of it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shares {
    /// Each track: (index, whole, instrument, each device).
    pub tracks: Vec<(usize, f32, f32, Vec<f32>)>,
    pub returns: Vec<(usize, f32, Vec<f32>)>,
    pub master: (f32, Vec<f32>),
}

fn snapshot(state: &SharedState, at: f64, chains: &[usize], returns: &[usize], master: usize) -> Snapshot {
    let cpu = &state.cpu;
    let devices = |target: Target, n: usize| (0..n).map(|d| cpu.device(target, d)).collect::<Vec<u64>>();
    Snapshot {
        at,
        tracks: chains.iter().enumerate().map(|(t, &n)| (cpu.track(t), cpu.instrument(t), devices(Target::Track(t), n))).collect(),
        returns: returns.iter().enumerate().map(|(r, &n)| (cpu.send_return(r), devices(Target::Return(r), n))).collect(),
        master: (cpu.master(), devices(Target::Master, master)),
    }
}

impl CpuMeter {
    /// Reads the engine now and then (`now`: seconds). `chains`: how many
    /// devices each track has; `returns`, each return; `master`, the master.
    pub fn update(&mut self, state: &SharedState, now: f64, chains: &[usize], returns: &[usize], master: usize) {
        state.cpu.set_detail(self.open);
        if self.last.is_some_and(|last| now - last < EVERY && now >= last) {
            return;
        }
        self.last = Some(now);
        self.load = state.cpu.take_load();
        self.dropouts = state.cpu.dropouts().saturating_sub(self.cleared);
        if !self.open {
            self.snapshot = None;
            return;
        }
        let next = snapshot(state, now, chains, returns, master);
        if let Some(before) = self.snapshot.take().filter(|b| b.tracks.len() == next.tracks.len() && b.returns.len() == next.returns.len() && next.at > b.at) {
            let seconds = (next.at - before.at) * 1e9;
            let share = |a: u64, b: u64| (b.saturating_sub(a) as f64 / seconds) as f32;
            let devices = |a: &[u64], b: &[u64]| b.iter().enumerate().map(|(i, &x)| share(a.get(i).copied().unwrap_or(x), x)).collect::<Vec<f32>>();
            self.shares = Shares {
                tracks: before.tracks.iter().zip(&next.tracks).enumerate().map(|(t, (a, b))| (t, share(a.0, b.0), share(a.1, b.1), devices(&a.2, &b.2))).collect(),
                returns: before.returns.iter().zip(&next.returns).enumerate().map(|(r, (a, b))| (r, share(a.0, b.0), devices(&a.1, &b.1))).collect(),
                master: (share(before.master.0, next.master.0), devices(&before.master.1, &next.master.1)),
            };
        }
        self.snapshot = Some(next);
    }

    /// The tracks taking the most, the heaviest first (those over 0,1 %).
    pub fn heaviest(&self) -> Vec<&(usize, f32, f32, Vec<f32>)> {
        let mut tracks: Vec<_> = self.shares.tracks.iter().filter(|t| t.1 > 0.001).collect();
        tracks.sort_by(|a, b| b.1.total_cmp(&a.1));
        tracks
    }
}

/// «23 %» (one decimal under 10).
pub fn percent(share: f32) -> String {
    let p = share * 100.0;
    // A decimal comma in Spanish, a point in English.
    if p >= 10.0 {
        format!("{p:.0} %")
    } else if crate::i18n::current() == crate::i18n::Language::Spanish {
        format!("{p:.1} %").replace('.', ",")
    } else {
        format!("{p:.1} %")
    }
}

fn load_color(load: f32) -> egui::Color32 {
    if load >= 0.9 {
        theme::METER_RED
    } else if load >= 0.7 {
        theme::ACCENT
    } else {
        TEXT_DIM
    }
}

/// The readout in the transport bar: the load with a small bar, and the
/// dropouts if there were any. A click opens the detail.
pub fn show(app: &mut PulseApp, ui: &mut egui::Ui) {
    if let Some(state) = app.audio.state() {
        let chains: Vec<usize> = app.project.tracks.iter().map(|t| t.devices.len()).collect();
        let returns: Vec<usize> = app.project.returns.iter().map(|r| r.devices.len()).collect();
        let now = ui.input(|i| i.time);
        app.cpu.update(&state, now, &chains, &returns, app.project.master.devices.len());
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(EVERY));
    }
    let meter = &app.cpu;
    let (mean, peak) = (meter.load.mean, meter.load.peak.max(meter.load.mean));
    let text = format!("CPU {}", percent(mean).replace(",0 ", " ").replace(".0 ", " "));
    let width = 96.0 + if meter.dropouts > 0 { 64.0 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, if response.hovered() { theme::CONTROL_HOVER } else { theme::BG });
    let bar = egui::Rect::from_min_size(rect.left_center() + vec2(6.0, -3.0), vec2(22.0, 6.0));
    painter.rect_filled(bar, 1.0, theme::CONTROL);
    let lit = egui::Rect::from_min_size(bar.min, vec2(bar.width() * mean.clamp(0.0, 1.0), bar.height()));
    painter.rect_filled(lit, 1.0, load_color(mean.max(peak * 0.8)));
    // The peak: a tick.
    let x = bar.left() + bar.width() * peak.clamp(0.0, 1.0);
    painter.line_segment([egui::pos2(x, bar.top() - 1.0), egui::pos2(x, bar.bottom() + 1.0)], egui::Stroke::new(1.5, load_color(peak)));
    painter.text(egui::pos2(bar.right() + 6.0, rect.center().y), Align2::LEFT_CENTER, &text, egui::FontId::proportional(11.0), if mean >= 0.7 { load_color(mean) } else { TEXT });
    if meter.dropouts > 0 {
        let cuts = if meter.dropouts == 1 { tr("1 corte").to_string() } else { fill(tr("{} cortes"), &[&meter.dropouts]) };
        painter.text(rect.right_center() - vec2(6.0, 0.0), Align2::RIGHT_CENTER, cuts, egui::FontId::proportional(11.0), theme::METER_RED);
    }
    let label = format!("Carga del audio · {text} · pico {} · {} cortes", percent(peak), meter.dropouts);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    let hint = fill(
        tr("Carga del audio: el tiempo que tarda Pulse en hacer cada trozo de sonido, sobre lo que dura. Media {}, pico {} en el último medio segundo. Cerca del 100 % el sonido se corta: sube el búfer (Ajustes → Audio), congela pistas o quita plugins pesados.\nCortes: {} (trozos que llegaron tarde, desde que los pusiste a cero).\nClic: lo que se lleva cada pista y cada plugin"),
        &[&percent(mean), &percent(peak), &meter.dropouts],
    );
    if response.on_hover_text(hint).clicked() {
        app.cpu.open = !app.cpu.open;
    }
}

/// The detail: each track's share (its instrument's and each device's), the
/// returns' and the master's, the heaviest first.
pub fn window(app: &mut PulseApp, ctx: &egui::Context) {
    if !app.cpu.open {
        return;
    }
    let mut open = true;
    let mut clear = false;
    egui::Window::new(tr("Rendimiento")).open(&mut open).resizable(true).default_width(380.0).pivot(Align2::RIGHT_TOP).default_pos(ctx.content_rect().right_top() + vec2(-16.0, 140.0)).show(ctx, |ui| {
        let meter = &app.cpu;
        ui.label(fill(tr("Carga del audio: media {} · pico {}"), &[&percent(meter.load.mean), &percent(meter.load.peak.max(meter.load.mean))]));
        ui.horizontal(|ui| {
            ui.label(RichText::new(fill(tr("Cortes: {}"), &[&meter.dropouts])).color(if meter.dropouts > 0 { theme::METER_RED } else { TEXT }));
            clear = ui.small_button(tr("Poner a cero")).on_hover_text(tr("Vuelve a contar los cortes desde ahora")).clicked();
        });
        ui.label(RichText::new(tr("Lo que se lleva cada parte del tiempo real (cada plugin suena en el hilo de audio, uno tras otro)")).color(TEXT_DIM).size(10.5));
        ui.separator();
        let heaviest = meter.heaviest();
        if heaviest.is_empty() {
            ui.label(RichText::new(tr("Midiendo… (dale a play para ver lo que pesa cada pista)")).color(TEXT_DIM));
        }
        let device_name = |devices: &[engine::model::Device], d: usize| match devices.get(d).map(|d| d.effect) {
            Some(engine::model::Effect::Plugin(id)) => format!("{} (plugin)", app.plugin_name(id)),
            Some(effect) => tr(effect.label()).to_string(),
            None => "?".into(),
        };
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            egui::Grid::new("cpu_detail").num_columns(2).spacing([16.0, 2.0]).striped(true).show(ui, |ui| {
                for (t, whole, instrument, devices) in heaviest {
                    let Some(track) = app.project.tracks.get(*t) else { continue };
                    ui.label(RichText::new(&track.name).strong());
                    ui.label(percent(*whole));
                    ui.end_row();
                    if *instrument > 0.001 {
                        let name = match track.instrument {
                            engine::model::InstrumentKind::Plugin(id) => format!("{} (plugin)", app.plugin_name(id)),
                            kind => kind.label().to_string(),
                        };
                        ui.label(RichText::new(format!("   {name}")).color(TEXT_DIM));
                        ui.label(RichText::new(percent(*instrument)).color(TEXT_DIM));
                        ui.end_row();
                    }
                    for (d, share) in devices.iter().enumerate().filter(|(_, s)| **s > 0.001) {
                        ui.label(RichText::new(format!("   {}", device_name(&track.devices, d))).color(TEXT_DIM));
                        ui.label(RichText::new(percent(*share)).color(TEXT_DIM));
                        ui.end_row();
                    }
                }
                for (r, whole, devices) in meter.shares.returns.iter().filter(|r| r.1 > 0.001) {
                    let Some(ret) = app.project.returns.get(*r) else { continue };
                    ui.label(RichText::new(&ret.name).strong());
                    ui.label(percent(*whole));
                    ui.end_row();
                    for (d, share) in devices.iter().enumerate().filter(|(_, s)| **s > 0.001) {
                        ui.label(RichText::new(format!("   {}", device_name(&ret.devices, d))).color(TEXT_DIM));
                        ui.label(RichText::new(percent(*share)).color(TEXT_DIM));
                        ui.end_row();
                    }
                }
                if meter.shares.master.0 > 0.001 {
                    ui.label(RichText::new("Master").strong());
                    ui.label(percent(meter.shares.master.0));
                    ui.end_row();
                }
            });
        });
    });
    if clear {
        app.cpu.cleared += app.cpu.dropouts;
        app.cpu.dropouts = 0;
    }
    if !open {
        app.cpu.open = false;
        if let Some(state) = app.audio.state() {
            state.cpu.set_detail(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slow plugin on a track (the test gain at 16 kHz takes 2 ms a
    /// block): the readout's load and the detail's shares say so, that track
    /// and that plugin first; dropouts count until put back to zero. Pictures
    /// (`PULSE_SHOTS=dir`, else the temporary folder) of the bar and the window.
    #[test]
    fn the_load_and_what_each_track_takes() {
        let (audio, mut engine) = crate::app::Audio::detached_at(16_000.0);
        let mut app = PulseApp::headless(crate::project::Project::demo());
        app.audio = audio;
        app.plugin_library.search(&[crate::plugins::tests::test_bundle().parent().unwrap().to_path_buf()]);
        let gain = app.plugin_library.found.iter().find(|p| !p.instrument).unwrap().clone();
        app.sync_all();
        app.add_plugin_effect(Target::Track(1), &gain);
        app.cpu.open = true;
        let state = app.audio.state().unwrap();
        let chains: Vec<usize> = app.project.tracks.iter().map(|t| t.devices.len()).collect();
        app.cpu.update(&state, 0.0, &chains, &[], 0);
        assert!(state.cpu.detail(), "open, the engine times each part");
        // Half a second of sound, in callbacks of 16 ms.
        let mut out = vec![0f32; 256 * 2];
        for _ in 0..31 {
            engine.process(&mut out, 2);
            app.audio.collect_garbage();
        }
        app.cpu.update(&state, 0.2, &chains, &[], 0);
        assert!(app.cpu.shares.tracks.is_empty(), "too soon: the same reading");
        app.cpu.update(&state, 0.6, &chains, &[], 0);
        let heaviest = app.cpu.heaviest();
        assert_eq!(heaviest.first().map(|t| t.0), Some(1), "{:?}", app.cpu.shares.tracks);
        let plugin = *heaviest[0].3.last().unwrap();
        assert!(plugin > 0.25, "about 2 ms of every 4: {plugin}");
        assert!(app.cpu.load.mean > 0.1, "{:?}", app.cpu.load);
        assert_eq!(percent(0.234), "23 %");
        assert_eq!(percent(0.0456), "4,6 %");
        // Callbacks of 1 ms: each one late, a dropout.
        let mut out = vec![0f32; 16 * 2];
        for _ in 0..5 {
            engine.process(&mut out, 2);
        }
        app.cpu.update(&state, 1.2, &chains, &[], 0);
        assert!(app.cpu.dropouts >= 5, "{}", app.cpu.dropouts);

        // The bar and the window, drawn.
        let dir = std::env::var_os("PULSE_SHOTS").map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("pulse-cpu"));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let size = vec2(1200.0, 520.0);
        let mut canvas = crate::offscreen::Canvas::default();
        let mut output = None;
        for frame in 0..4 {
            ctx.set_pixels_per_point(2.0);
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), time: Some(1.3 + frame as f64 * 0.01), ..Default::default() };
            output = Some(ctx.run_ui(input, |ui| {
                egui::Panel::top("t").show(ui, |ui| crate::transport::show(&mut app, ui));
                window(&mut app, ui.ctx());
            }));
            if frame < 3 {
                canvas.draw(&ctx, output.take().unwrap(), size, 2.0);
            }
        }
        canvas.draw(&ctx, output.unwrap(), size, 2.0).save_png(&dir.join("cpu.png")).unwrap();
        // Put back to zero; closed, no more timing.
        app.cpu.cleared += app.cpu.dropouts;
        app.cpu.open = false;
        app.cpu.update(&state, 2.0, &chains, &[], 0);
        assert_eq!(app.cpu.dropouts, 0);
        assert!(!state.cpu.detail());
    }
}
