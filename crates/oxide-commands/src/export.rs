//! Exporting the painting to other file formats: PNG, JPEG, BMP, TIFF,
//! WebP, and layered PSD.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use oxide_engine::{Color, Document, PsdOptions};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Bmp,
    Tiff,
    WebP,
    Psd,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 6] =
        [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Bmp, ExportFormat::Tiff, ExportFormat::WebP, ExportFormat::Psd];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Png => "PNG",
            ExportFormat::Jpeg => "JPEG",
            ExportFormat::Bmp => "Bitmap (BMP)",
            ExportFormat::Tiff => "TIFF",
            ExportFormat::WebP => "WebP (lossless)",
            ExportFormat::Psd => "Photoshop (PSD, layered)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Bmp => "bmp",
            ExportFormat::Tiff => "tif",
            ExportFormat::WebP => "webp",
            ExportFormat::Psd => "psd",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            ExportFormat::Png => "Lossless, with transparency if you leave out the paper.",
            ExportFormat::Jpeg => "Small files for sharing. No transparency: empty areas get a background color.",
            ExportFormat::Bmp => "Plain uncompressed image.",
            ExportFormat::Tiff => "High quality for printing, any size. Optional 16-bit.",
            ExportFormat::WebP => "Modern web format (lossless).",
            ExportFormat::Psd => "Keeps your layers (names, opacity, visibility) for Photoshop, Photopea, Krita, GIMP…",
        }
    }

    pub fn supports_transparency(self) -> bool {
        !matches!(self, ExportFormat::Jpeg)
    }

    pub fn supports_16_bit(self) -> bool {
        matches!(self, ExportFormat::Png | ExportFormat::Tiff | ExportFormat::Psd)
    }
}

/// Everything the Export dialog lets you choose (remembered between runs).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Off = transparent background wherever there's no paint.
    pub include_paper: bool,
    /// JPEG quality, 1..=100.
    pub jpeg_quality: u8,
    /// JPEG background for transparent areas; `None` = the paper color.
    pub jpeg_background: Option<[f32; 3]>,
    pub sixteen_bit: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { format: ExportFormat::Png, include_paper: true, jpeg_quality: 90, jpeg_background: None, sixteen_bit: false }
    }
}

/// Write `document` to `path` in the chosen format.
pub fn export(document: &Document, path: &Path, options: &ExportOptions) -> Result<(), String> {
    let (w, h) = (document.width() as u32, document.height() as u32);
    let dpi = document.dpi();
    let sixteen = options.sixteen_bit && options.format.supports_16_bit();
    let include_paper = options.include_paper || !options.format.supports_transparency();
    let err = |e: &dyn std::fmt::Display| e.to_string();

    if options.format == ExportFormat::Psd {
        let file = File::create(path).map_err(|e| err(&e))?;
        return document.write_psd(file, PsdOptions { include_paper: options.include_paper, sixteen_bit: sixteen }).map_err(|e| err(&e));
    }

    // JPEG: flatten onto a background color instead of the textured paper
    // only if the user picked a custom background.
    let pixels = match (options.format, options.jpeg_background) {
        (ExportFormat::Jpeg, Some([r, g, b])) => {
            flatten_onto(&document.composite_rgba_f32(document.bounds(), false), Color::rgb(r, g, b))
        }
        _ => document.composite_rgba_f32(document.bounds(), include_paper),
    };

    match options.format {
        ExportFormat::Png => write_png(path, &pixels, w, h, dpi, sixteen),
        ExportFormat::Jpeg => {
            let file = BufWriter::new(File::create(path).map_err(|e| err(&e))?);
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, options.jpeg_quality.clamp(1, 100));
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi.round().clamp(1.0, 65535.0) as u16));
            encoder.encode(&rgb8(&pixels), w, h, image::ExtendedColorType::Rgb8).map_err(|e| err(&e))
        }
        ExportFormat::Bmp => {
            image::save_buffer_with_format(path, &rgba8(&pixels), w, h, image::ExtendedColorType::Rgba8, image::ImageFormat::Bmp)
                .map_err(|e| err(&e))
        }
        ExportFormat::WebP => {
            image::save_buffer_with_format(path, &rgba8(&pixels), w, h, image::ExtendedColorType::Rgba8, image::ImageFormat::WebP)
                .map_err(|e| err(&e))
        }
        ExportFormat::Tiff => write_tiff(path, &pixels, w, h, dpi, sixteen),
        ExportFormat::Psd => unreachable!("handled above"),
    }
}

/// PNG via the `png` crate directly, so we can store DPI (the pHYs chunk).
fn write_png(path: &Path, pixels: &[[f32; 4]], w: u32, h: u32, dpi: f32, sixteen: bool) -> Result<(), String> {
    let file = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(if sixteen { png::BitDepth::Sixteen } else { png::BitDepth::Eight });
    // PNG stores pixels per *meter*.
    let ppm = (dpi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let data = if sixteen { rgba16_be(pixels) } else { rgba8(pixels) };
    writer.write_image_data(&data).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())
}

/// TIFF via the `tiff` crate directly, for DPI and compression.
fn write_tiff(path: &Path, pixels: &[[f32; 4]], w: u32, h: u32, dpi: f32, sixteen: bool) -> Result<(), String> {
    use tiff::encoder::{Compression, Rational, TiffEncoder, colortype};
    use tiff::tags::ResolutionUnit;
    let file = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let mut tiff = TiffEncoder::new(file).map_err(|e| e.to_string())?.with_compression(Compression::Deflate(Default::default()));
    let resolution = Rational { n: dpi.round().max(1.0) as u32, d: 1 };
    if sixteen {
        let mut image = tiff.new_image::<colortype::RGBA16>(w, h).map_err(|e| e.to_string())?;
        image.resolution(ResolutionUnit::Inch, resolution);
        let data: Vec<u16> = pixels.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 65535.0).round() as u16)).collect();
        image.write_data(&data).map_err(|e| e.to_string())
    } else {
        let mut image = tiff.new_image::<colortype::RGBA8>(w, h).map_err(|e| e.to_string())?;
        image.resolution(ResolutionUnit::Inch, resolution);
        image.write_data(&rgba8(pixels)).map_err(|e| e.to_string())
    }
}

fn to8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn rgba8(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels.iter().flat_map(|p| p.map(to8)).collect()
}

fn rgb8(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels.iter().flat_map(|p| [to8(p[0]), to8(p[1]), to8(p[2])]).collect()
}

fn rgba16_be(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels.iter().flat_map(|p| p.map(|c| (c.clamp(0.0, 1.0) * 65535.0).round() as u16)).flat_map(u16::to_be_bytes).collect()
}

/// Put straight-alpha pixels over a solid background (for JPEG).
fn flatten_onto(pixels: &[[f32; 4]], bg: Color) -> Vec<[f32; 4]> {
    pixels
        .iter()
        .map(|p| {
            let a = p[3];
            [p[0] * a + bg.r * (1.0 - a), p[1] * a + bg.g * (1.0 - a), p[2] * a + bg.b * (1.0 - a), 1.0]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_engine::{Brush, DabMode, Stroke};

    fn sample() -> Document {
        let mut doc = Document::new(40, 30, Color::rgb(0.9, 0.9, 0.8));
        doc.set_dpi(300.0);
        doc.begin_stroke();
        let brush = Brush { color: Color::rgb(1.0, 0.0, 0.0), radius: 6.0, ..Brush::default() };
        Stroke::begin(doc.active_canvas(), &brush, DabMode::Paint, 20.0, 15.0);
        doc.end_stroke();
        doc
    }

    /// Every format writes a file that the `image` crate can read back at the right size.
    #[test]
    fn every_raster_format_round_trips() {
        let doc = sample();
        let dir = std::env::temp_dir().join(format!("oxide-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for format in ExportFormat::ALL {
            if format == ExportFormat::Psd {
                continue;
            }
            for (include_paper, sixteen_bit) in [(true, false), (false, true)] {
                let options = ExportOptions { format, include_paper, sixteen_bit, ..ExportOptions::default() };
                let path = dir.join(format!("t-{}-{include_paper}-{sixteen_bit}.{}", format.extension(), format.extension()));
                export(&doc, &path, &options).unwrap_or_else(|e| panic!("{format:?}: {e}"));
                let img = image::open(&path).unwrap_or_else(|e| panic!("{format:?} reread: {e}")).to_rgba8();
                assert_eq!(img.dimensions(), (40, 30), "{format:?}");
                // The red dot survives.
                let center = img.get_pixel(20, 15);
                assert!(center[0] > 200 && center[1] < 60, "{format:?} center {center:?}");
                // Without paper, formats with alpha are transparent in the corner.
                if !include_paper && format.supports_transparency() {
                    assert_eq!(img.get_pixel(0, 0)[3], 0, "{format:?} corner should be transparent");
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jpeg_uses_the_background_color_for_empty_areas() {
        let doc = sample();
        let path = std::env::temp_dir().join(format!("oxide-bg-{}.jpg", std::process::id()));
        let options = ExportOptions { format: ExportFormat::Jpeg, jpeg_background: Some([0.0, 0.0, 1.0]), ..ExportOptions::default() };
        export(&doc, &path, &options).unwrap();
        let img = image::open(&path).unwrap().to_rgb8();
        let corner = img.get_pixel(0, 0);
        assert!(corner[2] > 200 && corner[0] < 40, "blue background: {corner:?}");
        std::fs::remove_file(&path).ok();
    }
}
