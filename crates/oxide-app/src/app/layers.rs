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
        // Canvas visibility.
        let mut paper_texture = settings.paper_visibility * 10.0;
        if ui.add(egui::Slider::new(&mut paper_texture, 0.0..=10.0).max_decimals(1).text("Paper texture"))
            .on_hover_text("How visible the texture is on bare paper")
            .changed()
        {
            settings.paper_visibility = paper_texture / 10.0;
        }
        let mut paint_texture = settings.paint_visibility * 10.0;
        if ui.add(egui::Slider::new(&mut paint_texture, 0.0..=10.0).max_decimals(1).text("Paint texture"))
            .on_hover_text("How much of the texture shows through the paint (thick paint hides the weave)")
            .changed()
        {
            settings.paint_visibility = paint_texture / 10.0;
        }
        ui.add_space(4.0);
        ui.label("Thick paint (impasto)");
        let mut depth = settings.impasto_depth * 10.0;
        if ui.add(egui::Slider::new(&mut depth, 0.0..=10.0).max_decimals(1).text("Impasto depth"))
            .on_hover_text("How strongly thick paint is lit: its ridges and brush marks. 0 = paint looks flat. (Brush thickness: Properties → Thickness.)")
            .changed()
        {
            settings.impasto_depth = depth / 10.0;
        }
        let mut gloss = settings.gloss * 10.0;
        if ui.add(egui::Slider::new(&mut gloss, 0.0..=10.0).max_decimals(1).text("Gloss"))
            .on_hover_text("How shiny thick paint is: highlights on ridges that face the light")
            .changed()
        {
            settings.gloss = gloss / 10.0;
        }
        let mut shadows = settings.shadows * 10.0;
        if ui.add(egui::Slider::new(&mut shadows, 0.0..=10.0).max_decimals(1).text("Shadows"))
            .on_hover_text("How dark the shadows thick paint casts on the paint and paper beside it")
            .changed()
        {
            settings.shadows = shadows / 10.0;
        }
        ui.add(egui::Slider::new(&mut settings.light_height, 10.0..=80.0).suffix("°").max_decimals(0).text("Light height"))
            .on_hover_text("How high the light is: low = long, raking shadows that show every brush mark; high = short shadows. (Its direction is the Light slider above.)");
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
        if self.transform.is_some() {
            ui.label("Transforming: Apply or Cancel first.");
            ui.disable();
        }
        // Collect what the user did, then apply it after drawing the list,
        // so we're not changing the layer list while looping over it.
        let mut select = None;
        let mut toggle_visible = None;
        let mut toggle_group_open = None;
        let mut toggle_group_visible = None;

        // Show the top layer first, like every paint app. A group's layers
        // sit together, under a header row for the group.
        let count = self.document.layers().len();
        let mut current_group: Option<u32> = None;
        for i in (0..count).rev() {
            let layer = &self.document.layers()[i];
            let active = i == self.document.active_index();
            if layer.group != current_group {
                current_group = layer.group;
                if let Some(group) = layer.group.and_then(|id| self.document.group(id)) {
                    let members = self.document.layers().iter().filter(|l| l.group == Some(group.id)).count();
                    let holds_active = self.document.layers()[self.document.active_index()].group == Some(group.id);
                    ui.horizontal(|ui| {
                        let eye = if group.visible { "👁" } else { "  " };
                        if ui.add(egui::Button::new(eye).min_size(Vec2::new(24.0, 0.0))).on_hover_text("Show/hide the group").clicked() {
                            toggle_group_visible = Some((group.id, !group.visible));
                        }
                        let arrow = if group.open { "▾" } else { "▸" };
                        if ui.add(egui::Button::new(arrow).frame(false)).on_hover_text("Open/close the group").clicked() {
                            toggle_group_open = Some((group.id, !group.open));
                        }
                        let text = egui::RichText::new(format!("📁 {}  ({members})", group.name)).strong();
                        let text = if group.visible { text } else { text.weak() };
                        let row = egui::Button::new(text).selected(holds_active && !group.open).min_size(Vec2::new(ui.available_width(), 0.0));
                        if ui.add(row).on_hover_text("Select the group's top layer (group settings show below)").clicked() {
                            select = Some(i);
                        }
                    });
                }
            }
            // Layers of a closed group are hidden in the list.
            if layer.group.and_then(|id| self.document.group(id)).is_some_and(|g| !g.open) {
                continue;
            }
            ui.horizontal(|ui| {
                if layer.group.is_some() {
                    ui.add_space(14.0); // indent group members
                }
                let eye = if layer.visible { "👁" } else { "  " };
                if ui.add(egui::Button::new(eye).min_size(Vec2::new(24.0, 0.0))).on_hover_text("Show/hide").clicked() {
                    toggle_visible = Some(i);
                }
                // Badges: T = tracing, 🔒 = locked, α = lock transparency.
                let mut label = layer.name.clone();
                if layer.tracing {
                    label = format!("T  {label}");
                } else if layer.locked {
                    label = format!("🔒 {label}");
                }
                if layer.lock_transparency() {
                    label.push_str("  α");
                }
                let name = if layer.visible {
                    egui::RichText::new(label)
                } else {
                    egui::RichText::new(label).weak()
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
        if let Some((id, open)) = toggle_group_open {
            self.document.set_group_open(id, open);
        }
        if let Some((id, visible)) = toggle_group_visible {
            self.document.set_group_visible(id, visible);
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
        let active = self.document.active_index();
        ui.horizontal(|ui| {
            if ui.button("Duplicate").on_hover_text("Copy this layer (⌘J)").clicked() {
                self.document.duplicate_active_layer();
            }
            if ui.add_enabled(active > 0, egui::Button::new("Merge ⏷")).on_hover_text("Merge into the layer below (⌘E). The merged paint is dried.").clicked() {
                self.document.merge_down();
            }
            if ui.button("Clear").on_hover_text("Erase everything on this layer (⌘⌫)").clicked() {
                self.document.clear_active_layer();
            }
            let in_group = self.document.layers()[active].group.is_some();
            if ui.add_enabled(!in_group, egui::Button::new("Group")).on_hover_text("Put this layer in a new group (⌥⌘G). Move other layers into it with ⏶/⏷.").clicked() {
                self.document.group_active_layer();
            }
        });
        let layer = &self.document.layers()[active];
        let (mut locked, mut alpha, mut tracing) = (layer.locked, layer.lock_transparency(), layer.tracing);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut locked, "Lock").on_hover_text("No painting on this layer (⌘/)").changed() {
                self.document.set_locked(active, locked);
            }
            if ui.checkbox(&mut alpha, "Lock transparency").on_hover_text("Paint only where there's already paint (/)").changed() {
                self.document.set_lock_transparency(active, alpha);
            }
        });
        if ui
            .checkbox(&mut tracing, "Tracing layer")
            .on_hover_text("Use this layer as a reference photo: new strokes on other layers take their color from it where they start (⌥⇧T)")
            .changed()
        {
            self.document.set_tracing(active, tracing);
        }

        // Settings for the active layer.
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
        // Per-layer impasto depth: a flat glaze over a thick underpainting.
        let own = self.document.layers()[active].impasto;
        ui.horizontal(|ui| {
            let mut has_own = own.is_some();
            if ui
                .checkbox(&mut has_own, "Own impasto depth")
                .on_hover_text("Give this layer its own impasto depth instead of the painting's (Paper panel). E.g. 0 for a flat glaze or detail layer over thick paint.")
                .changed()
            {
                let depth = has_own.then(|| self.document.layer_impasto(active));
                self.document.set_layer_impasto(active, depth);
                self.document.seal_history();
            }
            if let Some(depth) = own {
                let mut shown = depth * 10.0;
                if ui.add(egui::Slider::new(&mut shown, 0.0..=10.0).max_decimals(1)).changed() {
                    self.document.set_layer_impasto(active, Some(shown / 10.0));
                }
            }
        });
        // The current layer's group: name, opacity, ungroup.
        if let Some(group) = self.document.layers()[active].group.and_then(|id| self.document.group(id)).cloned() {
            ui.add_space(4.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("📁 Group");
                let editing = self.renaming_group.get_or_insert_with(|| group.name.clone());
                let response = ui.text_edit_singleline(editing);
                if response.lost_focus() {
                    let name = self.renaming_group.take().unwrap_or_default();
                    if !name.trim().is_empty() {
                        self.document.rename_group(group.id, name.trim().to_string());
                    }
                } else if !response.has_focus() {
                    self.renaming_group = None;
                }
            });
            let mut opacity = group.opacity * 100.0;
            if ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%").max_decimals(0).text("Group opacity"))
                .on_hover_text("Fades the whole group as one picture")
                .changed()
            {
                self.document.set_group_opacity(group.id, opacity / 100.0);
            }
            if ui.button("Ungroup").on_hover_text("Dissolve the group; its layers stay (⇧⌥⌘G)").clicked() {
                self.document.ungroup(group.id);
            }
        }

        let layer = &self.document.layers()[active];
        let warning = if layer.tracing {
            Some("This is the tracing layer. Paint on another layer.")
        } else if layer.locked {
            Some("This layer is locked.")
        } else if !layer.visible {
            Some("This layer is hidden. Show it to paint.")
        } else {
            None
        };
        if let Some(text) = warning {
            ui.colored_label(Color32::from_rgb(230, 170, 60), text);
        }
    }
}
