//! The Oxide desktop app: a painting workspace around the paint engine.

mod app;
// Shared with the AI-agent server and CLI (see crates/oxide-commands).
use oxide_commands::{export, tools};
mod color_wheel;
mod open_files;
mod palette_library;
mod panel_layout;
mod pen;
mod presets;
mod quick_panel;
mod settings;
mod smoothing;
mod theme;
mod tiles;

use eframe::egui;

fn main() -> eframe::Result {
    // Before the app finishes launching, so a double-clicked painting that starts Oxide isn't missed.
    open_files::install();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Oxide")
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            // An empty icon tells eframe not to put its own (the egui logo) on the
            // Dock while running, so macOS shows Oxide.app's icon, as Spotlight does.
            .with_icon(egui::IconData::default()),
        ..Default::default()
    };
    eframe::run_native("Oxide", options, Box::new(|cc| Ok(Box::new(app::OxideApp::new(cc)))))
}
