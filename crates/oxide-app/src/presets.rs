//! Brush presets: the built-in set plus your own, saved as small TOML files
//! in `~/Library/Application Support/Oxide/brushes/`.

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use oxide_engine::{Brush, Color, DabMode, Document, Stroke};
use serde::{Deserialize, Serialize};

use crate::app::PAPER;
use crate::tools::{BrushShape, Tool, ToolState};

pub const THUMB_W: usize = 96;
pub const THUMB_H: usize = 40;

/// A named brush shape. Presets are *shapes only*: any medium (oils, ink,
/// pencil, ...) can use any preset, and the medium decides how paint behaves.
///
/// Older preset files may contain medium settings (opacity, loading,
/// oiliness); serde simply ignores fields it doesn't know.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushPreset {
    pub name: String,
    pub radius: f32,
    pub hardness: f32,
    pub spacing: f32,
    #[serde(default = "one")]
    pub roundness: f32,
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub roughness: f32,
    #[serde(default)]
    pub streaks: f32,
    #[serde(default)]
    pub lift_off: f32,
    /// Paper-texture sensitivity. `None` = keep the tool's own setting
    /// (built-in shapes don't change how a medium feels the paper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<f32>,
}

fn one() -> f32 {
    1.0
}

impl BrushPreset {
    pub fn from_shape(name: &str, shape: &BrushShape) -> Self {
        BrushPreset {
            name: name.to_string(),
            radius: shape.radius,
            hardness: shape.hardness,
            spacing: shape.spacing,
            roundness: shape.roundness,
            angle: shape.angle,
            roughness: shape.roughness,
            streaks: shape.streaks,
            lift_off: shape.lift_off,
            grain: Some(shape.grain),
        }
    }

    /// This preset's shape, keeping `current`'s grain unless the preset has its own.
    pub fn apply_to(&self, current: &BrushShape) -> BrushShape {
        BrushShape { grain: self.grain.unwrap_or(current.grain), ..self.shape() }
    }

    pub fn shape(&self) -> BrushShape {
        BrushShape {
            radius: self.radius,
            hardness: self.hardness,
            spacing: self.spacing,
            roundness: self.roundness,
            angle: self.angle,
            roughness: self.roughness,
            streaks: self.streaks,
            lift_off: self.lift_off,
            grain: self.grain.unwrap_or(0.0),
        }
    }

    pub fn matches(&self, shape: &BrushShape) -> bool {
        self.apply_to(shape) == *shape
    }
}

fn builtin(name: &str, radius: f32, hardness: f32, spacing: f32, roundness: f32, angle: f32) -> BrushPreset {
    BrushPreset { name: name.to_string(), radius, hardness, spacing, roundness, angle, roughness: 0.0, streaks: 0.0, lift_off: 0.0, grain: None }
}

/// A built-in preset with a natural, imperfect tip.
fn natural(preset: BrushPreset, roughness: f32, streaks: f32, lift_off: f32) -> BrushPreset {
    BrushPreset { roughness, streaks, lift_off, ..preset }
}

fn builtin_presets() -> Vec<BrushPreset> {
    vec![
        builtin("Round", 12.0, 0.7, 0.05, 1.0, 0.0),
        builtin("Round Hard", 10.0, 0.95, 0.05, 1.0, 0.0),
        builtin("Round Soft", 18.0, 0.2, 0.06, 1.0, 0.0),
        builtin("Fine", 2.5, 0.95, 0.05, 1.0, 0.0),
        builtin("Flat", 16.0, 0.7, 0.04, 0.35, 0.0),
        builtin("Filbert", 14.0, 0.6, 0.04, 0.6, 0.0),
        builtin("Chisel", 12.0, 0.9, 0.04, 0.3, 30.0),
        builtin("Nib", 7.0, 0.95, 0.04, 0.2, 45.0),
        builtin("Knife", 18.0, 1.0, 0.03, 0.12, 0.0),
        builtin("Broad Soft", 40.0, 0.0, 0.05, 1.0, 0.0),
        builtin("Dotted", 4.0, 0.9, 1.0, 1.0, 0.0),
        // Natural brushes: ragged edges, bristle streaks, lift-off.
        natural(builtin("Worn Round", 14.0, 0.6, 0.04, 1.0, 0.0), 0.4, 0.45, 0.6),
        natural(builtin("Rough Round", 14.0, 0.55, 0.04, 1.0, 0.0), 0.75, 0.35, 0.6),
        natural(builtin("Bristle Flat", 16.0, 0.75, 0.04, 0.35, 0.0), 0.35, 0.85, 0.7),
        natural(builtin("Dry Flat", 16.0, 0.75, 0.04, 0.35, 0.0), 0.55, 1.0, 0.8),
        natural(builtin("Worn Filbert", 14.0, 0.6, 0.04, 0.6, 0.0), 0.5, 0.55, 0.65),
        natural(builtin("Scruffy", 20.0, 0.45, 0.04, 0.8, 15.0), 0.95, 0.7, 0.8),
        natural(builtin("Rough Knife", 18.0, 1.0, 0.03, 0.12, 0.0), 0.6, 0.3, 0.5),
    ]
}

pub struct PresetEntry {
    pub preset: BrushPreset,
    pub thumbnail: egui::TextureHandle,
    /// Where it's saved, for presets you made (built-ins have none).
    pub file: Option<PathBuf>,
}

pub struct PresetLibrary {
    pub entries: Vec<PresetEntry>,
    /// The last error from loading/saving, shown in the UI.
    pub error: Option<String>,
}

impl PresetLibrary {
    pub fn load(ctx: &egui::Context) -> Self {
        let mut library = PresetLibrary { entries: Vec::new(), error: None };
        for preset in builtin_presets() {
            library.push(ctx, preset, None);
        }
        if let Err(e) = library.load_user_presets(ctx) {
            library.error = Some(format!("Couldn't load your brushes: {e}"));
        }
        library
    }

    fn push(&mut self, ctx: &egui::Context, preset: BrushPreset, file: Option<PathBuf>) {
        let thumbnail = ctx.load_texture(
            format!("preset-{}", preset.name),
            thumbnail(&preset),
            egui::TextureOptions::LINEAR,
        );
        self.entries.push(PresetEntry { preset, thumbnail, file });
    }

    fn load_user_presets(&mut self, ctx: &egui::Context) -> std::io::Result<()> {
        let Some(dir) = brushes_dir() else { return Ok(()) };
        if !dir.exists() {
            return Ok(());
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        files.sort();
        for path in files {
            match fs::read_to_string(&path).map(|text| toml::from_str::<BrushPreset>(&text)) {
                Ok(Ok(preset)) => self.push(ctx, preset, Some(path)),
                Ok(Err(e)) => self.error = Some(format!("{}: {e}", path.display())),
                Err(e) => self.error = Some(format!("{}: {e}", path.display())),
            }
        }
        Ok(())
    }

    /// Save `shape` as a new preset file (or overwrite one with the same name).
    pub fn save(&mut self, ctx: &egui::Context, name: &str, shape: &BrushShape) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the brush a name first.".into());
        }
        let dir = brushes_dir().ok_or("Couldn't find the Application Support folder.")?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.toml", file_stem(name)));
        let preset = BrushPreset::from_shape(name, shape);
        let text = toml::to_string_pretty(&preset).map_err(|e| e.to_string())?;
        fs::write(&path, text).map_err(|e| e.to_string())?;

        self.entries.retain(|e| e.file.as_ref() != Some(&path));
        self.push(ctx, preset, Some(path));
        Ok(())
    }

    /// Delete one of your presets (built-ins can't be deleted).
    pub fn delete(&mut self, index: usize) {
        if let Some(path) = &self.entries[index].file {
            if let Err(e) = fs::remove_file(path) {
                self.error = Some(format!("Couldn't delete {}: {e}", path.display()));
                return;
            }
            self.entries.remove(index);
        }
    }
}

fn brushes_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Oxide").join("brushes"))
}

/// Turn a brush name into a safe file name ("Ink / Wet #2" -> "ink-wet-2").
fn file_stem(name: &str) -> String {
    let mut stem = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            stem.push(ch.to_ascii_lowercase());
        } else if !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    if stem.is_empty() { "brush".to_string() } else { stem.to_string() }
}

/// Paint a little S-curve with a brush using the real engine, so the UI
/// shows what the brush actually does.
pub fn render_preview(brush: &Brush, width: usize, height: usize) -> egui::ColorImage {
    let mut doc = Document::new(width, height, PAPER);
    let mut brush = brush.clone();
    // Big brushes would fill the whole preview; cap their size.
    brush.radius = brush.radius.min(height as f32 * 0.3);
    let (w, h) = (width as f32, height as f32);
    let margin = brush.radius + 2.0;
    let point = |t: f32| {
        let x = margin + t * (w - 2.0 * margin);
        let y = h / 2.0 + (t * std::f32::consts::TAU).sin() * h * 0.22;
        (x, y)
    };
    let canvas = doc.active_canvas();
    let (x, y) = point(0.0);
    let mut stroke = Stroke::begin(canvas, &brush, DabMode::Paint, x, y);
    for i in 1..=80 {
        let (x, y) = point(i as f32 / 80.0);
        stroke.line_to(canvas, &brush, x, y);
    }
    stroke.finish(canvas, &brush);
    egui::ColorImage::from_rgba_unmultiplied([width, height], &doc.composite_rgba8(doc.bounds()))
}

/// A preset thumbnail: the shape with plain dark ink, independent of medium.
fn thumbnail(preset: &BrushPreset) -> egui::ColorImage {
    let ink = ToolState { shape: preset.shape(), opacity: 1.0, loading: 1.0, oiliness: 0.0 };
    // (Ink is a dry medium, so the thumbnail shows only the shape and texture.)
    render_preview(&ink.brush(Tool::Ink, Color::rgb(0.15, 0.15, 0.18)), THUMB_W, THUMB_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(file_stem("Ink / Wet #2"), "ink-wet-2");
        assert_eq!(file_stem("  "), "brush");
        assert_eq!(file_stem("../../etc"), "etc");
    }

    #[test]
    fn preset_round_trips_through_toml() {
        let preset = builtin("Nib", 7.0, 0.95, 0.04, 0.2, 45.0);
        let text = toml::to_string_pretty(&preset).unwrap();
        assert_eq!(toml::from_str::<BrushPreset>(&text).unwrap(), preset);
    }

    #[test]
    fn old_files_still_load() {
        // Missing shape fields get defaults; old medium fields are ignored.
        let text = "name = \"Old\"\nradius = 5.0\nhardness = 0.5\nopacity = 1.0\nspacing = 0.1\nloading = 0.3\n";
        let preset: BrushPreset = toml::from_str(text).unwrap();
        assert_eq!(preset.roundness, 1.0);
        assert_eq!(preset.angle, 0.0);
    }
}
