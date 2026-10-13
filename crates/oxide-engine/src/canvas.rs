use std::sync::Arc;

use crate::paper::{Paper, surface_acceptance};
use crate::tip::DabTexture;
use crate::{Brush, Region};

/// An RGBA color with each channel in 0.0..=1.0 (not premultiplied).
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
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }
}

/// What a dab does to the pixels underneath.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DabMode {
    /// Lay the brush color on top.
    Paint,
    /// Remove paint (make pixels more transparent).
    Erase,
}

/// What a dab does to the *wetness* of the paint it lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wetness {
    /// Wet media (oils & acrylics): the paint it leaves is wet.
    Wet,
    /// Dry media (ink, pencil, ...): covers wet paint with dry paint.
    Dry,
    /// Leave wetness alone (smudging just moves paint around).
    Keep,
}

/// How a single dab is applied (beyond the brush's shape).
#[derive(Clone, Copy, Debug)]
pub struct DabStyle<'a> {
    pub color: Color,
    pub opacity: f32,
    pub mode: DabMode,
    pub wetness: Wetness,
    /// A natural, imperfect tip (ragged edges, streaks). `None` = perfect tip.
    pub texture: Option<&'a DabTexture>,
    /// How deep into the paper's valleys the paint reaches (0..=1).
    /// 1 = everywhere (loaded brush); low = only the peaks (dry, light touch).
    pub reach: f32,
}

/// A grid of pixels we paint into: one layer of a painting.
///
/// Pixels are stored as 32-bit floats, **premultiplied** by alpha
/// (a half-transparent red is stored as [0.5, 0, 0, 0.5]). Floats mean very
/// light strokes (airbrush at 5%) build up smoothly instead of getting
/// stuck on 8-bit rounding, and premultiplied alpha makes blending a simple
/// `src + dst * (1 - src_alpha)`.
///
/// Each layer has **two buffers** wet layer:
/// - **dry** paint underneath: locked in, wet brushes glide over it
/// - **wet** paint on top: wet brushes pick it up, blend and smear it
///
/// What you see is wet over dry. "Dry layer" merges wet into dry.
pub struct Canvas {
    width: usize,
    height: usize,
    dry: Vec<[f32; 4]>,
    wet: Vec<[f32; 4]>,
    /// The area changed since the last [`Canvas::take_dirty`] (for the screen).
    dirty: Option<Region>,
    /// The area changed since the last [`Canvas::take_changed`] (for undo).
    changed: Option<Region>,
    /// The paper this layer is painted on (shared by all layers of a document).
    surface: Option<Arc<Paper>>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: Color) -> Self {
        let mut canvas = Canvas {
            width,
            height,
            dry: vec![[0.0; 4]; width * height],
            wet: vec![[0.0; 4]; width * height],
            dirty: None,
            changed: None,
            surface: None,
        };
        canvas.clear(background);
        canvas
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Paint on this paper from now on (its texture shapes where paint lands).
    pub fn set_surface(&mut self, surface: Arc<Paper>) {
        self.surface = Some(surface);
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The dry paint (premultiplied RGBA), row by row.
    pub fn dry_pixels(&self) -> &[[f32; 4]] {
        &self.dry
    }

    /// The wet paint on top (premultiplied RGBA), row by row.
    pub fn wet_pixels(&self) -> &[[f32; 4]] {
        &self.wet
    }

    /// What pixel `i` looks like: wet paint over dry paint (premultiplied).
    pub fn combined(&self, i: usize) -> [f32; 4] {
        over(self.wet[i], self.dry[i])
    }

    /// The area covering the whole canvas.
    pub fn bounds(&self) -> Region {
        Region::new(0, 0, self.width, self.height)
    }

    /// The color of one pixel (wet over dry), or `None` if (x, y) is off the canvas.
    pub fn pixel(&self, x: usize, y: usize) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(unpremultiply(self.combined(y * self.width + x)))
    }

    /// How wet pixel (x, y) is: the opacity of its wet paint, 0..=1.
    pub fn wetness_at(&self, x: usize, y: usize) -> f32 {
        self.wet[y * self.width + x][3]
    }

    /// Is there any wet paint on this layer?
    pub fn has_wet_paint(&self) -> bool {
        self.wet.iter().any(|p| p[3] > 0.0)
    }

    pub fn clear(&mut self, color: Color) {
        self.dry.fill(premultiply(color));
        self.wet.fill([0.0; 4]);
        self.mark_changed(self.bounds());
    }

    /// Take (and reset) the area that changed since the last call.
    pub fn take_dirty(&mut self) -> Option<Region> {
        self.dirty.take()
    }

    /// Mark an area as needing to be redrawn on screen.
    pub fn mark_dirty(&mut self, region: Region) {
        self.dirty = Some(union(self.dirty, region));
    }

    /// Take (and reset) the area whose pixels changed since the last call.
    /// Undo uses this to know what a stroke touched.
    pub fn take_changed(&mut self) -> Option<Region> {
        self.changed.take()
    }

    fn mark_changed(&mut self, region: Region) {
        self.changed = Some(union(self.changed, region));
        self.mark_dirty(region);
    }

    /// Copy the (dry, wet) pixels in `region` out, row by row.
    pub fn read_region(&self, region: Region) -> (Vec<[f32; 4]>, Vec<[f32; 4]>) {
        (read_rows(&self.dry, self.width, region), read_rows(&self.wet, self.width, region))
    }

    /// Put pixels (as returned by [`Canvas::read_region`]) back into `region`.
    pub fn write_region(&mut self, region: Region, dry: &[[f32; 4]], wet: &[[f32; 4]]) {
        write_rows(&mut self.dry, self.width, region, dry);
        write_rows(&mut self.wet, self.width, region, wet);
        self.mark_changed(region);
    }

    /// Replace every pixel at once (used when loading a file).
    pub fn set_pixels(&mut self, dry: Vec<[f32; 4]>, wet: Option<Vec<[f32; 4]>>) {
        let n = self.width * self.height;
        assert_eq!(dry.len(), n, "pixel count must match canvas size");
        self.dry = dry;
        self.wet = wet.unwrap_or_else(|| vec![[0.0; 4]; n]);
        assert_eq!(self.wet.len(), n, "wet pixel count must match canvas size");
        self.mark_changed(self.bounds());
    }

    /// Swap both buffers with the given ones (drying, rewetting, and their undo).
    pub fn swap_buffers(&mut self, dry: &mut Vec<[f32; 4]>, wet: &mut Vec<[f32; 4]>) {
        assert_eq!(dry.len(), self.dry.len());
        assert_eq!(wet.len(), self.wet.len());
        std::mem::swap(&mut self.dry, dry);
        std::mem::swap(&mut self.wet, wet);
        self.mark_dirty(self.bounds());
    }

    /// The pixels a dab centered at (cx, cy) can touch, clamped to the canvas.
    fn dab_bounds(&self, cx: f32, cy: f32, brush: &Brush) -> Option<Region> {
        let r = brush.radius.max(0.5);
        let x0 = ((cx - r).floor().max(0.0)) as usize;
        let y0 = ((cy - r).floor().max(0.0)) as usize;
        let x1 = ((cx + r).ceil().max(0.0) as usize).min(self.width);
        let y1 = ((cy + r).ceil().max(0.0) as usize).min(self.height);
        let region = Region::new(x0, y0, x1, y1);
        (!region.is_empty()).then_some(region)
    }

    /// Stamp one brush mark (a "dab") centered at (cx, cy) with dry paint.
    ///
    /// Strokes are made of many overlapping dabs; see [`crate::Stroke`].
    pub fn dab(&mut self, cx: f32, cy: f32, brush: &Brush, mode: DabMode) {
        let style =
            DabStyle { color: brush.color, opacity: brush.opacity, mode, wetness: Wetness::Dry, texture: None, reach: 1.0 };
        self.dab_with(cx, cy, brush, &style);
    }

    /// Like [`Canvas::dab`], but with a specific color, opacity, wetness, and
    /// tip texture (wet strokes change these from dab to dab).
    pub fn dab_with(&mut self, cx: f32, cy: f32, brush: &Brush, style: &DabStyle) {
        let Some(Region { x0, y0, x1, y1 }) = self.dab_bounds(cx, cy, brush) else { return };
        let src = premultiply(style.color);
        // Only consult the paper if this brush feels it.
        let paper = self.surface.clone().filter(|_| brush.grain > 0.0 && style.mode == DabMode::Paint);
        for y in y0..y1 {
            for x in x0..x1 {
                // Offset from the dab center to this pixel's center.
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                let coverage = match style.texture {
                    Some(tex) => brush.textured_coverage(dx, dy, x, y, tex),
                    None => brush.coverage(dx, dy),
                };
                if coverage <= 0.0 {
                    continue;
                }
                let mut strength = (coverage * style.opacity).clamp(0.0, 1.0);
                if let Some(paper) = &paper {
                    strength *= surface_acceptance(paper.height_at(x, y), brush.grain, style.reach);
                }
                let i = y * self.width + x;
                let paint = scale(src, strength);
                match style.mode {
                    DabMode::Paint => match style.wetness {
                        // Wet paint goes into the wet buffer, on top.
                        Wetness::Wet => self.wet[i] = over(paint, self.wet[i]),
                        // Dry paint on top of wet paint: whatever wet paint is
                        // here dries underneath it, then the dry paint covers it.
                        Wetness::Dry => {
                            if self.wet[i][3] > 0.0 {
                                self.dry[i] = over(self.wet[i], self.dry[i]);
                                self.wet[i] = [0.0; 4];
                            }
                            self.dry[i] = over(paint, self.dry[i]);
                        }
                        // Smudging: moved paint stays wet where it's wet, dry where it's dry.
                        Wetness::Keep => {
                            if self.wet[i][3] > 0.01 {
                                self.wet[i] = over(paint, self.wet[i]);
                            } else {
                                self.dry[i] = over(paint, self.dry[i]);
                            }
                        }
                    },
                    DabMode::Erase => {
                        self.dry[i] = scale(self.dry[i], 1.0 - strength);
                        self.wet[i] = scale(self.wet[i], 1.0 - strength);
                    }
                }
            }
        }
        self.mark_changed(Region::new(x0, y0, x1, y1));
    }

    /// The average paint under a dab, weighted by the brush shape
    /// (premultiplied RGBA). This is what a wet brush "picks up".
    ///
    /// With `include_dry` false, only the *wet* paint counts: dry paint is
    /// locked in. With it true (the Smudge tool), everything counts.
    pub fn sample_under(&self, cx: f32, cy: f32, brush: &Brush, include_dry: bool) -> [f32; 4] {
        let Some(Region { x0, y0, x1, y1 }) = self.dab_bounds(cx, cy, brush) else { return [0.0; 4] };
        let mut sum = [0.0_f32; 4];
        let mut weight = 0.0;
        for y in y0..y1 {
            for x in x0..x1 {
                let w = brush.coverage(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if w > 0.0 {
                    let i = y * self.width + x;
                    let p = if include_dry { self.combined(i) } else { self.wet[i] };
                    for c in 0..4 {
                        sum[c] += p[c] * w;
                    }
                    weight += w;
                }
            }
        }
        if weight > 0.0 { scale(sum, 1.0 / weight) } else { [0.0; 4] }
    }
}

fn read_rows<T: Copy>(buffer: &[T], width: usize, region: Region) -> Vec<T> {
    let mut out = Vec::with_capacity(region.width() * region.height());
    for y in region.y0..region.y1 {
        let row = y * width;
        out.extend_from_slice(&buffer[row + region.x0..row + region.x1]);
    }
    out
}

fn write_rows<T: Copy>(buffer: &mut [T], width: usize, region: Region, data: &[T]) {
    let w = region.width();
    for (i, y) in (region.y0..region.y1).enumerate() {
        let row = y * width;
        buffer[row + region.x0..row + region.x1].copy_from_slice(&data[i * w..(i + 1) * w]);
    }
}

fn union(existing: Option<Region>, region: Region) -> Region {
    match existing {
        Some(d) => d.union(region),
        None => region,
    }
}

/// "Source over": put `src` on top of `dst` (both premultiplied).
pub(crate) fn over(src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let k = 1.0 - src[3];
    [src[0] + dst[0] * k, src[1] + dst[1] * k, src[2] + dst[2] * k, src[3] + dst[3] * k]
}

pub(crate) fn scale(p: [f32; 4], s: f32) -> [f32; 4] {
    [p[0] * s, p[1] * s, p[2] * s, p[3] * s]
}

pub(crate) fn premultiply(c: Color) -> [f32; 4] {
    [c.r * c.a, c.g * c.a, c.b * c.a, c.a]
}

pub(crate) fn unpremultiply(p: [f32; 4]) -> Color {
    if p[3] <= 0.0 {
        return Color::TRANSPARENT;
    }
    Color { r: p[0] / p[3], g: p[1] / p[3], b: p[2] / p[3], a: p[3] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 1e-4 && (a.g - b.g).abs() < 1e-4 && (a.b - b.b).abs() < 1e-4 && (a.a - b.a).abs() < 1e-4
    }

    #[test]
    fn new_canvas_is_filled_with_background() {
        let canvas = Canvas::new(4, 3, Color::WHITE);
        assert_eq!(canvas.dry_pixels().len(), 4 * 3);
        assert_eq!(canvas.pixel(3, 2), Some(Color::WHITE));
    }

    #[test]
    fn dab_paints_center_but_not_far_away() {
        let mut canvas = Canvas::new(50, 50, Color::WHITE);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        canvas.dab(25.0, 25.0, &brush, DabMode::Paint);

        assert_eq!(canvas.pixel(25, 25), Some(Color::BLACK));
        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn dab_off_the_edge_does_not_panic() {
        let mut canvas = Canvas::new(10, 10, Color::WHITE);
        let brush = Brush { radius: 8.0, ..Brush::default() };
        canvas.dab(-3.0, 12.0, &brush, DabMode::Paint);
        canvas.dab(100.0, 100.0, &brush, DabMode::Paint);
    }

    #[test]
    fn soft_edge_on_transparent_layer_keeps_its_color() {
        // On a transparent layer, a faint red dab must be faint *red*, not dark red.
        let mut canvas = Canvas::new(10, 10, Color::TRANSPARENT);
        let brush = Brush { color: Color::rgb(1.0, 0.0, 0.0), opacity: 0.3, radius: 4.0, ..Brush::default() };
        canvas.dab(5.0, 5.0, &brush, DabMode::Paint);
        let p = canvas.pixel(5, 5).unwrap();
        assert!(close(p, Color { r: 1.0, g: 0.0, b: 0.0, a: 0.3 }), "{p:?}");
    }

    #[test]
    fn light_strokes_build_up() {
        // 100 dabs at 2% should get close to full coverage (8-bit storage would stall).
        let mut canvas = Canvas::new(10, 10, Color::TRANSPARENT);
        let brush = Brush { opacity: 0.02, radius: 4.0, ..Brush::default() };
        for _ in 0..100 {
            canvas.dab(5.0, 5.0, &brush, DabMode::Paint);
        }
        let a = canvas.pixel(5, 5).unwrap().a;
        assert!(a > 0.85, "alpha only reached {a}");
    }

    #[test]
    fn erase_removes_paint() {
        let mut canvas = Canvas::new(10, 10, Color::BLACK);
        let brush = Brush { radius: 4.0, ..Brush::default() };
        canvas.dab(5.0, 5.0, &brush, DabMode::Erase);
        assert_eq!(canvas.pixel(5, 5).unwrap().a, 0.0);
        assert_eq!(canvas.pixel(0, 0), Some(Color::BLACK));
    }

    #[test]
    fn read_and_write_region_round_trip() {
        let mut canvas = Canvas::new(10, 10, Color::WHITE);
        let region = Region::new(2, 3, 6, 8);
        let saved = canvas.read_region(region);
        assert_eq!(saved.0.len(), 4 * 5);
        canvas.dab(4.0, 5.0, &Brush { radius: 2.0, ..Brush::default() }, DabMode::Paint);
        assert_eq!(canvas.pixel(4, 5), Some(Color::BLACK));
        canvas.write_region(region, &saved.0, &saved.1);
        assert_eq!(canvas.pixel(4, 5), Some(Color::WHITE));
    }

    #[test]
    fn wet_paint_sits_on_top_and_only_it_is_picked_up() {
        let mut canvas = Canvas::new(20, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        // Dry red underneath, wet blue on top.
        canvas.dab(10.0, 10.0, &Brush { color: Color::rgb(1.0, 0.0, 0.0), ..brush.clone() }, DabMode::Paint);
        let wet_blue = DabStyle { color: Color::rgb(0.0, 0.0, 1.0), opacity: 0.5, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0 };
        canvas.dab_with(10.0, 10.0, &brush, &wet_blue);
        assert!(canvas.has_wet_paint());

        // A wet brush picks up only the blue...
        let picked = unpremultiply(canvas.sample_under(10.0, 10.0, &brush, false));
        assert!(picked.b > 0.99 && picked.r < 0.01, "{picked:?}");
        // ...the Smudge tool sees both.
        let all = unpremultiply(canvas.sample_under(10.0, 10.0, &brush, true));
        assert!(all.r > 0.3 && all.b > 0.3, "{all:?}");
    }

    #[test]
    fn dry_media_dries_the_wet_paint_it_covers() {
        let mut canvas = Canvas::new(20, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        let wet = DabStyle { color: Color::BLACK, opacity: 1.0, mode: DabMode::Paint, wetness: Wetness::Wet, texture: None, reach: 1.0 };
        canvas.dab_with(10.0, 10.0, &brush, &wet);
        canvas.dab(10.0, 10.0, &Brush { color: Color::WHITE, ..brush }, DabMode::Paint);
        assert_eq!(canvas.wetness_at(10, 10), 0.0);
        assert_eq!(canvas.pixel(10, 10), Some(Color::WHITE));
    }

    #[test]
    fn dirty_region_covers_the_dab() {
        let mut canvas = Canvas::new(50, 50, Color::WHITE);
        canvas.take_dirty();
        canvas.dab(25.0, 25.0, &Brush { radius: 5.0, ..Brush::default() }, DabMode::Paint);
        let d = canvas.take_dirty().unwrap();
        assert!(d.x0 <= 20 && d.x1 >= 30 && d.y0 <= 20 && d.y1 >= 30, "{d:?}");
        assert!(canvas.take_dirty().is_none());
    }
}
