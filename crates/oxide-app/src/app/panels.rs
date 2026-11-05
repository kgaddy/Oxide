//! The side panels, each a column of sections (Tools, Properties, Brush
//! presets, Navigator, Color, Wet Media, Layers, Paper). Drag a section's
//! title to move it up or down, or over to the other side.

use eframe::egui::{self, Rect, Sense, Stroke as LineStroke, StrokeKind, Vec2};
use eframe::egui::collapsing_header::{CollapsingState, paint_default_icon};
use oxide_engine::{Color, PaintMode};

use super::canvas_view::view_xform;
use super::{Command, OxideApp, to_color32};
use crate::color_wheel::color_wheel;
use crate::panel_layout::{Section, Side};
use crate::presets::{THUMB_H, THUMB_W};
use crate::tools::{BrushShape, FUTURE_TOOLS, SWATCHES, Tool, ToolState};

/// Where the sections and columns were drawn this frame, for dropping a
/// dragged section (see [`OxideApp::section_drop`]).
#[derive(Default)]
pub(super) struct DockFrame {
    /// Each section drawn, top to bottom per side.
    sections: Vec<(Side, Section, Rect)>,
    columns: Vec<(Side, Rect)>,
}

impl OxideApp {
    pub(super) fn side_panel(&mut self, ui: &mut egui::Ui, side: Side) {
        self.dock_frame.columns.push((side, ui.max_rect()));
        // Don't shrink to the content: lay everything out to the panel's width.
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for section in self.app_settings.panels.side(side).to_vec() {
                let rect = self.section(ui, section);
                self.dock_frame.sections.push((side, section, rect));
            }
        });
    }

    /// One section: a fold arrow and a title you can click (to fold it) or
    /// drag (to move it), then its contents.
    fn section(&mut self, ui: &mut egui::Ui, section: Section) -> Rect {
        // Keyed by the section alone, so it stays folded or open when moved.
        let id = egui::Id::new(("panel-section", section));
        let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, !self.app_settings.panels.is_folded(section));
        let was_open = state.is_open();
        // The Quick section says whose items it's showing.
        let title = match section {
            Section::Quick => format!("Quick · {}", self.tool.label()),
            _ => section.title().to_string(),
        };
        let rect = ui.scope(|ui| {
            let header = ui.horizontal(|ui| {
                state.show_toggle_button(ui, paint_default_icon);
                let title = ui.add(egui::Label::new(title).selectable(false));
                let handle = egui::RichText::new(egui_phosphor::regular::DOTS_SIX_VERTICAL).weak();
                // The grip, and for Quick an Edit button, at the right; the
                // title is draggable up to them (so the button stays clickable).
                let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(handle).selectable(false));
                    if section == Section::Quick {
                        let editing = self.quick_editing;
                        let text = if editing { "Done".to_string() } else { format!("{} Edit", egui_phosphor::regular::PENCIL_SIMPLE) };
                        let hint = format!("Choose what's in the Quick section for {}", self.tool.label());
                        if ui.add(egui::Button::new(text).small().selected(editing)).on_hover_text(hint).clicked() {
                            self.quick_editing = !editing;
                            state.set_open(true);
                        }
                    }
                    ui.min_rect().left()
                });
                let grab = Rect::from_min_max(title.rect.left_top(), egui::pos2(right.inner - 2.0, title.rect.bottom()));
                ui.interact(grab, id.with("grab"), Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text("Drag to move this section (to the other side, too); click to fold it")
            });
            let grab = header.inner;
            if grab.clicked() {
                state.toggle(ui);
            }
            if grab.drag_started() {
                egui::DragAndDrop::set_payload(ui.ctx(), section);
            }
            let open = state.is_open();
            state.show_body_indented(&header.response, ui, |ui| self.section_contents(ui, section));
            open
        });
        // Remember folding between runs.
        if rect.inner != was_open {
            self.app_settings.panels.set_folded(section, !rect.inner);
            self.save_app_settings();
        }
        rect.response.rect
    }

    fn section_contents(&mut self, ui: &mut egui::Ui, section: Section) {
        match section {
            Section::Tools => self.tools_section(ui),
            Section::Quick => self.quick_section(ui),
            Section::Properties => {
                self.current_brush_label(ui);
                self.properties_section(ui);
            }
            Section::BrushPresets => {
                ui.weak(format!("Shapes for {}", self.tool.label()));
                self.presets_section(ui);
            }
            Section::Navigator => self.navigator_section(ui),
            Section::Color => self.color_section(ui),
            Section::WetMedia => self.wet_media_section(ui),
            Section::Layers => self.layers_section(ui),
            Section::Paper => self.paper_section(ui),
        }
    }

    /// While a section is dragged: show where it would land, and move it there
    /// when the mouse is let go. Called after both side panels are drawn.
    pub(super) fn section_drop(&mut self, ctx: &egui::Context) {
        let frame = std::mem::take(&mut self.dock_frame);
        let Some(dragged) = egui::DragAndDrop::payload::<Section>(ctx).map(|s| *s) else { return };
        let Some(pos) = ctx.pointer_latest_pos() else { return };
        // The column under the pointer, and the gap between sections nearest it.
        let target = frame.columns.iter().find(|(_, r)| r.contains(pos)).map(|&(side, column)| {
            let rects: Vec<Rect> = frame.sections.iter().filter(|(s, ..)| *s == side).map(|(.., r)| *r).collect();
            let index = rects.iter().filter(|r| r.center().y < pos.y).count();
            let y = match (index.checked_sub(1).map(|i| rects[i]), rects.get(index)) {
                (Some(above), Some(below)) => (above.bottom() + below.top()) / 2.0,
                (Some(above), None) => above.bottom() + 2.0,
                (None, Some(below)) => below.top() - 2.0,
                (None, None) => column.top() + 4.0,
            };
            (side, index, column, y.clamp(column.top() + 1.0, column.bottom() - 1.0))
        });

        if ctx.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ctx);
            if let Some((side, index, ..)) = target
                && self.app_settings.panels.move_section(dragged, side, index)
            {
                self.save_app_settings();
            }
            return;
        }

        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("section-drop")));
        let accent = ctx.global_style().visuals.selection.bg_fill;
        // Dim the section being moved, where it is now.
        if let Some(&(side, _, rect)) = frame.sections.iter().find(|(_, s, _)| *s == dragged)
            && let Some(&(_, column)) = frame.columns.iter().find(|(s, _)| *s == side)
        {
            painter.with_clip_rect(column).rect_filled(rect, 3.0, accent.gamma_multiply(0.15));
        }
        if let Some((_, _, column, y)) = target {
            let line = Rect::from_x_y_ranges(column.x_range().shrink(4.0), (y - 1.5)..=(y + 1.5));
            painter.rect_filled(line, 1.5, accent);
        }
        // The title, following the pointer.
        let font = egui::TextStyle::Button.resolve(&ctx.global_style());
        let text = ctx.global_style().visuals.strong_text_color();
        let galley = painter.layout_no_wrap(dragged.title().to_string(), font, text);
        let ghost = Rect::from_min_size(pos + Vec2::new(12.0, 4.0), galley.size()).expand2(Vec2::new(8.0, 4.0));
        let ghost_painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("section-drag")));
        ghost_painter.rect(ghost, 4.0, ctx.global_style().visuals.window_fill, LineStroke::new(1.0, accent), StrokeKind::Inside);
        ghost_painter.galley(ghost.min + Vec2::new(8.0, 4.0), galley, text);
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    fn tools_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                ui.selectable_value(&mut self.tool, tool, icon_text(tool_icon(tool)))
                    .on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()));
            }
            for name in FUTURE_TOOLS {
                ui.add_enabled(false, egui::Button::new(icon_text(future_tool_icon(name))))
                    .on_disabled_hover_text(format!("{name} (coming later)"));
            }
        });
    }

    fn properties_section(&mut self, ui: &mut egui::Ui) {
        let tool = self.tool;
        ui.horizontal(|ui| {
            if tool.paints() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::hover());
                ui.painter().rect_filled(rect, 3.0, to_color32(self.color));
                ui.painter().rect_stroke(rect, 3.0, LineStroke::new(1.0, crate::theme::outline(ui)), StrokeKind::Inside);
            }
            ui.strong(tool.label());
        });
        if tool == Tool::Picker {
            ui.weak("Click the canvas to pick a color. (With any tool, hold ⌥ and click.)");
            self.picker_sample(ui);
            return;
        }
        if tool == Tool::Select {
            self.select_properties(ui);
            return;
        }
        if tool == Tool::Hatch {
            // The hatch's kind first, with which ends of its lines point.
            self.hatch_mode(ui);
            taper_ends_controls(ui, &mut self.tool_states[tool.index()].shape);
        }
        ui.add_space(4.0);
        if tool.is_wet_medium() {
            self.wet_media_properties(ui);
            return;
        }

        let state = &mut self.tool_states[tool.index()];
        size_slider(ui, state);
        opacity_slider(ui, tool, state);
        if tool == Tool::Hatch {
            taper_slider(ui, &mut state.shape);
        } else if tool.paints() || tool == Tool::Eraser {
            taper_controls(ui, &mut state.shape);
        }

        if tool == Tool::Hatch {
            ui.add_space(4.0);
            self.hatch_properties(ui);
        }
        if tool.paints() || tool == Tool::Smudge {
            self.mouse_pressure_slider(ui);
        }
    }

    /// Oils & Acrylics and the Knife: the choices (paint mode, taper ends,
    /// checkboxes) at the top, then all the sliders together.
    fn wet_media_properties(&mut self, ui: &mut egui::Ui) {
        let tool = self.tool;

        // ----- Choices -----
        self.paint_mode_buttons(ui);
        taper_ends_controls(ui, &mut self.tool_states[tool.index()].shape);
        self.dirty_brush_row(ui);
        let mut off = false;
        ui.add_enabled(false, egui::Checkbox::new(&mut off, "Multicolor brush"))
            .on_disabled_hover_text("Load several colors on the brush: coming later");

        // ----- Sliders -----
        ui.add_space(4.0);
        let state = &mut self.tool_states[tool.index()];
        size_slider(ui, state);
        opacity_slider(ui, tool, state);
        taper_slider(ui, &mut state.shape);
        // Combined Oils & Acrylics: oiliness is what makes it
        // behave more like oil (blends, drags) or acrylic (covers).
        loading_slider(ui, state);
        length_slider(ui, state);
        oiliness_slider(ui, state);
        thickness_slider(ui, state);
        let mut placeholder = 0.0;
        ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=1.0).text("Impasto depth"))
            .on_disabled_hover_text("Paint thickness: coming with impasto (milestone 6)");
        ui.add_enabled(false, egui::Slider::new(&mut placeholder, 0.0..=1.0).text("Gloss"))
            .on_disabled_hover_text("Shine on thick paint: coming with impasto (milestone 6)");
        self.mouse_pressure_slider(ui);
    }

    /// Picker: how many pixels it averages.
    pub(super) fn picker_sample(&mut self, ui: &mut egui::Ui) {
        let label = |n: usize| if n == 1 { "Point".to_string() } else { format!("{n} x {n} average") };
        let old = self.app_settings.picker_sample;
        ui.horizontal(|ui| {
            ui.label("Sample size:");
            egui::ComboBox::from_id_salt("picker-sample").selected_text(label(old)).show_ui(ui, |ui| {
                for n in [1, 3, 5, 11, 31] {
                    ui.selectable_value(&mut self.app_settings.picker_sample, n, label(n));
                }
            });
        })
        .response
        .on_hover_text("Average the pixels in a square around the cursor, so one brush streak or speck doesn't decide the color");
        if self.app_settings.picker_sample != old {
            self.save_app_settings();
        }
    }

    /// Oils: the paint modes (keys 1–5). Knife: Spread or Scrape.
    pub(super) fn paint_mode_buttons(&mut self, ui: &mut egui::Ui) {
        let tool = self.tool;
        let mut clicked_mode = None;
        let state = &mut self.tool_states[tool.index()];
        if tool == Tool::Knife {
            // The knife's two jobs instead of paint modes.
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.scrape, false, "Spread")
                    .on_hover_text("Lay thick, smooth paint with crisp edges; drags wet paint along and pushes it into ridges");
                ui.selectable_value(&mut state.scrape, true, "Scrape")
                    .on_hover_text("An empty knife: scrapes wet paint off, leaving a thin smeared film (dried paint stays)");
            });
        } else {
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
        }
        if let Some(mode) = clicked_mode {
            self.set_paint_mode(mode);
        }
    }

    /// Dirty brush, with the paint left on it and a Clean button.
    pub(super) fn dirty_brush_row(&mut self, ui: &mut egui::Ui) {
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
    }

    pub(super) fn mouse_pressure_slider(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::Slider::new(&mut self.mouse_pressure, 0.05..=1.0).text("Pressure"))
            .on_hover_text("Pressure used when painting with a mouse (the pen uses its real pressure)");
    }

    /// The brush in use, by preset name, with its size: "Bristle Flat · size 14".
    /// "(edited)" once its shape is changed beyond the size.
    fn current_brush_label(&self, ui: &mut egui::Ui) {
        if !self.tool.paints() && self.tool != Tool::Eraser && self.tool != Tool::Smudge {
            return;
        }
        let shape = &self.state().shape;
        let find = |name: &str| self.presets.entries.iter().find(|e| e.preset.name == name);
        // The preset picked last for this tool (if it's still there), else one that matches.
        let picked = self.brush_names[self.tool.index()].as_deref().and_then(find);
        let (name, edited) = match picked {
            Some(e) => (e.preset.name.as_str(), !e.preset.matches_any_size(shape)),
            None => match self.presets.entries.iter().find(|e| e.preset.matches_any_size(shape)) {
                Some(e) => (e.preset.name.as_str(), false),
                None => ("Custom brush", false),
            },
        };
        ui.horizontal(|ui| {
            ui.weak("Brush");
            ui.strong(name).on_hover_text(format!("The brush in use for {}", self.tool.label()));
            if edited {
                ui.weak("(edited)").on_hover_text("Changed since you picked it (other than size)");
            }
            ui.weak(format!("· size {:.0}", shape.radius)).on_hover_text("Brush size (as on the Size slider): [ and ] change it");
        });
    }

    fn presets_section(&mut self, ui: &mut egui::Ui) {
        let mut apply = None;
        let mut delete = None;
        let mut move_to: Option<(usize, String)> = None;
        // Search across all groups.
        ui.add(egui::TextEdit::singleline(&mut self.brush_search).hint_text("🔍 Search brushes").desired_width(f32::INFINITY));
        let search = self.brush_search.trim().to_lowercase();
        let groups = self.presets.groups();
        // A wrapping grid of small tiles (thumbnail + name) that fits any panel width.
        let thumb = Vec2::new(THUMB_W as f32, THUMB_H as f32) * 0.75;
        let cell = Vec2::new(thumb.x + 8.0, thumb.y + 22.0);
        let current = self.state().shape;
        for group in &groups {
            let members: Vec<usize> = (0..self.presets.entries.len())
                .filter(|&i| self.presets.entries[i].group() == group.as_str())
                .filter(|&i| search.is_empty() || self.presets.entries[i].preset.name.to_lowercase().contains(&search))
                .collect();
            if members.is_empty() {
                continue;
            }
            let holds_current = members.iter().any(|&i| self.presets.entries[i].preset.matches(&current));
            let header = egui::CollapsingHeader::new(format!("{group}  ({})", members.len()))
                .id_salt(("brush-group", group.as_str()))
                .default_open(holds_current);
            // While searching, show every matching group open.
            let header = if search.is_empty() { header } else { header.open(Some(true)) };
            header.show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(4.0);
                    for &i in &members {
                        let entry = &self.presets.entries[i];
                        let selected = entry.preset.matches(&current);
                        let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
                        let response = response.on_hover_text(&entry.preset.name);
                        let visuals = ui.style().interact_selectable(&response, selected);
                        if selected || response.hovered() {
                            ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
                        }
                        let image_rect = egui::Rect::from_min_size(rect.min + Vec2::new(4.0, 3.0), thumb);
                        egui::Image::new(&entry.thumbnail).paint_at(ui, image_rect);
                        let name = ui.painter().layout(entry.preset.name.clone(), egui::FontId::proportional(11.0), visuals.text_color(), cell.x - 4.0);
                        let text_pos = egui::pos2(rect.center().x - name.size().x / 2.0, image_rect.max.y + 2.0);
                        ui.painter().with_clip_rect(rect).galley(text_pos, name, visuals.text_color());
                        if response.clicked() {
                            apply = Some(i);
                        }
                        // Your own presets: move to another group, or delete.
                        if entry.file.is_some() {
                            response.context_menu(|ui| {
                                ui.menu_button("Move to group", |ui| {
                                    for other in groups.iter().filter(|g| *g != group) {
                                        if ui.button(other).clicked() {
                                            move_to = Some((i, other.clone()));
                                        }
                                    }
                                    if !groups.iter().any(|g| g == crate::presets::MY_BRUSHES) && ui.button(crate::presets::MY_BRUSHES).clicked() {
                                        move_to = Some((i, crate::presets::MY_BRUSHES.to_string()));
                                    }
                                    ui.separator();
                                    ui.weak("New group: name it when saving in the Brush Creator");
                                });
                                if ui.button("Delete brush").clicked() {
                                    delete = Some(i);
                                }
                            });
                        }
                    }
                });
            });
        }
        if let Some((i, group)) = move_to {
            self.presets.set_group(i, &group);
        }
        if let Some(i) = apply {
            if self.tool == Tool::Picker {
                self.tool = Tool::OilsAcrylics;
            }
            let shape = self.presets.entries[i].preset.apply_to(&self.state().shape);
            self.state_mut().shape = shape;
            self.brush_names[self.tool.index()] = Some(self.presets.entries[i].preset.name.clone());
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
            ui.painter_at(thumb).add(egui::Shape::line(outline, LineStroke::new(1.5, ui.visuals().selection.bg_fill)));

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
            // A real button (a selectable label looks like plain text until it's on);
            // highlighted while the view is fitted.
            let fit = egui::Button::new("Fit to Window").selected(self.view.fit);
            if ui.add(fit).on_hover_text("Show the whole canvas, as big as the window allows (up to 100%); it stays fitted when the window resizes (. or ⌘0)").clicked() {
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
        self.current_colors(ui);
        self.wheel(ui);
        self.rgb_sliders(ui);
        ui.add_space(6.0);
        ui.label("Pigments");
        self.pigment_swatches(ui);
        self.recent_swatches(ui);
        ui.add_space(6.0);
        self.multicolor_controls(ui);
        if self.mix_key {
            ui.colored_label(ui.visuals().warn_fg_color, "Mix Color: picked colors mix into the current one");
        }
    }

    /// Primary and secondary color: the big swatch is what you paint with;
    /// click the small one (or ⌘\\) to swap them. Then the wheel's RYB/HSV.
    pub(super) fn current_colors(&mut self, ui: &mut egui::Ui) {
        let mut swap = false;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(54.0, 44.0), Sense::hover());
            let secondary = Rect::from_min_size(rect.min + Vec2::new(22.0, 16.0), Vec2::new(30.0, 26.0));
            let primary = Rect::from_min_size(rect.min, Vec2::new(36.0, 32.0));
            let back = ui.interact(secondary, ui.id().with("secondary-color"), Sense::click());
            ui.painter().rect_filled(secondary, 3.0, to_color32(self.secondary));
            ui.painter().rect_stroke(secondary, 3.0, LineStroke::new(1.0, crate::theme::outline(ui)), StrokeKind::Inside);
            ui.painter().rect_filled(primary, 3.0, to_color32(self.color));
            ui.painter().rect_stroke(primary, 3.0, LineStroke::new(1.5, ui.visuals().text_color()), StrokeKind::Inside);
            if back.on_hover_text("Secondary color: click (or ⌘\\) to swap").clicked() {
                swap = true;
            }
            ui.vertical(|ui| {
                ui.monospace(oxide_commands::palette::to_hex(self.color));
                ui.horizontal(|ui| {
                    if ui.small_button("⇄ Swap").on_hover_text("Swap primary and secondary colors (⌘\\)").clicked() {
                        swap = true;
                    }
                    if ui.small_button("Pick").on_hover_text("Pick a color from the painting: click the canvas, then you're back to your tool. (Or hold ⌥ and click the canvas with any tool; the Picker tool, I, keeps picking.)").clicked() {
                        self.pick_once();
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let s = &mut self.app_settings.ryb_wheel;
                let mut changed = ui.selectable_value(s, true, "RYB").on_hover_text("Painter's wheel: red–yellow–blue primaries, paint complements opposite (red–green, yellow–purple, blue–orange)").changed();
                changed |= ui.selectable_value(s, false, "HSV").on_hover_text("Screen colors: red–green–blue").changed();
                if changed {
                    self.save_app_settings();
                }
            });
        });
        if swap {
            self.swap_colors();
        }
    }

    pub(super) fn wheel(&mut self, ui: &mut egui::Ui) {
        let diameter = ui.available_width().min(220.0);
        let mut hsv = self.hsv;
        let ryb = self.app_settings.ryb_wheel;
        ui.vertical_centered(|ui| {
            if color_wheel(ui, &mut hsv, diameter, ryb) {
                self.set_hsv(hsv);
            }
        });
    }

    /// RGB sliders (0-255), like most paint apps.
    pub(super) fn rgb_sliders(&mut self, ui: &mut egui::Ui) {
        let mut rgb = [self.color.r, self.color.g, self.color.b].map(|v| (v * 255.0).round());
        let mut changed = false;
        for (value, name) in rgb.iter_mut().zip(["R", "G", "B"]) {
            changed |= ui.add(egui::Slider::new(value, 0.0..=255.0).max_decimals(0).text(name)).changed();
        }
        if changed {
            self.set_color(Color::rgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0));
        }
    }

    pub(super) fn pigment_swatches(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(3.0);
            for (name, color) in SWATCHES {
                if swatch(ui, color).on_hover_text(format!("{name}\n⌥-click: add to the MultiColored brush")).clicked() {
                    clicked = Some(color);
                }
            }
        });
        self.swatch_clicked(clicked, ui);
    }

    /// The last colors used, under a "Recent" label (nothing until there are some).
    pub(super) fn recent_swatches(&mut self, ui: &mut egui::Ui) {
        if self.recent_colors.is_empty() {
            return;
        }
        let mut clicked = None;
        ui.label("Recent");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(3.0);
            for &color in &self.recent_colors {
                if swatch(ui, color).clicked() {
                    clicked = Some(color);
                }
            }
        });
        self.swatch_clicked(clicked, ui);
    }

    /// Paint with a clicked swatch, or ⌥-click: load it onto the MultiColored brush.
    fn swatch_clicked(&mut self, color: Option<Color>, ui: &egui::Ui) {
        let Some(color) = color else { return };
        if ui.input(|i| i.modifiers.alt) {
            self.add_multicolor(color);
        } else {
            self.pick_color(color);
            if !self.tool.paints() {
                self.tool = Tool::OilsAcrylics;
            }
        }
    }

    /// MultiColored brush (⌥M): several colors loaded side by side.
    pub(super) fn multicolor_controls(&mut self, ui: &mut egui::Ui) {
        let mut load = None;
        let mut on = self.multicolor_on;
        if ui
            .checkbox(&mut on, "MultiColored brush   ⌥M")
            .on_hover_text("Load several colors side by side across the brush. A stroke starts striped with all of them, and they blend toward their average as you paint.")
            .changed()
        {
            self.run_command_multicolor();
        }
        if self.multicolor_on {
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(3.0);
                for (n, &color) in self.multicolor.iter().enumerate() {
                    if swatch(ui, color).on_hover_text("Click to remove").clicked() {
                        remove = Some(n);
                    }
                }
            });
            if let Some(n) = remove {
                self.multicolor.remove(n);
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(self.multicolor.len() < 6, egui::Button::new("Add current color")).clicked() {
                    load = Some(self.color);
                }
                if ui.button("Clear").clicked() {
                    self.multicolor.clear();
                }
            });
            if self.multicolor.len() < 2 {
                ui.weak("Load at least two colors (⌥-click swatches, or Add current color).");
            }
        }
        if let Some(color) = load {
            self.add_multicolor(color);
        }
    }
}

impl OxideApp {
    /// Wet media controls: dry or rewet the paint.
    fn wet_media_section(&mut self, ui: &mut egui::Ui) {
        let active = self.document.active_index();
        let wet_here = self.document.layers()[active].canvas.has_wet_paint();
        ui.label(if wet_here { "This layer has wet paint." } else { "This layer is dry." });
        self.dry_buttons(ui);
        let mut show = self.document.show_wet();
        if ui.checkbox(&mut show, "Show wet paint").on_hover_text("Tint wet areas blue").changed() {
            self.document.set_show_wet(show);
        }
    }

    /// Dry layer, Dry all, Rewet.
    pub(super) fn dry_buttons(&mut self, ui: &mut egui::Ui) {
        let active = self.document.active_index();
        let wet_here = self.document.layers()[active].canvas.has_wet_paint();
        let wet_anywhere = self.document.layers().iter().any(|l| l.canvas.has_wet_paint());
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(wet_here, egui::Button::new("Dry layer")).on_hover_text("Lock the paint in (⌘D)").clicked() {
                self.run(Command::DryLayer, ui.ctx());
            }
            if ui.add_enabled(wet_anywhere, egui::Button::new("Dry all")).on_hover_text("Dry every layer (⇧⌘D)").clicked() {
                self.run(Command::DryAll, ui.ctx());
            }
            if ui.button("Rewet").on_hover_text("Make this layer's paint wet again so it can be blended").clicked() {
                self.run(Command::RewetLayer, ui.ctx());
            }
        });
    }
}

/// A small clickable color square.
fn swatch(ui: &mut egui::Ui, color: Color) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let stroke = if response.hovered() {
        LineStroke::new(2.0, ui.visuals().text_color())
    } else {
        LineStroke::new(1.0, crate::theme::outline(ui))
    };
    ui.painter().rect_filled(rect, 2.0, to_color32(color));
    ui.painter().rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
    response
}

// The tool sliders, shared by Properties, the Quick section and Settings.

pub(super) fn size_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.shape.radius, 0.5..=200.0).logarithmic(true).text("Size"));
}

/// Opacity, or Strength for Smudge (kept in `oiliness`).
pub(super) fn opacity_slider(ui: &mut egui::Ui, tool: Tool, state: &mut ToolState) {
    if tool == Tool::Smudge {
        ui.add(egui::Slider::new(&mut state.oiliness, 0.05..=1.0).text("Strength")).on_hover_text("How far the smudge drags the paint");
    } else {
        ui.add(egui::Slider::new(&mut state.opacity, 0.01..=1.0).text("Opacity"));
    }
}

pub(super) fn loading_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.loading, 0.0..=1.0).text("Loading"))
        .on_hover_text("How much paint the brush lays down (⌘-drag up/down on the canvas)");
}

pub(super) fn length_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.length, 0.0..=1.0).text("Length"))
        .on_hover_text("How long the paint lasts: low = runs out fast, 1 = never runs out");
}

pub(super) fn oiliness_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.oiliness, 0.0..=1.0).text("Oiliness"))
        .on_hover_text("Low = acrylic-like: covers wet paint and stays clean.\nHigh = oil-like: picks up and blends wet paint.\n(Dried paint is never picked up.)");
}

pub(super) fn thickness_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.thickness, 0.0..=1.0).text("Thickness"))
        .on_hover_text("Impasto: how thick the paint goes on. Thick paint shows its brush marks in the light (Paper panel: Impasto depth, Gloss). 0 = flat.");
}

/// A drop-down of every brush preset, by group. Picking one sets `shape`
/// (size too, as in Brush presets) and `brush` to its name.
pub(super) fn brush_combo(ui: &mut egui::Ui, id: &str, presets: &crate::presets::PresetLibrary, shape: &mut BrushShape, brush: &mut Option<String>) {
    let picked = brush.clone().unwrap_or_else(|| "Custom brush".into());
    ui.horizontal(|ui| {
        ui.label("Brush");
        egui::ComboBox::from_id_salt(id).selected_text(picked).width(ui.available_width()).height(400.0).show_ui(ui, |ui| {
            for group in presets.groups() {
                ui.weak(&group);
                for entry in presets.entries.iter().filter(|e| e.group() == group) {
                    let name = &entry.preset.name;
                    if ui.selectable_label(brush.as_deref() == Some(name.as_str()), name).clicked() {
                        *shape = entry.preset.apply_to(shape);
                        *brush = Some(name.clone());
                    }
                }
            }
        });
    });
}

/// The Taper slider, and which ends it points (Properties and Brush Creator).
pub(super) fn taper_controls(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape) {
    taper_slider(ui, shape);
    taper_ends_controls(ui, shape);
}

/// Just the Taper slider (Hatch shows the ends choice at the top instead).
pub(super) fn taper_slider(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape) {
    ui.add(egui::Slider::new(&mut shape.taper, 0.0..=1.0).text("Taper"))
        .on_hover_text("Strokes come to a clean point, like an engraved line, even with a mouse (0 = off)");
}

/// Both ends / Reverse: which ends the taper points (greyed out while Taper
/// is 0, so they don't come and go).
fn taper_ends_controls(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape) {
    use oxide_engine::TaperEnds;
    ui.horizontal(|ui| {
        if shape.taper <= 0.0 {
            ui.disable();
        }
        let mut both = shape.taper_ends == TaperEnds::Both;
        if ui
            .checkbox(&mut both, "Both ends")
            .on_hover_text("Point both ends of the stroke. Off: full size at one end, tapering to a point at the other")
            .on_disabled_hover_text("Turn Taper up above 0 to point the ends of the stroke")
            .changed()
        {
            shape.taper_ends = if both { TaperEnds::Both } else { TaperEnds::End };
        }
        if !both {
            let reversed = shape.taper_ends == TaperEnds::Start;
            let hint = "Which end gets the point. Off: the stroke starts full size and tapers where it ends. On: it starts at a point and ends full size (Hatch: Break up moves to that end too).\n(Hatch lines end at the second rail, or where you finished the rails for Along.)";
            if ui.selectable_label(reversed, "⇄ Reverse").on_hover_text(hint).clicked() {
                shape.taper_ends = if reversed { TaperEnds::End } else { TaperEnds::Start };
            }
        }
    });
}

/// A tool button's icon, a little larger than the text so it reads at a glance.
pub(super) fn icon_text(icon: &str) -> egui::RichText {
    egui::RichText::new(icon).size(18.0)
}

/// The Phosphor icon on each tool's button (the name is in its tooltip).
pub(super) fn tool_icon(tool: Tool) -> &'static str {
    use egui_phosphor::regular as icon;
    match tool {
        Tool::OilsAcrylics => icon::PAINT_BRUSH_BROAD,
        Tool::Ink => icon::PEN_NIB,
        Tool::Pencil => icon::PENCIL_SIMPLE,
        Tool::Marker => icon::HIGHLIGHTER,
        Tool::Airbrush => icon::SPRAY_BOTTLE,
        Tool::Eraser => icon::ERASER,
        Tool::Smudge => icon::HAND_POINTING,
        Tool::Knife => icon::KNIFE,
        Tool::Hatch => icon::HASH,
        Tool::Select => icon::SELECTION,
        Tool::Picker => icon::EYEDROPPER,
    }
}

/// Icons for the tools in [`FUTURE_TOOLS`].
fn future_tool_icon(name: &str) -> &'static str {
    use egui_phosphor::regular as icon;
    match name {
        "Pastel" => icon::SCRIBBLE,
        "Blend" => icon::DROP_HALF,
        "Fill" => icon::PAINT_BUCKET,
        _ => icon::QUESTION,
    }
}
