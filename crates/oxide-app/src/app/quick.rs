//! The Quick section: the controls you picked for the current tool (tool
//! properties, colors, the Dry buttons), and the editor for choosing them.

use eframe::egui::{self, Rect, Vec2};

use super::panels::{loading_slider, oiliness_slider, opacity_slider, size_slider, taper_controls, thickness_slider, flat_checkbox};
use super::{Command, OxideApp};
use crate::quick_panel::{QuickGroup, QuickItem};
use crate::tools::Tool;

/// A Quick item being dragged in the editor: its place in the list.
struct QuickDrag(usize);

impl OxideApp {
    pub(super) fn quick_section(&mut self, ui: &mut egui::Ui) {
        let tool = self.tool;
        let items = self.app_settings.quick_items(tool);
        if self.quick_editing {
            self.quick_editor(ui, tool, items);
            return;
        }
        // (Edit, in the section's title bar, opens the editor.)
        if items.is_empty() {
            ui.weak(format!("Nothing here for {} yet: click Edit to add controls.", tool.label()));
        }
        for item in items {
            self.quick_item(ui, item);
        }
    }

    fn quick_item(&mut self, ui: &mut egui::Ui, item: QuickItem) {
        let tool = self.tool;
        let i = tool.index();
        match item {
            QuickItem::Brush => {
                super::panels::brush_combo(ui, "quick-brush", &self.presets, &mut self.tool_states[i].shape, &mut self.brush_names[i]);
            }
            QuickItem::Size => size_slider(ui, &mut self.tool_states[i]),
            QuickItem::Opacity => opacity_slider(ui, tool, &mut self.tool_states[i]),
            QuickItem::Taper => taper_controls(ui, &mut self.tool_states[i].shape),
            QuickItem::PaintMode => self.paint_mode_buttons(ui),
            QuickItem::Loading => loading_slider(ui, &mut self.tool_states[i]),
            QuickItem::Oiliness => oiliness_slider(ui, &mut self.tool_states[i]),
            QuickItem::Thickness => {
                flat_checkbox(ui, &mut self.tool_states[i]);
                thickness_slider(ui, &mut self.tool_states[i]);
            }
            QuickItem::DirtyBrush => self.dirty_brush_row(ui),
            QuickItem::Pressure => self.mouse_pressure_slider(ui),
            QuickItem::HatchKind => self.hatch_mode(ui),
            QuickItem::HatchOptions => self.hatch_properties(ui),
            QuickItem::SelectOptions => self.select_properties(ui),
            QuickItem::PickerSample => self.picker_sample(ui),
            QuickItem::Colors => self.current_colors(ui),
            QuickItem::Wheel => self.wheel(ui),
            QuickItem::RgbSliders => self.rgb_sliders(ui),
            QuickItem::Pigments => self.pigment_swatches(ui),
            QuickItem::RecentColors => self.recent_swatches(ui),
            QuickItem::MultiColor => self.multicolor_controls(ui),
            QuickItem::MixingPalette => {
                if ui.button("Mixing Palette").on_hover_text("Open or close the Mixing Palette (⇧⌘M)").clicked() {
                    self.run(Command::MixingPalette, ui.ctx());
                }
            }
            QuickItem::Dry => self.dry_buttons(ui),
            QuickItem::ShowWet => {
                let mut show = self.document.show_wet();
                if ui.checkbox(&mut show, "Show wet paint").on_hover_text("Tint wet areas blue").changed() {
                    self.document.set_show_wet(show);
                }
            }
        }
    }

    /// Choosing the items: the ones in use (drag to reorder, ✕ to take one
    /// out), then everything else that fits this tool, to add.
    fn quick_editor(&mut self, ui: &mut egui::Ui, tool: Tool, mut items: Vec<QuickItem>) {
        let before = items.clone();
        ui.weak(format!("What the Quick section shows for {}. Drag to reorder.", tool.label()));
        ui.add_space(2.0);

        let mut rows = Vec::new();
        let mut remove = None;
        for (n, item) in items.iter().enumerate() {
            let row = ui.horizontal(|ui| {
                ui.dnd_drag_source(egui::Id::new(("quick-item", n)), QuickDrag(n), |ui| {
                    ui.label(egui::RichText::new(egui_phosphor::regular::DOTS_SIX_VERTICAL).weak());
                    ui.label(item.label(tool));
                })
                .response
                .on_hover_text("Drag to move");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").on_hover_text("Take it out").clicked() {
                        remove = Some(n);
                    }
                });
            });
            rows.push(row.response.rect);
        }
        if let Some(n) = remove {
            items.remove(n);
        }
        self.quick_drop(ui, &rows, &mut items);

        let available: Vec<QuickItem> = QuickItem::ALL.into_iter().filter(|i| i.applies(tool) && !items.contains(i)).collect();
        if !available.is_empty() {
            ui.add_space(6.0);
            ui.strong("Add");
        }
        for group in QuickGroup::ALL {
            let in_group: Vec<QuickItem> = available.iter().copied().filter(|i| i.group() == group).collect();
            if in_group.is_empty() {
                continue;
            }
            ui.weak(group.label());
            ui.horizontal_wrapped(|ui| {
                for item in in_group {
                    if ui.small_button(format!("+ {}", item.label(tool))).clicked() {
                        items.push(item);
                    }
                }
            });
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Done").clicked() {
                self.quick_editing = false;
            }
            if ui.button("Reset").on_hover_text(format!("Back to the usual items for {}", tool.label())).clicked() {
                self.app_settings.set_quick_items(tool, None);
                self.save_app_settings();
                items = self.app_settings.quick_items(tool);
            }
        });
        if items != before && items != self.app_settings.quick_items(tool) {
            self.app_settings.set_quick_items(tool, Some(items));
            self.save_app_settings();
        }
    }

    /// While an item is dragged in the editor: a line where it would go, and
    /// the move when it's let go.
    fn quick_drop(&self, ui: &egui::Ui, rows: &[Rect], items: &mut Vec<QuickItem>) {
        let Some(dragged) = egui::DragAndDrop::payload::<QuickDrag>(ui.ctx()).map(|d| d.0) else { return };
        let Some(pos) = ui.ctx().pointer_latest_pos() else { return };
        let (Some(first), Some(last)) = (rows.first(), rows.last()) else { return };
        let area = first.union(*last).expand2(Vec2::new(0.0, 12.0));
        if !area.contains(pos) || dragged >= items.len() {
            return;
        }
        let index = rows.iter().filter(|r| r.center().y < pos.y).count();
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            let item = items.remove(dragged);
            items.insert(if dragged < index { index - 1 } else { index }, item);
            return;
        }
        let y = match index {
            0 => first.top(),
            n if n == rows.len() => last.bottom(),
            n => (rows[n - 1].bottom() + rows[n].top()) / 2.0,
        };
        let line = Rect::from_x_y_ranges(area.x_range(), (y - 1.0)..=(y + 1.0));
        ui.painter().rect_filled(line, 1.0, ui.visuals().selection.bg_fill);
    }
}
