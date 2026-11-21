//! Mixing palette helpers shared by the app and AI agents: a fresh palette,
//! hex colors, squeezing a swatch of paint onto the palette, notes.

use oxide_engine::{Brush, Color, DabMode, Document, Stroke};

/// Palette surface size in pixels.
pub const PALETTE_SIZE: (usize, usize) = (640, 400);
/// A clean, slightly warm white, like a real palette.
pub const PALETTE_COLOR: Color = Color::rgb(0.97, 0.96, 0.93);

/// A fresh, empty palette.
pub fn new_palette() -> Document {
    Document::new(PALETTE_SIZE.0, PALETTE_SIZE.1, PALETTE_COLOR)
}

/// Read a hex color: "#3A5F8B", "3a5f8b", or short "#38B".
pub fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    let (r, g, b) = match digits.as_slice() {
        [r, g, b] => (r * 17, g * 17, b * 17),
        [r1, r2, g1, g2, b1, b2] => (r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2),
        _ => return None,
    };
    Some(Color::rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
}

/// "#1C2A6B" for a color.
pub fn to_hex(c: Color) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", byte(c.r), byte(c.g), byte(c.b))
}

/// Add a line to the notes (on a new line, skipping exact duplicates).
pub fn add_note_line(notes: &mut String, line: &str) {
    if notes.lines().any(|l| l.trim() == line) {
        return;
    }
    if !notes.is_empty() && !notes.ends_with('\n') {
        notes.push('\n');
    }
    notes.push_str(line);
}

/// Squeeze a blob of wet paint onto the palette in the first empty spot
/// (scanning a grid from the top-left), as one undo step.
pub fn add_paint_swatch(palette: &mut Document, color: Color, natural: bool) {
    const RADIUS: f32 = 20.0;
    const GAP: f32 = 54.0;
    let (w, h) = (palette.width() as f32, palette.height() as f32);
    let empty_at = |x: f32, y: f32| {
        // Empty if the paint layer is (nearly) clear around this spot.
        let canvas = &palette.layers()[palette.active_index()].canvas;
        [(0.0, 0.0), (-RADIUS * 0.7, 0.0), (RADIUS * 0.7, 0.0), (0.0, -RADIUS * 0.7), (0.0, RADIUS * 0.7)]
            .iter()
            .all(|(dx, dy)| canvas.pixel((x + dx) as usize, (y + dy) as usize).is_none_or(|p| p.a < 0.05))
    };
    let spots = (0..)
        .map(|i| i as f32)
        .take_while(|row| 34.0 + row * GAP < h - RADIUS)
        .flat_map(|row| (0..).map(move |col| (34.0 + col as f32 * GAP, 34.0 + row * GAP)).take_while(move |(x, _)| *x < w - RADIUS));
    let (x, y) = spots.clone().find(|(x, y)| empty_at(*x, *y)).unwrap_or((w / 2.0, h / 2.0));

    let brush = Brush {
        color,
        radius: RADIUS,
        hardness: 0.85,
        roughness: 0.3,
        loading: 1.0,
        oiliness: 0.0001, // wet paint, but it doesn't pick anything up
        spacing: 0.05,
        mixing: if natural { oxide_engine::Mixing::Pigment } else { oxide_engine::Mixing::Rgb },
        ..Brush::default()
    };
    palette.begin_stroke();
    // A short little smear so it looks like a dab of paint, not a stamp.
    let mut stroke = Stroke::begin(palette.active_canvas(), &brush, DabMode::Paint, x - 4.0, y);
    stroke.line_to(palette.active_canvas(), &brush, x + 4.0, y + 2.0);
    stroke.finish(palette.active_canvas(), &brush);
    palette.end_stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_values_parse() {
        assert_eq!(parse_hex("#ff0000"), Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(parse_hex("  00FF00 "), Some(Color::rgb(0.0, 1.0, 0.0)));
        assert_eq!(parse_hex("#00f"), Some(Color::rgb(0.0, 0.0, 1.0)));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("blue"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn named_paints_are_listed_once() {
        let mut notes = String::from("My shadow mix");
        add_note_line(&mut notes, "• Ultramarine Blue (#1C2A6B)");
        add_note_line(&mut notes, "• Burnt Umber (#3D2B1F)");
        add_note_line(&mut notes, "• Ultramarine Blue (#1C2A6B)");
        assert_eq!(notes, "My shadow mix\n• Ultramarine Blue (#1C2A6B)\n• Burnt Umber (#3D2B1F)");
        assert_eq!(to_hex(Color::rgb(1.0, 0.5, 0.0)), "#FF8000");
    }

    #[test]
    fn swatches_land_in_empty_spots_and_are_wet() {
        let mut palette = new_palette();
        let red = Color::rgb(0.86, 0.13, 0.12);
        add_paint_swatch(&mut palette, red, true);
        add_paint_swatch(&mut palette, Color::rgb(0.16, 0.2, 0.62), true);
        let canvas = &palette.layers()[0].canvas;
        assert!(canvas.has_wet_paint());
        // First swatch at the first grid spot, second one next to it (not on top).
        let first = canvas.pixel(34, 34).unwrap();
        let second = canvas.pixel(88, 34).unwrap();
        assert!(first.r > 0.7 && first.b < 0.3, "red swatch: {first:?}");
        assert!(second.b > 0.5 && second.r < 0.3, "blue swatch next to it: {second:?}");
        assert_eq!(palette.undo_description(), Some("Paint"));
    }
}
