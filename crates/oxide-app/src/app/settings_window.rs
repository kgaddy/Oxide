//! Settings (⌘,): what each tool starts with when Oxide opens.

use eframe::egui;
use oxide_engine::PaintMode;

use super::OxideApp;
use super::panels::{
    brush_combo, flat_checkbox, icon_text, loading_slider, oiliness_slider, opacity_slider, size_slider, taper_controls, thickness_slider, tool_icon,
};
use crate::settings::ToolStart;
use crate::tools::{Tool, ToolState};

pub(super) struct SettingsWindow {
    pub open: bool,
    /// The tool whose starting values are shown.
    tool: Tool,
    /// Changed since the settings file was last written (written once the
    /// mouse is up, not on every frame of a slider drag).
    unsaved: bool,
}

impl Default for SettingsWindow {
    fn default() -> Self {
        SettingsWindow { open: false, tool: Tool::OilsAcrylics, unsaved: false }
    }
}

/// Tools with starting values (Select and the Picker have no brush).
fn has_start(tool: Tool) -> bool {
    !matches!(tool, Tool::Select | Tool::Picker)
}

impl OxideApp {
    /// Open Settings on the tool in hand.
    pub(super) fn show_settings(&mut self) {
        self.settings.open = true;
        if has_start(self.tool) {
            self.settings.tool = self.tool;
        }
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings.open;
        egui::Window::new("Settings").open(&mut open).resizable(false).default_width(340.0).show(ctx, |ui| {
            ui.strong("Color picker");
            ui.horizontal(|ui| {
                use crate::settings::ColorStyle;
                let style = &mut self.app_settings.color_style;
                let mut changed =
                    ui.selectable_value(style, ColorStyle::Wheel, "Wheel").on_hover_text("A hue ring with a saturation/brightness square inside").changed();
                changed |= ui
                    .selectable_value(style, ColorStyle::Classic, "Classic")
                    .on_hover_text("A saturation/brightness square with hue, saturation and brightness sliders")
                    .changed();
                if changed {
                    self.settings.unsaved = true;
                }
            });
            ui.separator();
            ui.strong("Tool starting values");
            ui.weak("What each tool starts with when Oxide opens. The tool you're painting with doesn't change until you click Use Now.");
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for tool in Tool::ALL.into_iter().filter(|t| has_start(*t)) {
                    ui.selectable_value(&mut self.settings.tool, tool, icon_text(tool_icon(tool))).on_hover_text(tool.label());
                }
            });
            ui.separator();
            self.tool_start_editor(ui);
        });
        self.settings.open = open;
        if self.settings.unsaved && !ctx.input(|i| i.pointer.any_down()) {
            self.settings.unsaved = false;
            self.save_app_settings();
        }
    }

    fn tool_start_editor(&mut self, ui: &mut egui::Ui) {
        let tool = self.settings.tool;
        let saved = self.app_settings.tool_starts.contains_key(tool.label());
        let (mut state, mut brush) = self.app_settings.tool_start(tool);
        let before = (state, brush.clone());

        ui.horizontal(|ui| {
            ui.strong(tool.label());
            ui.weak(if saved { "(your values)" } else { "(built-in values)" });
        });
        ui.add_space(4.0);

        brush_combo(ui, "start-brush", &self.presets, &mut state.shape, &mut brush);
        start_controls(ui, tool, &mut state);

        if (state, brush.clone()) != before {
            self.app_settings.set_tool_start(tool, Some(ToolStart::new(&state, brush.clone())));
            self.settings.unsaved = true;
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Use Current").on_hover_text(format!("Start with {} as it's set up right now", tool.label())).clicked() {
                let start = ToolStart::new(&self.tool_states[tool.index()], self.brush_names[tool.index()].clone());
                self.app_settings.set_tool_start(tool, Some(start));
                self.settings.unsaved = true;
            }
            if ui.button("Use Now").on_hover_text(format!("Set {} to these values now", tool.label())).clicked() {
                self.tool_states[tool.index()] = state;
                self.brush_names[tool.index()] = brush;
            }
            if ui.add_enabled(saved, egui::Button::new("Reset")).on_hover_text(format!("Start {} with the built-in values again", tool.label())).clicked() {
                self.app_settings.set_tool_start(tool, None);
                self.settings.unsaved = true;
            }
        });
    }
}

/// The starting values a tool has: like its Properties, plus pressure response
/// and paper grain.
fn start_controls(ui: &mut egui::Ui, tool: Tool, state: &mut ToolState) {
    if tool.is_wet_medium() {
        flat_checkbox(ui, state);
    }
    size_slider(ui, state);
    opacity_slider(ui, tool, state);
    taper_controls(ui, &mut state.shape);

    if tool == Tool::Knife {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.scrape, false, "Spread").on_hover_text("Lay thick, smooth paint");
            ui.selectable_value(&mut state.scrape, true, "Scrape").on_hover_text("An empty knife: scrapes wet paint off");
        });
    } else if tool.is_wet_medium() {
        let modes = [(PaintMode::Paint, "Paint"), (PaintMode::PaintMix, "Paint & Mix"), (PaintMode::PaintBlend, "Paint & Blend"), (PaintMode::Blend, "Blend")];
        let label = modes.iter().find(|(m, _)| *m == state.paint_mode).map_or("", |(_, l)| l);
        egui::ComboBox::from_label("Paint mode").selected_text(label).show_ui(ui, |ui| {
            for (mode, label) in modes {
                ui.selectable_value(&mut state.paint_mode, mode, label);
            }
        });
    }
    if tool.is_wet_medium() {
        loading_slider(ui, state);
        oiliness_slider(ui, state);
        thickness_slider(ui, state);
    }

    ui.add_space(4.0);
    ui.weak("Pen pressure changes");
    ui.add(egui::Slider::new(&mut state.shape.size_pressure, 0.0..=1.0).text("Size")).on_hover_text("0 = pressure doesn't change the size");
    ui.add(egui::Slider::new(&mut state.shape.opacity_pressure, 0.0..=1.0).text("Opacity")).on_hover_text("0 = pressure doesn't change the opacity");
    ui.add(egui::Slider::new(&mut state.shape.grain, 0.0..=1.0).text("Paper grain")).on_hover_text("How much the paper texture shows in the stroke");
}
