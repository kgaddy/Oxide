//! App settings that last between runs, saved to
//! `~/Library/Application Support/Oxide/settings.toml`: recent files,
//! favorite and custom canvas sizes, and the last New Artwork choices.

use std::fs;
use std::path::PathBuf;

use oxide_commands::presets::BrushPreset;
use oxide_engine::{PaintMode, PaperSettings};
use serde::{Deserialize, Serialize};

use crate::quick_panel::QuickItem;
use crate::tools::{Tool, ToolState};

const MAX_RECENT: usize = 10;

/// Units for canvas sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    Pixel,
    Cm,
    Inch,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::Pixel, Unit::Cm, Unit::Inch];

    pub fn label(self) -> &'static str {
        match self {
            Unit::Pixel => "pixel",
            Unit::Cm => "cm",
            Unit::Inch => "inch",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Unit::Pixel => "px",
            Unit::Cm => "cm",
            Unit::Inch => "inches",
        }
    }

    /// Convert a length in this unit to pixels at `dpi`.
    pub fn to_px(self, value: f64, dpi: f64) -> f64 {
        match self {
            Unit::Pixel => value,
            Unit::Cm => value / 2.54 * dpi,
            Unit::Inch => value * dpi,
        }
    }

    /// Convert pixels at `dpi` to this unit.
    pub fn px_to(self, px: f64, dpi: f64) -> f64 {
        match self {
            Unit::Pixel => px,
            Unit::Cm => px / dpi * 2.54,
            Unit::Inch => px / dpi,
        }
    }
}

/// A named canvas size like "A4, 21.00 x 29.70 cm, 200 DPI".
/// Stored in portrait (width <= height); the orientation buttons swap it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SizePreset {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub unit: Unit,
    pub dpi: f64,
}

impl SizePreset {
    pub fn new(name: &str, width: f64, height: f64, unit: Unit, dpi: f64) -> Self {
        SizePreset { name: name.to_string(), width, height, unit, dpi }
    }

    /// The line shown in the list.
    pub fn label(&self) -> String {
        match self.unit {
            Unit::Pixel => format!("{}, {} x {} px", self.name, self.width, self.height),
            unit => format!("{}, {:.2} x {:.2} {}, {} DPI", self.name, self.width, self.height, unit.short(), self.dpi),
        }
    }

    pub fn pixels(&self) -> (f64, f64) {
        (self.unit.to_px(self.width, self.dpi), self.unit.to_px(self.height, self.dpi))
    }
}

/// What was chosen in New Artwork last time, to start from next time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewArtworkChoice {
    pub width_px: f64,
    pub height_px: f64,
    pub dpi: f64,
    pub unit: Unit,
    pub surface: PaperSettings,
    pub paper_color: [f32; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub show_new_artwork_on_startup: bool,
    /// Most recent first.
    pub recent_files: Vec<PathBuf>,
    /// Names of starred size presets.
    pub favorite_sizes: Vec<String>,
    /// Sizes you added yourself (shown under Favorites).
    pub custom_sizes: Vec<SizePreset>,
    pub last_new_artwork: Option<NewArtworkChoice>,
    /// Paper colors you added in Select Canvas.
    pub custom_paper_colors: Vec<[f32; 3]>,
    /// Pen pressure curve: below 1 = soft (light touches count for more),
    /// above 1 = firm.
    pub pressure_curve: f32,
    /// Last-used export format and options.
    pub export: crate::export::ExportOptions,
    /// The last folder used by each kind of file dialog, kept separate so
    /// e.g. importing a palette doesn't change where exports are saved.
    pub last_folders: std::collections::BTreeMap<String, PathBuf>,
    /// Mix colors like real paint (blue + yellow = green) instead of like light.
    pub natural_colors: bool,
    /// Whether the Reference Image window is open (the photo itself is saved
    /// in each painting).
    pub reference_open: bool,
    /// Stroke smoothing 0..=1 (the stabilizer's string length).
    pub stroke_smoothing: f32,
    /// Pressure smoothing 0..=1.
    pub pressure_smoothing: f32,
    /// Flip the pen's tilt direction, and turn it by an offset (degrees).
    pub invert_tilt: bool,
    pub tilt_offset: f32,
    /// Flip the direction of barrel rotation.
    pub invert_rotation: bool,
    /// Show the color wheel as a painter's RYB wheel instead of RGB.
    pub ryb_wheel: bool,
    /// The Picker averages a square this many pixels wide (1 = one pixel).
    pub picker_sample: usize,
    /// The UI theme's name (View → Theme).
    pub theme: String,
    /// Which sections are in each side panel, in order (drag their titles).
    pub panels: crate::panel_layout::PanelLayout,
    /// What tools start with when Oxide opens (Settings → Tools), by tool
    /// name. Tools not listed start with their built-in values.
    pub tool_starts: std::collections::BTreeMap<String, ToolStart>,
    /// What the Quick section shows for each tool, by tool name. Tools not
    /// listed show their default items.
    pub quick_items: std::collections::BTreeMap<String, Vec<QuickItem>>,
}

/// A tool's starting values: its brush and medium settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolStart {
    /// The brush preset it starts with (shown as "Brush …" in Properties).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush: Option<String>,
    pub opacity: f32,
    pub loading: f32,
    pub oiliness: f32,
    pub length: f32,
    pub thickness: f32,
    pub paint_mode: PaintMode,
    #[serde(default)]
    pub scrape: bool,
    /// The brush shape (size, hardness, taper, pressure response, …).
    pub shape: BrushPreset,
}

impl ToolStart {
    pub fn new(state: &ToolState, brush: Option<String>) -> Self {
        ToolStart {
            brush,
            opacity: state.opacity,
            loading: state.loading,
            oiliness: state.oiliness,
            length: state.length,
            thickness: state.thickness,
            paint_mode: state.paint_mode,
            scrape: state.scrape,
            shape: BrushPreset::from_shape("", "", &state.shape),
        }
    }

    pub fn state(&self) -> ToolState {
        ToolState {
            shape: self.shape.shape(),
            opacity: self.opacity,
            loading: self.loading,
            oiliness: self.oiliness,
            length: self.length,
            paint_mode: self.paint_mode,
            erase: false,
            thickness: self.thickness,
            scrape: self.scrape,
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            show_new_artwork_on_startup: true,
            recent_files: Vec::new(),
            favorite_sizes: Vec::new(),
            custom_sizes: Vec::new(),
            last_new_artwork: None,
            custom_paper_colors: Vec::new(),
            pressure_curve: 0.6,
            last_folders: std::collections::BTreeMap::new(),
            export: crate::export::ExportOptions::default(),
            natural_colors: true,
            reference_open: false,
            stroke_smoothing: 0.0,
            pressure_smoothing: 0.0,
            invert_tilt: false,
            tilt_offset: 0.0,
            invert_rotation: false,
            ryb_wheel: false,
            picker_sample: 1,
            theme: crate::theme::DEFAULT_THEME.into(),
            panels: crate::panel_layout::PanelLayout::default(),
            tool_starts: std::collections::BTreeMap::new(),
            quick_items: std::collections::BTreeMap::new(),
        }
    }
}

impl AppSettings {
    /// Load saved settings, or defaults if there are none (or they can't be read).
    pub fn load() -> Self {
        let mut settings: AppSettings = settings_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        settings.panels.repair();
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path().ok_or("Couldn't find the Application Support folder.")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| e.to_string())
    }

    /// Put `path` at the top of the recent list.
    pub fn add_recent(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| *p != path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(MAX_RECENT);
    }

    pub fn is_favorite(&self, name: &str) -> bool {
        self.favorite_sizes.iter().any(|n| n == name)
    }

    pub fn toggle_favorite(&mut self, name: &str) {
        if self.is_favorite(name) {
            self.favorite_sizes.retain(|n| n != name);
        } else {
            self.favorite_sizes.push(name.to_string());
        }
    }

    /// What `tool` starts with: your saved values, or the built-in ones.
    pub fn tool_start(&self, tool: Tool) -> (ToolState, Option<String>) {
        match self.tool_starts.get(tool.label()) {
            Some(start) => (start.state(), start.brush.clone()),
            None => (tool.default_state(), None),
        }
    }

    /// The Quick section's items for `tool`, top to bottom.
    pub fn quick_items(&self, tool: Tool) -> Vec<QuickItem> {
        match self.quick_items.get(tool.label()) {
            Some(items) => items.iter().copied().filter(|i| i.applies(tool)).collect(),
            None => QuickItem::defaults(tool),
        }
    }

    /// Set the Quick section's items for `tool`; `None` goes back to the defaults.
    pub fn set_quick_items(&mut self, tool: Tool, items: Option<Vec<QuickItem>>) {
        match items {
            Some(items) => self.quick_items.insert(tool.label().to_string(), items),
            None => self.quick_items.remove(tool.label()),
        };
    }

    /// Save `tool`'s starting values; `None` goes back to the built-in ones.
    pub fn set_tool_start(&mut self, tool: Tool, start: Option<ToolStart>) {
        match start {
            Some(start) => self.tool_starts.insert(tool.label().to_string(), start),
            None => self.tool_starts.remove(tool.label()),
        };
    }
}

/// Kinds of file dialogs, each with its own remembered folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogFolder {
    /// Open / Save / Save As for .oxide paintings.
    Paintings,
    /// Export (PNG, JPEG, TIFF, PSD, …).
    Exports,
    /// Import Image as Layer, Open Reference Image.
    Images,
    /// Import brush tips (.abr, .gbr, .gih, images).
    Tips,
    /// Recorded actions (.jsonl scripts for oxide-run).
    Recordings,
}

impl DialogFolder {
    fn key(self) -> &'static str {
        match self {
            DialogFolder::Paintings => "paintings",
            DialogFolder::Exports => "exports",
            DialogFolder::Images => "images",
            DialogFolder::Tips => "tips",
            DialogFolder::Recordings => "recordings",
        }
    }

    /// Where a dialog of this kind starts the very first time.
    fn default_folder(self) -> Option<PathBuf> {
        match self {
            DialogFolder::Paintings | DialogFolder::Recordings => dirs::document_dir(),
            DialogFolder::Exports | DialogFolder::Images => dirs::picture_dir(),
            DialogFolder::Tips => dirs::download_dir(),
        }
        .or_else(dirs::home_dir)
    }
}

impl AppSettings {
    /// The folder a dialog of this kind should open in: the last one used
    /// (if it still exists), otherwise a sensible default.
    pub fn folder(&self, kind: DialogFolder) -> Option<PathBuf> {
        self.last_folders.get(kind.key()).filter(|p| p.is_dir()).cloned().or_else(|| kind.default_folder())
    }

    /// Remember the folder of a file just chosen in a dialog of this kind.
    pub fn remember_folder(&mut self, kind: DialogFolder, file: &std::path::Path) {
        if let Some(dir) = file.parent() {
            self.last_folders.insert(kind.key().to_string(), dir.to_path_buf());
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Oxide").join("settings.toml"))
}

/// Common print sizes (portrait). Includes standard artist canvas-board sizes.
pub fn print_presets() -> Vec<SizePreset> {
    vec![
        SizePreset::new("A4 Default", 21.0, 29.7, Unit::Cm, 200.0),
        SizePreset::new("A4", 21.0, 29.7, Unit::Cm, 300.0),
        SizePreset::new("A3", 29.7, 42.0, Unit::Cm, 200.0),
        SizePreset::new("A5", 14.8, 21.0, Unit::Cm, 300.0),
        SizePreset::new("Letter", 8.5, 11.0, Unit::Inch, 300.0),
        SizePreset::new("Canvas 8x10", 8.0, 10.0, Unit::Inch, 300.0),
        SizePreset::new("Canvas 9x12", 9.0, 12.0, Unit::Inch, 200.0),
        SizePreset::new("Canvas 11x14", 11.0, 14.0, Unit::Inch, 200.0),
        SizePreset::new("Canvas 12x16", 12.0, 16.0, Unit::Inch, 200.0),
        SizePreset::new("Canvas 16x20", 16.0, 20.0, Unit::Inch, 150.0),
        SizePreset::new("Canvas 18x24", 18.0, 24.0, Unit::Inch, 150.0),
        SizePreset::new("Quarter Sheet", 11.0, 15.0, Unit::Inch, 200.0),
        SizePreset::new("Half Sheet", 15.0, 22.0, Unit::Inch, 150.0),
    ]
}

/// Common screen sizes (portrait).
pub fn screen_presets() -> Vec<SizePreset> {
    vec![
        SizePreset::new("HD", 1080.0, 1920.0, Unit::Pixel, 72.0),
        SizePreset::new("QHD", 1440.0, 2560.0, Unit::Pixel, 72.0),
        SizePreset::new("4K", 2160.0, 3840.0, Unit::Pixel, 72.0),
        SizePreset::new("Square", 2048.0, 2048.0, Unit::Pixel, 72.0),
        SizePreset::new("Instagram Portrait", 1080.0, 1350.0, Unit::Pixel, 72.0),
        SizePreset::new("iPad Pro", 2048.0, 2732.0, Unit::Pixel, 72.0),
        SizePreset::new("Sketch", 1100.0, 1600.0, Unit::Pixel, 72.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_at_200_dpi_is_1654_by_2339_px() {
        let a4 = &print_presets()[0];
        let (w, h) = a4.pixels();
        assert_eq!((w.round(), h.round()), (1654.0, 2339.0));
    }

    #[test]
    fn units_round_trip() {
        for unit in Unit::ALL {
            let px = unit.to_px(12.5, 300.0);
            assert!((unit.px_to(px, 300.0) - 12.5).abs() < 1e-9);
        }
    }

    #[test]
    fn recent_files_are_unique_newest_first_and_capped() {
        let mut s = AppSettings::default();
        for i in 0..15 {
            s.add_recent(PathBuf::from(format!("/tmp/{i}.oxide")));
        }
        s.add_recent(PathBuf::from("/tmp/12.oxide"));
        assert_eq!(s.recent_files.len(), MAX_RECENT);
        assert_eq!(s.recent_files[0], PathBuf::from("/tmp/12.oxide"));
        assert_eq!(s.recent_files.iter().filter(|p| p.ends_with("12.oxide")).count(), 1);
    }

    #[test]
    fn dialog_folders_are_remembered_separately() {
        let mut s = AppSettings::default();
        let tmp = std::env::temp_dir();
        s.remember_folder(DialogFolder::Exports, &tmp.join("painting.png"));
        assert_eq!(s.folder(DialogFolder::Exports), Some(tmp.clone()));
        // Other kinds are untouched (they fall back to their defaults).
        assert_ne!(s.folder(DialogFolder::Paintings), Some(tmp.clone()));
        // A remembered folder that no longer exists falls back to the default.
        s.remember_folder(DialogFolder::Images, std::path::Path::new("/definitely/not/here/x.png"));
        assert_ne!(s.folder(DialogFolder::Images), Some(PathBuf::from("/definitely/not/here")));
    }

    #[test]
    fn settings_round_trip_through_toml() {
        let mut s = AppSettings::default();
        s.toggle_favorite("A3");
        s.custom_sizes.push(SizePreset::new("Mine", 10.0, 20.0, Unit::Cm, 300.0));
        let text = toml::to_string_pretty(&s).unwrap();
        let back: AppSettings = toml::from_str(&text).unwrap();
        assert!(back.is_favorite("A3"));
        assert_eq!(back.custom_sizes, s.custom_sizes);
        // Missing fields fall back to defaults.
        let empty: AppSettings = toml::from_str("").unwrap();
        assert!(empty.show_new_artwork_on_startup);
    }

    #[test]
    fn tool_starts_round_trip_and_reset() {
        let mut s = AppSettings::default();
        assert_eq!(s.tool_start(Tool::Ink), (Tool::Ink.default_state(), None));
        let mut ink = Tool::Ink.default_state();
        ink.shape.radius = 7.5;
        ink.opacity = 0.4;
        s.set_tool_start(Tool::Ink, Some(ToolStart::new(&ink, Some("Fine Liner".into()))));
        let back: AppSettings = toml::from_str(&toml::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back.tool_start(Tool::Ink), (ink, Some("Fine Liner".into())));
        // Other tools are untouched.
        assert_eq!(back.tool_start(Tool::Pencil).0, Tool::Pencil.default_state());
        s.set_tool_start(Tool::Ink, None);
        assert_eq!(s.tool_start(Tool::Ink).0, Tool::Ink.default_state());
    }

    #[test]
    fn quick_items_round_trip_and_reset() {
        let mut s = AppSettings::default();
        assert_eq!(s.quick_items(Tool::Ink), QuickItem::defaults(Tool::Ink));
        // Items that don't fit the tool are left out (Loading means nothing for ink).
        s.set_quick_items(Tool::Ink, Some(vec![QuickItem::Pigments, QuickItem::Size, QuickItem::Loading]));
        let back: AppSettings = toml::from_str(&toml::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back.quick_items(Tool::Ink), [QuickItem::Pigments, QuickItem::Size]);
        s.set_quick_items(Tool::Ink, None);
        assert_eq!(s.quick_items(Tool::Ink), QuickItem::defaults(Tool::Ink));
    }

    #[test]
    fn every_tool_state_survives_a_tool_start() {
        for tool in Tool::ALL {
            let state = tool.default_state();
            assert_eq!(ToolStart::new(&state, None).state(), state, "{}", tool.label());
        }
    }
}
