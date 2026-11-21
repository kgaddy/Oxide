//! Importing `.reb` paintings (a zip-based layered format from another
//! natural-media app) as an Oxide document.
//!
//! What's inside a `.reb` (format version 4, as far as we've seen):
//!
//! ```text
//! artwork.xml            canvas size, paper, and the layer list (bottom first)
//! layerN.png             a layer's colors: 8-bit RGBA, straight alpha. Cropped
//!                        to the painted area; a tEXt chunk "offset" = "x y"
//!                        says where its top-left corner sits on the canvas.
//! layerN_flow.dat        one byte per pixel: how wet the paint is (best guess)
//! layerN_structure.dat   one or two bytes per pixel: paint thickness (best guess)
//! paper.png, canvas.png  paper texture tile and a thumbnail (not used)
//! ```
//!
//! The `.dat` files start with a 4-byte tag:
//!
//! - `ZERO` + u32 count: all zeros.
//! - `BBOX` + six big-endian u32 (x0, y0, x1, y1 inclusive, grid width,
//!   grid height), then a type tag + u32 count + `count` values covering just
//!   the box.
//! - a bare type tag (`UCHA` = u8, `UI_2` = big-endian u16) + u32 count +
//!   values covering the whole grid.
//!
//! The thickness grid is the canvas size; the wetness grid has a 4-pixel
//! border on every side (so it's 8 pixels wider and taller).
//!
//! The wetness and thickness meanings and scales are guesses from looking at
//! real files: tune [`RELIEF_SCALE`] and the wetness mapping as we learn more.

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use oxide_engine::{BlendMode, Canvas, Color, Document, Layer, MAX_RELIEF, PaperKind, PaperSettings};
use zip::ZipArchive;

/// Oxide relief per thickness unit. Typical strokes in the files we've seen
/// are ~100 units thick, which this makes ~0.02: just a hint of body. The
/// source app renders its thickness very subtly (paper grain shows through,
/// no ridges); 1/1000 already embossed every stroke edge, and anything much
/// thicker fills the paper grain that the originals keep.
pub const RELIEF_SCALE: f32 = 1.0 / 5000.0;

/// Blur radius (pixels) for the thickness map. Each stroke is stored as a
/// flat-topped step, and Oxide's lighting turns those steps into hard
/// embossed outlines; softening them keeps the body without the edges.
pub const RELIEF_BLUR: usize = 3;

/// Soften a grid with three box-blur passes of `radius` (about a Gaussian).
fn smooth(grid: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return grid.to_vec();
    }
    let mut a = grid.to_vec();
    let mut b = vec![0.0_f32; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, width, height, radius, true);
        box_pass(&b, &mut a, width, height, radius, false);
    }
    a
}

/// One box blur along rows (`horizontal`) or columns, clamping at the edges.
fn box_pass(src: &[f32], dst: &mut [f32], width: usize, height: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (width, height) } else { (height, width) };
    let idx = |line: usize, k: usize| if horizontal { line * width + k } else { k * width + line };
    let n = (2 * r + 1) as f32;
    for line in 0..lines {
        let at = |k: i64| src[idx(line, k.clamp(0, len as i64 - 1) as usize)];
        let mut sum: f32 = (-(r as i64)..=r as i64).map(at).sum();
        for k in 0..len {
            dst[idx(line, k)] = sum / n;
            sum += at(k as i64 + r as i64 + 1) - at(k as i64 - r as i64);
        }
    }
}

/// The wetness grid's border on each side, in pixels.
const FLOW_BORDER: usize = 4;

/// Does this path look like a `.reb` painting?
pub fn is_reb(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("reb"))
}

/// Read a `.reb` file into a new document.
pub fn load_reb(path: &Path) -> Result<Document, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    read_reb(std::io::BufReader::new(file))
}

/// Read a `.reb` archive into a new document.
pub fn read_reb<R: Read + Seek>(reader: R) -> Result<Document, String> {
    let mut zip = ZipArchive::new(reader).map_err(|e| format!("not a .reb archive ({e})"))?;
    let xml = String::from_utf8_lossy(&entry(&mut zip, "artwork.xml")?).into_owned();

    let canvas_tag = tags(&xml, "canvas").into_iter().next().ok_or("artwork.xml has no <canvas>")?;
    let layer_tags = tags(&xml, "layer");
    if layer_tags.is_empty() {
        return Err("the painting has no layers".into());
    }
    // The layers say their pixel size; the canvas size is fractional.
    let size = |key: &str, layer_key: &str| -> Result<usize, String> {
        layer_tags[0]
            .get(layer_key)
            .and_then(|v| v.parse::<usize>().ok())
            .or_else(|| canvas_tag.get(key).and_then(|v| v.parse::<f32>().ok()).map(|v| v.round() as usize))
            .filter(|&v| v > 0)
            .ok_or_else(|| format!("bad canvas {key}"))
    };
    let (width, height) = (size("width", "width")?, size("height", "height")?);

    let paper_tag = tags(&xml, "paper").into_iter().next().unwrap_or_default();
    let paper = paper_tag.get("bg_color").and_then(|c| parse_color(c)).unwrap_or(Color::WHITE);
    let surface = paper_settings(&paper_tag);

    let mut layers = Vec::with_capacity(layer_tags.len());
    for (index, tag) in layer_tags.iter().enumerate() {
        layers.push(read_layer(&mut zip, index, tag, width, height)?);
    }
    let active = tags(&xml, "layers").first().and_then(|t| t.get("active_pos")).and_then(|v| v.parse().ok()).unwrap_or(layers.len() - 1);
    Ok(Document::from_layers(width, height, paper, surface, layers, active))
}

fn read_layer<R: Read + Seek>(zip: &mut ZipArchive<R>, index: usize, tag: &Attrs, width: usize, height: usize) -> Result<Layer, String> {
    let name = tag.get("name").cloned().unwrap_or_else(|| format!("Layer {}", index + 1));
    let file = tag.get("filename").cloned().unwrap_or_else(|| format!("layer{index}.png"));
    let n = width * height;

    // Colors (straight alpha, sRGB) → premultiplied.
    let mut color = vec![[0.0_f32; 4]; n];
    if let Ok(bytes) = entry(zip, &file) {
        let LayerImage { rgba, width: pw, height: ph, offset: (ox, oy) } = decode_png(&bytes).map_err(|e| format!("layer \"{name}\": {e}"))?;
        for y in 0..ph {
            for x in 0..pw {
                let (cx, cy) = (ox + x as i64, oy + y as i64);
                if !(0..width as i64).contains(&cx) || !(0..height as i64).contains(&cy) {
                    continue;
                }
                let p = &rgba[(y * pw + x) * 4..][..4];
                let a = p[3] as f32 / 255.0;
                let c = |v: u8| v as f32 / 255.0 * a;
                color[cy as usize * width + cx as usize] = [c(p[0]), c(p[1]), c(p[2]), a];
            }
        }
    }

    let stem = file.trim_end_matches(".png");
    let wetness = read_grid(zip, &format!("{stem}_flow.dat"), width, height, FLOW_BORDER)?
        .map(|g| g.into_iter().map(|v| (v / 255.0).clamp(0.0, 1.0)).collect::<Vec<_>>());
    let relief = read_grid(zip, &format!("{stem}_structure.dat"), width, height, 0)?.map(|g| {
        let g = smooth(&g, width, height, RELIEF_BLUR);
        g.into_iter().map(|v| (v * RELIEF_SCALE).min(MAX_RELIEF)).collect::<Vec<_>>()
    });

    // Split each pixel into wet paint over dry paint so it still looks the
    // same: wet = c·w, and dry is whatever makes "wet over dry" equal c.
    let mut canvas = Canvas::new(width, height, Color::TRANSPARENT);
    let (dry, wet, wet_relief) = match &wetness {
        Some(w) => {
            let mut dry = Vec::with_capacity(n);
            let mut wet = Vec::with_capacity(n);
            for (c, &w) in color.iter().zip(w) {
                let w = if c[3] > 0.0 { w } else { 0.0 };
                let wp = c.map(|v| v * w);
                let cover = 1.0 - wp[3];
                let k = if cover > 1e-6 { (1.0 - w) / cover } else { 0.0 };
                wet.push(wp);
                dry.push(c.map(|v| v * k));
            }
            let wet_relief = relief.as_ref().map(|r| r.iter().zip(w).map(|(h, w)| h * w).collect());
            (dry, Some(wet), wet_relief)
        }
        None => (color, None, None),
    };
    canvas.set_pixels(dry, wet);
    if let Some(relief) = relief {
        canvas.set_relief(relief, wet_relief);
    }
    canvas.take_changed();
    canvas.set_preserve_alpha(flag(tag, "locked_alpha"));

    Ok(Layer {
        name,
        canvas,
        visible: tag.get("visibility").is_none_or(|v| v != "0"),
        opacity: tag.get("opacity").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.0, 1.0),
        locked: flag(tag, "locked"),
        tracing: false,
        impasto: None,
        group: None,
        blend: tag.get("blending_mode").map_or(BlendMode::Normal, |m| blend_mode(m)),
        mask: None,
    })
}

/// Read a `.dat` grid as one value per canvas pixel (None if the file is
/// missing). `border` is how much bigger than the canvas the grid is on each side.
fn read_grid<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str, width: usize, height: usize, border: usize) -> Result<Option<Vec<f32>>, String> {
    let Ok(bytes) = entry(zip, name) else { return Ok(None) };
    let bad = || format!("{name} is damaged or in an unknown format");
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let (gw, gh) = (width + 2 * border, height + 2 * border);

    let (x0, y0, bw, bh, body) = match bytes.get(0..4).ok_or_else(bad)? {
        b"ZERO" => return Ok(None),
        b"BBOX" => {
            let v: Vec<usize> = (0..6).map(|k| u32_at(4 + 4 * k)).collect::<Option<_>>().ok_or_else(bad)?;
            let (x0, y0, x1, y1) = (v[0], v[1], v[2], v[3]);
            if x1 < x0 || y1 < y0 {
                return Ok(None);
            }
            (x0, y0, x1 - x0 + 1, y1 - y0 + 1, 28)
        }
        _ => (0, 0, gw, gh, 0),
    };
    let kind = bytes.get(body..body + 4).ok_or_else(bad)?;
    let count = u32_at(body + 4).ok_or_else(bad)?;
    let data = &bytes[body + 8..];
    if count != bw * bh {
        return Err(bad());
    }
    let values: Vec<f32> = match kind {
        b"UCHA" => data.get(..count).ok_or_else(bad)?.iter().map(|&v| v as f32).collect(),
        b"UI_2" => data.get(..count * 2).ok_or_else(bad)?.as_chunks::<2>().0.iter().map(|b| u16::from_be_bytes(*b) as f32).collect(),
        _ => return Err(bad()),
    };

    let mut out = vec![0.0_f32; width * height];
    for by in 0..bh {
        for bx in 0..bw {
            let (gx, gy) = ((x0 + bx) as i64 - border as i64, (y0 + by) as i64 - border as i64);
            if (0..width as i64).contains(&gx) && (0..height as i64).contains(&gy) {
                out[gy as usize * width + gx as usize] = values[by * bw + bx];
            }
        }
    }
    Ok(Some(out))
}

/// A decoded layer PNG: RGBA8 pixels, size, and where its top-left sits on the canvas.
struct LayerImage {
    rgba: Vec<u8>,
    width: usize,
    height: usize,
    offset: (i64, i64),
}

/// Decode a layer PNG to RGBA8, with its "offset" text (top-left on the canvas).
fn decode_png(bytes: &[u8]) -> Result<LayerImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let offset = reader
        .info()
        .uncompressed_latin1_text
        .iter()
        .find(|t| t.keyword == "offset")
        .and_then(|t| {
            let mut it = t.text.split_whitespace().map(|v| v.parse::<i64>().ok());
            Some((it.next()??, it.next()??))
        })
        .unwrap_or((0, 0));
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = w * h;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("unexpected indexed PNG".into()),
    };
    Ok(LayerImage { rgba, width: w, height: h, offset })
}

fn entry<R: Read + Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut file = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(bytes)
}

/// The paper: its name says the kind; texture strength is the depth.
fn paper_settings(tag: &Attrs) -> PaperSettings {
    let name = tag.get("name").map(|n| n.to_lowercase()).unwrap_or_default();
    let kind = [
        ("rough", PaperKind::Rough),
        ("cold", PaperKind::ColdPress),
        ("hot", PaperKind::Smooth),
        ("smooth", PaperKind::Smooth),
        ("handmade", PaperKind::Handmade),
        ("washi", PaperKind::Washi),
        ("linen", PaperKind::Linen),
        ("canvas", PaperKind::Canvas),
        ("panel", PaperKind::Panel),
        ("gesso", PaperKind::Panel),
    ]
    .into_iter()
    .find(|(word, _)| name.contains(word))
    .map_or(PaperKind::ColdPress, |(_, kind)| kind);
    let num = |key: &str| tag.get(key).and_then(|v| v.parse::<f32>().ok());
    PaperSettings {
        kind,
        scale: num("scale").unwrap_or(1.0).clamp(0.25, 4.0),
        depth: num("texture_strength").unwrap_or(0.5).clamp(0.0, 1.0),
        ..PaperSettings::default()
    }
}

/// `#AARRGGBB` (or `#RRGGBB`) → color.
fn parse_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    let rgb = match hex.len() {
        8 => &hex[2..],
        6 => hex,
        _ => return None,
    };
    let v = u32::from_str_radix(rgb, 16).ok()?;
    let c = |shift: u32| ((v >> shift) & 0xff) as f32 / 255.0;
    Some(Color::rgb(c(16), c(8), c(0)))
}

fn blend_mode(name: &str) -> BlendMode {
    match name.to_uppercase().replace([' ', '-', '_'], "").as_str() {
        "MULTIPLY" => BlendMode::Multiply,
        "DARKEN" => BlendMode::Darken,
        "COLORBURN" => BlendMode::ColorBurn,
        "SCREEN" => BlendMode::Screen,
        "LIGHTEN" => BlendMode::Lighten,
        "COLORDODGE" => BlendMode::ColorDodge,
        "OVERLAY" => BlendMode::Overlay,
        "SOFTLIGHT" => BlendMode::SoftLight,
        "HARDLIGHT" => BlendMode::HardLight,
        "DIFFERENCE" => BlendMode::Difference,
        "EXCLUSION" => BlendMode::Exclusion,
        "HUE" => BlendMode::Hue,
        "SATURATION" => BlendMode::Saturation,
        "COLOR" => BlendMode::Color,
        "LUMINOSITY" => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

fn flag(tag: &Attrs, key: &str) -> bool {
    tag.get(key).is_some_and(|v| v == "1")
}

type Attrs = std::collections::HashMap<String, String>;

/// The attributes of every `<name …>` tag in `xml`, in order. The manifest
/// is flat and simple, so a tiny scanner is enough.
fn tags(xml: &str, name: &str) -> Vec<Attrs> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        rest = after;
        // `<layer` must not match `<layers`.
        if !after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let Some(end) = after.find('>') else { break };
        out.push(attributes(&after[..end]));
    }
    out
}

fn attributes(mut s: &str) -> Attrs {
    let mut attrs = Attrs::new();
    while let Some(eq) = s.find('=') {
        let key = s[..eq].trim().to_string();
        let value = s[eq + 1..].trim_start();
        let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') else { break };
        let Some(close) = value[1..].find(quote) else { break };
        attrs.insert(key, unescape(&value[1..1 + close]));
        s = &value[close + 2..];
    }
    attrs
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn png_with_offset(w: u32, h: u32, rgba: &[u8], offset: Option<&str>) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            if let Some(o) = offset {
                enc.add_text_chunk("offset".into(), o.into()).unwrap();
            }
            enc.write_header().unwrap().write_image_data(rgba).unwrap();
        }
        out
    }

    /// `bbox` = x0, y0, x1, y1 (inclusive), grid width, grid height.
    fn bbox_dat(bbox: [u32; 6], kind: &[u8], values: &[u8], count: u32) -> Vec<u8> {
        let mut d = b"BBOX".to_vec();
        for v in bbox {
            d.extend(v.to_be_bytes());
        }
        d.extend(kind);
        d.extend(count.to_be_bytes());
        d.extend(values);
        d
    }

    #[test]
    fn reads_layers_offsets_wetness_and_thickness() {
        let xml = "<?xml version='1.0'?><artwork><canvas width='9.6' height='8'/>\
            <paper name='Some Rough' bg_color='#ff102030' texture_strength='0.3'/>\
            <layers active_pos='1'>\
            <layer width='10' height='8' opacity='0.5' visibility='0' blending_mode='MULTIPLY' filename='layer0.png' name='Back &amp; ground' locked='1'/>\
            <layer width='10' height='8' opacity='1' visibility='1' blending_mode='NORMAL' filename='layer1.png' name='Top'/>\
            </layers></artwork>";
        // Layer 0: a 2×1 red patch at (3, 2).
        let l0 = png_with_offset(2, 1, &[255, 0, 0, 255, 255, 0, 0, 128], Some("3 2"));
        // Layer 1: full size, green, opaque.
        let l1 = png_with_offset(10, 8, &[0, 255, 0, 255].repeat(80), None);
        // Layer 1 wetness: fully wet at canvas (3, 2) and (4, 2) (grid +4 border).
        let flow = bbox_dat([7, 6, 8, 6, 18, 16], b"UCHA", &[255, 0], 2);
        // Layer 1 thickness: 16-bit, 5000 everywhere (→ relief 1 after smoothing).
        let mut st = b"UI_2".to_vec();
        st.extend(80u32.to_be_bytes());
        let vals = [5000u16; 80];
        st.extend(vals.iter().flat_map(|v| v.to_be_bytes()));

        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("artwork.xml", xml.as_bytes()),
            ("layer0.png", &l0),
            ("layer1.png", &l1),
            ("layer1_flow.dat", &flow),
            ("layer1_structure.dat", &st),
            ("layer0_flow.dat", b"ZERO\0\0\0\x50"),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let doc = read_reb(Cursor::new(bytes)).unwrap();

        assert_eq!((doc.width(), doc.height()), (10, 8));
        assert_eq!(doc.active_index(), 1);
        let back = &doc.layers()[0];
        assert_eq!(back.name, "Back & ground");
        assert!(!back.visible && back.locked);
        assert_eq!((back.opacity, back.blend), (0.5, BlendMode::Multiply));
        let px = back.canvas.dry_pixels();
        assert_eq!(px[2 * 10 + 3], [1.0, 0.0, 0.0, 1.0]);
        assert!((px[2 * 10 + 4][3] - 128.0 / 255.0).abs() < 1e-6 && px[2 * 10 + 4][0] == px[2 * 10 + 4][3], "premultiplied");
        assert_eq!(px[0][3], 0.0, "outside the patch stays empty");
        assert!(!back.canvas.has_wet_paint());

        let top = &doc.layers()[1];
        let i = 2 * 10 + 3;
        assert_eq!(top.canvas.wet_pixels()[i], [0.0, 1.0, 0.0, 1.0], "wet pixel is all wet");
        assert_eq!(top.canvas.wet_pixels()[i + 1][3], 0.0, "dry neighbor");
        assert_eq!(top.canvas.combined(i + 1), [0.0, 1.0, 0.0, 1.0]);
        assert!((top.canvas.relief()[i] - 1.0).abs() < 1e-4);
        assert!((top.canvas.wet_relief()[i] - 1.0).abs() < 1e-4, "wet paint's thickness is wet");
        assert_eq!(top.canvas.wet_relief()[i + 1], 0.0, "dry paint's thickness is dry");

        assert_eq!(doc.paper(), Color::rgb(0x10 as f32 / 255.0, 0x20 as f32 / 255.0, 0x30 as f32 / 255.0));
        assert_eq!(doc.surface().settings().kind, PaperKind::Rough);
    }

    #[test]
    fn smoothing_keeps_flat_areas_and_softens_steps() {
        let (w, h) = (40, 3);
        let step: Vec<f32> = (0..w * h).map(|i| if i % w < 20 { 0.0 } else { 100.0 }).collect();
        let soft = smooth(&step, w, h, 2);
        assert!((soft[0]).abs() < 1e-4 && (soft[w - 1] - 100.0).abs() < 1e-4, "far from the step: unchanged");
        assert!(soft[19] > 0.0 && soft[20] < 100.0, "the step is softened");
        let total = |g: &[f32]| g.iter().sum::<f32>();
        assert!((total(&soft) - total(&step)).abs() < 1e-2, "smoothing keeps the amount of paint");
    }

    #[test]
    fn partly_wet_paint_looks_the_same() {
        let xml = "<artwork><canvas width='1' height='1'/><layers><layer width='1' height='1' filename='layer0.png' name='a'/></layers></artwork>";
        let l0 = png_with_offset(1, 1, &[200, 100, 50, 180], None);
        let mut flow = b"UCHA".to_vec();
        flow.extend(81u32.to_be_bytes()); // 9×9 grid (1 + 2·4 border)
        let mut f = vec![0u8; 81];
        f[4 * 9 + 4] = 128;
        flow.extend(f);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [("artwork.xml", xml.as_bytes()), ("layer0.png", &l0), ("layer0_flow.dat", &flow)] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let doc = read_reb(Cursor::new(zip.finish().unwrap().into_inner())).unwrap();
        let c = doc.layers()[0].canvas.combined(0);
        let a = 180.0 / 255.0;
        let want = [200.0 / 255.0 * a, 100.0 / 255.0 * a, 50.0 / 255.0 * a, a];
        for k in 0..4 {
            assert!((c[k] - want[k]).abs() < 1e-5, "{c:?} vs {want:?}");
        }
        assert!(doc.layers()[0].canvas.has_wet_paint());
    }
}
