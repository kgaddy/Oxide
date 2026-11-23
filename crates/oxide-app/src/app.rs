//! App state and the overall window layout: menu bar, left/right icon rails with popovers, canvas.

mod brush_creator;
mod canvas_view;
pub use canvas_view::ViewXform;
mod bench;
mod export_dialog;
mod files;
mod hatch;
mod help;
mod layers;
mod menus;
mod mixing_palette;
mod new_artwork;
mod panels;
mod quick;
mod recording;
mod reference_image;
mod select_canvas;
mod select_tool;
mod settings_window;
mod theming;
mod transform;

use std::path::PathBuf;

use eframe::egui::ecolor::{hsv_from_rgb, rgb_from_hsv};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use oxide_engine::{Brush, Color, Document, Mixing, PaintMode, PaperKind, PaperSettings, Stroke};

use crate::panel_layout::Side;
use crate::pen::{PenInput, apply_curve};
use crate::presets::PresetLibrary;
use crate::settings::AppSettings;
use crate::tiles::TiledTexture;
use crate::tools::{Tool, ToolState};
use files::{PendingAction, Status};

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
    /// View rotation in degrees (the painting itself is never rotated).
    pub rotation: f32,
    /// Mirror the view left-right (Shift+F), to check the drawing with fresh eyes.
    pub flip: bool,
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
    /// The stroke's end drawn ahead of time (held-back lift-off dabs and the
    /// gap to the cursor), restored before the stroke goes on (see `Stroke::preview`).
    stroke_preview: Option<oxide_engine::Preview>,
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
    /// Minimal layout: the open popovers, in the order opened, each with its rail button's rect.
    popovers: Vec<panels::Popover>,
    /// The Quick section is showing its editor (choose its items).
    quick_editing: bool,
    /// The layer name being typed in the Layers panel (committed on Enter).
    renaming: Option<String>,
    /// A layer being renamed right in the list (double-click): its index,
    /// the name so far, and whether its text box has been focused yet.
    renaming_row: Option<(usize, String, bool)>,
    /// The tool to go back to after picking a color: while ⌥ is held over the
    /// canvas (`true`), or after one pick from the Pick button (`false`).
    pick_return: Option<(Tool, bool)>,

    // ----- Files -----
    file_path: Option<PathBuf>,
    /// `document.state_id()` when last saved/opened; different = unsaved changes.
    saved_state: u64,
    /// `document.reference` when last saved/opened (a different photo = unsaved changes).
    saved_reference: Option<std::sync::Arc<Vec<u8>>>,
    /// An action waiting on the "Save changes?" dialog.
    pending: Option<PendingAction>,
    /// "New group…" on a brush: (preset index, name typed so far) while the dialog is open.
    new_group: Option<(usize, String)>,
    /// Set once the user confirmed quitting, so the close isn't intercepted again.
    allow_close: bool,
    status: Option<Status>,
    /// The UI theme (colors, shapes, fonts), and its file if it's a user theme.
    themes: theming::ThemeState,
    /// The window title we last set (to avoid re-sending it every frame).
    title: String,
    /// Recent files, favorite sizes, ... (saved between runs).
    app_settings: AppSettings,
    new_artwork: new_artwork::NewArtwork,
    select_canvas: select_canvas::SelectCanvas,
    export_dialog: export_dialog::ExportDialog,
    mixing_palette: mixing_palette::MixingPalette,
    /// Pressure used when painting with a mouse (pressure-based paint modes need it).
    mouse_pressure: f32,
    /// A V/A mode key that's down: the mode before it, and when it was pressed
    /// (released quickly = switch; held = only while held).
    mode_key: Option<(Key, ToolState, std::time::Instant)>,
    /// The paint mode to return to when V toggles Blend off again.
    last_paint_mode: PaintMode,
    /// ⌘-drag on the canvas: size and loading when the drag started.
    adjust_drag: Option<(egui::Pos2, f32, f32)>,
    /// The drawing tablet: pressure, tilt, rotation.
    pen: PenInput,
    show_pen_settings: bool,
    /// The Settings window (⌘,): tool starting values.
    settings: settings_window::SettingsWindow,
    /// Window → Reference Image (a photo to look at while painting).
    reference: reference_image::ReferenceImage,
    /// Help → Keyboard Shortcuts window.
    show_shortcuts: bool,
    /// Help → Oxide Help: the wiki's Using Oxide pages.
    help: help::Help,
    /// The Brushes panel's search box.
    brush_search: String,
    /// The group name being edited in the Layers panel.
    renaming_group: Option<String>,
    /// The Select tool: its shape, the selection being dragged, the outline
    /// drawing cache, and the Feather amount.
    select_kind: select_tool::SelectKind,
    select_drag: Option<select_tool::SelectDrag>,
    selection_outline: select_tool::OutlineCache,
    feather_radius: f32,
    /// The last copied paint (keeps wetness and thickness), and a fingerprint
    /// of the image we put on the system clipboard, to know if it's still ours.
    clip: Option<(oxide_engine::Clip, u64)>,
    /// The preset picked last for each tool (for the brush name in Properties).
    brush_names: [Option<String>; Tool::COUNT],
    /// The secondary color (the second swatch; ⌘\\ swaps).
    secondary: Color,
    /// Mix Color: X is held, so picked colors mix into the current one.
    mix_key: bool,
    /// MultiColored brush (⌥M): the colors loaded side by side, and whether it's on.
    multicolor: Vec<Color>,
    multicolor_on: bool,
    /// The macOS menu bar (None until installed on the first frame, or if it failed).
    #[cfg(target_os = "macos")]
    native_menu: Option<menus::native::NativeMenu>,
    /// Tried to install the native menu bar already.
    #[cfg(target_os = "macos")]
    menu_installed: bool,
    /// Stroke and pressure smoothing for the stroke being painted.
    stabilizer: crate::smoothing::Stabilizer,
    /// The Transform tool (⌘T), while it's active.
    transform: Option<transform::TransformTool>,
    /// How long our code took to build the last frames (ms, smoothed), shown
    /// in the status bar so slowdowns are easy to spot.
    frame_ms: f32,
    /// The last frame's CPU time, unsmoothed (for the benchmark).
    frame_ms_raw: f32,
    /// `OXIDE_BENCH=1`: the painting benchmark (see `app/bench.rs`).
    bench: Option<bench::Bench>,
    /// The Hatch tool: its settings and the rails being drawn.
    hatch: hatch::HatchTool,
    /// File → Record Actions: what you do, written down as an oxide-run script.
    recorder: Option<oxide_commands::recorder::Recorder>,
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
    /// File → Import Script as Layer: paint an oxide-run .jsonl onto a new layer.
    ImportScript,
    /// File → Import Brushes: Photoshop, Procreate and GIMP brushes, or images, as brush tips.
    ImportBrushes,
    OpenReference,
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
    RotateLeft,
    RotateRight,
    ResetRotation,
    FlipView,
    Greyscale,
    DuplicateLayer,
    MergeDown,
    MergeVisible,
    MergeVisibleToNew,
    ClearLayer,
    LockLayer,
    LockTransparency,
    TracingLayer,
    Transform,
    NewLayer,
    DeleteLayer,
    MoveLayerUp,
    MoveLayerDown,
    RewetLayer,
    NaturalColors,
    ToggleLeft,
    ToggleRight,
    /// Window → Reset Panel Layout: every section back where it started.
    ResetPanels,
    ToggleReference,
    ToggleBrushCreator,
    TogglePenSettings,
    ShowShortcuts,
    /// Help → Oxide Help (where it was left).
    ShowHelp,
    /// Help → the current tool's page.
    ToolHelp,
    /// Help → the wiki online, in the browser.
    OpenWiki,
    /// Settings… (⌘,).
    Settings,
    MultiColor,
    SwapColors,
    GroupLayer,
    Ungroup,
    Copy,
    Cut,
    Paste,
    SelectAll,
    Deselect,
    InvertSelection,
    /// Select → Feather by n pixels.
    Feather(u8),
    FillSelection,
    ClearSelection,
    /// File → Open Recent → item n.
    OpenRecent(u8),
    /// View → Theme → item n.
    Theme(u8),
    OpenThemesFolder,
    ReloadThemes,
    ClearRecent,
    /// File → Record Actions: start recording, or stop and save the script.
    Record,
}

impl Command {
    /// Commands that can be used without ending a transform (view changes).
    fn keeps_transform(self) -> bool {
        use Command::*;
        matches!(
            self,
            Fit | ActualSize
                | ZoomIn
                | ZoomOut
                | RotateLeft
                | RotateRight
                | ResetRotation
                | FlipView
                | Greyscale
                | MixingPalette
                | NaturalColors
                | MultiColor
                | SwapColors
                | ToggleLeft
                | ToggleRight
                | ResetPanels
                | ToggleReference
                | ToggleBrushCreator
                | TogglePenSettings
                | ShowShortcuts
                | ShowHelp
                | ToolHelp
                | OpenWiki
                | Settings
                | Theme(_)
                | OpenThemesFolder
                | ReloadThemes
                | Record
        )
    }
}

/// Keyboard shortcuts. Order matters: egui ignores *extra* Shift/Alt when
/// matching, so ⇧⌘S must be checked before ⌘S or it would trigger Save.
const SHORTCUTS: [(Command, KeyboardShortcut); 41] = [
    // View: checked before ⌘0 (Fit), since extra modifiers would otherwise match it.
    (Command::ResetRotation, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::Num0)),
    // ⇧⌥⌘G before ⌥⌘G.
    (Command::Ungroup, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT).plus(Modifiers::SHIFT), Key::G)),
    (Command::GroupLayer, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::G)),
    (Command::RotateLeft, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::ArrowLeft)),
    (Command::RotateRight, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::ArrowRight)),
    (Command::FlipView, KeyboardShortcut::new(Modifiers::SHIFT, Key::F)),
    (Command::MixingPalette, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M)),
    (Command::DryAll, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::D)),
    (Command::DryLayer, KeyboardShortcut::new(Modifiers::COMMAND, Key::D)),
    (Command::SaveAs, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S)),
    (Command::SaveIteration, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::S)),
    (Command::Save, KeyboardShortcut::new(Modifiers::COMMAND, Key::S)),
    (Command::Redo, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z)),
    (Command::Undo, KeyboardShortcut::new(Modifiers::COMMAND, Key::Z)),
    (Command::Export, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E)),
    // Layers: ⌥⌘E before ⌘E.
    (Command::MergeVisible, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::E)),
    (Command::MergeDown, KeyboardShortcut::new(Modifiers::COMMAND, Key::E)),
    (Command::DuplicateLayer, KeyboardShortcut::new(Modifiers::COMMAND, Key::J)),
    (Command::Transform, KeyboardShortcut::new(Modifiers::COMMAND, Key::T)),
    (Command::MultiColor, KeyboardShortcut::new(Modifiers::ALT, Key::M)),
    (Command::SwapColors, KeyboardShortcut::new(Modifiers::COMMAND, Key::Backslash)),
    (Command::ClearLayer, KeyboardShortcut::new(Modifiers::COMMAND, Key::Backspace)),
    (Command::LockLayer, KeyboardShortcut::new(Modifiers::COMMAND, Key::Slash)),
    (Command::TracingLayer, KeyboardShortcut::new(Modifiers::ALT.plus(Modifiers::SHIFT), Key::T)),
    (Command::New, KeyboardShortcut::new(Modifiers::COMMAND, Key::N)),
    (Command::Open, KeyboardShortcut::new(Modifiers::COMMAND, Key::O)),
    (Command::ImportImage, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::I)),
    // Selections (⇧⌘A before ⌘A; ⇧⌘I import above ⌘I).
    (Command::Deselect, KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::A)),
    (Command::Copy, KeyboardShortcut::new(Modifiers::COMMAND, Key::C)),
    (Command::Cut, KeyboardShortcut::new(Modifiers::COMMAND, Key::X)),
    (Command::Paste, KeyboardShortcut::new(Modifiers::COMMAND, Key::V)),
    (Command::SelectAll, KeyboardShortcut::new(Modifiers::COMMAND, Key::A)),
    (Command::InvertSelection, KeyboardShortcut::new(Modifiers::COMMAND, Key::I)),
    (Command::FillSelection, KeyboardShortcut::new(Modifiers::ALT, Key::Backspace)),
    (Command::Quit, KeyboardShortcut::new(Modifiers::COMMAND, Key::Q)),
    (Command::Settings, KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma)),
    (Command::Fit, KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0)),
    (Command::ActualSize, KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1)),
    (Command::ZoomIn, KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals)),
    (Command::ZoomIn, KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus)),
    (Command::ZoomOut, KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus)),
];

impl OxideApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        // A pen tip wobbles a little when you tap. egui only counts a press as a
        // click if it moves less than 6 points, so pen taps on buttons were often
        // seen as tiny drags and ignored. Be more forgiving.
        ctx.options_mut(|o| o.input_options.max_click_dist = 16.0);
        let app_settings = AppSettings::load();
        let (themes, theme_problem) = theming::ThemeState::start(ctx, &app_settings.theme);
        crate::open_files::set_wake(ctx);
        // Brush tips imported earlier (Photoshop, GIMP, images).
        oxide_commands::tip_import::load_library();
        let new_artwork = new_artwork::NewArtwork::from_settings(&app_settings);
        let reference_open = app_settings.reference_open;
        // Each tool's starting values (Settings → Tools), else the built-in ones.
        let starts = Tool::ALL.map(|tool| app_settings.tool_start(tool));
        // Behind the New Artwork dialog: the last-used (or default) canvas,
        // so there's something to paint on if you cancel or turn the dialog off.
        let mut document = new_artwork.spec().create();
        document.take_dirty();
        let texture = TiledTexture::new(ctx, &document);
        let c = Color::BLACK;
        OxideApp {
            saved_state: document.state_id(),
            saved_reference: None,
            document,
            hsv: hsv_from_rgb([c.r, c.g, c.b]),
            color: c,
            tool: Tool::OilsAcrylics,
            tool_states: starts.clone().map(|(state, _)| state),
            recent_colors: Vec::new(),
            stroke: None,
            stroke_preview: None,
            dirty_brush: true,
            carried_color: None,
            view: View { fit: true, zoom: 1.0, pan: egui::Vec2::ZERO, last_scale: 1.0, rotation: 0.0, flip: false },
            texture,
            presets: PresetLibrary::load(ctx),
            brush_creator: brush_creator::BrushCreator::default(),
            last_workspace: egui::Rect::NOTHING,
            last_hover: None,
            cursor_dir: None,
            show_left: true,
            show_right: true,
            popovers: app_settings.open_popovers.iter().map(panels::Popover::from_saved).collect(),
            quick_editing: false,
            renaming: None,
            renaming_row: None,
            pick_return: None,
            file_path: None,
            pending: None,
            new_group: None,
            allow_close: false,
            status: theme_problem.map(|text| Status { text, is_error: true }),
            themes,
            title: String::new(),
            app_settings,
            new_artwork,
            select_canvas: select_canvas::SelectCanvas::default(),
            export_dialog: export_dialog::ExportDialog::default(),
            mixing_palette: mixing_palette::MixingPalette::default(),
            frame_ms: 0.0,
            frame_ms_raw: 0.0,
            bench: bench::Bench::requested().then(Default::default),
            pen: PenInput::new(cc),
            mouse_pressure: 1.0,
            mode_key: None,
            last_paint_mode: PaintMode::PaintMix,
            adjust_drag: None,
            show_pen_settings: false,
            settings: Default::default(),
            transform: None,
            stabilizer: Default::default(),
            show_shortcuts: false,
            help: Default::default(),
            brush_search: String::new(),
            renaming_group: None,
            select_kind: Default::default(),
            select_drag: None,
            selection_outline: Default::default(),
            feather_radius: 8.0,
            clip: None,
            multicolor: Vec::new(),
            multicolor_on: false,
            secondary: Color::WHITE,
            brush_names: starts.map(|(_, brush)| brush),
            mix_key: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            #[cfg(target_os = "macos")]
            menu_installed: false,
            reference: reference_image::ReferenceImage::new(reference_open),
            hatch: Default::default(),
            recorder: None,
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
        if self.multicolor_active() {
            brush.colors = self.multicolor.clone();
        }
        brush
    }

    /// The Color panel's MultiColored checkbox (same as ⌥M).
    fn run_command_multicolor(&mut self) {
        self.multicolor_on = !self.multicolor_on;
        if self.multicolor_on && self.multicolor.is_empty() {
            self.multicolor.push(self.color);
        }
    }

    /// ⌘C / ⌘X: the selected paint (or the whole layer) to Oxide's clipboard
    /// and, as an image, to the Mac's (so other apps can paste it).
    fn copy(&mut self, cut: bool) {
        let clip = if cut { self.document.cut_selected() } else { self.document.copy_selected() };
        let Some(clip) = clip else {
            return self.info(if cut {
                "Nothing to cut here (the selected part is empty, or the layer can't be erased: locked or lock transparency)."
            } else {
                "Nothing to copy here (the selected part of this layer is empty)."
            });
        };
        let rgba = clip.rgba8();
        let fingerprint = fingerprint(&rgba);
        let image = arboard::ImageData { width: clip.w, height: clip.h, bytes: std::borrow::Cow::Owned(rgba) };
        let verb = if cut { "Cut" } else { "Copied" };
        match arboard::Clipboard::new().and_then(|mut c| c.set_image(image)) {
            Ok(()) => self.info(format!("{verb} {} × {} px", clip.w, clip.h)),
            Err(e) => self.error(format!("{verb} inside Oxide, but not to the Mac clipboard: {e}")),
        }
        self.clip = Some((clip, fingerprint));
        self.record(if cut { oxide_commands::Command::Cut(Default::default()) } else { oxide_commands::Command::Copy(Default::default()) });
    }

    /// ⌘V: paste on a new layer, then Transform to place it. Our own copy
    /// keeps its wetness and thickness; an image from another app comes in centered.
    fn paste(&mut self) {
        let system = arboard::Clipboard::new().and_then(|mut c| c.get_image()).ok();
        // Our own copy, if the Mac clipboard still holds it (or holds no image).
        let ours = match (&system, &self.clip) {
            (Some(img), Some((clip, fp))) if fingerprint(&img.bytes) == *fp => Some(clip.clone()),
            (None, Some((clip, _))) => Some(clip.clone()),
            _ => None,
        };
        if ours.is_none() && system.is_none() {
            return self.info("Nothing to paste (copy something, or an image in another app).");
        }
        // Deselect first, so the transform frames the pasted paint and undo
        // goes Transform → Paste → Deselect.
        self.document.deselect();
        match (ours, system) {
            (Some(clip), _) => {
                self.document.paste(&clip);
                let deselect = oxide_commands::command::SelectionArgs { action: "none".into(), radius: None };
                self.record(oxide_commands::Command::Selection(deselect));
                self.record(oxide_commands::Command::Paste(Default::default()));
            }
            (None, Some(img)) => {
                if !self.document.paste_image(img.width, img.height, &img.bytes) {
                    return self.error("The clipboard image couldn't be read.");
                }
                self.with_recorder(|r, _, doc| {
                    r.comment("(an image from another app was pasted here; not recorded)");
                    r.snapshot(doc);
                });
            }
            (None, None) => unreachable!("checked above"),
        }
        self.start_transform();
    }

    /// Merge Down, warning when the layer's blend mode can't survive it.
    fn merge_down(&mut self) {
        let changes_look = self.document.merge_down_changes_look();
        let mode = self.document.layers()[self.document.active_index()].blend;
        if !self.document.merge_down() {
            return self.info("Nothing below to merge into");
        }
        if changes_look {
            self.info(format!("Merged. Where the layer below was empty, the {} paint now covers instead of blending (⌘Z to undo).", mode.label()));
        }
    }

    /// Is the MultiColored brush on, with at least two colors loaded?
    fn multicolor_active(&self) -> bool {
        self.multicolor_on && self.multicolor.len() >= 2 && self.tool.paints()
    }

    /// A color picked from a swatch, the canvas, the palette or the reference
    /// image. Holding X (Mix Color) mixes it into the current color like
    /// paint instead of replacing it.
    fn pick_color(&mut self, picked: Color) {
        if self.mix_key {
            let mixing = if self.app_settings.natural_colors { Mixing::Pigment } else { Mixing::Rgb };
            let mixed = oxide_engine::mix_paint(Color { a: 1.0, ..self.color }, Color { a: 1.0, ..picked }, 0.5, mixing);
            self.set_color(mixed);
        } else {
            self.set_color(picked);
        }
    }

    /// Swap the primary and secondary colors (⌘\\).
    fn swap_colors(&mut self) {
        let primary = self.color;
        self.set_color(self.secondary);
        self.secondary = primary;
    }

    /// Load another color onto the MultiColored brush (up to 6).
    fn add_multicolor(&mut self, color: Color) {
        let color = Color { a: 1.0, ..color };
        if self.multicolor.len() >= 6 {
            return self.info("The MultiColored brush holds up to 6 colors.");
        }
        self.multicolor.push(color);
        self.multicolor_on = true;
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
        // Typing in a text field: ⌘A/C/X/V are for the text, not the canvas.
        if ctx.egui_wants_keyboard_input() && matches!(command, Command::Copy | Command::Cut | Command::Paste | Command::SelectAll) {
            // Without the Mac menu bar, egui already sent its own Copy/Cut/Paste
            // event with the key press; adding another would paste twice.
            let already = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!((command, e), (Command::Copy, egui::Event::Copy) | (Command::Cut, egui::Event::Cut) | (Command::Paste, egui::Event::Paste(_)))
                })
            });
            if !already {
                let event = match command {
                    Command::Copy => egui::Event::Copy,
                    Command::Cut => egui::Event::Cut,
                    Command::Paste => egui::Event::Paste(arboard::Clipboard::new().and_then(|mut c| c.get_text()).unwrap_or_default()),
                    _ => egui::Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
                };
                ctx.input_mut(|i| i.events.push(event));
            }
            return;
        }
        // While transforming: ⌘T again or most commands apply it first; undo cancels it.
        if self.transform.is_some() && !command.keeps_transform() {
            match command {
                Command::Undo | Command::Redo => self.cancel_transform(),
                _ => self.apply_transform(),
            }
            if matches!(command, Command::Undo | Command::Redo | Command::Transform) {
                return;
            }
        }
        let state_before = self.document.state_id();
        match command {
            Command::Record => self.toggle_recording(),
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
            Command::ImportScript => self.import_script(),
            Command::ImportBrushes => self.import_tips(ctx, false),
            Command::OpenReference => self.open_reference_image(),
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
            Command::RotateLeft => self.change_view(-15.0, false, false),
            Command::RotateRight => self.change_view(15.0, false, false),
            Command::ResetRotation => self.change_view(0.0, false, true),
            Command::FlipView => self.change_view(0.0, true, false),
            Command::Greyscale => {
                let on = !self.texture.greyscale();
                self.texture.set_greyscale(&self.document, on);
            }
            Command::Transform => self.start_transform(),
            Command::NewLayer => self.document.add_layer(),
            Command::DeleteLayer => {
                if self.document.layers().len() > 1 {
                    self.document.delete_active_layer();
                }
            }
            Command::MoveLayerUp => self.document.move_active_layer(1),
            Command::MoveLayerDown => self.document.move_active_layer(-1),
            Command::RewetLayer => self.document.rewet_layer(),
            Command::NaturalColors => {
                self.app_settings.natural_colors = !self.app_settings.natural_colors;
                self.save_app_settings();
                let mode = if self.app_settings.natural_colors { "paint (pigment)" } else { "light (RGB)" };
                self.info(format!("Colors now mix like {mode}"));
            }
            Command::ToggleLeft => self.show_left = !self.show_left,
            Command::ToggleRight => self.show_right = !self.show_right,
            Command::ResetPanels => {
                self.app_settings.panels = Default::default();
                self.popovers.clear();
                self.save_app_settings();
            }
            Command::ToggleReference => {
                self.reference.open = !self.reference.open;
                self.app_settings.reference_open = self.reference.open;
                self.save_app_settings();
            }
            Command::ToggleBrushCreator => self.brush_creator.open = !self.brush_creator.open,
            Command::TogglePenSettings => self.show_pen_settings = !self.show_pen_settings,
            Command::ShowShortcuts => self.show_shortcuts = true,
            Command::ShowHelp => self.help.open = true,
            Command::ToolHelp => self.help.show_page(&help::tool_page(self.tool)),
            Command::OpenWiki => ctx.open_url(egui::OpenUrl::new_tab(help::ONLINE)),
            Command::Settings => self.show_settings(),
            Command::SwapColors => self.swap_colors(),
            Command::Copy => self.copy(false),
            Command::Cut => self.copy(true),
            Command::Paste => self.paste(),
            Command::SelectAll => self.document.select_all(),
            Command::Deselect => self.document.deselect(),
            Command::InvertSelection => self.document.invert_selection(),
            Command::Feather(r) => self.document.feather_selection(r as f32),
            Command::FillSelection => {
                if !self.document.fill_selection(self.color) {
                    self.info("Select an area first (Select tool, L), on a layer you can paint on.");
                }
            }
            Command::ClearSelection => {
                if self.document.selection().is_some() && !self.document.clear_selected() {
                    self.info("This layer can't be erased (locked, hidden, tracing, or lock transparency is on).");
                }
            }
            Command::GroupLayer => {
                if self.document.group_active_layer().is_none() {
                    self.info("This layer is already in a group.");
                }
            }
            Command::Ungroup => {
                if let Some(id) = self.document.layers()[self.document.active_index()].group {
                    self.document.ungroup(id);
                }
            }
            Command::OpenRecent(n) => {
                if let Some(path) = self.app_settings.recent_files.get(n as usize).cloned() {
                    self.request(PendingAction::OpenPath(path), ctx);
                }
            }
            Command::Theme(n) => self.choose_theme(n as usize, ctx),
            Command::OpenThemesFolder => self.open_themes_folder(),
            Command::ReloadThemes => self.reload_theme(ctx),
            Command::ClearRecent => {
                self.app_settings.recent_files.clear();
                self.save_app_settings();
            }
            Command::MultiColor => {
                self.run_command_multicolor();
                if self.multicolor_on && self.multicolor.len() < 2 {
                    self.info("MultiColored brush: add colors with \"Add current color\" or ⌥-click swatches (Color panel).");
                }
            }
            Command::DuplicateLayer => self.document.duplicate_active_layer(),
            Command::MergeDown => self.merge_down(),
            Command::MergeVisible => {
                if !self.document.merge_visible() {
                    self.info("Need at least two visible layers to merge");
                }
            }
            Command::MergeVisibleToNew => self.document.merge_visible_to_new(),
            Command::ClearLayer => {
                self.document.clear_active_layer();
            }
            Command::LockLayer => {
                let i = self.document.active_index();
                let on = !self.document.layers()[i].locked;
                self.document.set_locked(i, on);
            }
            Command::LockTransparency => {
                let i = self.document.active_index();
                let on = !self.document.layers()[i].lock_transparency();
                self.document.set_lock_transparency(i, on);
            }
            Command::TracingLayer => {
                let i = self.document.active_index();
                let on = !self.document.layers()[i].tracing;
                self.document.set_tracing(i, on);
            }
        }
        self.record_command(command, state_before);
    }

    /// Install (first frame) and run the macOS menu bar. Returns false if
    /// there isn't one, so the in-window menu bar is shown instead.
    #[cfg(target_os = "macos")]
    fn native_menu_frame(&mut self, ctx: &egui::Context) -> bool {
        if !self.menu_installed {
            self.menu_installed = true;
            self.native_menu = menus::native::NativeMenu::install(&self.menu_spec(), ctx);
        }
        let Some(menu) = &self.native_menu else { return false };
        let chosen = menu.chosen();
        // Ignore menu picks while a dialog is up (except Quit), like the in-window menus.
        let dialog_open = self.pending.is_some() || self.new_artwork.open || self.select_canvas.open || self.export_dialog.open;
        for command in chosen {
            if !dialog_open || command == Command::Quit {
                self.run(command, ctx);
            }
        }
        // (Checked again: a command above may have opened a dialog.)
        let mut spec = self.menu_spec();
        if self.pending.is_some() || self.new_artwork.open || self.select_canvas.open || self.export_dialog.open {
            menus::disable_all(&mut spec);
        }
        if let Some(menu) = &mut self.native_menu {
            menu.sync(&spec);
        }
        true
    }

    #[cfg(not(target_os = "macos"))]
    fn native_menu_frame(&mut self, _ctx: &egui::Context) -> bool {
        false
    }

    fn menu_handles_shortcut(&self, command: Command) -> bool {
        #[cfg(target_os = "macos")]
        return self.native_menu.as_ref().is_some_and(|m| m.handles_shortcut(command));
        #[cfg(not(target_os = "macos"))]
        return {
            let _ = command;
            false
        };
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Collect first, run after: running a command (e.g. Quit) talks to the
        // egui context, which is locked while we're inside `input_mut`.
        let mut commands = Vec::new();
        ctx.input_mut(|i| {
            for (command, shortcut) in &SHORTCUTS {
                // Keys the macOS menu bar handles go to it, not here.
                if self.menu_handles_shortcut(*command) {
                    continue;
                }
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
        let mut view_keys = Vec::new();
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
            // View keys: G greyscale, 0 = 100%, . = fit to screen.
            if i.modifiers.is_none() {
                if i.key_pressed(Key::G) {
                    view_keys.push(Command::Greyscale);
                }
                if i.key_pressed(Key::Num0) {
                    view_keys.push(Command::ActualSize);
                }
                if i.key_pressed(Key::Period) {
                    view_keys.push(Command::Fit);
                }
                if i.key_pressed(Key::Slash) {
                    view_keys.push(Command::LockTransparency);
                }
                // ⌫ / Delete erases what's selected (with a selection).
                if (i.key_pressed(Key::Backspace) || i.key_pressed(Key::Delete)) && self.document.selection().is_some() {
                    view_keys.push(Command::ClearSelection);
                }
            }
            if self.tool == Tool::OilsAcrylics && i.modifiers.is_none() {
                // Paint modes 1–5.
                let modes = [
                    (Key::Num1, Some(PaintMode::Paint)),
                    (Key::Num2, Some(PaintMode::PaintMix)),
                    (Key::Num3, Some(PaintMode::PaintBlend)),
                    (Key::Num4, Some(PaintMode::Blend)),
                    (Key::Num5, None),
                ];
                for (key, mode) in modes {
                    if i.key_pressed(key) {
                        self.set_paint_mode(mode);
                    }
                }
                // V (Blend) and A (Erase): tap to switch, hold to use only while held.
                for key in [Key::V, Key::A] {
                    if i.key_pressed(key) && self.mode_key.is_none() {
                        let before = *self.state();
                        self.toggle_mode_key(key);
                        self.mode_key = Some((key, before, std::time::Instant::now()));
                    }
                }
            }
            if let Some((key, before, pressed)) = self.mode_key
                && i.key_released(key)
            {
                if pressed.elapsed().as_secs_f32() > 0.35 {
                    // Held: go back to how it was.
                    *self.state_mut() = ToolState { shape: self.state().shape, ..before };
                }
                self.mode_key = None;
            }
            if i.key_pressed(Key::CloseBracket) {
                let shape = &mut self.state_mut().shape;
                shape.radius = (shape.radius * 1.15).min(200.0);
            }
        });
        for command in view_keys {
            self.run(command, ctx);
        }
    }

    /// Pick a paint mode for the wet medium (`None` = Erase).
    fn set_paint_mode(&mut self, mode: Option<PaintMode>) {
        let state = self.state_mut();
        match mode {
            Some(mode) => {
                state.paint_mode = mode;
                state.erase = false;
                if mode != PaintMode::Blend {
                    self.last_paint_mode = mode;
                }
            }
            None => state.erase = true,
        }
    }

    /// V: paint mode ⇄ Blend. A: paint mode ⇄ Erase.
    fn toggle_mode_key(&mut self, key: Key) {
        let state = *self.state();
        if key == Key::V {
            if state.paint_mode == PaintMode::Blend && !state.erase {
                self.set_paint_mode(Some(self.last_paint_mode));
            } else {
                self.set_paint_mode(Some(PaintMode::Blend));
            }
        } else if state.erase {
            self.state_mut().erase = false;
        } else {
            self.set_paint_mode(None);
        }
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
            ui.separator();
            ui.strong("Smoothing");
            let s = &mut self.app_settings;
            let mut stroke = s.stroke_smoothing * 100.0;
            if ui
                .add(egui::Slider::new(&mut stroke, 0.0..=100.0).max_decimals(0).text("Stroke"))
                .on_hover_text("Steadier lines: the brush trails the pen on a string and ignores small wobbles. Higher = steadier, with more lag.")
                .changed()
            {
                s.stroke_smoothing = stroke / 100.0;
                changed = true;
            }
            let mut pressure = s.pressure_smoothing * 100.0;
            if ui
                .add(egui::Slider::new(&mut pressure, 0.0..=100.0).max_decimals(0).text("Pressure"))
                .on_hover_text("Steadier pressure: smooths sudden jumps in size and opacity")
                .changed()
            {
                s.pressure_smoothing = pressure / 100.0;
                changed = true;
            }
            ui.add_space(6.0);
            ui.separator();
            ui.strong("Tilt & rotation");
            changed |= ui.checkbox(&mut s.invert_tilt, "Invert tilt").on_hover_text("If flat brushes lean the wrong way").changed();
            changed |= ui
                .add(egui::Slider::new(&mut s.tilt_offset, -180.0..=180.0).suffix("°").max_decimals(0).text("Tilt offset"))
                .on_hover_text("Turn the tilt direction (e.g. if the tablet is rotated)")
                .changed();
            changed |= ui.checkbox(&mut s.invert_rotation, "Invert pen rotation").changed();
            let tilt = crate::pen::adjust_tilt(pen.tilt, s.invert_tilt, s.tilt_offset);
            let rotation = crate::pen::adjust_rotation(pen.rotation, s.invert_rotation);
            ui.horizontal(|ui| {
                // A little compass: the line points the way the pen leans.
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(44.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let c = rect.center();
                painter.circle_stroke(c, 20.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
                painter.line_segment([c, c + egui::Vec2::new(tilt.0, tilt.1) * 20.0], egui::Stroke::new(2.5, ui.visuals().selection.bg_fill));
                ui.label(format!("Tilt {:+.2}, {:+.2}\nRotation {:.0}°\nLean the pen: the line should point the same way.", tilt.0, tilt.1, rotation));
            });
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
            match self.recorder.as_ref().map(|r| r.strokes()) {
                Some(strokes) => {
                    let stop = egui::Button::new(egui::RichText::new("⏹ Stop").color(ui.visuals().error_fg_color));
                    if ui.add(stop).on_hover_text("Stop recording and save the script (.jsonl)").clicked() {
                        self.run(Command::Record, ui.ctx());
                    }
                    ui.colored_label(ui.visuals().error_fg_color, format!("⏺ REC · {strokes} strokes"));
                }
                None => {
                    if ui.button("⏺ Record").on_hover_text("Write down what you paint and do as an oxide-run script (.jsonl), to replay or build on").clicked()
                    {
                        self.run(Command::Record, ui.ctx());
                    }
                }
            }
            ui.separator();
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
            if self.view.rotation != 0.0 || self.view.flip || self.texture.greyscale() {
                ui.separator();
                let mut parts = Vec::new();
                if self.view.rotation != 0.0 {
                    parts.push(format!("rotated {:.0}°", self.view.rotation));
                }
                if self.view.flip {
                    parts.push("flipped".to_string());
                }
                if self.texture.greyscale() {
                    parts.push("greyscale".to_string());
                }
                ui.colored_label(self.themes.theme.colors.info, format!("View: {}", parts.join(", ")))
                    .on_hover_text("These change only how you see the painting, not the painting itself");
            }
            ui.separator();
            let frame = format!("{:.0} ms/frame", self.frame_ms);
            if self.frame_ms > 30.0 {
                ui.colored_label(self.themes.theme.colors.warning, frame)
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
        self.pen.pump();
        self.handle_close_request(&ctx);
        self.watch_theme_file(&ctx);
        self.bench_tick(&ctx);
        if self.pending.is_none() && !self.new_artwork.open && !self.select_canvas.open && !self.export_dialog.open {
            self.handle_shortcuts(&ctx);
        }
        // Files dropped on the window, or sent by Finder (double-click, Open With).
        let mut arrived: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        arrived.extend(crate::open_files::take());
        if !arrived.is_empty() && self.pending.is_none() {
            self.open_files(arrived, &ctx);
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.content_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop-hint")));
            painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(120));
            painter.text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Drop to open a painting, add an image as a layer, or import brush tips",
                egui::FontId::proportional(20.0),
                egui::Color32::WHITE,
            );
        }
        // Mix Color: while X is held (and you're not typing), picked colors mix in.
        self.mix_key = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::X) && i.modifiers.is_none());

        self.alt_pick(&ctx);

        if !self.native_menu_frame(&ctx) {
            egui::Panel::top("menu_bar").show(ui, |ui| self.menu_bar(ui));
        }
        egui::Panel::bottom("status_bar").show(ui, |ui| self.status_bar(ui));

        // The side panels are icon rails; each section opens as a popover.
        if self.show_left {
            egui::Panel::left("left_rail").exact_size(44.0).resizable(false).show(ui, |ui| self.rail(ui, Side::Left));
        }
        if self.show_right {
            egui::Panel::right("right_rail").exact_size(44.0).resizable(false).show(ui, |ui| self.rail(ui, Side::Right));
        }
        self.popover_windows(&ctx);

        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.themes.theme.colors.workspace)).show(ui, |ui| self.canvas_view(ui));

        self.brush_creator_window(&ctx);
        self.new_group_dialog(&ctx);
        self.pen_settings_window(&ctx);
        self.settings_window(&ctx);
        self.mixing_palette_window(&ctx);
        self.transform_window(&ctx);
        self.reference_window(&ctx);
        self.shortcuts_window(&ctx);
        self.help_window(&ctx);
        self.new_artwork_dialog(&ctx);
        // Drawn after New Artwork so it sits on top when opened from there.
        self.select_canvas_dialog(&ctx);
        self.export_dialog(&ctx);
        self.pending_dialog(&ctx);

        // Slider drags (opacity, paper color) become one undo step: close the
        // step once the mouse button is up.
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if !pointer_down {
            self.document.seal_history();
        }
        self.recording_frame(pointer_down);
        // Undo/redo/opening change pixels outside of painting: show them.
        self.upload_changes();
        self.update_title(&ctx);

        let ms = started.elapsed().as_secs_f32() * 1000.0;
        self.frame_ms_raw = ms;
        self.frame_ms = if self.frame_ms == 0.0 { ms } else { self.frame_ms * 0.9 + ms * 0.1 };
    }
}

impl OxideApp {
    /// Holding ⌥ over the canvas turns any tool (but Select) into the Picker until you let go.
    fn alt_pick(&mut self, ctx: &egui::Context) {
        let (alt, busy, hover) = ctx.input(|i| {
            let alt = i.modifiers.alt && !i.modifiers.command && !i.modifiers.shift && !i.modifiers.ctrl;
            (alt, i.pointer.any_down(), i.pointer.hover_pos())
        });
        if busy {
            return; // never switch tools mid-drag
        }
        let over_canvas = hover.is_some_and(|p| self.last_workspace.contains(p));
        if alt && over_canvas && !ctx.egui_wants_keyboard_input() {
            // (Not the Select tool: there ⌥ subtracts from the selection.)
            if !matches!(self.tool, Tool::Picker | Tool::Select) && self.stroke.is_none() && self.transform.is_none() && self.pick_return.is_none() {
                self.pick_return = Some((self.tool, true));
                self.tool = Tool::Picker;
            }
        } else if let Some((tool, true)) = self.pick_return {
            self.pick_return = None;
            if self.tool == Tool::Picker {
                self.tool = tool;
            }
        }
    }

    /// The Pick button: pick one color from the canvas, then go back to the tool.
    pub(super) fn pick_once(&mut self) {
        if self.tool != Tool::Picker {
            self.pick_return = Some((self.tool, false));
            self.tool = Tool::Picker;
        }
    }
}

pub fn to_color32(c: Color) -> egui::Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgb(byte(c.r), byte(c.g), byte(c.b))
}

/// A quick fingerprint of clipboard image bytes (to recognize our own copy).
fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a over a sample of the bytes (fast even for big images).
    let step = (bytes.len() / 65_536).max(1);
    bytes.iter().step_by(step).fold(0xcbf2_9ce4_8422_2325u64 ^ bytes.len() as u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}
