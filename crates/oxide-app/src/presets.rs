//! Brush presets: the built-in set plus your own, saved as small TOML files
//! in `~/Library/Application Support/Oxide/brushes/`.

use std::fs;
use std::path::PathBuf;

use eframe::egui;
use oxide_engine::{Bristles, Brush, Color, DabMode, Document, Rotation, Stroke, Tip};
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
    /// Which group it's listed under in the Brushes panel. Empty (older
    /// files) = "My Brushes" for your presets.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
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
    #[serde(default, skip_serializing_if = "is_zero")]
    pub taper: f32,
    /// How the tip turns while painting. Missing in older files: fixed.
    #[serde(default)]
    pub rotation: Rotation,
    /// Pressure response. `None` = keep the tool's own setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_pressure: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_pressure: Option<f32>,
    /// Paper-texture sensitivity. `None` = keep the tool's own setting
    /// (built-in shapes don't change how a medium feels the paper).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<f32>,
    /// A bristle brush's hairs (None = a stamped tip).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bristles: Option<Bristles>,
    /// Texture tip (missing in older files: round).
    #[serde(default)]
    pub tip: Tip,
    #[serde(default)]
    pub jitter_angle: f32,
    #[serde(default)]
    pub jitter_size: f32,
    #[serde(default)]
    pub scatter: f32,
}

fn one() -> f32 {
    1.0
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl BrushPreset {
    pub fn from_shape(name: &str, group: &str, shape: &BrushShape) -> Self {
        BrushPreset {
            name: name.to_string(),
            group: group.to_string(),
            radius: shape.radius,
            hardness: shape.hardness,
            spacing: shape.spacing,
            roundness: shape.roundness,
            angle: shape.angle,
            roughness: shape.roughness,
            streaks: shape.streaks,
            lift_off: shape.lift_off,
            taper: shape.taper,
            rotation: shape.rotation,
            size_pressure: Some(shape.size_pressure),
            opacity_pressure: Some(shape.opacity_pressure),
            grain: Some(shape.grain),
            bristles: shape.bristles,
            tip: shape.tip,
            jitter_angle: shape.jitter_angle,
            jitter_size: shape.jitter_size,
            scatter: shape.scatter,
        }
    }

    /// This preset's shape, keeping `current`'s grain unless the preset has its own.
    pub fn apply_to(&self, current: &BrushShape) -> BrushShape {
        BrushShape {
            grain: self.grain.unwrap_or(current.grain),
            size_pressure: self.size_pressure.unwrap_or(current.size_pressure),
            opacity_pressure: self.opacity_pressure.unwrap_or(current.opacity_pressure),
            ..self.shape()
        }
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
            taper: self.taper,
            rotation: self.rotation,
            size_pressure: self.size_pressure.unwrap_or(0.0),
            opacity_pressure: self.opacity_pressure.unwrap_or(0.0),
            grain: self.grain.unwrap_or(0.0),
            bristles: self.bristles,
            tip: self.tip,
            jitter_angle: self.jitter_angle,
            jitter_size: self.jitter_size,
            scatter: self.scatter,
        }
    }

    pub fn matches(&self, shape: &BrushShape) -> bool {
        self.apply_to(shape) == *shape
    }

    /// Same as [`Self::matches`], but any size counts (resizing with [ ] or
    /// ⌘-drag doesn't make it a different brush).
    pub fn matches_any_size(&self, shape: &BrushShape) -> bool {
        BrushShape { radius: shape.radius, ..self.apply_to(shape) } == *shape
    }
}

fn builtin(name: &str, radius: f32, hardness: f32, spacing: f32, roundness: f32, angle: f32) -> BrushPreset {
    BrushPreset { name: name.to_string(), group: String::new(), radius, hardness, spacing, roundness, angle, roughness: 0.0, streaks: 0.0, lift_off: 0.0, taper: 0.0, rotation: Rotation::None, size_pressure: None, opacity_pressure: None, grain: None, bristles: None, tip: Tip::Round, jitter_angle: 0.0, jitter_size: 0.0, scatter: 0.0 }
}

/// A built-in preset whose tip turns to follow the stroke (flat brushes, knives).
fn follow(preset: BrushPreset) -> BrushPreset {
    BrushPreset { rotation: Rotation::FollowStroke, ..preset }
}

/// A built-in preset with a natural, imperfect tip.
fn natural(preset: BrushPreset, roughness: f32, streaks: f32, lift_off: f32) -> BrushPreset {
    BrushPreset { roughness, streaks, lift_off, ..preset }
}

/// A built-in texture-tip brush, with per-dab jitter (angle, size, scatter).
fn textured(preset: BrushPreset, tip: Tip, jitter_angle: f32, jitter_size: f32, scatter: f32) -> BrushPreset {
    BrushPreset { tip, jitter_angle, jitter_size, scatter, ..preset }
}

/// A built-in engraving brush: crisp, and every stroke tapers to a point at both ends.
fn engraver(name: &str, radius: f32, taper: f32) -> BrushPreset {
    BrushPreset { taper, ..builtin(name, radius, 1.0, 0.05, 1.0, 0.0) }
}

/// A built-in bristle brush (separate hairs).
fn hairs(preset: BrushPreset, count: u32, thickness: f32, spread: f32, stiffness: f32) -> BrushPreset {
    BrushPreset { bristles: Some(Bristles { count, thickness, spread, stiffness }), ..preset }
}

/// The group for presets you save, unless you pick another.
pub const MY_BRUSHES: &str = "My Brushes";

/// Which group a built-in preset belongs to.
fn builtin_group(preset: &BrushPreset) -> &'static str {
    match preset.name.as_str() {
        "Flat" | "Filbert" | "Chisel" | "Nib" => "Flat & Filbert",
        "Knife" | "Rough Knife" => "Knife",
        _ if preset.taper > 0.0 => "Engraving",
        _ if preset.bristles.is_some() => "Bristle",
        _ if preset.tip != Tip::Round => "Texture",
        _ if preset.roughness > 0.0 || preset.streaks > 0.0 => "Natural",
        _ => "Round",
    }
}

fn builtin_presets() -> Vec<BrushPreset> {
    let mut presets = builtin_list();
    for p in &mut presets {
        p.group = builtin_group(p).to_string();
    }
    presets
}

fn builtin_list() -> Vec<BrushPreset> {
    vec![
        builtin("Round", 12.0, 0.7, 0.05, 1.0, 0.0),
        builtin("Round Hard", 10.0, 0.95, 0.05, 1.0, 0.0),
        builtin("Round Soft", 18.0, 0.2, 0.06, 1.0, 0.0),
        builtin("Fine", 2.5, 0.95, 0.05, 1.0, 0.0),
        follow(builtin("Flat", 16.0, 0.7, 0.04, 0.35, 0.0)),
        follow(builtin("Filbert", 14.0, 0.6, 0.04, 0.6, 0.0)),
        builtin("Chisel", 12.0, 0.9, 0.04, 0.3, 30.0),
        builtin("Nib", 7.0, 0.95, 0.04, 0.2, 45.0),
        follow(builtin("Knife", 18.0, 1.0, 0.03, 0.12, 0.0)),
        builtin("Broad Soft", 40.0, 0.0, 0.05, 1.0, 0.0),
        builtin("Dotted", 4.0, 0.9, 1.0, 1.0, 0.0),
        // Engraving: swelling lines that come to a point at both ends.
        engraver("Engraver", 3.0, 0.6),
        engraver("Engraver Fine", 1.5, 0.8),
        engraver("Engraver Bold", 6.0, 0.5),
        // Natural brushes: ragged edges, bristle streaks, lift-off.
        natural(builtin("Worn Round", 14.0, 0.6, 0.04, 1.0, 0.0), 0.4, 0.45, 0.6),
        natural(builtin("Rough Round", 14.0, 0.55, 0.04, 1.0, 0.0), 0.75, 0.35, 0.6),
        follow(natural(builtin("Streaky Flat", 16.0, 0.75, 0.04, 0.35, 0.0), 0.35, 0.85, 0.7)),
        follow(natural(builtin("Dry Flat", 16.0, 0.75, 0.04, 0.35, 0.0), 0.55, 1.0, 0.8)),
        follow(natural(builtin("Worn Filbert", 14.0, 0.6, 0.04, 0.6, 0.0), 0.5, 0.55, 0.65)),
        follow(natural(builtin("Scruffy", 20.0, 0.45, 0.04, 0.8, 15.0), 0.95, 0.7, 0.8)),
        follow(natural(builtin("Rough Knife", 18.0, 1.0, 0.03, 0.12, 0.0), 0.6, 0.3, 0.5)),
        // Bristle brushes: real hairs that each carry paint.
        hairs(builtin("Bristle Round", 14.0, 0.7, 0.05, 1.0, 0.0), 60, 0.5, 0.5, 0.6),
        follow(hairs(builtin("Bristle Flat", 18.0, 0.7, 0.05, 0.3, 0.0), 56, 0.5, 0.4, 0.7)),
        follow(hairs(builtin("Bristle Filbert", 16.0, 0.7, 0.05, 0.6, 0.0), 64, 0.5, 0.5, 0.55)),
        follow(hairs(builtin("Sparse Flat", 18.0, 0.7, 0.05, 0.3, 0.0), 18, 0.2, 0.6, 0.6)),
        hairs(builtin("Soft Hair Round", 12.0, 0.7, 0.05, 1.0, 0.0), 90, 0.6, 0.8, 0.25),
        // Texture tips (images stamped along the stroke, turned at random).
        textured(builtin("Sponge", 26.0, 0.7, 0.3, 1.0, 0.0), Tip::Sponge, 1.0, 0.3, 0.1),
        textured(builtin("Chalk", 10.0, 0.7, 0.08, 1.0, 0.0), Tip::Chalk, 1.0, 0.1, 0.0),
        textured(builtin("Splatter", 34.0, 0.7, 0.7, 1.0, 0.0), Tip::Splatter, 1.0, 0.5, 0.5),
        follow(textured(builtin("Dry Brush", 18.0, 0.7, 0.04, 0.8, 0.0), Tip::DryBrush, 0.0, 0.0, 0.0)),
        textured(builtin("Rag", 32.0, 0.7, 0.35, 1.0, 0.0), Tip::Rag, 1.0, 0.2, 0.1),
        textured(builtin("Speckle", 22.0, 0.7, 0.5, 1.0, 0.0), Tip::Speckle, 1.0, 0.3, 0.3),
    ]
}

pub struct PresetEntry {
    pub preset: BrushPreset,
    pub thumbnail: egui::TextureHandle,
    /// Where it's saved, for presets you made (built-ins have none).
    pub file: Option<PathBuf>,
}

impl PresetEntry {
    /// The group it's listed under (your older presets: "My Brushes").
    pub fn group(&self) -> &str {
        if self.preset.group.is_empty() { MY_BRUSHES } else { &self.preset.group }
    }
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

    /// The groups, in the order they first appear (built-ins first).
    pub fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = Vec::new();
        for entry in &self.entries {
            let group = entry.group();
            if !groups.iter().any(|g| g == group) {
                groups.push(group.to_string());
            }
        }
        groups
    }

    /// Move one of your presets to another group (rewrites its file).
    pub fn set_group(&mut self, index: usize, group: &str) {
        let entry = &mut self.entries[index];
        let Some(path) = entry.file.clone() else { return };
        entry.preset.group = group.trim().to_string();
        match toml::to_string_pretty(&entry.preset).map_err(|e| e.to_string()).and_then(|t| fs::write(&path, t).map_err(|e| e.to_string())) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("Couldn't move {}: {e}", entry.preset.name)),
        }
    }

    /// Save `shape` as a preset file in `group` (empty = My Brushes),
    /// overwriting one with the same name.
    pub fn save_in(&mut self, ctx: &egui::Context, name: &str, group: &str, shape: &BrushShape) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the brush a name first.".into());
        }
        let dir = brushes_dir().ok_or("Couldn't find the Application Support folder.")?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.toml", file_stem(name)));
        let group = if group.trim().is_empty() { MY_BRUSHES } else { group.trim() };
        let preset = BrushPreset::from_shape(name, group, shape);
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
    let ink = ToolState { shape: preset.shape(), ..Tool::Ink.default_state() };
    // (Ink is a dry medium, so the thumbnail shows only the shape and texture.)
    render_preview(&ink.brush(Tool::Ink, Color::rgb(0.15, 0.15, 0.18)), THUMB_W, THUMB_H)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_brushes_are_grouped() {
        let presets = builtin_presets();
        let group = |name: &str| presets.iter().find(|p| p.name == name).map(|p| p.group.clone()).unwrap();
        assert_eq!(group("Round"), "Round");
        assert_eq!(group("Filbert"), "Flat & Filbert");
        assert_eq!(group("Worn Round"), "Natural");
        assert_eq!(group("Bristle Flat"), "Bristle");
        assert_eq!(group("Knife"), "Knife");
        assert_eq!(group("Sponge"), "Texture");
        assert!(presets.iter().all(|p| !p.group.is_empty()));
    }

    #[test]
    fn older_preset_files_have_no_group_and_new_ones_keep_theirs() {
        let old: BrushPreset = toml::from_str("name = \"Mine\"\nradius = 10.0\nhardness = 0.5\nspacing = 0.1\n").unwrap();
        assert!(old.group.is_empty(), "listed under My Brushes");
        let saved = BrushPreset::from_shape("Leaf", "Foliage", &BrushShape::default());
        let back: BrushPreset = toml::from_str(&toml::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.group, "Foliage");
    }

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
