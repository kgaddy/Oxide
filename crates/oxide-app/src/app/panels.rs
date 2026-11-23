//! The side rails and their popovers. Each side is a narrow column of icon
//! buttons (the tools, then one button per section); a section's button opens
//! it as a floating popover you can drag by its title bar and resize from its
//! bottom-right grip. Any number can be open at once, and they are remembered.

use eframe::egui::{self, Rect, Sense, Stroke as LineStroke, StrokeKind, Vec2};
use oxide_engine::{Color, PaintMode};

use super::canvas_view::view_xform;
use super::{Command, OxideApp, to_color32};
use crate::color_wheel::{classic_picker, color_wheel};
use crate::panel_layout::{SavedPopover, Section, Side};
use crate::presets::{THUMB_H, THUMB_W};
use crate::tools::{BrushShape, FUTURE_TOOLS, SWATCHES, Tool, ToolState};

/// One open popover in the minimal layout.
#[derive(Clone, Copy)]
pub(super) struct Popover {
    section: Section,
    /// Its rail button, which it sits beside until it's dragged elsewhere.
    anchor: Rect,
    /// Where it was dragged to (its top-left corner), if it was.
    moved: Option<egui::Pos2>,
    /// Where it was drawn last frame.
    rect: Rect,
    /// A height set by dragging the bottom-right grip; otherwise it fits its content.
    height: Option<f32>,
    /// How tall its scrolling body was drawn last frame (where a resize starts from).
    body_h: f32,
}

/// What one popover reported after being drawn.
struct PopoverDrawn {
    rect: Rect,
    close: bool,
    /// How far the title bar was dragged this frame.
    drag: Vec2,
    body_h: f32,
    /// How far the resize grip was dragged up or down this frame.
    resize: f32,
}

impl Popover {
    /// A popover opened again from the saved settings; its rail button's rect
    /// is filled in when the rail is drawn.
    pub(super) fn from_saved(saved: &SavedPopover) -> Popover {
        Popover {
            section: saved.section,
            anchor: Rect::NOTHING,
            moved: saved.pos.map(|[x, y]| egui::pos2(x, y)),
            rect: Rect::NOTHING,
            height: saved.height,
            body_h: 0.0,
        }
    }
}

impl OxideApp {
    /// Minimal layout: a narrow column of icon buttons. The tools are shown
    /// directly; every other section on this side is a button that opens it
    /// as a popover.
    pub(super) fn rail(&mut self, ui: &mut egui::Ui, side: Side) {
        ui.vertical_centered(|ui| {
            for section in self.app_settings.panels.side(side).to_vec() {
                if section == Section::Tools {
                    for tool in Tool::ALL {
                        ui.selectable_value(&mut self.tool, tool, icon_text(tool_icon(tool))).on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()));
                    }
                    ui.separator();
                    continue;
                }
                let at = self.popovers.iter().position(|p| p.section == section);
                let open = at.is_some();
                let button = ui.add(egui::Button::new(icon_text(section_icon(section))).selected(open));
                let button = button.on_hover_text(section.title());
                if let Some(i) = at {
                    self.popovers[i].anchor = button.rect;
                }
                if button.clicked() {
                    match at {
                        Some(i) => {
                            self.popovers.remove(i);
                        }
                        None => self.popovers.push(Popover { section, anchor: button.rect, moved: None, rect: button.rect, height: None, body_h: 0.0 }),
                    }
                }
            }
        });
    }

    /// Minimal layout: every open section, floating beside its rail. They stay
    /// open (even while painting) until their rail button or ✕ is clicked.
    /// Several on one side sit side by side, the first opened nearest the rail;
    /// drag a title to put one anywhere.
    pub(super) fn popover_windows(&mut self, ctx: &egui::Context) {
        let mut offset = [0.0_f32; 2];
        let mut closed = Vec::new();
        for i in 0..self.popovers.len() {
            let pop = self.popovers[i];
            let right_side = self.app_settings.panels.right.contains(&pop.section);
            let used = &mut offset[usize::from(right_side)];
            let drawn = self.popover_window(ctx, pop, right_side, *used);
            let (rect, close, drag) = (drawn.rect, drawn.close, drawn.drag);
            if pop.moved.is_none() {
                *used += rect.width() + 8.0;
            }
            self.popovers[i].rect = rect;
            self.popovers[i].body_h = drawn.body_h;
            if drag != Vec2::ZERO {
                self.popovers[i].moved = Some(rect.min + drag);
            }
            if drawn.resize != 0.0 {
                let most = (ctx.content_rect().height() - 120.0).max(200.0);
                self.popovers[i].height = Some((drawn.body_h + drawn.resize).clamp(80.0, most));
            }
            if close {
                closed.push(pop.section);
            }
        }
        self.popovers.retain(|p| !closed.contains(&p.section));
        // Remember which are open and where (once the mouse is up, not on every frame of a drag).
        if !ctx.input(|i| i.pointer.any_down()) {
            let now: Vec<SavedPopover> =
                self.popovers.iter().map(|p| SavedPopover { section: p.section, pos: p.moved.map(|m| [m.x, m.y]), height: p.height }).collect();
            if now != self.app_settings.open_popovers {
                self.app_settings.open_popovers = now;
                self.save_app_settings();
            }
        }
    }

    /// One popover, `shift` pixels further out from its rail unless it was
    /// moved.
    fn popover_window(&mut self, ctx: &egui::Context, pop: Popover, right_side: bool, shift: f32) -> PopoverDrawn {
        let Popover { section, anchor, moved, height, .. } = pop;
        let (pos, pivot) = match moved {
            Some(p) => (p, egui::Align2::LEFT_TOP),
            None if right_side => (egui::pos2(anchor.left() - 6.0 - shift, anchor.top()), egui::Align2::RIGHT_TOP),
            None => (egui::pos2(anchor.right() + 6.0 + shift, anchor.top()), egui::Align2::LEFT_TOP),
        };
        // As tall as the window allows: the area is nudged up from the button to fit.
        let max_height = (ctx.content_rect().height() - 120.0).max(200.0);
        // A height you set with the grip wins; the body is then exactly that tall.
        let (body_min, body_max) = match height {
            Some(h) => (h.min(max_height), h.min(max_height)),
            None => (0.0, max_height),
        };
        let mut body_h = pop.body_h;
        let mut resize = 0.0;
        let width = match section {
            Section::Properties | Section::BrushPresets => 340.0,
            Section::Color | Section::Layers | Section::Quick => 300.0,
            _ => 260.0,
        };
        let title = match section {
            Section::Quick => format!("Quick · {}", self.tool.label()),
            _ => section.title().to_string(),
        };
        let mut close = false;
        let mut drag = Vec2::ZERO;
        let area = egui::Area::new(egui::Id::new(("minimal-popover", section)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .pivot(pivot)
            .constrain_to(ctx.content_rect())
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    // The whole title bar drags the popover. The drag area is added
                    // first, so the buttons on top of it still win their clicks.
                    let bar = Rect::from_min_size(ui.cursor().min, Vec2::new(width, ui.spacing().interact_size.y));
                    let grab = ui.interact(bar, egui::Id::new(("popover-grab", section)), Sense::drag()).on_hover_cursor(egui::CursorIcon::Grab);
                    drag = grab.drag_delta();
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(title).strong()).selectable(false));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let x = egui::Button::new(egui::RichText::new(egui_phosphor::regular::X).weak()).small().frame(false);
                            close = ui.add(x).on_hover_text("Close").clicked();
                            let help = egui::Button::new(egui::RichText::new(egui_phosphor::regular::QUESTION).weak()).small().frame(false);
                            if ui.add(help).on_hover_text(format!("Help for {}", section.title())).clicked() {
                                self.help.show_page(&super::help::section_page(section, self.tool));
                            }
                            if section == Section::Quick {
                                let editing = self.quick_editing;
                                let text = if editing { "Done".to_string() } else { format!("{} Edit", egui_phosphor::regular::PENCIL_SIMPLE) };
                                if ui.add(egui::Button::new(text).small().selected(editing)).clicked() {
                                    self.quick_editing = !editing;
                                }
                            }
                        });
                    });
                    ui.separator();
                    // The area's own room is just what it used last frame, so a short
                    // first frame would keep it short. Give the contents their own budget.
                    let room = Rect::from_min_size(ui.cursor().min, Vec2::new(width, max_height + 20.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                        let scroll =
                            egui::ScrollArea::vertical().min_scrolled_height(body_min).max_height(body_max).auto_shrink([false, true]).show(ui, |ui| {
                                match section {
                                    Section::Properties => {
                                        self.current_brush_label(ui);
                                        self.properties_section(ui);
                                    }
                                    Section::BrushPresets => {
                                        ui.weak(format!("Shapes for {}", self.tool.label()));
                                        self.presets_section(ui);
                                    }
                                    _ => self.section_contents(ui, section),
                                }
                                // Room under the last control, so it isn't clipped at the scroll edge.
                                ui.add_space(14.0);
                            });
                        body_h = scroll.inner_rect.height();
                        // The resize grip, bottom right: drag it up or down (vertical only).
                        let (strip, grip) = ui.allocate_exact_size(Vec2::new(width, 14.0), Sense::drag());
                        let grip = grip.on_hover_cursor(egui::CursorIcon::ResizeVertical).on_hover_text("Drag to change the height");
                        resize = grip.drag_delta().y;
                        let color = ui.style().visuals.widgets.noninteractive.fg_stroke.color;
                        let corner = strip.right_bottom() - Vec2::new(2.0, 2.0);
                        for k in [4.0, 8.0, 12.0] {
                            ui.painter().line_segment([corner - Vec2::new(k, 0.0), corner - Vec2::new(0.0, k)], LineStroke::new(1.0, color));
                        }
                    });
                });
            });
        PopoverDrawn { rect: area.response.rect, close, drag, body_h, resize }
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

    fn tools_section(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for tool in Tool::ALL {
                ui.selectable_value(&mut self.tool, tool, icon_text(tool_icon(tool))).on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()));
            }
            for name in FUTURE_TOOLS {
                ui.add_enabled(false, egui::Button::new(icon_text(future_tool_icon(name)))).on_disabled_hover_text(format!("{name} (coming later)"));
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
            taper_ends_controls(ui, &mut self.tool_states[tool.index()].shape, true);
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

    /// Oils & Acrylics and the Knife: the choices (Flat paint, paint mode,
    /// checkboxes) at the top, then all the sliders together.
    fn wet_media_properties(&mut self, ui: &mut egui::Ui) {
        let tool = self.tool;

        // ----- Choices -----
        flat_checkbox(ui, &mut self.tool_states[tool.index()]);
        self.paint_mode_buttons(ui);
        self.dirty_brush_row(ui);
        let mut off = false;
        ui.add_enabled(false, egui::Checkbox::new(&mut off, "Multicolor brush")).on_disabled_hover_text("Load several colors on the brush: coming later");

        // ----- Sliders -----
        ui.add_space(4.0);
        let state = &mut self.tool_states[tool.index()];
        size_slider(ui, state);
        opacity_slider(ui, tool, state);
        taper_controls(ui, &mut state.shape);
        // Combined Oils & Acrylics: oiliness is what makes it
        // behave more like oil (blends, drags) or acrylic (covers).
        loading_slider(ui, state);
        oiliness_slider(ui, state);
        thickness_slider(ui, state);
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

    /// "New group…" on a brush: ask for the group's name, then move the brush there.
    pub(super) fn new_group_dialog(&mut self, ctx: &egui::Context) {
        let Some((index, mut name)) = self.new_group.take() else { return };
        let Some(entry) = self.presets.entries.get(index) else { return };
        let brush = entry.preset.name.clone();
        let (mut ok, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("new-brush-group")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("New group");
            ui.label(format!("Move \"{brush}\" into a new group called:"));
            let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
            edit.request_focus();
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let ready = !name.trim().is_empty();
                ok = (ui.add_enabled(ready, egui::Button::new("Create")).clicked() || enter) && ready;
                cancel = ui.button("Cancel").clicked();
            });
        });
        if ok {
            self.presets.set_group(index, name.trim());
        } else if !cancel && !modal.should_close() {
            self.new_group = Some((index, name)); // still asking
        }
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
        let mut ask_new_group: Option<usize> = None;
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
            let header =
                egui::CollapsingHeader::new(format!("{group}  ({})", members.len())).id_salt(("brush-group", group.as_str())).default_open(holds_current);
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
                                    if ui.button("New group…").clicked() {
                                        ask_new_group = Some(i);
                                        ui.close();
                                    }
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
        if let Some(i) = ask_new_group {
            self.new_group = Some((i, String::new()));
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
            if ui
                .add(fit)
                .on_hover_text("Show the whole canvas, as big as the window allows (up to 100%); it stays fitted when the window resizes (. or ⌘0)")
                .clicked()
            {
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
            if ui
                .add(egui::Slider::new(&mut rotation, -180.0..=180.0).suffix("°").max_decimals(0))
                .on_hover_text("Rotate the view (or hold R and drag on the canvas)")
                .changed()
            {
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
                    if ui.small_button(format!("{} Swap", egui_phosphor::regular::ARROWS_LEFT_RIGHT)).on_hover_text("Swap primary and secondary colors (⌘\\)").clicked() {
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
        if self.app_settings.color_style == crate::settings::ColorStyle::Classic {
            if classic_picker(ui, &mut hsv, ryb) {
                self.set_hsv(hsv);
            }
            return;
        }
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

    /// Dry layer and Rewet (Dry all stays in the Layer menu).
    pub(super) fn dry_buttons(&mut self, ui: &mut egui::Ui) {
        let active = self.document.active_index();
        let wet_here = self.document.layers()[active].canvas.has_wet_paint();
        // Big, colored buttons so they are easy to find: Dry layer is a yellow
        // orange and Rewet the complementary blue.
        let c = egui::Color32::from_rgb;
        let dry_layer_fill = c(245, 175, 40);
        let rewet_fill = c(45, 110, 200);
        let big = |text: &str, fill: egui::Color32, ink: egui::Color32| {
            egui::Button::new(egui::RichText::new(text).size(14.0).strong().color(ink)).fill(fill).min_size(Vec2::new(0.0, 30.0))
        };
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().button_padding = Vec2::new(14.0, 4.0);
            if ui
                .add_enabled(wet_here, big(&format!("{}  Dry layer", egui_phosphor::regular::HAIR_DRYER), dry_layer_fill, egui::Color32::WHITE))
                .on_hover_text("Lock the paint in (⌘D)")
                .clicked()
            {
                self.run(Command::DryLayer, ui.ctx());
            }
            if ui
                .add(big(&format!("{}  Rewet", egui_phosphor::regular::SHOWER), rewet_fill, egui::Color32::WHITE))
                .on_hover_text("Make this layer's paint wet again so it can be blended")
                .clicked()
            {
                self.run(Command::RewetLayer, ui.ctx());
            }
        });
        ui.add_space(6.0);
    }
}

/// A small clickable color square.
fn swatch(ui: &mut egui::Ui, color: Color) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
    let stroke = if response.hovered() { LineStroke::new(2.0, ui.visuals().text_color()) } else { LineStroke::new(1.0, crate::theme::outline(ui)) };
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
    ui.add(egui::Slider::new(&mut state.loading, 0.0..=1.0).text("Loading")).on_hover_text("How much paint the brush lays down (⌘-drag up/down on the canvas)");
}

pub(super) fn oiliness_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add(egui::Slider::new(&mut state.oiliness, 0.0..=1.0).text("Oiliness")).on_hover_text(
        "Low = acrylic-like: covers wet paint and stays clean.\nHigh = oil-like: picks up and blends wet paint.\n(Dried paint is never picked up.)",
    );
}

pub(super) fn thickness_slider(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.add_enabled(!state.flat, egui::Slider::new(&mut state.thickness, 0.0..=1.0).text("Thickness"))
        .on_hover_text("Impasto: how thick the paint goes on. Thick paint shows its brush marks in the light (Paper panel: Impasto depth, Gloss). 0 = flat.")
        .on_disabled_hover_text("Flat paint is on: the brush adds no thickness.");
}

/// Wet media: Flat paint on/off (no thickness; levels thick paint it covers).
pub(super) fn flat_checkbox(ui: &mut egui::Ui, state: &mut ToolState) {
    ui.checkbox(&mut state.flat, "Flat paint").on_hover_text(
        "For blocking in: the paint goes on perfectly flat and levels any thick paint it covers (wet or dried), so going over an area again and again never builds it up.",
    );
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

/// The Taper and Rough ends sliders, and which ends they work on
/// (Properties, Settings, Quick and Brush Creator), boxed together. The
/// ends choice only shows once either is on.
pub(super) fn taper_controls(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        taper_slider(ui, shape);
        ui.add(egui::Slider::new(&mut shape.rough_ends, 0.0..=1.0).text("Rough ends")).on_hover_text(
            "The ends of the stroke break up into dry, streaky bristle marks, over the taper or, without one, a few brush widths (0 = clean ends)",
        );
        if shape.taper > 0.0 || shape.rough_ends > 0.0 {
            taper_ends_controls(ui, shape, false);
        }
    });
}

/// Just the Taper slider (Hatch shows the ends choice at the top instead).
pub(super) fn taper_slider(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape) {
    ui.add(egui::Slider::new(&mut shape.taper, 0.0..=1.0).text("Taper"))
        .on_hover_text("Strokes come to a clean point, like an engraved line, even with a mouse (0 = off)");
}

/// Both ends, and with `reverse` (Hatch only) which end gets the point.
/// Hatch shows them at the top, greyed out while Taper is 0; elsewhere Both
/// ends appears under Taper, and off means the stroke tapers where it ends.
fn taper_ends_controls(ui: &mut egui::Ui, shape: &mut crate::tools::BrushShape, reverse: bool) {
    use oxide_engine::TaperEnds;
    ui.horizontal(|ui| {
        if shape.taper <= 0.0 && shape.rough_ends <= 0.0 {
            ui.disable();
        }
        let mut both = shape.taper_ends == TaperEnds::Both;
        if ui
            .checkbox(&mut both, "Both ends")
            .on_hover_text("Taper and roughen both ends of the stroke. Off: only where it ends")
            .on_disabled_hover_text("Turn Taper up above 0 to point the ends of the stroke")
            .changed()
        {
            shape.taper_ends = if both { TaperEnds::Both } else { TaperEnds::End };
        }
        if reverse && !both {
            let reversed = shape.taper_ends == TaperEnds::Start;
            let hint = "Which end gets the point. Off: the stroke starts full size and tapers where it ends. On: it starts at a point and ends full size (Hatch: Break up moves to that end too).\n(Hatch lines end at the second rail, or where you finished the rails for Along.)";
            if ui.selectable_label(reversed, format!("{} Reverse", egui_phosphor::regular::ARROWS_LEFT_RIGHT)).on_hover_text(hint).clicked() {
                shape.taper_ends = if reversed { TaperEnds::End } else { TaperEnds::Start };
            }
        }
    });
}

/// The rail icon for a section in the minimal layout.
fn section_icon(section: Section) -> &'static str {
    use egui_phosphor::regular as icon;
    match section {
        Section::Tools => icon::PAINT_BRUSH_BROAD,
        Section::Quick => icon::LIGHTNING,
        Section::Properties => icon::SLIDERS_HORIZONTAL,
        Section::BrushPresets => icon::SQUARES_FOUR,
        Section::Navigator => icon::COMPASS,
        Section::Color => icon::PALETTE,
        Section::WetMedia => icon::DROP,
        Section::Layers => icon::STACK,
        Section::Paper => icon::NOTEPAD,
    }
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
