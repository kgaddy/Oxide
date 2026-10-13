//! The Brush Creator window: edit the current tool's brush *shape* with a
//! live preview (painted with the current medium), and save the shape as a preset.

use eframe::egui::{self, Vec2};

use super::OxideApp;
use oxide_engine::Brush;

use crate::presets::render_preview;

const PREVIEW_W: usize = 300;
const PREVIEW_H: usize = 90;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Stroke,
    Shape,
}

#[derive(Default)]
pub struct BrushCreator {
    pub open: bool,
    tab: Tab,
    /// Name typed into the "Save as preset" box.
    name: String,
    /// Result of the last save, shown under the button.
    message: Option<Result<String, String>>,
    preview: Option<egui::TextureHandle>,
    /// The brush the preview was rendered for, so we only re-render when
    /// something changed.
    preview_for: Option<Brush>,
}

impl OxideApp {
    pub(super) fn brush_creator_window(&mut self, ctx: &egui::Context) {
        let mut open = self.brush_creator.open;
        egui::Window::new("Brush Creator")
            .open(&mut open)
            .default_width(320.0)
            .resizable(false)
            .show(ctx, |ui| self.brush_creator_contents(ui));
        self.brush_creator.open = open;
    }

    fn brush_creator_contents(&mut self, ui: &mut egui::Ui) {
        // --- Live preview, painted with the current tool's medium ---
        let current = self.stroke_brush_preview();
        let creator = &mut self.brush_creator;
        if creator.preview_for.as_ref() != Some(&current) {
            let image = render_preview(&current, PREVIEW_W, PREVIEW_H);
            match &mut creator.preview {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => creator.preview = Some(ui.ctx().load_texture("brush-preview", image, egui::TextureOptions::LINEAR)),
            }
            creator.preview_for = Some(current);
        }
        if let Some(texture) = &creator.preview {
            ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::new(PREVIEW_W as f32, PREVIEW_H as f32)));
        }
        ui.weak(format!("Editing the {} brush", self.tool.label()));

        // --- Tabs ---
        let creator = &mut self.brush_creator;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut creator.tab, Tab::Stroke, "Stroke");
            ui.selectable_value(&mut creator.tab, Tab::Shape, "Shape & Grain");
        });
        ui.separator();

        let shape = &mut self.tool_states[self.tool.index()].shape;
        match creator.tab {
            Tab::Stroke => {
                ui.add(egui::Slider::new(&mut shape.radius, 0.5..=200.0).logarithmic(true).text("Size"));
                ui.add(egui::Slider::new(&mut shape.spacing, 0.02..=1.0).text("Spacing"))
                    .on_hover_text("Distance between dabs, as a fraction of the brush width");
                ui.add(egui::Slider::new(&mut shape.lift_off, 0.0..=1.0).text("Lift-off"))
                    .on_hover_text("How much the end of a stroke thins, fades, and breaks up as the brush lifts off");
                disabled_slider(ui, "Smoothing", "Coming with pen support (milestone 2)");
                disabled_slider(ui, "Size from pressure", "Coming with pen support (milestone 2)");
            }
            Tab::Shape => {
                ui.add(egui::Slider::new(&mut shape.hardness, 0.0..=1.0).text("Hardness"));
                ui.add(egui::Slider::new(&mut shape.roundness, 0.05..=1.0).text("Roundness"))
                    .on_hover_text("1 = round; lower = flat, like a chisel or calligraphy nib");
                ui.add(egui::Slider::new(&mut shape.angle, -90.0..=90.0).suffix("°").max_decimals(0).text("Angle"));
                ui.add(egui::Slider::new(&mut shape.roughness, 0.0..=1.0).text("Roughness"))
                    .on_hover_text("Ragged, grainy edges instead of a perfect outline");
                ui.add(egui::Slider::new(&mut shape.streaks, 0.0..=1.0).text("Bristle streaks"))
                    .on_hover_text("Streaks along the stroke, like the hairs of a real brush");
                ui.add(egui::Slider::new(&mut shape.grain, 0.0..=1.0).text("Grain"))
                    .on_hover_text("How much the paper texture shows: high = catches the tooth (dry brush, pencil); 0 = ignores the paper");
                disabled_slider(ui, "Bristles", "Coming with bristle brushes");
            }
        }
        ui.weak("Opacity, loading and oiliness belong to the medium: see Properties.");

        // --- Save as preset ---
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut creator.name).hint_text("Brush name").desired_width(180.0));
            if ui.button("Save as preset").clicked() {
                creator.message = Some(
                    self.presets
                        .save(ui.ctx(), &creator.name, &self.tool_states[self.tool.index()].shape)
                        .map(|()| format!("Saved \"{}\"", creator.name.trim())),
                );
            }
        });
        match &creator.message {
            Some(Ok(text)) => {
                ui.weak(text);
            }
            Some(Err(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            None => {}
        }
    }
}

impl OxideApp {
    /// What the preview paints with: the current tool's brush in a dark
    /// color, so light paint colors still show up on the paper.
    fn stroke_brush_preview(&self) -> Brush {
        let tool = if self.tool.paints() { self.tool } else { crate::tools::Tool::Ink };
        self.state().brush(tool, oxide_engine::Color::rgb(0.15, 0.15, 0.18))
    }
}

fn disabled_slider(ui: &mut egui::Ui, label: &str, why: &str) {
    let mut value = 0.0;
    ui.add_enabled(false, egui::Slider::new(&mut value, 0.0..=1.0).text(label))
        .on_disabled_hover_text(why);
}
