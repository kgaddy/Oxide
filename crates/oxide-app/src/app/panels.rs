//! The side panels: Tools, Properties, Brush presets (left) and
//! Navigator, Color, Layers (right).

use eframe::egui::{self, Color32, CollapsingHeader, Rect, Sense, Stroke as LineStroke, StrokeKind, Vec2};
use oxide_engine::{Brush, Canvas, Color, Stroke};

use super::canvas_view::canvas_rect;
use super::{OxideApp, PAPER, to_color32, to_image};
use crate::color_wheel::color_wheel;
use crate::tools::{FUTURE_TOOLS, PRESETS, SWATCHES, Tool};

const THUMB_W: usize = 96;
const THUMB_H: usize = 40;

impl OxideApp {
    pub(super) fn left_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            CollapsingHeader::new("Tools").default_open(true).show(ui, |ui| self.tools_section(ui));
            CollapsingHeader::new("Properties").default_open(true).show(ui, |ui| self.properties_section(ui));
            CollapsingHeader::new("Brush presets").default_open(true).show(ui, |ui| self.presets_section(ui));
        });
    }

    pub(super) fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            CollapsingHeader::new("Navigator").default_open(true).show(ui, |ui| self.navigator_section(ui));
            CollapsingHeader::new("Color").default_open(true).show(ui, |ui| self.color_section(ui));
            CollapsingHeader::new("Layers").default_open(true).show(ui, |ui| self.layers_section(ui));
        });
    }

    fn tools_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                ui.selectable_value(&mut self.tool, tool, tool.label())
                    .on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()));
            }
            for name in FUTURE_TOOLS {
                ui.add_enabled(false, egui::Button::new(name))
                    .on_disabled_hover_text("Coming later");
            }
        });
    }

    fn properties_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, to_color32(self.brush.color));
            ui.painter().rect_stroke(rect, 3.0, LineStroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
            ui.label(format!("{} tool", self.tool.label()));
        });
        ui.add_space(4.0);
        ui.add(egui::Slider::new(&mut self.brush.radius, 0.5..=200.0).logarithmic(true).text("Size"));
        ui.add(egui::Slider::new(&mut self.brush.hardness, 0.0..=1.0).text("Hardness"));
        ui.add(egui::Slider::new(&mut self.brush.opacity, 0.01..=1.0).text("Opacity"));
        ui.add(egui::Slider::new(&mut self.brush.spacing, 0.02..=1.0).text("Spacing"));
        // paint properties, shown so the plan is visible.
        let mut placeholder = 100.0;
        ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=100.0).text("Loading"))
            .on_disabled_hover_text("Paint on the brush: coming with wet paint (milestone 4)");
        ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=100.0).text("Oiliness"))
            .on_disabled_hover_text("Coming with wet paint (milestone 4)");
    }

    fn presets_section(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("presets").spacing([6.0, 6.0]).show(ui, |ui| {
            for (i, (preset, thumb)) in PRESETS.iter().zip(&self.preset_thumbnails).enumerate() {
                let selected = preset.matches(&self.brush);
                let button = egui::Button::image_and_text(
                    egui::Image::new(thumb).fit_to_exact_size(Vec2::new(THUMB_W as f32, THUMB_H as f32) * 0.8),
                    preset.name,
                )
                .selected(selected);
                if ui.add(button).clicked() {
                    preset.apply(&mut self.brush);
                    self.tool = Tool::Brush;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    }

    fn navigator_section(&mut self, ui: &mut egui::Ui) {
        let canvas_size = Vec2::new(self.canvas.width() as f32, self.canvas.height() as f32);
        let width = ui.available_width();
        let thumb_scale = width / canvas_size.x;
        let (thumb, response) = ui.allocate_exact_size(canvas_size * thumb_scale, Sense::click_and_drag());
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter().image(self.texture.id(), thumb, uv, Color32::WHITE);

        // Outline the part of the canvas that's visible in the workspace.
        let workspace = self.last_workspace;
        if workspace.is_positive() {
            let (canvas_on_screen, scale) = canvas_rect(&mut self.view, workspace, canvas_size, 24.0);
            let visible = workspace.intersect(canvas_on_screen);
            if visible.is_positive() {
                let to_thumb = |p: egui::Pos2| thumb.min + (p - canvas_on_screen.min) / scale * thumb_scale;
                let frame = Rect::from_min_max(to_thumb(visible.min), to_thumb(visible.max));
                ui.painter().rect_stroke(frame, 0.0, LineStroke::new(1.5, Color32::from_rgb(52, 132, 228)), StrokeKind::Inside);
            }

            // Click or drag in the navigator to move the view there.
            if let (true, Some(pos)) = (response.is_pointer_button_down_on(), response.interact_pointer_pos()) {
                self.view.unfit();
                let target = (pos - thumb.min) / thumb_scale; // canvas pixels
                self.view.pan = (canvas_size / 2.0 - target) * self.view.zoom;
            }
        }

        ui.horizontal(|ui| {
            let mut percent = self.view.last_scale * 100.0;
            let slider = egui::Slider::new(&mut percent, 5.0..=3200.0).logarithmic(true).suffix("%").max_decimals(0);
            if ui.add(slider).changed() {
                self.view.unfit();
                self.view.zoom = percent / 100.0;
            }
        });
        ui.horizontal(|ui| {
            if ui.selectable_label(self.view.fit, "Fit").clicked() {
                self.view.fit = true;
            }
            if ui.button("100%").clicked() {
                self.view.actual_size();
            }
            if ui.button("−").clicked() {
                self.view.zoom_by(0.8);
            }
            if ui.button("+").clicked() {
                self.view.zoom_by(1.25);
            }
        });
    }

    fn color_section(&mut self, ui: &mut egui::Ui) {
        let diameter = ui.available_width().min(220.0);
        let mut hsv = self.hsv;
        ui.vertical_centered(|ui| {
            if color_wheel(ui, &mut hsv, diameter) {
                self.set_hsv(hsv);
            }
        });

        // RGB sliders (0-255), like most paint apps.
        let mut rgb = [self.brush.color.r, self.brush.color.g, self.brush.color.b].map(|v| (v * 255.0).round());
        let mut changed = false;
        for (value, name) in rgb.iter_mut().zip(["R", "G", "B"]) {
            changed |= ui.add(egui::Slider::new(value, 0.0..=255.0).max_decimals(0).text(name)).changed();
        }
        if changed {
            self.set_color(Color::rgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0));
        }

        ui.add_space(6.0);
        ui.label("Pigments");
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(3.0);
            for (name, color) in SWATCHES {
                if swatch(ui, color).on_hover_text(name).clicked() {
                    picked = Some(color);
                }
            }
        });
        if !self.recent_colors.is_empty() {
            ui.label("Recent");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(3.0);
                for &color in &self.recent_colors {
                    if swatch(ui, color).clicked() {
                        picked = Some(color);
                    }
                }
            });
        }
        if let Some(color) = picked {
            self.set_color(color);
            self.tool = Tool::Brush;
        }
    }

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        ui.add_enabled_ui(false, |ui| {
            ui.horizontal(|ui| {
                ui.label("👁");
                ui.strong("Layer 1");
            });
            ui.horizontal(|ui| {
                ui.label("👁");
                ui.label("Paper");
            });
        });
        ui.weak("Layers arrive in milestone 7.");
    }
}

/// A small clickable color square.
fn swatch(ui: &mut egui::Ui, color: Color) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let stroke = if response.hovered() {
        LineStroke::new(2.0, Color32::WHITE)
    } else {
        LineStroke::new(1.0, Color32::from_gray(70))
    };
    ui.painter().rect_filled(rect, 2.0, to_color32(color));
    ui.painter().rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
    response
}

/// Paint a little S-curve with each preset using the real engine, so the
/// buttons show what the brush actually does.
pub(super) fn render_preset_thumbnails(ctx: &egui::Context) -> Vec<egui::TextureHandle> {
    PRESETS
        .iter()
        .map(|preset| {
            let mut canvas = Canvas::new(THUMB_W, THUMB_H, PAPER);
            let mut brush = Brush { color: Color::rgb(0.15, 0.15, 0.18), ..Brush::default() };
            preset.apply(&mut brush);
            // Big brushes would fill the whole thumbnail; cap their size.
            brush.radius = brush.radius.min(THUMB_H as f32 * 0.3);
            let point = |t: f32| {
                let x = 8.0 + t * (THUMB_W as f32 - 16.0);
                let y = THUMB_H as f32 / 2.0 + (t * std::f32::consts::TAU).sin() * THUMB_H as f32 * 0.22;
                (x, y)
            };
            let (x, y) = point(0.0);
            let mut stroke = Stroke::begin(&mut canvas, &brush, x, y);
            for i in 1..=60 {
                let (x, y) = point(i as f32 / 60.0);
                stroke.line_to(&mut canvas, &brush, x, y);
            }
            ctx.load_texture(format!("preset-{}", preset.name), to_image(&canvas), egui::TextureOptions::LINEAR)
        })
        .collect()
}
