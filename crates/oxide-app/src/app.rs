//! App state and the overall window layout: menu bar, left/right panels, canvas.

mod canvas_view;
mod panels;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use eframe::egui::ecolor::{hsv_from_rgb, rgb_from_hsv};
use oxide_engine::{Brush, Canvas, Color, Stroke};

use crate::tools::Tool;

const CANVAS_WIDTH: usize = 1600;
const CANVAS_HEIGHT: usize = 1100;

/// Warm off-white, like real paper. The eraser paints with this too.
pub const PAPER: Color = Color::rgb(0.96, 0.94, 0.89);

const MAX_RECENT_COLORS: usize = 10;

/// How the canvas is placed in the workspace.
pub struct View {
    /// When true, the canvas is scaled to fit the window (and `zoom`/`pan` are ignored).
    pub fit: bool,
    /// Screen points per canvas pixel.
    pub zoom: f32,
    /// Offset of the canvas center from the workspace center, in screen points.
    pub pan: egui::Vec2,
    /// The scale actually used last frame (useful while `fit` is on).
    pub last_scale: f32,
}

impl View {
    pub const MIN_ZOOM: f32 = 0.05;
    pub const MAX_ZOOM: f32 = 32.0;

    /// Switch from "fit" to manual zoom, starting from what's on screen now.
    fn unfit(&mut self) {
        if self.fit {
            self.fit = false;
            self.zoom = self.last_scale;
            self.pan = egui::Vec2::ZERO;
        }
    }

    fn zoom_by(&mut self, factor: f32) {
        self.unfit();
        self.zoom = (self.zoom * factor).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }

    fn actual_size(&mut self) {
        self.fit = false;
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }
}

pub struct OxideApp {
    canvas: Canvas,
    brush: Brush,
    tool: Tool,
    /// The current color as hue/saturation/value, kept separately from
    /// `brush.color` so the hue doesn't jump around when saturation is 0.
    hsv: (f32, f32, f32),
    recent_colors: Vec<Color>,
    /// The stroke being painted right now, if the button is down.
    stroke: Option<Stroke>,
    view: View,
    /// The canvas pixels uploaded to the GPU so egui can draw them.
    texture: egui::TextureHandle,
    /// Set when the canvas changed and the texture needs re-uploading.
    dirty: bool,
    /// Small stroke previews for the brush preset buttons.
    preset_thumbnails: Vec<egui::TextureHandle>,
    /// Workspace and canvas rectangles from the last frame (for the navigator).
    last_workspace: egui::Rect,
    show_left: bool,
    show_right: bool,
}

impl OxideApp {
    pub fn new(ctx: &egui::Context) -> Self {
        apply_theme(ctx);
        let canvas = Canvas::new(CANVAS_WIDTH, CANVAS_HEIGHT, PAPER);
        let texture = ctx.load_texture("canvas", to_image(&canvas), egui::TextureOptions::LINEAR);
        let brush = Brush::default();
        let c = brush.color;
        OxideApp {
            canvas,
            hsv: hsv_from_rgb([c.r, c.g, c.b]),
            brush,
            tool: Tool::Brush,
            recent_colors: Vec::new(),
            stroke: None,
            view: View { fit: true, zoom: 1.0, pan: egui::Vec2::ZERO, last_scale: 1.0 },
            texture,
            dirty: false,
            preset_thumbnails: panels::render_preset_thumbnails(ctx),
            last_workspace: egui::Rect::NOTHING,
            show_left: true,
            show_right: true,
        }
    }

    /// Set the brush color from anywhere (swatch, picker, ...) and keep the wheel in sync.
    fn set_color(&mut self, color: Color) {
        self.brush.color = Color { a: 1.0, ..color };
        self.hsv = hsv_from_rgb([color.r, color.g, color.b]);
    }

    /// Set the brush color from the wheel's hue/saturation/value.
    fn set_hsv(&mut self, hsv: (f32, f32, f32)) {
        self.hsv = hsv;
        let [r, g, b] = rgb_from_hsv(hsv);
        self.brush.color = Color::rgb(r, g, b);
    }

    /// Remember a color in the "recent" row (most recent first, no duplicates).
    fn remember_color(&mut self, color: Color) {
        self.recent_colors.retain(|c| *c != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(MAX_RECENT_COLORS);
    }

    fn new_canvas(&mut self) {
        self.canvas.clear(PAPER);
        self.dirty = true;
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New canvas").shortcut_text(shortcut_text(ui, NEW))).clicked() {
                    self.new_canvas();
                }
                ui.add_enabled(false, egui::Button::new("Open…"));
                ui.add_enabled(false, egui::Button::new("Save…"));
                ui.separator();
                if ui.add(egui::Button::new("Quit").shortcut_text(shortcut_text(ui, QUIT))).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                ui.add_enabled(false, egui::Button::new("Undo"))
                    .on_disabled_hover_text("Coming in milestone 7");
                ui.add_enabled(false, egui::Button::new("Redo"))
                    .on_disabled_hover_text("Coming in milestone 7");
            });
            ui.menu_button("View", |ui| {
                if ui.add(egui::Button::new("Fit to window").shortcut_text(shortcut_text(ui, FIT))).clicked() {
                    self.view.fit = true;
                }
                if ui.add(egui::Button::new("Actual size").shortcut_text(shortcut_text(ui, ACTUAL))).clicked() {
                    self.view.actual_size();
                }
                if ui.add(egui::Button::new("Zoom in").shortcut_text(shortcut_text(ui, ZOOM_IN))).clicked() {
                    self.view.zoom_by(1.25);
                }
                if ui.add(egui::Button::new("Zoom out").shortcut_text(shortcut_text(ui, ZOOM_OUT))).clicked() {
                    self.view.zoom_by(0.8);
                }
            });
            ui.menu_button("Window", |ui| {
                ui.checkbox(&mut self.show_left, "Tools & brushes (left)");
                ui.checkbox(&mut self.show_right, "Navigator, color & layers (right)");
            });
            ui.menu_button("Help", |ui| {
                ui.label("Oxide 0.1: natural-media painting");
                ui.separator();
                ui.label("B brush · E eraser · I picker");
                ui.label("[ / ] brush size");
                ui.label("Space+drag or middle-drag to pan");
                ui.label("Pinch or ⌘+scroll to zoom");
            });
        });
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input_mut(|i| {
            if i.consume_shortcut(&NEW) {
                self.canvas.clear(PAPER);
                self.dirty = true;
            }
            if i.consume_shortcut(&QUIT) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if i.consume_shortcut(&FIT) {
                self.view.fit = true;
            }
            if i.consume_shortcut(&ACTUAL) {
                self.view.actual_size();
            }
            if i.consume_shortcut(&ZOOM_IN) || i.consume_shortcut(&ZOOM_IN_PLUS) {
                self.view.zoom_by(1.25);
            }
            if i.consume_shortcut(&ZOOM_OUT) {
                self.view.zoom_by(0.8);
            }
        });

        // Single-letter shortcuts, but not while typing in a text field.
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        ctx.input(|i| {
            for tool in Tool::ALL {
                let key = Key::from_name(tool.shortcut()).expect("tool shortcuts are letter keys");
                if i.key_pressed(key) && i.modifiers.is_none() {
                    self.tool = tool;
                }
            }
            if i.key_pressed(Key::OpenBracket) {
                self.brush.radius = (self.brush.radius / 1.15).max(0.5);
            }
            if i.key_pressed(Key::CloseBracket) {
                self.brush.radius = (self.brush.radius * 1.15).min(200.0);
            }
        });
    }
}

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
const ACTUAL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_IN_PLUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);

fn shortcut_text(ui: &egui::Ui, shortcut: KeyboardShortcut) -> String {
    ui.ctx().format_shortcut(&shortcut)
}

impl eframe::App for OxideApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));

        if self.show_left {
            egui::Panel::left("left_panel")
                .default_size(250.0)
                .min_size(200.0)
                .show(ui, |ui| self.left_panel(ui));
        }
        if self.show_right {
            egui::Panel::right("right_panel")
                .default_size(270.0)
                .min_size(220.0)
                .show(ui, |ui| self.right_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(egui::Color32::from_gray(52)))
            .show(ui, |ui| self.canvas_view(ui));
    }
}

/// Dark theme with a blue accent, in the spirit of another app's workspace.
fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    let accent = egui::Color32::from_rgb(52, 132, 228);
    visuals.selection.bg_fill = accent;
    visuals.hyperlink_color = accent;
    visuals.panel_fill = egui::Color32::from_gray(32);
    visuals.window_fill = egui::Color32::from_gray(36);
    visuals.extreme_bg_color = egui::Color32::from_gray(22);
    ctx.set_visuals(visuals);
}

pub fn to_image(canvas: &Canvas) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([canvas.width(), canvas.height()], canvas.pixels())
}

pub fn to_color32(c: Color) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
}
