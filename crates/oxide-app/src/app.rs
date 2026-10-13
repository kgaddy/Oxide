//! App state and the overall window layout: menu bar, left/right panels, canvas.

mod brush_creator;
mod canvas_view;
mod export_dialog;
mod files;
mod layers;
mod mixing_palette;
mod new_artwork;
mod select_canvas;
mod panels;

use std::path::PathBuf;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use eframe::egui::ecolor::{hsv_from_rgb, rgb_from_hsv};
use oxide_engine::{Brush, Color, Document, Mixing, PaperKind, PaperSettings, Stroke};

use crate::presets::PresetLibrary;
use crate::pen::{PenInput, apply_curve};
use crate::settings::AppSettings;
use crate::tiles::TiledTexture;
use files::{PendingAction, Status};
use crate::tools::{Tool, ToolState};


/// Warm off-white, like real paper.
pub const PAPER: Color = Color::rgb(0.96, 0.94, 0.89);

/// New paintings start on a primed canvas weave (Oxide is about oils first).
pub fn default_surface() -> PaperSettings {
    PaperSettings { kind: PaperKind::Canvas, depth: 0.4, ..PaperSettings::default() }
}

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
    document: Document,
    /// The current paint color (shared by all painting tools).
    color: Color,
    tool: Tool,
    /// Each tool's own brush shape and medium settings, indexed by `Tool::index`.
    tool_states: [ToolState; Tool::COUNT],
    /// The current color as hue/saturation/value, kept separately from
    /// `brush.color` so the hue doesn't jump around when saturation is 0.
    hsv: (f32, f32, f32),
    recent_colors: Vec<Color>,
    /// The stroke being painted right now, if the button is down.
    stroke: Option<Stroke>,
    /// Keep picked-up paint on the brush between strokes.
    dirty_brush: bool,
    /// The paint left on a dirty brush from the last stroke.
    carried_color: Option<Color>,
    view: View,
    /// The flattened painting uploaded to the GPU (as tiles) so egui can draw it.
    texture: TiledTexture,
    presets: PresetLibrary,
    brush_creator: brush_creator::BrushCreator,
    /// Workspace and canvas rectangles from the last frame (for the navigator).
    last_workspace: egui::Rect,
    /// Where the pointer was last frame and which way it's moving (smoothed),
    /// so the cursor can show which way a rotating tip is facing.
    last_hover: Option<egui::Pos2>,
    cursor_dir: Option<(f32, f32)>,
    show_left: bool,
    show_right: bool,
    /// The layer name being typed in the Layers panel (committed on Enter).
    renaming: Option<String>,

    // ----- Files -----
    file_path: Option<PathBuf>,
    /// `document.state_id()` when last saved/opened; different = unsaved changes.
    saved_state: u64,
    /// An action waiting on the "Save changes?" dialog.
    pending: Option<PendingAction>,
    /// Set once the user confirmed quitting, so the close isn't intercepted again.
    allow_close: bool,
    status: Option<Status>,
    /// The window title we last set (to avoid re-sending it every frame).
    title: String,
    /// Recent files, favorite sizes, ... (saved between runs).
    app_settings: AppSettings,
    new_artwork: new_artwork::NewArtwork,
    select_canvas: select_canvas::SelectCanvas,
    export_dialog: export_dialog::ExportDialog,
    mixing_palette: mixing_palette::MixingPalette,
    /// The drawing tablet: pressure, tilt, rotation.
    pen: PenInput,
    show_pen_settings: bool,
    /// How long our code took to build the last frames (ms, smoothed), shown
    /// in the status bar so slowdowns are easy to spot.
    frame_ms: f32,
}

/// Everything you can do from a menu or keyboard shortcut.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Command {
    New,
    Open,
    Save,
    SaveAs,
    SaveIteration,
    ImportImage,
    Export,
    Quit,
    Undo,
    Redo,
    Fit,
    ActualSize,
    ZoomIn,
    ZoomOut,
    DryLayer,
    DryAll,
    MixingPalette,
}

/// Keyboard shortcuts. Order matters: egui ignores *extra* Shift/Alt when
/// matching, so ⇧⌘S must be checked before ⌘S or it would trigger Save.
const SHORTCUTS: [(Command, KeyboardShortcut); 18] = [
    (Command::MixingPalette, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M)),
    (Command::DryAll, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::D)),
    (Command::DryLayer, KeyboardShortcut::new(Modifiers::COMMAND, Key::D)),
    (Command::SaveAs, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S)),
    (Command::SaveIteration, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::S)),
    (Command::Save, KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
    (Command::Redo, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z)),
    (Command::Undo, KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)),
    (Command::Export, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E)),
    (Command::New, KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
    (Command::Open, KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
    (Command::ImportImage, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::I)),
    (Command::Quit, KeyboardShortcut::new(Modifiers::COMMAND, Key::Q)),
    (Command::Fit, KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0)),
    (Command::ActualSize, KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1)),
    (Command::ZoomIn, KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals)),
    (Command::ZoomIn, KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus)),
    (Command::ZoomOut, KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus)),
];

impl OxideApp {
    pub fn new(ctx: &egui::Context) -> Self {
        apply_theme(ctx);
        // A pen tip wobbles a little when you tap. egui only counts a press as a
        // click if it moves less than 6 points, so pen taps on buttons were often
        // seen as tiny drags and ignored. Be more forgiving.
        ctx.options_mut(|o| o.input_options.max_click_dist = 16.0);
        let app_settings = AppSettings::load();
        let new_artwork = new_artwork::NewArtwork::from_settings(&app_settings);
        // Behind the New Artwork dialog: the last-used (or default) canvas,
        // so there's something to paint on if you cancel or turn the dialog off.
        let mut document = new_artwork.spec().create();
        document.take_dirty();
        let texture = TiledTexture::new(ctx, &document);
        let c = Color::BLACK;
        OxideApp {
            saved_state: document.state_id(),
            document,
            hsv: hsv_from_rgb([c.r, c.g, c.b]),
            color: c,
            tool: Tool::OilsAcrylics,
            tool_states: Tool::ALL.map(Tool::default_state),
            recent_colors: Vec::new(),
            stroke: None,
            dirty_brush: true,
            carried_color: None,
            view: View { fit: true, zoom: 1.0, pan: egui::Vec2::ZERO, last_scale: 1.0 },
            texture,
            presets: PresetLibrary::load(ctx),
            brush_creator: brush_creator::BrushCreator::default(),
            last_workspace: egui::Rect::NOTHING,
            last_hover: None,
            cursor_dir: None,
            show_left: true,
            show_right: true,
            renaming: None,
            file_path: None,
            pending: None,
            allow_close: false,
            status: None,
            title: String::new(),
            app_settings,
            new_artwork,
            select_canvas: select_canvas::SelectCanvas::default(),
            export_dialog: export_dialog::ExportDialog::default(),
            mixing_palette: mixing_palette::MixingPalette::default(),
            frame_ms: 0.0,
            pen: PenInput::new(ctx),
            show_pen_settings: false,
        }
    }

    /// The current tool's shape and settings.
    fn state(&self) -> &ToolState {
        &self.tool_states[self.tool.index()]
    }

    fn state_mut(&mut self) -> &mut ToolState {
        &mut self.tool_states[self.tool.index()]
    }

    /// The engine brush for the current tool, color, and settings.
    fn current_brush(&self) -> Brush {
        let mut brush = self.state().brush(self.tool, self.color);
        brush.mixing = if self.app_settings.natural_colors { Mixing::Pigment } else { Mixing::Rgb };
        brush
    }

    /// Set the brush color from anywhere (swatch, picker, ...) and keep the wheel in sync.
    fn set_color(&mut self, color: Color) {
        // Picking a new color means fresh paint: clean the brush.
        self.carried_color = None;
        self.color = Color { a: 1.0, ..color };
        self.hsv = hsv_from_rgb([color.r, color.g, color.b]);
    }

    /// Set the brush color from the wheel's hue/saturation/value.
    fn set_hsv(&mut self, hsv: (f32, f32, f32)) {
        self.hsv = hsv;
        self.carried_color = None;
        let [r, g, b] = rgb_from_hsv(hsv);
        self.color = Color::rgb(r, g, b);
    }

    /// Remember a color in the "recent" row (most recent first, no duplicates).
    fn remember_color(&mut self, color: Color) {
        self.recent_colors.retain(|c| *c != color);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(MAX_RECENT_COLORS);
    }

    /// Re-upload only the part of the painting that changed since last frame.
    fn upload_changes(&mut self) {
        if let Some(region) = self.document.take_dirty()
            && !region.is_empty()
        {
            self.texture.update(&self.document, region);
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        // Don't undo or open files in the middle of a stroke.
        if self.stroke.is_some() {
            return;
        }
        match command {
            Command::New => self.new_artwork.open = true,
            Command::Open => self.request(PendingAction::Open, ctx),
            Command::Quit => self.request(PendingAction::Quit, ctx),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::SaveIteration => {
                self.save_iteration();
            }
            Command::ImportImage => self.import_image(),
            Command::Export => self.export_dialog.open = true,
            Command::Undo => self.document.undo(),
            Command::Redo => self.document.redo(),
            Command::Fit => self.view.fit = true,
            Command::ActualSize => self.view.actual_size(),
            Command::ZoomIn => self.view.zoom_by(1.25),
            Command::ZoomOut => self.view.zoom_by(0.8),
            Command::DryLayer => self.document.dry_layer(),
            Command::DryAll => self.document.dry_all(),
            Command::MixingPalette => self.mixing_palette.open = !self.mixing_palette.open,
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        let mut natural_changed = false;
        // A menu item that shows its keyboard shortcut and records the click.
        let mut item = |ui: &mut egui::Ui, label: &str, command: Command, enabled: bool| {
            let mut button = egui::Button::new(label);
            if let Some((_, shortcut)) = SHORTCUTS.iter().find(|(c, _)| *c == command) {
                button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
            }
            if ui.add_enabled(enabled, button).clicked() {
                clicked = Some(command);
            }
        };

        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item(ui, "New", Command::New, true);
                item(ui, "Open…", Command::Open, true);
                ui.separator();
                item(ui, "Save", Command::Save, true);
                item(ui, "Save As…", Command::SaveAs, true);
                item(ui, "Save Iteration", Command::SaveIteration, true);
                ui.separator();
                item(ui, "Import Image as Layer…", Command::ImportImage, true);
                item(ui, "Export…", Command::Export, true);
                ui.separator();
                item(ui, "Quit", Command::Quit, true);
            });
            ui.menu_button("Edit", |ui| {
                let undo = self.document.undo_description();
                let redo = self.document.redo_description();
                item(ui, &undo.map_or("Undo".into(), |d| format!("Undo {d}")), Command::Undo, undo.is_some());
                item(ui, &redo.map_or("Redo".into(), |d| format!("Redo {d}")), Command::Redo, redo.is_some());
                ui.separator();
                if ui
                    .checkbox(&mut self.app_settings.natural_colors, "Natural Colors")
                    .on_hover_text("On: colors mix like real paint (blue + yellow = green).\nOff: they mix like light on a screen (blue + yellow = gray).")
                    .changed()
                {
                    natural_changed = true;
                }
            });
            ui.menu_button("View", |ui| {
                item(ui, "Fit to window", Command::Fit, true);
                item(ui, "Actual size", Command::ActualSize, true);
                item(ui, "Zoom in", Command::ZoomIn, true);
                item(ui, "Zoom out", Command::ZoomOut, true);
            });
            ui.menu_button("Layer", |ui| {
                if ui.button("New layer").clicked() {
                    self.document.add_layer();
                }
                if ui.add_enabled(self.document.layers().len() > 1, egui::Button::new("Delete layer")).clicked() {
                    self.document.delete_active_layer();
                }
                ui.separator();
                if ui.button("Move layer up").clicked() {
                    self.document.move_active_layer(1);
                }
                if ui.button("Move layer down").clicked() {
                    self.document.move_active_layer(-1);
                }
                ui.separator();
                item(ui, "Dry Layer", Command::DryLayer, true);
                item(ui, "Dry All Layers", Command::DryAll, true);
                if ui.button("Rewet Layer").clicked() {
                    self.document.rewet_layer();
                }
            });
            ui.menu_button("Window", |ui| {
                ui.checkbox(&mut self.brush_creator.open, "Brush creator");
                ui.checkbox(&mut self.show_pen_settings, "Pen settings");
                let mut palette_open = self.mixing_palette.open;
                let shortcut = SHORTCUTS.iter().find(|(c, _)| *c == Command::MixingPalette).map(|(_, s)| ui.ctx().format_shortcut(s));
                if ui.checkbox(&mut palette_open, format!("Mixing palette   {}", shortcut.unwrap_or_default())).changed() {
                    self.mixing_palette.open = palette_open;
                }
                ui.checkbox(&mut self.show_left, "Tools & brushes (left)");
                ui.checkbox(&mut self.show_right, "Navigator, color & layers (right)");
            });
            ui.menu_button("Help", |ui| {
                ui.label("Oxide 0.1: natural-media painting");
                ui.separator();
                ui.label("B oils & acrylics · N ink · P pencil · M marker · A airbrush");
                ui.label("E eraser · S smudge · I picker");
                ui.label("[ / ] brush size");
                ui.label("⌘Z undo · ⇧⌘Z redo");
                ui.label("⌘D dry layer · ⇧⌘D dry all");
                ui.label("Space+drag or middle-drag to pan");
                ui.label("Pinch or ⌘+scroll to zoom");
            });
        });

        if let Some(command) = clicked {
            self.run(command, &ui.ctx().clone());
        }
        if natural_changed {
            self.save_app_settings();
            let mode = if self.app_settings.natural_colors { "paint (pigment)" } else { "light (RGB)" };
            self.info(format!("Colors now mix like {mode}"));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Collect first, run after: running a command (e.g. Quit) talks to the
        // egui context, which is locked while we're inside `input_mut`.
        let mut commands = Vec::new();
        ctx.input_mut(|i| {
            for (command, shortcut) in &SHORTCUTS {
                if i.consume_shortcut(shortcut) {
                    commands.push(*command);
                }
            }
        });
        for command in commands {
            self.run(command, ctx);
        }

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
                let shape = &mut self.state_mut().shape;
                shape.radius = (shape.radius / 1.15).max(0.5);
            }
            if i.key_pressed(Key::CloseBracket) {
                let shape = &mut self.state_mut().shape;
                shape.radius = (shape.radius * 1.15).min(200.0);
            }
        });
    }

    /// Pressure curve and a live meter, to tune how light touches feel.
    fn pen_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_pen_settings;
        let mut changed = false;
        egui::Window::new("Pen Settings").open(&mut open).resizable(false).default_width(320.0).show(ctx, |ui| {
            let pen = self.pen.read();
            if !self.pen.available() {
                ui.colored_label(ui.visuals().error_fg_color, "Couldn't listen to the tablet.");
            } else if pen.in_proximity {
                ui.label("✔ Pen detected");
            } else {
                ui.weak("Bring the pen near the tablet…");
            }
            ui.add_space(6.0);

            let raw = pen.pressure;
            let curve = self.app_settings.pressure_curve;
            ui.label(format!("Pressure from the pen: {:.0}%", raw * 100.0));
            ui.add(egui::ProgressBar::new(raw).desired_width(280.0));
            ui.label(format!("After the curve (what brushes get): {:.0}%", apply_curve(raw, curve) * 100.0));
            ui.add(egui::ProgressBar::new(apply_curve(raw, curve)).desired_width(280.0));
            ui.add_space(6.0);

            // Shown as Soft (left) .. Firm (right); stored as an exponent.
            let mut firmness = curve.log2();
            ui.horizontal(|ui| {
                ui.label("Soft");
                if ui.add(egui::Slider::new(&mut firmness, -1.5..=1.0).show_value(false)).changed() {
                    self.app_settings.pressure_curve = 2f32.powf(firmness);
                    changed = true;
                }
                ui.label("Firm");
            });
            ui.weak("Softer = light touches give bigger, darker marks.");
            ui.add_space(6.0);
            ui.label(format!("Tilt: {:+.2}, {:+.2}   Rotation: {:.0}°", pen.tilt.0, pen.tilt.1, pen.rotation));
            ui.add_space(6.0);
            ui.separator();
            ui.weak("Diagnostics");
            ui.monospace(format!("events seen:      {}", pen.events_seen));
            ui.monospace(format!("with pen data:    {}", pen.pen_events_seen));
            ui.monospace(format!("last event:       {}", pen.last_description));
            ui.monospace(format!("last raw pressure: {:.3}", pen.last_raw_pressure));
            ui.monospace(format!("pen touching:     {}", pen.touching()));
            ui.add_space(6.0);
            ui.weak("If very light touches don't register at all, also lower the click/activation pressure in the XP-Pen driver settings.");
            // Keep the meter moving while this window is open.
            ui.ctx().request_repaint();
        });
        self.show_pen_settings = open;
        if changed {
            self.save_app_settings();
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} × {} px · {:.0}% · {}",
                self.document.width(),
                self.document.height(),
                self.view.last_scale * 100.0,
                self.document.layers()[self.document.active_index()].name,
            ));
            let pen = self.pen.read();
            if pen.in_proximity {
                ui.separator();
                let pressure = apply_curve(pen.pressure, self.app_settings.pressure_curve);
                ui.weak(format!("Pen {:.0}%", pressure * 100.0)).on_hover_text("Pen pressure after the pressure curve (Window → Pen settings)");
            }
            ui.separator();
            let frame = format!("{:.0} ms/frame", self.frame_ms);
            if self.frame_ms > 30.0 {
                ui.colored_label(egui::Color32::from_rgb(230, 170, 60), frame)
                    .on_hover_text("Over ~30 ms per frame feels laggy. Try a release build: cargo run --release -p oxide-app");
            } else {
                ui.weak(frame).on_hover_text("Time Oxide spends building each frame (lower is smoother)");
            }
            if let Some(status) = &self.status {
                ui.separator();
                if status.is_error {
                    ui.colored_label(ui.visuals().error_fg_color, &status.text);
                } else {
                    ui.weak(&status.text);
                }
            }
        });
    }
}

impl eframe::App for OxideApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = std::time::Instant::now();
        let ctx = ui.ctx().clone();
        self.handle_close_request(&ctx);
        if self.pending.is_none() && !self.new_artwork.open && !self.select_canvas.open && !self.export_dialog.open {
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));

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

        self.brush_creator_window(&ctx);
        self.pen_settings_window(&ctx);
        self.mixing_palette_window(&ctx);
        self.new_artwork_dialog(&ctx);
        // Drawn after New Artwork so it sits on top when opened from there.
        self.select_canvas_dialog(&ctx);
        self.export_dialog(&ctx);
        self.pending_dialog(&ctx);

        // Slider drags (opacity, paper color) become one undo step: close the
        // step once the mouse button is up.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.document.seal_history();
        }
        // Undo/redo/opening change pixels outside of painting: show them.
        self.upload_changes();
        self.update_title(&ctx);

        let ms = started.elapsed().as_secs_f32() * 1000.0;
        self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms * 0.9 + ms * 0.1 };
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


pub fn to_color32(c: Color) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
}
