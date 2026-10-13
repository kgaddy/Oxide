//! The side panels: Tools, Properties, Brush presets (left) and
//! Navigator, Color, Layers (right).

use eframe::egui::{self, Color32, CollapsingHeader, Sense, Stroke as LineStroke, StrokeKind, Vec2};
use oxide_engine::{Color, PaintMode};

use super::canvas_view::view_xform;
use super::{OxideApp, to_color32};
use crate::color_wheel::color_wheel;
use crate::presets::{THUMB_H, THUMB_W};
use crate::tools::{FUTURE_TOOLS, SWATCHES, Tool};

impl OxideApp {
    pub(super) fn left_panel(&mut self, ui: &mut egui::Ui) {
        // Don't shrink to the content: lay everything out to the panel's width.
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            CollapsingHeader::new("Tools").default_open(true).show(ui, |ui| self.tools_section(ui));
            CollapsingHeader::new("Properties").default_open(true).show(ui, |ui| self.properties_section(ui));
            CollapsingHeader::new("Brush presets").default_open(true).show(ui, |ui| {
                ui.weak(format!("Shapes for {}", self.tool.label()));
                self.presets_section(ui);
            });
        });
    }

    pub(super) fn right_panel(&mut self, ui: &mut egui::Ui) {
        // Don't shrink to the content: lay everything out to the panel's width.
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            CollapsingHeader::new("Navigator").default_open(true).show(ui, |ui| self.navigator_section(ui));
            CollapsingHeader::new("Color").default_open(true).show(ui, |ui| self.color_section(ui));
            CollapsingHeader::new("Wet Media").default_open(true).show(ui, |ui| self.wet_media_section(ui));
            CollapsingHeader::new("Layers").default_open(true).show(ui, |ui| self.layers_section(ui));
            CollapsingHeader::new("Paper").default_open(true).show(ui, |ui| self.paper_section(ui));
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
        let tool = self.tool;
        let mut clicked_mode = None;
        ui.horizontal(|ui| {
            if tool.paints() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, to_color32(self.color));
                ui.painter().rect_stroke(rect, 3.0, LineStroke::new(1.0, Color32::from_gray(90)), StrokeKind::Inside);
            }
            ui.strong(tool.label());
        });
        if tool == Tool::Picker {
            ui.weak("Click the canvas to pick a color.");
            return;
        }
        ui.add_space(4.0);

        let state = &mut self.tool_states[tool.index()];
        ui.add(egui::Slider::new(&mut state.shape.radius, 0.5..=200.0).logarithmic(true).text("Size"));
        match tool {
            Tool::Smudge => {
                ui.add(egui::Slider::new(&mut state.oiliness, 0.05..=1.0).text("Strength"))
                    .on_hover_text("How far the smudge drags the paint");
            }
            _ => {
                ui.add(egui::Slider::new(&mut state.opacity, 0.01..=1.0).text("Opacity"));
            }
        }

        if tool.is_wet_medium() {
            // Paint modes (keys 1–5; V toggles Blend, A toggles Erase).
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                let modes = [
                    (Some(PaintMode::Paint), "1 Paint", "Lay the color down without mixing it with what's on the canvas"),
                    (Some(PaintMode::PaintMix), "2 Mix", "Paint & Mix: firm pressure paints, light pressure mixes with the wet paint"),
                    (Some(PaintMode::PaintBlend), "3 Blend+", "Paint & Blend: firm pressure paints, light pressure only blends"),
                    (Some(PaintMode::Blend), "4 Blend", "No paint of its own: light pressure blends, firm pressure smudges (V toggles)"),
                    (None, "5 Erase", "Remove paint (A toggles)"),
                ];
                for (mode, label, hint) in modes {
                    let active = match mode {
                        Some(m) => !state.erase && state.paint_mode == m,
                        None => state.erase,
                    };
                    if ui.selectable_label(active, label).on_hover_text(hint).clicked() {
                        clicked_mode = Some(mode);
                    }
                }
            });
            // another app's combined Oils & Acrylics: oiliness is what makes it
            // behave more like oil (blends, drags) or acrylic (covers).
            ui.add(egui::Slider::new(&mut state.loading, 0.0..=1.0).text("Loading"))
                .on_hover_text("How much paint the brush lays down (⌘-drag up/down on the canvas)");
            ui.add(egui::Slider::new(&mut state.length, 0.0..=1.0).text("Length"))
                .on_hover_text("How long the paint lasts: low = runs out fast, 1 = never runs out");
            ui.add(egui::Slider::new(&mut state.oiliness, 0.0..=1.0).text("Oiliness"))
                .on_hover_text("Low = acrylic-like: covers wet paint and stays clean.\nHigh = oil-like: picks up and blends wet paint.\n(Dried paint is never picked up.)");
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.dirty_brush, "Dirty brush")
                    .on_hover_text("Start the next stroke with the paint left on the brush; it cleans up as you paint")
                    .changed()
                    && !self.dirty_brush
                {
                    self.carried_color = None;
                }
                if let Some(carried) = self.carried_color {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, to_color32(carried));
                    if ui.button("Clean").on_hover_text("Wipe the brush: back to the chosen color").clicked() {
                        self.carried_color = None;
                    }
                }
            });
            let mut placeholder = 0.0;
            ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=1.0).text("Impasto depth"))
                .on_disabled_hover_text("Paint thickness: coming with impasto (milestone 6)");
            ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=1.0).text("Gloss"))
                .on_disabled_hover_text("Shine on thick paint: coming with impasto (milestone 6)");
            let mut off = false;
            ui.add_enabled(false, egui::Checkbox::new(&mut off, "Multicolor brush"))
                .on_disabled_hover_text("Load several colors on the brush: coming later");
        }
        if let Some(mode) = clicked_mode {
            self.set_paint_mode(mode);
        }
        if tool.paints() || tool == Tool::Smudge {
            ui.add(egui::Slider::new(&mut self.mouse_pressure, 0.05..=1.0).text("Pressure"))
                .on_hover_text("Pressure used when painting with a mouse (the pen uses its real pressure)");
        }
    }

    fn presets_section(&mut self, ui: &mut egui::Ui) {
        let mut apply = None;
        let mut delete = None;
        // A wrapping grid of small tiles (thumbnail + name) that fits any panel width.
        let thumb = Vec2::new(THUMB_W as f32, THUMB_H as f32) * 0.75;
        let cell = Vec2::new(thumb.x + 8.0, thumb.y + 22.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
            for (i, entry) in self.presets.entries.iter().enumerate() {
                let selected = entry.preset.matches(&self.state().shape);
                let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
                let response = response.on_hover_text(&entry.preset.name);
                let visuals = ui.style().interact_selectable(&response, selected);
                if selected || response.hovered() {
                    ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
                }
                let image_rect = egui::Rect::from_min_size(rect.min + Vec2::new(4.0, 3.0), thumb);
                egui::Image::new(&entry.thumbnail).paint_at(ui, image_rect);
                let name = ui.painter().layout(
                    entry.preset.name.clone(),
                    egui::FontId::proportional(11.0),
                    visuals.text_color(),
                    cell.x - 4.0,
                );
                let text_pos = egui::pos2(rect.center().x - name.size().x / 2.0, image_rect.max.y + 2.0);
                ui.painter().with_clip_rect(rect).galley(text_pos, name, visuals.text_color());
                if response.clicked() {
                    apply = Some(i);
                }
                if entry.file.is_some() {
                    response.context_menu(|ui| {
                        if ui.button("Delete brush").clicked() {
                            delete = Some(i);
                        }
                    });
                }
            }
        });
        if let Some(i) = apply {
            if self.tool == Tool::Picker {
                self.tool = Tool::OilsAcrylics;
            }
            let shape = self.presets.entries[i].preset.apply_to(&self.state().shape);
            self.state_mut().shape = shape;
        }
        if let Some(i) = delete {
            self.presets.delete(i);
        }
        ui.add_space(4.0);
        if ui.button("Brush creator…").clicked() {
            self.brush_creator.open = true;
        }
        if let Some(error) = &self.presets.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }

    fn navigator_section(&mut self, ui: &mut egui::Ui) {
        let canvas_size = Vec2::new(self.document.width() as f32, self.document.height() as f32);
        let width = ui.available_width();
        let thumb_scale = width / canvas_size.x;
        let (thumb, response) = ui.allocate_exact_size(canvas_size * thumb_scale, Sense::click_and_drag());
        self.texture.paint(&ui.painter_at(thumb), thumb);

        // Outline the part of the canvas that's visible in the workspace
        // (a rotated box when the view is rotated).
        let workspace = self.last_workspace;
        if workspace.is_positive() {
            let xf = view_xform(&mut self.view, workspace, canvas_size, 24.0);
            let to_thumb = |canvas: Vec2| thumb.min + canvas * thumb_scale;
            let mut outline: Vec<egui::Pos2> = [workspace.left_top(), workspace.right_top(), workspace.right_bottom(), workspace.left_bottom()]
                .iter()
                .map(|c| to_thumb(xf.to_canvas(*c)))
                .collect();
            outline.push(outline[0]);
            ui.painter_at(thumb).add(egui::Shape::line(outline, LineStroke::new(1.5, Color32::from_rgb(52, 132, 228))));

            // Click or drag in the navigator to move the view there.
            if let (true, Some(pos)) = (response.is_pointer_button_down_on(), response.interact_pointer_pos()) {
                let target = (pos - thumb.min) / thumb_scale; // canvas pixels
                if self.view.fit {
                    self.view.unfit();
                }
                self.view.keep_point_at(workspace, canvas_size, target, workspace.center());
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
            if ui.selectable_label(self.view.fit, "Fit").on_hover_text("Fit to screen (. or ⌘0)").clicked() {
                self.view.fit = true;
            }
            if ui.button("100%").on_hover_text("Actual size (0 or ⌘1)").clicked() {
                self.view.actual_size();
            }
            if ui.button("−").clicked() {
                self.view.zoom_by(0.8);
            }
            if ui.button("+").clicked() {
                self.view.zoom_by(1.25);
            }
        });

        // Rotate / flip / greyscale: the view only, never the painting.
        let mut rotation = self.view.rotation;
        let mut rotate_by = 0.0;
        let mut reset = false;
        let mut flip = false;
        ui.horizontal(|ui| {
            if ui.button("⟲").on_hover_text("Rotate view left 15° (⌘⌥←)").clicked() {
                rotate_by = -15.0;
            }
            if ui.add(egui::Slider::new(&mut rotation, -180.0..=180.0).suffix("°").max_decimals(0)).on_hover_text("Rotate the view (or hold R and drag on the canvas)").changed() {
                rotate_by = rotation - self.view.rotation;
            }
            if ui.button("⟳").on_hover_text("Rotate view right 15° (⌘⌥→)").clicked() {
                rotate_by = 15.0;
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(self.view.rotation != 0.0, egui::Button::new("Reset")).on_hover_text("Reset rotation (⌘⌥0)").clicked() {
                reset = true;
            }
            if ui.selectable_label(self.view.flip, "Flip").on_hover_text("Mirror the view left-right (Shift+F): check your drawing with fresh eyes").clicked() {
                flip = true;
            }
            let mut grey = self.texture.greyscale();
            if ui.checkbox(&mut grey, "Greyscale").on_hover_text("See values without color (G)").changed() {
                self.texture.set_greyscale(&self.document, grey);
            }
        });
        if rotate_by != 0.0 || reset || flip {
            self.change_view(rotate_by, flip, reset);
        }
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
        let mut rgb = [self.color.r, self.color.g, self.color.b].map(|v| (v * 255.0).round());
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
            if !self.tool.paints() {
                self.tool = Tool::OilsAcrylics;
            }
        }
    }
}

impl OxideApp {
    /// wet media controls: dry or rewet the paint.
    fn wet_media_section(&mut self, ui: &mut egui::Ui) {
        let active = self.document.active_index();
        let wet_here = self.document.layers()[active].canvas.has_wet_paint();
        let wet_anywhere = self.document.layers().iter().any(|l| l.canvas.has_wet_paint());
        ui.label(if wet_here { "This layer has wet paint." } else { "This layer is dry." });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(wet_here, egui::Button::new("Dry layer")).on_hover_text("Lock the paint in (⌘D)").clicked() {
                self.document.dry_layer();
            }
            if ui.add_enabled(wet_anywhere, egui::Button::new("Dry all")).on_hover_text("Dry every layer (⇧⌘D)").clicked() {
                self.document.dry_all();
            }
            if ui.button("Rewet").on_hover_text("Make this layer's paint wet again so it can be blended").clicked() {
                self.document.rewet_layer();
            }
        });
        let mut show = self.document.show_wet();
        if ui.checkbox(&mut show, "Show wet paint").on_hover_text("Tint wet areas blue").changed() {
            self.document.set_show_wet(show);
        }
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
