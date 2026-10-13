//! The New Artwork dialog: canvas size in px/cm/inches with
//! DPI and orientation, Print/Screen/Favorites size presets, the paper to
//! paint on with a live preview, and the recent files list.

use eframe::egui::{self, Color32, Vec2};
use oxide_engine::{Color, Paper, PaperSettings};

use super::files::{NewSpec, PendingAction};
use super::select_canvas::CanvasTarget;
use super::{OxideApp, PAPER, default_surface, to_color32};
use crate::settings::{AppSettings, NewArtworkChoice, SizePreset, Unit, print_presets, screen_presets};

const PREVIEW: [usize; 2] = [110, 150];
/// Width and body height of the dialog, in points.
const DIALOG_SIZE: [f32; 2] = [980.0, 560.0];
/// Canvases bigger than this (in pixels per side) aren't allowed.
const MAX_SIDE: f64 = 16_384.0;
/// Each layer stores dry + wet paint as 4 floats each: 32 bytes per pixel.
const BYTES_PER_PIXEL_PER_LAYER: f64 = 32.0;

#[derive(Clone, Copy, PartialEq)]
enum SizeTab {
    Print,
    Screen,
    Favorites,
}

pub struct NewArtwork {
    pub open: bool,
    width_px: f64,
    height_px: f64,
    dpi: f64,
    unit: Unit,
    keep_ratio: bool,
    tab: SizeTab,
    /// Name of the size preset picked in the list, if any.
    selected: Option<String>,
    /// Name typed for a new custom size.
    new_name: String,
    surface: PaperSettings,
    paper_color: Color,
    preview: Option<egui::TextureHandle>,
    preview_for: Option<(PaperSettings, [u8; 3])>,
}

impl NewArtwork {
    /// Start from the last choices (or A4 at 200 DPI on canvas).
    pub fn from_settings(settings: &AppSettings) -> Self {
        let (width_px, height_px, dpi, unit, surface, paper_color) = match &settings.last_new_artwork {
            Some(c) => (c.width_px, c.height_px, c.dpi, c.unit, c.surface, Color::rgb(c.paper_color[0], c.paper_color[1], c.paper_color[2])),
            None => {
                let a4 = &print_presets()[0];
                let (w, h) = a4.pixels();
                (w.round(), h.round(), a4.dpi, a4.unit, default_surface(), PAPER)
            }
        };
        NewArtwork {
            open: settings.show_new_artwork_on_startup,
            width_px,
            height_px,
            dpi,
            unit,
            keep_ratio: false,
            tab: SizeTab::Print,
            selected: None,
            new_name: String::new(),
            surface,
            paper_color,
            preview: None,
            preview_for: None,
        }
    }

    /// The new painting this dialog describes.
    pub fn spec(&self) -> NewSpec {
        NewSpec {
            width: self.width_px.round().clamp(16.0, MAX_SIDE) as usize,
            height: self.height_px.round().clamp(16.0, MAX_SIDE) as usize,
            dpi: self.dpi as f32,
            surface: self.surface,
            paper: self.paper_color,
        }
    }

    fn choice(&self) -> NewArtworkChoice {
        let c = self.paper_color;
        NewArtworkChoice {
            width_px: self.width_px.round(),
            height_px: self.height_px.round(),
            dpi: self.dpi,
            unit: self.unit,
            surface: self.surface,
            paper_color: [c.r, c.g, c.b],
        }
    }

    fn apply_preset(&mut self, preset: &SizePreset) {
        let (w, h) = preset.pixels();
        let landscape = self.width_px > self.height_px;
        // Keep the current orientation.
        (self.width_px, self.height_px) = if landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) };
        self.dpi = preset.dpi;
        self.unit = preset.unit;
        self.selected = Some(preset.name.clone());
    }
}

impl NewArtwork {
    /// Set the canvas chosen in Select Canvas.
    pub fn set_canvas(&mut self, surface: PaperSettings, color: Color) {
        self.surface = surface;
        self.paper_color = color;
    }
}

/// A short name for a surface, e.g. "Canvas" or "White Simple".
pub fn paper_name(surface: PaperSettings) -> String {
    if surface.depth == 0.0 { "White Simple".to_string() } else { surface.kind.label().to_string() }
}

impl OxideApp {
    pub(super) fn new_artwork_dialog(&mut self, ctx: &egui::Context) {
        if !self.new_artwork.open || self.pending.is_some() {
            return;
        }
        let mut action = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("new-artwork")).show(ctx, |ui| {
            ui.set_width(DIALOG_SIZE[0]);
            ui.vertical_centered(|ui| ui.heading("New Artwork"));
            ui.add_space(6.0);
            // A fixed-height body: inside a row, a vertical separator takes all
            // the height it's offered, and a dialog would offer it unlimited.
            let body = Vec2::new(DIALOG_SIZE[0], DIALOG_SIZE[1]);
            ui.allocate_ui_with_layout(body, egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_height(DIALOG_SIZE[1]);
                ui.vertical(|ui| {
                    ui.set_width(190.0);
                    self.recent_column(ui, &mut action);
                });
                ui.separator();
                ui.vertical(|ui| self.size_and_paper_column(ui));
            });
            ui.separator();
            ui.horizontal(|ui| {
                let mut dont_show = !self.app_settings.show_new_artwork_on_startup;
                if ui.checkbox(&mut dont_show, "Don't show on startup").changed() {
                    self.app_settings.show_new_artwork_on_startup = !dont_show;
                    self.save_app_settings();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  OK  ").fill(ui.visuals().selection.bg_fill)).clicked() {
                        action = Some(PendingAction::New(self.new_artwork.spec()));
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        });
        if modal.should_close() {
            close = true;
        }
        if let Some(action) = action {
            if matches!(action, PendingAction::New(_)) {
                self.app_settings.last_new_artwork = Some(self.new_artwork.choice());
                self.save_app_settings();
            }
            self.new_artwork.open = false;
            self.request(action, ctx);
        } else if close {
            self.new_artwork.open = false;
        }
    }

    fn recent_column(&mut self, ui: &mut egui::Ui, action: &mut Option<PendingAction>) {
        if ui.button("Open…").clicked() {
            *action = Some(PendingAction::Open);
        }
        ui.separator();
        ui.label("Recent:");
        egui::ScrollArea::vertical().id_salt("recent").max_height(420.0).show(ui, |ui| {
            if self.app_settings.recent_files.is_empty() {
                ui.weak("No recent paintings yet.");
            }
            for path in &self.app_settings.recent_files {
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                let exists = path.exists();
                let response = ui.add_enabled(exists, egui::Button::new(name).frame(false));
                if response.on_hover_text(path.display().to_string()).clicked() {
                    *action = Some(PendingAction::OpenPath(path.clone()));
                }
            }
        });
        ui.add_space(8.0);
        if ui.add_enabled(!self.app_settings.recent_files.is_empty(), egui::Button::new("Clear List")).clicked() {
            self.app_settings.recent_files.clear();
            self.save_app_settings();
        }
    }

    fn size_and_paper_column(&mut self, ui: &mut egui::Ui) {
        // ---------- Canvas size ----------
        ui.horizontal_top(|ui| {
            let na = &mut self.new_artwork;
            ui.vertical(|ui| {
                ui.strong("Canvas Size:");
                egui::Grid::new("size-grid").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
                    let (unit, dpi) = (na.unit, na.dpi);
                    let decimals = if unit == Unit::Pixel { 0 } else { 3 };
                    let speed = if unit == Unit::Pixel { 1.0 } else { 0.01 };

                    ui.label("Width:");
                    let mut w = unit.px_to(na.width_px, dpi);
                    if ui.add(egui::DragValue::new(&mut w).speed(speed).max_decimals(decimals).range(0.01..=1.0e6)).changed() {
                        let new_px = unit.to_px(w, dpi).clamp(16.0, MAX_SIDE);
                        if na.keep_ratio && na.width_px > 0.0 {
                            na.height_px = (na.height_px * new_px / na.width_px).clamp(16.0, MAX_SIDE);
                        }
                        na.width_px = new_px;
                        na.selected = None;
                    }
                    ui.label(unit.label());
                    ui.end_row();

                    ui.label("Height:");
                    let mut h = unit.px_to(na.height_px, dpi);
                    if ui.add(egui::DragValue::new(&mut h).speed(speed).max_decimals(decimals).range(0.01..=1.0e6)).changed() {
                        let new_px = unit.to_px(h, dpi).clamp(16.0, MAX_SIDE);
                        if na.keep_ratio && na.height_px > 0.0 {
                            na.width_px = (na.width_px * new_px / na.height_px).clamp(16.0, MAX_SIDE);
                        }
                        na.height_px = new_px;
                        na.selected = None;
                    }
                    ui.label(unit.label());
                    ui.end_row();
                });
                ui.checkbox(&mut na.keep_ratio, "Keep ratio");
            });
            ui.add_space(20.0); // (a vertical separator here would stretch to the full height)
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label("DPI:");
                    // Changing DPI keeps the *physical* size for cm/inch (more pixels),
                    // and the pixel size for pixels.
                    let old = na.dpi;
                    egui::ComboBox::from_id_salt("dpi")
                        .selected_text(format!("{}", na.dpi))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for dpi in [72.0, 96.0, 150.0, 200.0, 300.0, 600.0] {
                                ui.selectable_value(&mut na.dpi, dpi, format!("{dpi}"));
                            }
                        });
                    ui.add(egui::DragValue::new(&mut na.dpi).range(10.0..=2400.0).speed(1.0));
                    if na.dpi != old && na.unit != Unit::Pixel {
                        let k = na.dpi / old;
                        na.width_px = (na.width_px * k).clamp(16.0, MAX_SIDE);
                        na.height_px = (na.height_px * k).clamp(16.0, MAX_SIDE);
                    }
                });
                ui.horizontal(|ui| {
                    let portrait = na.height_px >= na.width_px;
                    if ui.selectable_label(portrait, "▯ Portrait").clicked() && !portrait {
                        std::mem::swap(&mut na.width_px, &mut na.height_px);
                    }
                    if ui.selectable_label(!portrait, "▭ Landscape").clicked() && portrait {
                        std::mem::swap(&mut na.width_px, &mut na.height_px);
                    }
                });
            });
            ui.add_space(20.0); // (a vertical separator here would stretch to the full height)
            ui.vertical(|ui| {
                for unit in Unit::ALL {
                    ui.radio_value(&mut na.unit, unit, unit.label());
                }
            });
            ui.add_space(20.0); // (a vertical separator here would stretch to the full height)
            ui.vertical(|ui| {
                ui.strong("Canvas Size Info:");
                let (w, h, dpi) = (na.width_px.round(), na.height_px.round(), na.dpi);
                ui.label(format!("{w} x {h} pixels"));
                ui.label(format!("{:.2} x {:.2} cm", Unit::Cm.px_to(w, dpi), Unit::Cm.px_to(h, dpi)));
                ui.label(format!("{:.2} x {:.2} inches", Unit::Inch.px_to(w, dpi), Unit::Inch.px_to(h, dpi)));
                let mb = w * h * BYTES_PER_PIXEL_PER_LAYER / (1024.0 * 1024.0);
                let text = format!("≈ {mb:.0} MB per layer");
                if mb > 1024.0 {
                    ui.colored_label(Color32::from_rgb(230, 170, 60), text).on_hover_text(
                        "Very large: each layer (plus undo) needs this much memory. Painting may be slow.",
                    );
                } else {
                    ui.weak(text);
                }
            });
        });
        ui.separator();

        // ---------- Size presets + paper ----------
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(540.0);
                self.size_presets(ui);
            });
            ui.add_space(20.0); // (a vertical separator here would stretch to the full height)
            ui.vertical(|ui| self.paper_picker(ui));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Color profile:");
            ui.add_enabled_ui(false, |ui| {
                egui::ComboBox::from_id_salt("color-profile").selected_text("sRGB (color management coming later)").show_ui(ui, |_| {});
            });
        });
    }

    fn size_presets(&mut self, ui: &mut egui::Ui) {
        let na = &mut self.new_artwork;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut na.tab, SizeTab::Print, "   Print   ");
            ui.selectable_value(&mut na.tab, SizeTab::Screen, "   Screen   ");
            ui.selectable_value(&mut na.tab, SizeTab::Favorites, "  ★ Favorites  ");
        });

        let all: Vec<SizePreset> = print_presets().into_iter().chain(screen_presets()).collect();
        let (list, custom): (Vec<SizePreset>, bool) = match na.tab {
            SizeTab::Print => (print_presets(), false),
            SizeTab::Screen => (screen_presets(), false),
            SizeTab::Favorites => {
                let starred = all.iter().filter(|p| self.app_settings.is_favorite(&p.name)).cloned();
                (starred.chain(self.app_settings.custom_sizes.iter().cloned()).collect(), true)
            }
        };

        let mut apply = None;
        let mut star = None;
        egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(4.0).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("sizes").min_scrolled_height(220.0).max_height(220.0).show(ui, |ui| {
                ui.set_min_height(220.0);
                if list.is_empty() {
                    ui.weak("Star a size, or add your own below.");
                }
                for preset in &list {
                    ui.horizontal(|ui| {
                        let selected = na.selected.as_deref() == Some(preset.name.as_str());
                        let fav = self.app_settings.is_favorite(&preset.name);
                        let is_custom = self.app_settings.custom_sizes.iter().any(|c| c.name == preset.name);
                        let row_width = ui.available_width() - 30.0;
                        let row = egui::Button::new(preset.label()).selected(selected).frame(selected).min_size(Vec2::new(row_width, 0.0));
                        if ui.add(row).clicked() {
                            apply = Some(preset.clone());
                        }
                        if !is_custom && ui.button(if fav { "★" } else { "☆" }).on_hover_text("Favorite").clicked() {
                            star = Some(preset.name.clone());
                        }
                    });
                }
            });
        });
        if let Some(preset) = apply {
            na.apply_preset(&preset);
        }
        if let Some(name) = star {
            self.app_settings.toggle_favorite(&name);
            self.save_app_settings();
        }

        // Add the current size as your own preset; remove a selected one.
        let na = &mut self.new_artwork;
        let mut add = None;
        let mut remove = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut na.new_name).hint_text("Name for the current size").desired_width(440.0));
            if ui.button("⊕").on_hover_text("Save the current size as a favorite").clicked() && !na.new_name.trim().is_empty() {
                let (w, h) = (na.unit.px_to(na.width_px.min(na.height_px), na.dpi), na.unit.px_to(na.width_px.max(na.height_px), na.dpi));
                let round = |v: f64| if na.unit == Unit::Pixel { v.round() } else { (v * 100.0).round() / 100.0 };
                add = Some(SizePreset::new(na.new_name.trim(), round(w), round(h), na.unit, na.dpi));
            }
            let can_remove = custom && na.selected.as_ref().is_some_and(|s| self.app_settings.custom_sizes.iter().any(|c| &c.name == s));
            if ui.add_enabled(can_remove, egui::Button::new("⊖")).on_hover_text("Remove the selected custom size").clicked() {
                remove = na.selected.clone();
            }
        });
        if let Some(preset) = add {
            self.app_settings.custom_sizes.retain(|c| c.name != preset.name);
            self.new_artwork.selected = Some(preset.name.clone());
            self.app_settings.custom_sizes.push(preset);
            self.new_artwork.new_name.clear();
            self.new_artwork.tab = SizeTab::Favorites;
            self.save_app_settings();
        }
        if let Some(name) = remove {
            self.app_settings.custom_sizes.retain(|c| c.name != name);
            self.new_artwork.selected = None;
            self.save_app_settings();
        }
    }

    fn paper_picker(&mut self, ui: &mut egui::Ui) {
        let na = &mut self.new_artwork;
        ui.strong("Canvas:");
        // Re-render the preview only when the paper changes.
        let key = (na.surface, to_color32(na.paper_color).to_array()[..3].try_into().expect("3 bytes"));
        if na.preview_for != Some(key) {
            let image = paper_preview(na.surface, na.paper_color);
            match &mut na.preview {
                Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
                None => na.preview = Some(ui.ctx().load_texture("paper-preview", image, egui::TextureOptions::NEAREST)),
            }
            na.preview_for = Some(key);
        }
        // Click the preview (or the button) to open Select Canvas.
        let mut open_picker = false;
        if let Some(texture) = &na.preview {
            let image = egui::Image::new(texture)
                .fit_to_exact_size(Vec2::new(PREVIEW[0] as f32, PREVIEW[1] as f32))
                .sense(egui::Sense::click());
            if ui.add(image).on_hover_text("Click to choose the canvas").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                open_picker = true;
            }
        }
        ui.label(paper_name(na.surface));
        if ui.button("Select…").clicked() {
            open_picker = true;
        }
        if open_picker {
            let (surface, color) = (na.surface, na.paper_color);
            self.select_canvas.open_for(CanvasTarget::NewArtwork, surface, color);
        }
    }

    pub(super) fn save_app_settings(&mut self) {
        if let Err(e) = self.app_settings.save() {
            self.error(format!("Couldn't save settings: {e}"));
        }
    }
}

/// A small lit swatch of the paper, so you can see it before starting.
fn paper_preview(settings: PaperSettings, color: Color) -> egui::ColorImage {
    let paper = Paper::new(settings);
    let [w, h] = PREVIEW;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let s = paper.shade_at(x, y);
            let c = |v: f32| ((v * s).clamp(0.0, 1.0) * 255.0).round() as u8;
            bytes.extend([c(color.r), c(color.g), c(color.b), 255]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([w, h], &bytes)
}
