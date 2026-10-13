//! The Oxide desktop app: a workspace around the paint engine.

mod app;
mod color_wheel;
mod export;
mod palette_library;
mod pen;
mod presets;
mod settings;
mod tiles;
mod tools;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oxide")
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Oxide",
        options,
        Box::new(|cc| Ok(Box::new(app::OxideApp::new(&cc.egui_ctx)))),
    )
}
