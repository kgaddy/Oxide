use crate::Brush;

/// An RGBA color with each channel in 0.0..=1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }
}

/// A grid of pixels we paint into.
///
/// Pixels are stored row by row as 8-bit RGBA, which is the format the
/// screen (and egui) wants. Later milestones will add more data per pixel
/// (wetness, paint height for impasto, ...).
pub struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: Color) -> Self {
        let mut canvas = Canvas {
            width,
            height,
            pixels: vec![0; width * height * 4],
        };
        canvas.clear(background);
        canvas
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Raw RGBA bytes, 4 per pixel, row by row.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The color of one pixel, or `None` if (x, y) is off the canvas.
    pub fn pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) * 4;
        let p = &self.pixels[i..i + 4];
        Some(Color {
            r: p[0] as f32 / 255.0,
            g: p[1] as f32 / 255.0,
            b: p[2] as f32 / 255.0,
            a: p[3] as f32 / 255.0,
        })
    }

    pub fn clear(&mut self, color: Color) {
        let rgba = [to_u8(color.r), to_u8(color.g), to_u8(color.b), to_u8(color.a)];
        for pixel in self.pixels.as_chunks_mut::<4>().0 {
            *pixel = rgba;
        }
    }

    /// Stamp one round brush mark (a "dab") centered at (cx, cy).
    ///
    /// Strokes are made of many overlapping dabs; see [`crate::Stroke`].
    pub fn dab(&mut self, cx: f32, cy: f32, brush: &Brush) {
        let r = brush.radius.max(0.5);
        // Only visit pixels inside the dab's bounding box, clamped to the canvas.
        let x0 = ((cx - r).floor().max(0.0)) as usize;
        let y0 = ((cy - r).floor().max(0.0)) as usize;
        let x1 = ((cx + r).ceil().max(0.0) as usize).min(self.width);
        let y1 = ((cy + r).ceil().max(0.0) as usize).min(self.height);

        for y in y0..y1 {
            for x in x0..x1 {
                // Distance from the dab center to this pixel's center.
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let dist = (dx * dx + dy * dy).sqrt();

                let coverage = brush.coverage(dist);
                if coverage > 0.0 {
                    self.blend(x, y, brush.color, coverage * brush.opacity);
                }
            }
        }
    }

    /// Paint `color` over the pixel at (x, y) with the given strength (0..=1).
    /// This is standard "source over" alpha blending.
    fn blend(&mut self, x: usize, y: usize, color: Color, strength: f32) {
        let i = (y * self.width + x) * 4;
        let p = &mut self.pixels[i..i + 4];
        let a = (strength * color.a).clamp(0.0, 1.0);
        p[0] = mix(p[0], color.r, a);
        p[1] = mix(p[1], color.g, a);
        p[2] = mix(p[2], color.b, a);
        p[3] = to_u8(p[3] as f32 / 255.0 * (1.0 - a) + a);
    }
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Linear interpolation from an existing 8-bit channel toward a new value.
fn mix(old: u8, new: f32, amount: f32) -> u8 {
    to_u8(old as f32 / 255.0 * (1.0 - amount) + new * amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_canvas_is_filled_with_background() {
        let canvas = Canvas::new(4, 3, Color::WHITE);
        assert_eq!(canvas.pixels().len(), 4 * 3 * 4);
        assert!(canvas.pixels().iter().all(|&b| b == 255));
    }

    #[test]
    fn dab_paints_center_but_not_far_away() {
        let mut canvas = Canvas::new(50, 50, Color::WHITE);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        canvas.dab(25.0, 25.0, &brush);

        assert_eq!(canvas.pixel(25, 25), Some(Color::BLACK));
        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn dab_off_the_edge_does_not_panic() {
        let mut canvas = Canvas::new(10, 10, Color::WHITE);
        let brush = Brush { radius: 8.0, ..Brush::default() };
        canvas.dab(-3.0, 12.0, &brush);
        canvas.dab(100.0, 100.0, &brush);
    }
}
