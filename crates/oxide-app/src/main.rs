//! The Oxide desktop app: a workspace around the paint engine.

mod app;
// Shared with the AI-agent server and CLI (see crates/oxide-commands).
use oxide_commands::{export, tools};
mod color_wheel;
mod palette_library;
mod pen;
mod presets;
mod settings;
mod smoothing;
mod tiles;

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
