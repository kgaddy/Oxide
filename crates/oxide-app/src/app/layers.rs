//! The Layers panel (list, visibility, opacity, order) and the Paper panel
//! (surface texture and color).

use eframe::egui::{self, Color32, Vec2};
use oxide_engine::{Color, PaperKind};

use super::{OxideApp, to_color32};

impl OxideApp {
    /// Choose the surface: paper/canvas type, texture size and depth, light, color.
    pub(super) fn paper_section(&mut self, ui: &mut egui::Ui) {
        let mut settings = self.document.surface().settings();
        let before = settings;
        if ui.button("Select Canvas…").on_hover_text("Choose texture and color").clicked() {
            let paper = self.document.paper();
            self.select_canvas.open_for(super::select_canvas::CanvasTarget::Document, settings, paper);
        }

        egui::ComboBox::from_id_salt("paper-kind")
            .selected_text(settings.kind.label())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for kind in PaperKind::ALL {
                    ui.selectable_value(&mut settings.kind, kind, kind.label());
                }
            });
        ui.add(egui::Slider::new(&mut settings.depth, 0.0..=1.0).text("Depth"))
            .on_hover_text("How deep the texture is. 0 = perfectly smooth");
        ui.add(egui::Slider::new(&mut settings.scale, 0.5..=3.0).text("Scale"))
            .on_hover_text("Size of the bumps or the weave");
        ui.add(egui::Slider::new(&mut settings.light_angle, 0.0..=360.0).suffix("°").max_decimals(0).text("Light"))
            .on_hover_text("Direction the light comes from (225° = top-left)");
        if settings != before {
            self.document.set_surface(settings);
        }

        ui.horizontal(|ui| {
            let mut paper = to_color32(self.document.paper());
            if ui.color_edit_button_srgba(&mut paper).on_hover_text("Paper color").changed() {
                let [r, g, b, _] = paper.to_array();
                self.document.set_paper(Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0));
            }
            ui.label("Color");
        });
    }

    pub(super) fn layers_section(&mut self, ui: &mut egui::Ui) {
        // Collect what the user did, then apply it after drawing the list,
        // so we're not changing the layer list while looping over it.
        let mut select = None;
        let mut toggle_visible = None;

        // Show the top layer first, like every paint app.
        let count = self.document.layers().len();
        for i in (0..count).rev() {
            let layer = &self.document.layers()[i];
            let active = i == self.document.active_index();
            ui.horizontal(|ui| {
                let eye = if layer.visible { "👁" } else { "  " };
                if ui.add(egui::Button::new(eye).min_size(Vec2::new(24.0, 0.0))).on_hover_text("Show/hide").clicked() {
                    toggle_visible = Some(i);
                }
                let name = if layer.visible {
                    egui::RichText::new(&layer.name)
                } else {
                    egui::RichText::new(&layer.name).weak()
                };
                let row = egui::Button::new(name).selected(active).min_size(Vec2::new(ui.available_width(), 0.0));
                if ui.add(row).clicked() {
                    select = Some(i);
                }
            });
        }

        // The paper is always at the bottom (its settings are in the Paper panel).
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(24.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, to_color32(self.document.paper()));
            ui.weak(format!("Paper: {}", self.document.surface().settings().kind.label()));
        });

        if let Some(i) = select {
            self.document.set_active(i);
        }
        if let Some(i) = toggle_visible {
            self.document.toggle_visible(i);
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("+").on_hover_text("New layer").clicked() {
                self.document.add_layer();
            }
            let can_delete = count > 1;
            if ui.add_enabled(can_delete, egui::Button::new("🗑")).on_hover_text("Delete layer").clicked() {
                self.document.delete_active_layer();
            }
            if ui.button("⏶").on_hover_text("Move up").clicked() {
                self.document.move_active_layer(1);
            }
            if ui.button("⏷").on_hover_text("Move down").clicked() {
                self.document.move_active_layer(-1);
            }
        });

        // Settings for the active layer.
        let active = self.document.active_index();
        let mut opacity = self.document.layers()[active].opacity * 100.0;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Name");
            // Edit a copy and commit when you press Enter or click away, so
            // a rename is one undo step instead of one per letter.
            let editing = self.renaming.get_or_insert_with(|| self.document.layers()[active].name.clone());
            let response = ui.text_edit_singleline(editing);
            if response.lost_focus() {
                let name = self.renaming.take().unwrap_or_default();
                if !name.trim().is_empty() {
                    self.document.rename_layer(active, name);
                }
            } else if !response.has_focus() {
                // Not editing: keep showing the current name (it may change via undo).
                self.renaming = None;
            }
        });
        if ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%").max_decimals(0).text("Opacity")).changed() {
            self.document.set_opacity(active, opacity / 100.0);
        }
        if !self.document.layers()[active].visible {
            ui.colored_label(Color32::from_rgb(230, 170, 60), "This layer is hidden. Show it to paint.");
        }
    }
}
