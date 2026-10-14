mod app;
mod device;
mod piano_roll;
mod project;
mod session;
mod transport;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Pulse")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Pulse",
        options,
        Box::new(|cc| Ok(Box::new(app::PulseApp::new(cc)))),
    )
}
