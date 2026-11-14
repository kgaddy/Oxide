//! Oxide's native file format (`.oxide`).
//!
//! An `.oxide` file is a zip archive containing:
//!
//! ```text
//! mimetype            "application/x-oxide" (first, uncompressed, like OpenRaster)
//! document.toml       size, paper color, layer list
//! layers/0.f32        raw pixels for each layer: little-endian f32
//! layers/1.f32        premultiplied RGBA, row by row (compressed by zip)
//! palette.oxide       the mixing palette, if any
//! reference           the reference photo's image file, if any
//! ```
//!
//! Keeping full float precision means nothing is lost when you save and
//! reopen. Later versions will add more per-layer data (wetness, impasto
//! height) as extra files, which is why it's a zip and not one blob.

use std::io::{Read, Seek, Write};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{Canvas, Color, Document, Layer, PaperSettings};

const MIMETYPE: &str = "application/x-oxide";
const FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    width: usize,
    height: usize,
    paper: [f32; 3],
    /// The paper texture. Missing in older files: smooth paper.
    #[serde(default = "PaperSettings::flat")]
    surface: PaperSettings,
    /// Print resolution. Missing in older files: the default.
    #[serde(default = "default_dpi")]
    dpi: f32,
    /// Free-text notes. Missing in older files: none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    active_layer: usize,
    /// Bottom layer first.
    layers: Vec<LayerInfo>,
    /// Layer groups (missing in older files: none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<GroupInfo>,
}

#[derive(Serialize, Deserialize)]
struct GroupInfo {
    id: u32,
    name: String,
    visible: bool,
    opacity: f32,
    #[serde(default)]
    open: bool,
}

#[derive(Serialize, Deserialize)]
struct LayerInfo {
    name: String,
    visible: bool,
    opacity: f32,
    pixels: String,
    /// The layer's wet paint (same format as `pixels`), only if it has any.
    #[serde(default)]
    wet_pixels: Option<String>,
    /// Paint thickness (impasto): one little-endian f32 per pixel, only if the layer has any.
    #[serde(default)]
    relief: Option<String>,
    /// How much of that thickness is still wet (same format), only if any is.
    #[serde(default)]
    wet_relief: Option<String>,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    lock_transparency: bool,
    #[serde(default)]
    tracing: bool,
    /// The layer's own impasto depth (missing = uses the painting's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    impasto: Option<f32>,
    /// The id of the group it's in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<u32>,
    /// Blend mode ("multiply", ...; missing = normal).
    #[serde(default, skip_serializing_if = "is_normal")]
    blend: crate::BlendMode,
    /// The layer mask (one f32 per pixel, 1 = shown), if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mask: Option<String>,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    mask_enabled: bool,
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_normal(mode: &crate::BlendMode) -> bool {
    *mode == crate::BlendMode::Normal
}

/// Something went wrong reading or writing a file.
#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    Zip(zip::result::ZipError),
    /// The file isn't a valid Oxide document (with a reason).
    Invalid(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Io(e) => write!(f, "{e}"),
            FileError::Zip(e) => write!(f, "not a valid Oxide file ({e})"),
            FileError::Invalid(why) => write!(f, "not a valid Oxide file ({why})"),
        }
    }
}

impl std::error::Error for FileError {}

// These let the `?` operator convert library errors into FileError automatically.
impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Io(e)
    }
}

impl From<zip::result::ZipError> for FileError {
    fn from(e: zip::result::ZipError) -> Self {
        FileError::Zip(e)
    }
}

impl Document {
    /// Write the document in `.oxide` format.
    pub fn save<W: Write + Seek>(&self, writer: W) -> Result<(), FileError> {
        let mut zip = ZipWriter::new(writer);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("mimetype", stored)?;
        zip.write_all(MIMETYPE.as_bytes())?;

        let paper = self.paper();
        let manifest = Manifest {
            format_version: FORMAT_VERSION,
            width: self.width(),
            height: self.height(),
            paper: [paper.r, paper.g, paper.b],
            surface: self.surface().settings(),
            dpi: self.dpi(),
            notes: self.notes.clone(),
            // Only groups that still have layers.
            groups: self
                .groups()
                .iter()
                .filter(|g| self.layers().iter().any(|l| l.group == Some(g.id)))
                .map(|g| GroupInfo { id: g.id, name: g.name.clone(), visible: g.visible, opacity: g.opacity, open: g.open })
                .collect(),
            active_layer: self.active_index(),
            layers: self
                .layers()
                .iter()
                .enumerate()
                .map(|(i, layer)| LayerInfo {
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    pixels: format!("layers/{i}.f32"),
                    // Only store wetness for layers that have wet paint.
                    wet_pixels: layer.canvas.has_wet_paint().then(|| format!("layers/{i}.wet.f32")),
                    relief: layer.canvas.relief().iter().any(|&h| h > 0.0).then(|| format!("layers/{i}.relief.f32")),
                    wet_relief: layer.canvas.wet_relief().iter().any(|&h| h > 0.0).then(|| format!("layers/{i}.wet-relief.f32")),
                    locked: layer.locked,
                    lock_transparency: layer.lock_transparency(),
                    tracing: layer.tracing,
                    impasto: layer.impasto,
                    group: layer.group,
                    blend: layer.blend,
                    mask: layer.mask.as_ref().map(|_| format!("layers/{i}.mask.f32")),
                    mask_enabled: layer.mask.as_ref().is_none_or(|m| m.enabled),
                })
                .collect(),
        };
        let toml = toml::to_string_pretty(&manifest).map_err(|e| FileError::Invalid(e.to_string()))?;
        zip.start_file("document.toml", deflated)?;
        zip.write_all(toml.as_bytes())?;

        for (info, layer) in manifest.layers.iter().zip(self.layers()) {
            zip.start_file(info.pixels.as_str(), deflated)?;
            zip.write_all(&pixels_to_bytes(layer.canvas.dry_pixels()))?;
            if let Some(path) = &info.wet_pixels {
                zip.start_file(path.as_str(), deflated)?;
                zip.write_all(&pixels_to_bytes(layer.canvas.wet_pixels()))?;
            }
            let mask: &[f32] = layer.mask.as_ref().map_or(&[], |m| m.data.as_slice());
            for (path, values) in [(&info.relief, layer.canvas.relief()), (&info.wet_relief, layer.canvas.wet_relief()), (&info.mask, mask)] {
                if let Some(path) = path {
                    zip.start_file(path.as_str(), deflated)?;
                    zip.write_all(&values.iter().flat_map(|h| h.to_le_bytes()).collect::<Vec<u8>>())?;
                }
            }
        }

        // The mixing palette, as a small .oxide inside the .oxide.
        if let Some(palette) = &self.palette {
            let mut bytes = std::io::Cursor::new(Vec::new());
            palette.save(&mut bytes)?;
            zip.start_file("palette.oxide", stored)?;
            zip.write_all(bytes.get_ref())?;
        }

        // The reference photo, as its original image file (already compressed).
        if let Some(reference) = &self.reference {
            zip.start_file("reference", stored)?;
            zip.write_all(reference)?;
        }

        zip.finish()?;
        Ok(())
    }

    /// Read a document saved with [`Document::save`].
    pub fn load<R: Read + Seek>(reader: R) -> Result<Document, FileError> {
        let mut zip = ZipArchive::new(reader)?;

        let mut mimetype = String::new();
        zip.by_name("mimetype")?.read_to_string(&mut mimetype)?;
        if mimetype != MIMETYPE {
            return Err(FileError::Invalid(format!("unexpected type \"{mimetype}\"")));
        }

        let mut text = String::new();
        zip.by_name("document.toml")?.read_to_string(&mut text)?;
        let manifest: Manifest = toml::from_str(&text).map_err(|e| FileError::Invalid(e.to_string()))?;
        if manifest.format_version > FORMAT_VERSION {
            return Err(FileError::Invalid(format!(
                "made by a newer Oxide (format {}); please update",
                manifest.format_version
            )));
        }
        if manifest.layers.is_empty() || manifest.width == 0 || manifest.height == 0 {
            return Err(FileError::Invalid("no layers or zero size".into()));
        }

        let pixel_count = manifest.width * manifest.height;
        let mut layers = Vec::with_capacity(manifest.layers.len());
        for info in &manifest.layers {
            let dry = read_pixels(&mut zip, &info.pixels, pixel_count, &info.name)?;
            let wet = match &info.wet_pixels {
                Some(path) => Some(read_pixels(&mut zip, path, pixel_count, &info.name)?),
                None => None,
            };
            let mut canvas = Canvas::new(manifest.width, manifest.height, Color::TRANSPARENT);
            canvas.set_pixels(dry, wet);
            if let Some(path) = &info.relief {
                let relief = read_floats(&mut zip, path, pixel_count, &info.name)?;
                let wet_relief = match &info.wet_relief {
                    Some(path) => Some(read_floats(&mut zip, path, pixel_count, &info.name)?),
                    None => None,
                };
                canvas.set_relief(relief, wet_relief);
            }
            canvas.take_changed();
            canvas.set_preserve_alpha(info.lock_transparency);
            layers.push(Layer {
                name: info.name.clone(),
                canvas,
                visible: info.visible,
                opacity: info.opacity,
                locked: info.locked,
                tracing: info.tracing,
                impasto: info.impasto,
                group: info.group,
                blend: info.blend,
                mask: match &info.mask {
                    Some(path) => Some(crate::LayerMask { data: read_floats(&mut zip, path, pixel_count, &info.name)?, enabled: info.mask_enabled }),
                    None => None,
                },
            });
        }

        let [r, g, b] = manifest.paper;
        let mut doc = Document::from_layers(
            manifest.width,
            manifest.height,
            Color::rgb(r, g, b),
            manifest.surface,
            layers,
            manifest.active_layer,
        );
        doc.set_dpi(manifest.dpi);
        doc.notes = manifest.notes;
        doc.set_groups(
            manifest
                .groups
                .into_iter()
                .map(|g| crate::LayerGroup { id: g.id, name: g.name, visible: g.visible, opacity: g.opacity, open: g.open })
                .collect(),
        );

        // Optional mixing palette (older files don't have one).
        if let Ok(mut entry) = zip.by_name("palette.oxide") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            doc.palette = Document::load(std::io::Cursor::new(bytes)).ok().map(Box::new);
        }
        // Optional reference photo.
        if let Ok(mut entry) = zip.by_name("reference") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            doc.reference = Some(std::sync::Arc::new(bytes));
        }
        Ok(doc)
    }
}

fn default_dpi() -> f32 {
    crate::document::DEFAULT_DPI
}

fn pixels_to_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels.iter().flatten().flat_map(|channel| channel.to_le_bytes()).collect()
}

/// One little-endian f32 per pixel (paint thickness maps).
fn read_floats<R: Read + Seek>(zip: &mut ZipArchive<R>, path: &str, pixel_count: usize, layer_name: &str) -> Result<Vec<f32>, FileError> {
    let mut bytes = Vec::with_capacity(pixel_count * 4);
    zip.by_name(path)?.read_to_end(&mut bytes)?;
    if bytes.len() != pixel_count * 4 {
        return Err(FileError::Invalid(format!("layer \"{layer_name}\" has the wrong relief size")));
    }
    Ok(bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
}

fn read_pixels<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    path: &str,
    pixel_count: usize,
    layer_name: &str,
) -> Result<Vec<[f32; 4]>, FileError> {
    let mut bytes = Vec::with_capacity(pixel_count * 16);
    zip.by_name(path)?.read_to_end(&mut bytes)?;
    if bytes.len() != pixel_count * 16 {
        return Err(FileError::Invalid(format!("layer \"{layer_name}\" has the wrong size")));
    }
    Ok(bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|px| {
            let channel = |i: usize| f32::from_le_bytes([px[i], px[i + 1], px[i + 2], px[i + 3]]);
            [channel(0), channel(4), channel(8), channel(12)]
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Brush, DabMode, DabStyle, Wetness};
    use std::io::Cursor;

    #[test]
    fn save_and_load_round_trip() {
        let paper = Color::rgb(0.9, 0.85, 0.8);
        let mut doc = Document::new(30, 20, paper);
        doc.begin_stroke();
        let soft = Brush { color: Color::rgb(0.2, 0.4, 0.9), opacity: 0.3, hardness: 0.0, ..Brush::default() };
        let relief = Some(crate::ReliefDab { build: 0.7, smear_to: 0.0, smear: 0.0, plow: 0.0, push: 0.0, flat: false, scrape: 0.0, level: false });
        let wet = DabStyle { color: soft.color, opacity: 0.3, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief, bands: None };
        doc.active_canvas().dab_with(10.0, 10.0, &soft, &wet);
        doc.end_stroke();
        doc.add_layer();
        doc.rename_layer(1, "Ink".into());
        doc.set_opacity(1, 0.5);
        doc.toggle_visible(1);
        let linen = PaperSettings {
            kind: crate::PaperKind::Linen,
            scale: 1.5,
            depth: 0.7,
            light_angle: 200.0,
            paper_visibility: 0.6,
            paint_visibility: 0.15,
            impasto_depth: 0.8,
            gloss: 0.6,
            shadows: 0.7,
            light_height: 30.0,
        };
        doc.set_surface(linen);
        doc.set_dpi(300.0);
        doc.set_tracing(0, true);
        doc.set_lock_transparency(1, true);
        doc.set_layer_impasto(1, Some(0.2));

        let mut file = Cursor::new(Vec::new());
        doc.save(&mut file).unwrap();
        file.set_position(0);
        let loaded = Document::load(file).unwrap();

        assert_eq!((loaded.width(), loaded.height()), (30, 20));
        assert_eq!(loaded.paper(), paper);
        assert_eq!(loaded.surface().settings(), linen);
        assert_eq!(loaded.dpi(), 300.0);
        assert!(loaded.layers()[0].tracing && loaded.layers()[0].locked, "tracing + lock saved");
        assert!(loaded.layers()[1].lock_transparency(), "lock transparency saved");
        assert_eq!(loaded.layers()[1].impasto, Some(0.2), "per-layer impasto saved");
        assert_eq!(loaded.layers()[0].impasto, None);
        assert!(loaded.palette.is_none(), "no palette saved, none loaded");
        assert!(loaded.reference.is_none(), "no reference saved, none loaded");
        assert_eq!(loaded.layers().len(), 2);
        assert_eq!(loaded.active_index(), 1);
        let ink = &loaded.layers()[1];
        assert_eq!((ink.name.as_str(), ink.visible, ink.opacity), ("Ink", false, 0.5));
        // Pixels survive exactly, float precision included.
        assert_eq!(loaded.layers()[0].canvas.dry_pixels(), doc.layers()[0].canvas.dry_pixels());
        // Wet paint stays wet.
        assert!(loaded.layers()[0].canvas.has_wet_paint());
        assert_eq!(loaded.layers()[0].canvas.wet_pixels(), doc.layers()[0].canvas.wet_pixels());
        assert_eq!(loaded.layers()[0].canvas.relief(), doc.layers()[0].canvas.relief(), "impasto saved");
        assert_eq!(loaded.layers()[0].canvas.wet_relief(), doc.layers()[0].canvas.wet_relief(), "wet impasto saved");
        assert!(doc.layers()[0].canvas.wet_relief().iter().any(|&h| h > 0.0));
        assert_eq!(loaded.surface().settings(), linen);
    }

    #[test]
    fn mixing_palette_is_saved_inside_the_painting() {
        let mut doc = Document::new(20, 20, Color::WHITE);
        let mut palette = Document::new(16, 8, Color::rgb(0.9, 0.9, 0.9));
        let wet = DabStyle { color: Color::rgb(0.0, 0.5, 0.2), opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0, pigment: None, relief: None, bands: None };
        palette.active_canvas().dab_with(8.0, 4.0, &Brush { radius: 3.0, ..Brush::default() }, &wet);
        palette.notes = "Paints:\n• Ultramarine Blue (#1C2A6B)\n• Burnt Umber \"quoted\" — ok".into();
        doc.palette = Some(Box::new(palette));

        let mut file = Cursor::new(Vec::new());
        doc.save(&mut file).unwrap();
        file.set_position(0);
        let loaded = Document::load(file).unwrap();
        let palette = loaded.palette.expect("palette comes back");
        assert_eq!((palette.width(), palette.height()), (16, 8));
        assert!(palette.layers()[0].canvas.has_wet_paint(), "palette paint stays wet");
        assert!(palette.notes.contains("Ultramarine Blue (#1C2A6B)") && palette.notes.contains("\"quoted\" — ok"), "notes survive");
    }

    #[test]
    fn reference_photo_is_saved_inside_the_painting() {
        let mut doc = Document::new(8, 8, Color::WHITE);
        let photo = b"\x89PNG not really, but any bytes come back as they were".to_vec();
        doc.reference = Some(std::sync::Arc::new(photo.clone()));

        let mut file = Cursor::new(Vec::new());
        doc.save(&mut file).unwrap();
        file.set_position(0);
        let loaded = Document::load(file).unwrap();
        assert_eq!(loaded.reference.as_deref(), Some(&photo));
    }

    #[test]
    fn rejects_files_that_are_not_oxide() {
        let result = Document::load(Cursor::new(b"definitely not a zip".to_vec()));
        assert!(result.is_err());
    }
}
