//! The Select Canvas dialog: a grid of paper/canvas textures
//! with lit thumbnails, texture scale and depth, and a palette of canvas colors.

use eframe::egui::{self, Color32, Sense, Stroke as LineStroke, StrokeKind, Vec2};
use oxide_engine::{Color, Paper, PaperKind, PaperSettings};

use super::{OxideApp, PAPER, to_color32};

const THUMB: usize = 104;

/// A named surface in the texture grid.
pub struct PaperPreset {
    pub name: &'static str,
    pub settings: PaperSettings,
}

fn paper_presets() -> Vec<PaperPreset> {
    let p = |name, kind, scale, depth| PaperPreset { name, settings: PaperSettings { kind, scale, depth, ..PaperSettings::default() } };
    vec![
        p("Hot Pressed", PaperKind::Smooth, 1.0, 0.35),
        p("Cold Pressed", PaperKind::ColdPress, 1.0, 0.5),
        p("Rough", PaperKind::Rough, 1.0, 0.6),
        p("Handmade", PaperKind::Handmade, 1.0, 0.6),
        p("Washi", PaperKind::Washi, 1.0, 0.5),
        p("Canvas", PaperKind::Canvas, 1.0, 0.4),
        p("Fine Canvas", PaperKind::Canvas, 0.6, 0.35),
        p("Linen", PaperKind::Linen, 1.0, 0.45),
        p("Gesso Panel", PaperKind::Panel, 1.0, 0.4),
        p("White Simple", PaperKind::Smooth, 1.0, 0.0),
    ]
}

/// Paper tones to choose from (plus any you add).
const CANVAS_COLORS: [[f32; 3]; 22] = [
    [0.96, 0.94, 0.89], // default
    [0.0, 0.0, 0.0],
    [0.17, 0.17, 0.17],
    [0.70, 0.70, 0.70],
    [0.78, 0.78, 0.78],
    [0.89, 0.86, 0.85],
    [0.90, 0.90, 0.89],
    [0.94, 0.94, 0.94],
    [0.97, 0.98, 1.0],
    [1.0, 1.0, 1.0],
    [0.96, 0.89, 0.78],
    [0.92, 0.91, 0.88],
    [0.93, 0.89, 0.80],
    [0.91, 0.89, 0.84],
    [0.75, 0.74, 0.69],
    [0.98, 0.96, 0.86],
    [0.93, 0.92, 0.89],
    [0.85, 0.85, 0.80],
    [0.60, 0.85, 1.0],
    [0.99, 0.88, 0.97],
    [0.20, 0.17, 0.16],
    [0.05, 0.08, 0.20],
];

/// Which painting the dialog is choosing a canvas for.
#[derive(Clone, Copy, PartialEq)]
pub enum CanvasTarget {
    /// The New Artwork dialog's choice.
    NewArtwork,
    /// The painting that's open now.
    Document,
}

pub struct SelectCanvas {
    pub open: bool,
    target: CanvasTarget,
    settings: PaperSettings,
    color: Color,
    /// Built the first time the dialog opens.
    thumbnails: Vec<(PaperPreset, egui::TextureHandle)>,
}

impl Default for SelectCanvas {
    fn default() -> Self {
        SelectCanvas {
            open: false,
            target: CanvasTarget::Document,
            settings: PaperSettings::default(),
            color: PAPER,
            thumbnails: Vec::new(),
        }
    }
}

impl SelectCanvas {
    /// Open the dialog starting from the given paper and color.
    pub fn open_for(&mut self, target: CanvasTarget, settings: PaperSettings, color: Color) {
        self.open = true;
        self.target = target;
        self.settings = settings;
        self.color = color;
    }
}

impl OxideApp {
    pub(super) fn select_canvas_dialog(&mut self, ctx: &egui::Context) {
        if !self.select_canvas.open {
            return;
        }
        if self.select_canvas.thumbnails.is_empty() {
            self.select_canvas.thumbnails = paper_presets()
                .into_iter()
                .map(|preset| {
                    let image = paper_thumbnail(preset.settings);
                    let texture = ctx.load_texture(format!("paper-{}", preset.name), image, egui::TextureOptions::NEAREST);
                    (preset, texture)
                })
                .collect();
        }

        let mut ok = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("select-canvas")).show(ctx, |ui| {
            ui.set_width(900.0);
            ui.vertical_centered(|ui| ui.heading("Select Canvas"));
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(470.0);
                    self.texture_column(ui);
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.set_width(400.0);
                    self.color_column(ui);
                });
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  OK  ").fill(ui.visuals().selection.bg_fill)).clicked() {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if modal.should_close() {
            cancel = true;
        }

        if ok {
            let sc = &self.select_canvas;
            match sc.target {
                CanvasTarget::NewArtwork => self.new_artwork.set_canvas(sc.settings, sc.color),
                CanvasTarget::Document => {
                    let (settings, color) = (sc.settings, sc.color);
                    self.document.set_surface(settings);
                    self.document.seal_history();
                    if color != self.document.paper() {
                        self.document.set_paper(color);
                        self.document.seal_history();
                    }
                }
            }
            self.select_canvas.open = false;
        } else if cancel {
            self.select_canvas.open = false;
        }
    }

    fn texture_column(&mut self, ui: &mut egui::Ui) {
        let sc = &mut self.select_canvas;
        ui.strong("Canvas Texture");
        egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6.0).show(ui, |ui| {
            ui.set_min_size(Vec2::new(458.0, 330.0));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                for (preset, texture) in &sc.thumbnails {
                    // Same kind + depth = this preset (scale is adjusted separately).
                    let selected = preset.settings.kind == sc.settings.kind
                        && (preset.settings.depth - sc.settings.depth).abs() < 0.01
                        && (preset.settings.depth == 0.0 || preset.name != "Fine Canvas" || sc.settings.scale < 0.8);
                    let cell = Vec2::new(THUMB as f32 + 8.0, THUMB as f32 + 24.0);
                    let (rect, response) = ui.allocate_exact_size(cell, Sense::click());
                    let response = response.on_hover_text(preset.name);
                    let visuals = ui.style().interact_selectable(&response, selected);
                    ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
                    if selected {
                        ui.painter().rect_stroke(rect, 3.0, LineStroke::new(2.0, ui.visuals().selection.bg_fill), StrokeKind::Inside);
                    }
                    let image_rect = egui::Rect::from_min_size(rect.min + Vec2::splat(4.0), Vec2::splat(THUMB as f32));
                    egui::Image::new(texture).paint_at(ui, image_rect);
                    ui.painter().text(
                        egui::pos2(rect.center().x, image_rect.max.y + 3.0),
                        egui::Align2::CENTER_TOP,
                        preset.name,
                        egui::FontId::proportional(12.0),
                        visuals.text_color(),
                    );
                    if response.clicked() {
                        let light_angle = sc.settings.light_angle;
                        // A new surface keeps your light direction and texture visibility.
                        sc.settings = PaperSettings {
                            light_angle,
                            paper_visibility: sc.settings.paper_visibility,
                            paint_visibility: sc.settings.paint_visibility,
                            ..preset.settings
                        };
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::Button::new("Import paper…"))
                .on_disabled_hover_text("Use a scan of your own paper: coming later");
        });
        ui.add_space(4.0);
        let mut percent = sc.settings.scale * 100.0;
        if ui.add(egui::Slider::new(&mut percent, 50.0..=300.0).suffix("%").max_decimals(0).text("Texture Scale")).changed() {
            sc.settings.scale = percent / 100.0;
        }
        ui.add(egui::Slider::new(&mut sc.settings.depth, 0.0..=1.0).text("Depth"));
        let mut off = false;
        ui.add_enabled(false, egui::Checkbox::new(&mut off, "Use Deckled Edges"))
            .on_disabled_hover_text("Torn, uneven paper edges: coming later");
    }

    fn color_column(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let mut c = to_color32(self.select_canvas.color);
            if ui.color_edit_button_srgba(&mut c).on_hover_text("Pick any color").changed() {
                let [r, g, b, _] = c.to_array();
                self.select_canvas.color = Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
            }
            ui.strong("Canvas Color");
        });

        let custom = self.app_settings.custom_paper_colors.clone();
        let colors: Vec<(Color, bool)> = CANVAS_COLORS
            .iter()
            .map(|&[r, g, b]| (Color::rgb(r, g, b), false))
            .chain(custom.iter().map(|&[r, g, b]| (Color::rgb(r, g, b), true)))
            .collect();

        let mut picked = None;
        egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6.0).show(ui, |ui| {
            ui.set_min_size(Vec2::new(388.0, 330.0));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(5.0);
                for (i, (color, is_custom)) in colors.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::click());
                    let selected = to_color32(*color) == to_color32(self.select_canvas.color);
                    ui.painter().rect_filled(rect.shrink(2.0), 3.0, to_color32(*color));
                    let stroke = if selected {
                        LineStroke::new(2.0, ui.visuals().selection.bg_fill)
                    } else if response.hovered() {
                        LineStroke::new(1.5, Color32::WHITE)
                    } else {
                        LineStroke::new(1.0, Color32::from_gray(70))
                    };
                    ui.painter().rect_stroke(rect.shrink(1.0), 3.0, stroke, StrokeKind::Inside);
                    if i == 0 {
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Default", egui::FontId::proportional(10.0), Color32::from_gray(60));
                    }
                    let tip = if i == 0 { "Default paper color" } else if *is_custom { "Your color" } else { "Paper tone" };
                    if response.on_hover_text(tip).clicked() {
                        picked = Some(*color);
                    }
                }
            });
        });
        if let Some(color) = picked {
            self.select_canvas.color = color;
        }

        // + saves the current color to your palette, − removes it again.
        let current = self.select_canvas.color;
        let current_rgb = [current.r, current.g, current.b];
        let is_saved = self.app_settings.custom_paper_colors.iter().any(|c| to_color32(Color::rgb(c[0], c[1], c[2])) == to_color32(current));
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(is_saved, egui::Button::new("−")).on_hover_text("Remove this color from your palette").clicked() {
                    self.app_settings
                        .custom_paper_colors
                        .retain(|c| to_color32(Color::rgb(c[0], c[1], c[2])) != to_color32(current));
                    self.save_app_settings();
                }
                if ui.add_enabled(!is_saved, egui::Button::new("+")).on_hover_text("Add the current color to the palette").clicked() {
                    self.app_settings.custom_paper_colors.push(current_rgb);
                    self.save_app_settings();
                }
            });
        });
    }
}

/// A grayscale, lit square of a paper (a texture thumbnail).
fn paper_thumbnail(settings: PaperSettings) -> egui::ColorImage {
    let paper = Paper::new(settings);
    let mut bytes = Vec::with_capacity(THUMB * THUMB * 4);
    for y in 0..THUMB {
        for x in 0..THUMB {
            // Flat papers show plain white; textured ones a neutral gray so the relief reads.
            let base = if settings.depth == 0.0 { 1.0 } else { 0.78 };
            let v = ((base * paper.shade_at(x, y)).clamp(0.0, 1.0) * 255.0).round() as u8;
            bytes.extend([v, v, v, 255]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([THUMB, THUMB], &bytes)
}
