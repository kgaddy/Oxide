//! Window → Reference Image: a floating window with a photo to paint from,
//! like a reference photo pinned beside an easel. It's only for looking at (and
//! picking colors from); it's not part of the image, but it's saved in the
//! painting's file, so each painting keeps its own photo.
//!
//! Scroll or pinch to zoom, drag to pan, double-click to fit. With the
//! Picker tool (I), or while holding ⌥, click to pick a color.

use std::sync::Arc;

use eframe::egui::{self, Color32, CursorIcon, Rect, Sense, Vec2};
use oxide_engine::Color;

use super::OxideApp;
use crate::settings::DialogFolder;
use crate::tools::Tool;

/// egui can't show textures bigger than this; larger photos are scaled down.
const MAX_SIDE: u32 = 2048;

#[derive(Default)]
pub struct ReferenceImage {
    pub open: bool,
    image: Option<image::RgbaImage>,
    texture: Option<egui::TextureHandle>,
    /// Show the photo in greys, to judge values.
    greyscale: bool,
    /// 1 = fit to the window.
    zoom: f32,
    pan: Vec2,
    /// The document's photo bytes that `image` was decoded from, to notice
    /// when the painting (or its photo) changes.
    shown: Option<Arc<Vec<u8>>>,
}

impl ReferenceImage {
    pub fn new(open: bool) -> Self {
        ReferenceImage { open, ..Default::default() }
    }

    /// Show the painting's photo, if it isn't already showing.
    fn sync(&mut self, ctx: &egui::Context, reference: Option<&Arc<Vec<u8>>>) -> Result<(), String> {
        if self.shown.as_ref().map(Arc::as_ptr) == reference.map(Arc::as_ptr) {
            return Ok(());
        }
        self.shown = reference.cloned();
        self.image = None;
        self.texture = None;
        match reference {
            Some(bytes) => self.load(ctx, bytes),
            None => Ok(()),
        }
    }

    fn load(&mut self, ctx: &egui::Context, bytes: &[u8]) -> Result<(), String> {
        let img = image::load_from_memory(bytes).map_err(|e| format!("Couldn't show the reference photo: {e}"))?;
        let img = if img.width().max(img.height()) > MAX_SIDE {
            img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::CatmullRom)
        } else {
            img
        };
        self.image = Some(img.to_rgba8());
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        self.update_texture(ctx);
        Ok(())
    }

    fn update_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else { return };
        let size = [img.width() as usize, img.height() as usize];
        let pixels: Vec<Color32> = img
            .pixels()
            .map(|p| {
                let [r, g, b, a] = p.0;
                if self.greyscale {
                    let l = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
                    Color32::from_rgba_unmultiplied(l, l, l, a)
                } else {
                    Color32::from_rgba_unmultiplied(r, g, b, a)
                }
            })
            .collect();
        let image = egui::ColorImage::new(size, pixels);
        self.texture = Some(ctx.load_texture("reference-image", image, egui::TextureOptions::LINEAR));
    }

    /// Where the photo is drawn inside `area`.
    fn image_rect(&self, area: Rect) -> Option<Rect> {
        let img = self.image.as_ref()?;
        let size = Vec2::new(img.width() as f32, img.height() as f32);
        let fit = (area.width() / size.x).min(area.height() / size.y);
        Some(Rect::from_center_size(area.center() + self.pan, size * fit * self.zoom))
    }
}

impl OxideApp {
    /// File → Open Reference Image…: pick a photo for this painting and show it.
    pub(super) fn open_reference_image(&mut self) {
        let Some(path) = self
            .dialog(DialogFolder::Images)
            .add_filter("Images", &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"])
            .pick_file()
        else {
            return;
        };
        self.remember_folder(DialogFolder::Images, &path);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => return self.error(format!("Couldn't open {}: {e}", path.display())),
        };
        if let Err(e) = image::guess_format(&bytes) {
            return self.error(format!("Couldn't open {}: {e}", path.display()));
        }
        self.document.reference = Some(Arc::new(bytes));
        self.reference.open = true;
        self.app_settings.reference_open = true;
        self.save_app_settings();
    }

    pub(super) fn reference_window(&mut self, ctx: &egui::Context) {
        if !self.reference.open {
            return;
        }
        // Follow the painting: a different file brings its own photo (or none).
        if let Err(e) = self.reference.sync(ctx, self.document.reference.as_ref()) {
            self.error(e);
        }

        let mut open = true;
        let mut pick_file = false;
        let mut remove = false;
        let mut picked = None;
        egui::Window::new("Reference Image")
            .open(&mut open)
            .resizable(true)
            .default_size([340.0, 440.0])
            .min_size([180.0, 160.0])
            .default_pos(ctx.content_rect().left_top() + Vec2::new(280.0, 60.0))
            .show(ctx, |ui| {
                let r = &mut self.reference;
                ui.horizontal(|ui| {
                    pick_file = ui.button("Open…").on_hover_text("Choose a reference photo for this painting").clicked();
                    if ui.add_enabled(r.image.is_some(), egui::Button::new("Fit")).on_hover_text("Fit to the window (or double-click the photo)").clicked() {
                        r.zoom = 1.0;
                        r.pan = Vec2::ZERO;
                    }
                    if ui.add_enabled(r.image.is_some(), egui::Button::new("Greyscale").selected(r.greyscale)).on_hover_text("Show the values only").clicked() {
                        r.greyscale = !r.greyscale;
                        r.update_texture(ui.ctx());
                    }
                    remove = ui.add_enabled(r.shown.is_some(), egui::Button::new("Remove")).on_hover_text("Take the photo off this painting").clicked();
                    if r.image.is_some() {
                        ui.weak(format!("{:.0}%", r.zoom * 100.0));
                    }
                });

                let (area, response) = ui.allocate_exact_size(ui.available_size().max(Vec2::new(160.0, 120.0)), Sense::click_and_drag());
                let painter = ui.painter_at(area);
                painter.rect_filled(area, 0.0, ui.visuals().extreme_bg_color);
                let (Some(texture), Some(rect)) = (&r.texture, r.image_rect(area)) else {
                    painter.text(area.center(), egui::Align2::CENTER_CENTER, "Open… a photo to paint from", egui::FontId::proportional(14.0), ui.visuals().weak_text_color());
                    return;
                };
                painter.image(texture.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);

                let picking = self.tool == Tool::Picker || ui.input(|i| i.modifiers.alt);
                if let Some(hover) = response.hover_pos() {
                    // Zoom around the pointer.
                    let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta().y));
                    let factor = if zoom_delta != 1.0 { zoom_delta } else { (scroll / 200.0).exp() };
                    if factor != 1.0 {
                        let new_zoom = (r.zoom * factor).clamp(0.25, 20.0);
                        let anchor = hover - area.center() - r.pan;
                        r.pan -= anchor * (new_zoom / r.zoom - 1.0);
                        r.zoom = new_zoom;
                    }
                    if picking {
                        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                    }
                }
                if response.double_clicked() {
                    r.zoom = 1.0;
                    r.pan = Vec2::ZERO;
                } else if picking && (response.clicked() || response.dragged()) {
                    if let (Some(pos), Some(img)) = (response.interact_pointer_pos(), &r.image) {
                        let uv = (pos - rect.min) / rect.size();
                        if (0.0..1.0).contains(&uv.x) && (0.0..1.0).contains(&uv.y) {
                            let p = img.get_pixel((uv.x * img.width() as f32) as u32, (uv.y * img.height() as f32) as u32);
                            picked = Some(Color::rgb(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0));
                        }
                    }
                } else if response.dragged() {
                    r.pan += response.drag_delta();
                }
            });

        if let Some(c) = picked {
            self.pick_color(c);
            self.remember_color(self.color);
        }
        if pick_file {
            self.open_reference_image();
        }
        if remove {
            self.document.reference = None;
        }
        if open != self.reference.open || open != self.app_settings.reference_open {
            self.reference.open = open;
            self.app_settings.reference_open = open;
            self.save_app_settings();
        }
    }
}
