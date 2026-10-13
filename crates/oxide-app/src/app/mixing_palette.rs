//! The Mixing Palette panel: a small surface next to the
//! painting where you mix paint with the real engine, then pick the mixed
//! color to paint with. The palette is a little `Document` of its own (own
//! undo), stored on the painting and saved inside the `.oxide` file.

use eframe::egui::{self, Color32, Sense, Stroke as LineStroke, Vec2};
use oxide_engine::{Brush, Color, DabMode, Document, Stroke, StrokePoint};

use super::{OxideApp, to_color32};
use crate::palette_library::{self, PaletteLibrary, THUMB};
use crate::tiles::TiledTexture;
use crate::tools::Tool;
use oxide_commands::palette::{PALETTE_SIZE, add_note_line, add_paint_swatch, new_palette, parse_hex, to_hex};


#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteTool {
    Paint,
    Blend,
    Eraser,
    Pick,
}

impl PaletteTool {
    const ALL: [PaletteTool; 4] = [PaletteTool::Paint, PaletteTool::Blend, PaletteTool::Eraser, PaletteTool::Pick];

    fn label(self) -> &'static str {
        match self {
            PaletteTool::Paint => "Paint",
            PaletteTool::Blend => "Blend",
            PaletteTool::Eraser => "Eraser",
            PaletteTool::Pick => "Pick",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            PaletteTool::Paint => "Lay your current color on the palette (wet, mixes with what's there)",
            PaletteTool::Blend => "Push and mix the paint already on the palette",
            PaletteTool::Eraser => "Wipe paint off the palette",
            PaletteTool::Pick => "Take a mixed color to paint with",
        }
    }
}

/// Small / Medium / Large brush radius on the palette.
const SIZES: [(&str, f32); 3] = [("S", 6.0), ("M", 14.0), ("L", 28.0)];

pub struct MixingPalette {
    pub open: bool,
    tool: PaletteTool,
    size: usize,
    opacity: f32,
    stroke: Option<Stroke>,
    texture: Option<TiledTexture>,
    /// `palette.state_id()` when the painting was last saved/opened.
    pub saved_state: u64,
    // ----- Library -----
    library: PaletteLibrary,
    /// Name being typed for "Save to library", while that row is shown.
    save_name: Option<String>,
    /// Library entry being renamed, and the new name typed so far.
    renaming: Option<(usize, String)>,
    /// Result of the last library action, shown under the library.
    message: Option<Result<String, String>>,
    /// Hex color typed in the "Add swatch" field.
    hex: String,
    /// Problem with the typed hex value, if any.
    hex_error: Option<String>,
    /// Paint name typed next to the hex value (added to the notes).
    paint_name: String,
    /// The palette's notes when the painting was last saved/opened.
    pub saved_notes: String,
}

impl Default for MixingPalette {
    fn default() -> Self {
        MixingPalette {
            open: false,
            tool: PaletteTool::Paint,
            size: 1,
            opacity: 1.0,
            stroke: None,
            texture: None,
            saved_state: 0,
            library: PaletteLibrary::default(),
            save_name: None,
            renaming: None,
            message: None,
            hex: String::new(),
            hex_error: None,
            paint_name: String::new(),
            saved_notes: String::new(),
        }
    }
}

impl MixingPalette {
    /// Forget the GPU texture (e.g. after opening another painting).
    pub fn reset_view(&mut self) {
        self.texture = None;
        self.stroke = None;
    }
}


impl OxideApp {
    pub(super) fn mixing_palette_window(&mut self, ctx: &egui::Context) {
        if !self.mixing_palette.open {
            return;
        }
        let mut open = true;
        egui::Window::new("Mixing Palette")
            .open(&mut open)
            .resizable(false)
            .default_pos(ctx.content_rect().right_bottom() - Vec2::new(720.0, 520.0))
            .show(ctx, |ui| self.mixing_palette_contents(ui));
        if !open {
            self.mixing_palette.open = false;
        }
    }

    fn mixing_palette_contents(&mut self, ui: &mut egui::Ui) {
        // The palette lives on the painting; make one on first use.
        if self.document.palette.is_none() {
            self.document.palette = Some(Box::new(new_palette()));
            self.mixing_palette.saved_state = 0;
        }

        // ---------- Toolbar ----------
        let mp = &mut self.mixing_palette;
        let mut clear = false;
        let mut undo = false;
        let mut redo = false;
        let mut background = None;
        let mut import = false;
        ui.horizontal(|ui| {
            for tool in PaletteTool::ALL {
                ui.selectable_value(&mut mp.tool, tool, tool.label()).on_hover_text(tool.hint());
            }
            ui.separator();
            for (i, (label, _)) in SIZES.iter().enumerate() {
                ui.selectable_value(&mut mp.size, i, *label).on_hover_text("Brush size");
            }
            ui.separator();
            ui.add(egui::Slider::new(&mut mp.opacity, 0.05..=1.0).show_value(false).text("Opacity"));
            ui.separator();
            ui.menu_button("☰ Menu", |ui| {
                undo = ui.button("Undo").clicked();
                redo = ui.button("Redo").clicked();
                ui.separator();
                clear = ui.button("Clear palette").clicked();
                ui.separator();
                if ui.button("Save to library…").clicked() {
                    mp.save_name = Some(String::new());
                }
                import = ui.button("Import palette…").clicked();
                ui.separator();
                ui.horizontal(|ui| {
                    let palette = self.document.palette.as_ref().expect("made above");
                    let mut c = to_color32(palette.paper());
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        let [r, g, b, _] = c.to_array();
                        background = Some(Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0));
                    }
                    ui.label("Background color");
                });
            });
        });

        // "Save to library" name row.
        let mut save_now = None;
        // Ask before borrowing `save_name` mutably (Rust won't allow both at once).
        let needs_focus = mp.save_name_needs_focus();
        if let Some(name) = &mut mp.save_name {
            ui.horizontal(|ui| {
                ui.label("Save palette as");
                let field = ui.add(egui::TextEdit::singleline(name).hint_text("e.g. Landscape greens").desired_width(220.0));
                if needs_focus {
                    field.request_focus();
                }
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Save").clicked() || enter {
                    save_now = Some(name.clone());
                }
                if ui.button("Cancel").clicked() {
                    save_now = Some(String::new());
                }
            });
        }
        if let Some(name) = save_now {
            if !name.trim().is_empty() {
                let palette = self.document.palette.as_ref().expect("made above");
                let result = self.mixing_palette.library.save(ui.ctx(), &name, palette);
                self.mixing_palette.message = Some(
                    result.map(|()| format!("Saved \"{}\" to the library ({})", name.trim(), palette_library::display_dir())),
                );
            }
            self.mixing_palette.save_name = None;
        }
        let mut import_dialog = rfd::FileDialog::new().add_filter("Oxide palette", &["oxide"]);
        if let Some(dir) = palette_library::library_dir() {
            // Start where palettes are saved, not on the Desktop.
            import_dialog = import_dialog.set_directory(dir);
        }
        if import && let Some(path) = import_dialog.pick_file() {
            let result = self.mixing_palette.library.import(ui.ctx(), &path);
            self.mixing_palette.message = Some(result.map(|()| "Imported into the library".to_string()));
        }

        let palette = self.document.palette.as_mut().expect("made above");
        if undo {
            palette.undo();
        }
        if redo {
            palette.redo();
        }
        if clear {
            // Clear the paint, keep the notes.
            let paper = palette.paper();
            let notes = std::mem::take(&mut palette.notes);
            **palette = Document::new(PALETTE_SIZE.0, PALETTE_SIZE.1, paper);
            palette.notes = notes;
            self.mixing_palette.texture = None;
        }
        if let Some(color) = background {
            palette.set_paper(color);
            palette.seal_history();
        }

        // ---------- Add a swatch of paint (hex value or current color) ----------
        let mut add_swatch = None;
        let mp = &mut self.mixing_palette;
        ui.horizontal(|ui| {
            ui.label("Add swatch");
            ui.add(egui::TextEdit::singleline(&mut mp.paint_name).hint_text("Paint name (optional)").desired_width(150.0))
                .on_hover_text("e.g. Ultramarine Blue. Named swatches are listed in the palette's notes.");
            let field = ui.add(egui::TextEdit::singleline(&mut mp.hex).hint_text("#3A5F8B").desired_width(90.0).font(egui::TextStyle::Monospace));
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            // Live preview chip for a valid value.
            let parsed = parse_hex(&mp.hex);
            let (chip, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
            match parsed {
                Some(c) => ui.painter().rect_filled(chip, 3.0, to_color32(c)),
                None => ui.painter().rect_stroke(chip, 3.0, LineStroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside),
            };
            if ui.button("Add").on_hover_text("Put a dab of this paint on the palette").clicked() || enter {
                match parsed {
                    Some(c) => {
                        add_swatch = Some(c);
                        mp.hex_error = None;
                    }
                    None => mp.hex_error = Some("Use a hex color like #3A5F8B or #38B".into()),
                }
            }
            if ui.button("+ current color").on_hover_text("Put a dab of the color you're painting with on the palette").clicked() {
                add_swatch = Some(self.color);
            }
            if let Some(e) = &mp.hex_error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
        if let Some(color) = add_swatch {
            let palette = self.document.palette.as_mut().expect("made above");
            add_paint_swatch(palette, color, self.app_settings.natural_colors);
            // A named paint goes into the notes: "• Ultramarine Blue (#1C2A6B)".
            let mp = &mut self.mixing_palette;
            let name = mp.paint_name.trim();
            if !name.is_empty() {
                add_note_line(&mut palette.notes, &format!("• {name} ({})", to_hex(color)));
            }
            mp.paint_name.clear();
            mp.hex.clear();
        }

        // ---------- The palette surface ----------
        let palette = self.document.palette.as_mut().expect("made above");
        let size = Vec2::new(palette.width() as f32, palette.height() as f32);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

        let pen = self.pen.read();
        let pen_down = pen.touching() && response.hovered();
        let down = ui.input(|i| i.pointer.primary_down()) || pen_down;
        let pointer = response.interact_pointer_pos().or_else(|| response.hover_pos().filter(|_| pen_down)).filter(|_| down);

        // The brush: the current color as wet oil, at palette size.
        let mp = &mut self.mixing_palette;
        let natural = self.app_settings.natural_colors;
        let mut brush = self.tool_states[Tool::OilsAcrylics.index()].brush(Tool::OilsAcrylics, self.color);
        brush.radius = SIZES[mp.size].1;
        brush.opacity = mp.opacity;
        brush.lift_off = 0.3;
        brush.mixing = if natural { oxide_engine::Mixing::Pigment } else { oxide_engine::Mixing::Rgb };
        let (brush, mode) = match mp.tool {
            PaletteTool::Paint => (Brush { loading: 0.9, oiliness: brush.oiliness.max(0.5), ..brush }, DabMode::Paint),
            PaletteTool::Blend => (Brush { loading: 0.0, oiliness: 0.85, pickup_dry: true, ..brush }, DabMode::Paint),
            PaletteTool::Eraser => (brush, DabMode::Erase),
            PaletteTool::Pick => (brush, DabMode::Paint),
        };

        let mut picked = None;
        match pointer {
            Some(pos) => {
                let p = pos - rect.min;
                if mp.tool == PaletteTool::Pick {
                    if p.x >= 0.0 && p.y >= 0.0 {
                        picked = palette.composite_pixel(p.x as usize, p.y as usize);
                    }
                } else {
                    let pressure = if pen.is_pen && pen.pressure > 0.0 {
                        crate::pen::apply_curve(pen.pressure, self.app_settings.pressure_curve)
                    } else {
                        1.0
                    };
                    let point = StrokePoint { x: p.x, y: p.y, pressure, tilt: None, rotation: None };
                    match &mut mp.stroke {
                        None => {
                            palette.begin_stroke();
                            mp.stroke = Some(Stroke::begin_at(palette.active_canvas(), &brush, mode, point, None));
                        }
                        Some(stroke) => stroke.line_to_point(palette.active_canvas(), &brush, point),
                    }
                }
            }
            None => {
                if let Some(mut stroke) = mp.stroke.take() {
                    stroke.finish(palette.active_canvas(), &brush);
                    palette.end_stroke();
                }
            }
        }
        if !down {
            palette.seal_history();
        }

        // ---------- Draw ----------
        let texture = mp.texture.get_or_insert_with(|| {
            palette.take_dirty();
            TiledTexture::new(ui.ctx(), palette)
        });
        if let Some(region) = palette.take_dirty() {
            texture.update(palette, region);
        }
        let painter = ui.painter_at(rect);
        texture.paint(&painter, rect);
        painter.rect_stroke(rect, 2.0, LineStroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Outside);

        if let Some(hover) = response.hover_pos() {
            if mp.tool == PaletteTool::Pick {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else {
                painter.circle_stroke(hover, brush.radius, LineStroke::new(1.5, Color32::from_black_alpha(140)));
                painter.circle_stroke(hover, brush.radius, LineStroke::new(0.75, Color32::from_white_alpha(200)));
                ui.ctx().set_cursor_icon(egui::CursorIcon::None);
            }
        }

        ui.horizontal(|ui| {
            let (chip, _) = ui.allocate_exact_size(Vec2::new(28.0, 16.0), Sense::hover());
            ui.painter().rect_filled(chip, 2.0, to_color32(self.color));
            ui.weak("Current color · mix here, then Pick to paint with it");
        });

        if let Some(color) = picked {
            self.set_color(color);
            self.remember_color(color);
        }

        // ---------- Notes ----------
        let palette = self.document.palette.as_mut().expect("made above");
        egui::CollapsingHeader::new("Notes").default_open(true).show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut palette.notes)
                    .hint_text("Paints used, e.g.\n• Ultramarine Blue\n• Burnt Umber\n• Titanium White")
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
        });

        self.palette_library_section(ui);
    }

    /// Saved palettes: click to load a copy; right-click to rename, duplicate, delete.
    fn palette_library_section(&mut self, ui: &mut egui::Ui) {
        let mp = &mut self.mixing_palette;
        if !mp.library.loaded
            && let Err(e) = mp.library.refresh(ui.ctx())
        {
            mp.message = Some(Err(e));
        }
        let mut load = None;
        let mut save_requested = false;
        let mut reveal_error = None;
        let mut action: Option<(&str, usize)> = None;
        let mut commit_rename = None;

        egui::CollapsingHeader::new(format!("Library ({})", mp.library.entries.len())).default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("💾 Save this palette to the library…").clicked() {
                    save_requested = true;
                }
                if ui.button("Show folder").on_hover_text(palette_library::display_dir()).clicked()
                    && let Err(e) = palette_library::reveal_in_finder()
                {
                    reveal_error = Some(e);
                }
                if mp.library.entries.is_empty() {
                    ui.weak("No saved palettes yet.");
                }
            });
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (i, entry) in mp.library.entries.iter().enumerate() {
                        ui.vertical(|ui| {
                            let size = Vec2::new(THUMB[0] as f32, THUMB[1] as f32);
                            let image = egui::Image::new(&entry.thumbnail).fit_to_exact_size(size).sense(Sense::click());
                            let hover = if entry.notes.trim().is_empty() {
                                "Click to load a copy into this painting's palette".to_string()
                            } else {
                                format!("{}\n\nClick to load a copy into this painting's palette", entry.notes.trim())
                            };
                            let response = ui.add(image).on_hover_text(hover);
                            if response.clicked() {
                                load = Some(i);
                            }
                            response.context_menu(|ui| {
                                if ui.button("Rename").clicked() {
                                    action = Some(("rename", i));
                                }
                                if ui.button("Duplicate").clicked() {
                                    action = Some(("duplicate", i));
                                }
                                if ui.button("Delete").clicked() {
                                    action = Some(("delete", i));
                                }
                            });
                            match &mut mp.renaming {
                                Some((index, name)) if *index == i => {
                                    let field = ui.add(egui::TextEdit::singleline(name).desired_width(size.x));
                                    field.request_focus();
                                    if field.lost_focus() {
                                        commit_rename = Some((i, name.clone()));
                                    }
                                }
                                _ => {
                                    ui.add(egui::Label::new(egui::RichText::new(&entry.name).small()).truncate());
                                }
                            }
                        });
                    }
                });
            });
            match &mp.message {
                Some(Ok(text)) => {
                    ui.weak(text);
                }
                Some(Err(text)) => {
                    ui.colored_label(ui.visuals().error_fg_color, text);
                }
                None => {}
            }
        });

        if let Some(e) = reveal_error {
            self.mixing_palette.message = Some(Err(e));
        }
        if save_requested {
            // Opens the name row at the top of the panel.
            self.mixing_palette.save_name = Some(String::new());
        }
        let ctx = ui.ctx().clone();
        let lib = &mut self.mixing_palette.library;
        let result = match action {
            Some(("rename", i)) => {
                self.mixing_palette.renaming = Some((i, lib.entries[i].name.clone()));
                None
            }
            Some(("duplicate", i)) => Some(lib.duplicate(&ctx, i).map(|()| "Duplicated".to_string())),
            Some(("delete", i)) => {
                let name = lib.entries[i].name.clone();
                Some(lib.delete(&ctx, i).map(|()| format!("Deleted \"{name}\"")))
            }
            _ => None,
        };
        if let Some(result) = result {
            self.mixing_palette.message = Some(result);
        }
        if let Some((i, name)) = commit_rename {
            self.mixing_palette.renaming = None;
            if name.trim() != self.mixing_palette.library.entries[i].name && !name.trim().is_empty() {
                let result = self.mixing_palette.library.rename(&ctx, i, &name);
                self.mixing_palette.message = Some(result.map(|()| "Renamed".to_string()));
            }
        }
        if let Some(i) = load {
            match self.mixing_palette.library.load_copy(i) {
                Ok(doc) => {
                    let name = self.mixing_palette.library.entries[i].name.clone();
                    self.document.palette = Some(Box::new(doc));
                    self.mixing_palette.reset_view();
                    // The painting's palette changed: that's an unsaved change.
                    self.mixing_palette.saved_state = u64::MAX;
                    self.mixing_palette.message = Some(Ok(format!("Loaded a copy of \"{name}\"")));
                }
                Err(e) => self.mixing_palette.message = Some(Err(e)),
            }
        }
    }
}

impl MixingPalette {
    /// Focus the name field once, right after "Save to library…" opens it.
    fn save_name_needs_focus(&self) -> bool {
        self.save_name.as_ref().is_some_and(|n| n.is_empty())
    }
}

