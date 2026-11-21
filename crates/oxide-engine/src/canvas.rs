use std::sync::Arc;

use crate::paper::{Paper, surface_acceptance};
use crate::spectral::{Pigment, mix_pigments};
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
    /// The paint color prepared for pigment mixing. When set, wet paint landing
    /// on *different* wet paint mixes like real pigment instead of blending
    /// like light (ultramarine + burnt umber → near-black, not purple).
    pub pigment: Option<&'a Pigment>,
    /// Paint thickness change (impasto), for wet media. None = flat media.
    pub relief: Option<ReliefDab>,
    /// MultiColored brush: colors in bands across the tip (and, for pigment
    /// mixing, each band's pigment). None = one color (`color`).
    pub bands: Option<Bands<'a>>,
    /// Build up instead of stacking (wet paint): `opacity` is how solid the
    /// finished stroke is, and each dab lays down this share of it (about
    /// 1 / how many dabs overlap a pixel). Anywhere the brush covers fairly
    /// well builds up to the full opacity; only the soft rim stays lighter.
    /// None = each dab paints `coverage × opacity`.
    pub flow: Option<Flow>,
}

/// How a wet brush builds up its stroke (see [`DabStyle::flow`]).
#[derive(Clone, Copy, Debug)]
pub struct Flow {
    /// The share of the stroke's opacity each dab lays down.
    pub share: f32,
    /// How solid the brush is by its settings (loading × opacity, before pen
    /// pressure), 0..=1. A firm brush covers dab by dab instead of building
    /// up in thin layers, which wet paint underneath would mix into.
    pub firm: f32,
}

/// A grid of pixels we paint into: one layer of a painting.
///
/// Pixels are stored as 32-bit floats, **premultiplied** by alpha
/// (a half-transparent red is stored as [0.5, 0, 0, 0.5]). Floats mean very
/// light strokes (airbrush at 5%) build up smoothly instead of getting
/// stuck on 8-bit rounding, and premultiplied alpha makes blending a simple
/// `src + dst * (1 - src_alpha)`.
///
/// Each layer has **two buffers**, a wet layer over a dry one:
/// - **dry** paint underneath: locked in, wet brushes glide over it
/// - **wet** paint on top: wet brushes pick it up, blend and smear it
///
/// The tallest paint can pile up on one layer (relief units: 1 = a thick,
/// loaded stroke).
pub const MAX_RELIEF: f32 = 2.0;

/// A layer's full contents: dry paint, wet paint (premultiplied RGBA) and
/// relief (paint thickness), whole-canvas or for one region.
#[derive(Clone, Debug, Default)]
pub struct Buffers {
    pub dry: Vec<[f32; 4]>,
    pub wet: Vec<[f32; 4]>,
    pub relief: Vec<f32>,
    /// How much of `relief` is still wet (movable) paint.
    pub wet_relief: Vec<f32>,
}

impl Buffers {
    /// Nothing painted, `n` pixels.
    pub fn empty(n: usize) -> Self {
        Buffers { dry: vec![[0.0; 4]; n], wet: vec![[0.0; 4]; n], relief: vec![0.0; n], wet_relief: vec![0.0; n] }
    }

    pub fn bytes(&self) -> usize {
        (self.dry.len() + self.wet.len()) * 16 + (self.relief.len() + self.wet_relief.len()) * 4
    }
}

/// How a dab changes the paint's thickness (impasto). Only wet media have one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReliefDab {
    /// Fresh paint piles up to this thickness (times the tip's coverage, so
    /// bristle streaks leave ridges).
    pub build: f32,
    /// Paint dragged by the brush levels the surface toward this thickness...
    pub smear_to: f32,
    /// ...this strongly (0 = not at all).
    pub smear: f32,
    /// How hard the brush plows through *wet* thick paint: it scrapes paint
    /// from under its middle (0 = glides over it)...
    pub plow: f32,
    /// ...and shoves this much up into ridges along its sides.
    pub push: f32,
    /// A knife: fresh paint is flat-topped (no grooves) with a crisp edge.
    pub flat: bool,
    /// A knife scraping: how much of the *wet* paint under it comes off (0..=1).
    pub scrape: f32,
    /// Flat paint: level the surface under the brush, wet and dried paint
    /// alike, instead of building on it.
    pub level: bool,
}

/// MultiColored brush: the colors loaded across the tip for one dab.
#[derive(Clone, Copy, Debug)]
pub struct Bands<'a> {
    pub colors: &'a [Color],
    pub pigments: Option<&'a [Pigment]>,
}

/// What you see is wet over dry. "Dry layer" merges wet into dry.
#[derive(Clone)]
pub struct Canvas {
    width: usize,
    height: usize,
    dry: Vec<[f32; 4]>,
    wet: Vec<[f32; 4]>,
    /// Paint thickness (impasto), 0 = flat. Lit when the painting is shown.
    relief: Vec<f32>,
    /// The part of `relief` that is still wet: the next stroke can move it.
    /// Dried thickness stays put, and new paint piles on top of it.
    wet_relief: Vec<f32>,
    /// Might any pixel have relief? (Exact after whole-layer changes; set as
    /// soon as paint builds up. Lets flat paintings skip impasto lighting.)
    has_relief: bool,
    /// How many pixels have any wet paint. Kept up to date as we paint, so
    /// asking "is anything wet?" is instant instead of scanning millions of
    /// pixels (the UI asks every frame).
    wet_count: usize,
    /// The area changed since the last [`Canvas::take_dirty`] (for the screen).
    dirty: Option<Region>,
    /// The area changed since the last [`Canvas::take_changed`] (for undo).
    changed: Option<Region>,
    /// The paper this layer is painted on (shared by all layers of a document).
    surface: Option<Arc<Paper>>,
    /// Lock transparency: paint only where there's already paint (recolor
    /// existing strokes); transparent areas stay transparent.
    preserve_alpha: bool,
    /// The selection while a stroke is painted (0..=1 per pixel): dabs only
    /// work inside it. None = no selection.
    clip: Option<Arc<Vec<f32>>>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, background: Color) -> Self {
        let mut canvas = Canvas {
            width,
            height,
            dry: vec![[0.0; 4]; width * height],
            wet: vec![[0.0; 4]; width * height],
            relief: vec![0.0; width * height],
            wet_relief: vec![0.0; width * height],
            has_relief: false,
            wet_count: 0,
            dirty: None,
            changed: None,
            surface: None,
            preserve_alpha: false,
            clip: None,
        };
        canvas.clear(background);
        canvas
    }

    pub fn width(&self) -> usize {
        self.width
    }

    /// Limit painting to a selection (None = everywhere).
    pub fn set_clip(&mut self, clip: Option<Arc<Vec<f32>>>) {
        self.clip = clip;
    }

    /// Erase (`color` None) or fill with `color` (dry paint), by `amount`
    /// 0..=1 per pixel (a selection), within `region`. With lock
    /// transparency, erasing does nothing and filling only recolors paint
    /// that's already there (its coverage stays the same).
    pub fn apply_amount(&mut self, amount: &[f32], region: Region, color: Option<Color>) {
        if self.preserve_alpha && color.is_none() {
            return;
        }
        let paint = color.map(premultiply);
        for y in region.y0..region.y1 {
            for x in region.x0..region.x1 {
                let i = y * self.width + x;
                let k = amount[i];
                if k <= 0.0 {
                    continue;
                }
                match paint {
                    None => {
                        let was_wet = self.wet[i][3] > 0.0;
                        self.dry[i] = scale(self.dry[i], 1.0 - k);
                        self.wet[i] = scale(self.wet[i], 1.0 - k);
                        self.relief[i] *= 1.0 - k;
                        self.wet_relief[i] *= 1.0 - k;
                        if was_wet && self.wet[i][3] <= 0.0 {
                            self.wet_count -= 1;
                        }
                    }
                    Some(p) => {
                        // Fill: wet paint here dries under the new paint (like dry media).
                        if self.wet[i][3] > 0.0 {
                            self.dry[i] = over(self.wet[i], self.dry[i]);
                            self.wet[i] = [0.0; 4];
                            self.wet_relief[i] = 0.0;
                            self.wet_count -= 1;
                        }
                        self.dry[i] = if self.preserve_alpha {
                            // Recolor toward the fill, keeping how much paint is here.
                            let d = self.dry[i];
                            std::array::from_fn(|c| if c == 3 { d[3] } else { d[c] + (p[c] * d[3] - d[c]) * k })
                        } else {
                            over(scale(p, k), self.dry[i])
                        };
                    }
                }
            }
        }
        self.mark_changed(region);
    }

    pub fn preserve_alpha(&self) -> bool {
        self.preserve_alpha
    }

    pub fn set_preserve_alpha(&mut self, on: bool) {
        self.preserve_alpha = on;
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

    /// Paint thickness (impasto), row by row.
    pub fn relief(&self) -> &[f32] {
        &self.relief
    }

    /// How much of the thickness is still wet, row by row.
    pub fn wet_relief(&self) -> &[f32] {
        &self.wet_relief
    }

    /// Is there (possibly) any thick paint on this layer?
    pub fn has_relief(&self) -> bool {
        self.has_relief
    }

    /// Replace the thickness maps (loading a file, merging): total, and how
    /// much of it is wet (None = all dry).
    pub fn set_relief(&mut self, relief: Vec<f32>, wet_relief: Option<Vec<f32>>) {
        assert_eq!(relief.len(), self.relief.len(), "relief size must match canvas size");
        self.wet_relief = wet_relief.unwrap_or_else(|| vec![0.0; relief.len()]);
        assert_eq!(self.wet_relief.len(), relief.len(), "wet relief size must match canvas size");
        self.relief = relief;
        self.has_relief = self.relief.iter().any(|&h| h > 0.0);
        self.mark_changed(self.bounds());
    }

    /// A copy of everything on this layer.
    pub fn buffers(&self) -> Buffers {
        Buffers { dry: self.dry.clone(), wet: self.wet.clone(), relief: self.relief.clone(), wet_relief: self.wet_relief.clone() }
    }

    /// The paint at (x, y) (premultiplied): only the wet paint, or with
    /// `include_dry` everything. None off the canvas.
    pub fn paint_at(&self, x: f32, y: f32, include_dry: bool) -> Option<[f32; 4]> {
        if x < 0.0 || y < 0.0 || x >= self.width as f32 || y >= self.height as f32 {
            return None;
        }
        let i = y as usize * self.width + x as usize;
        Some(if include_dry { self.combined(i) } else { self.wet[i] })
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
        self.wet_count > 0
    }

    /// Recount wet pixels after the wet buffer was replaced wholesale.
    fn recount_wet(&mut self) {
        self.wet_count = self.wet.iter().filter(|p| p[3] > 0.0).count();
    }

    pub fn clear(&mut self, color: Color) {
        self.dry.fill(premultiply(color));
        self.wet.fill([0.0; 4]);
        self.relief.fill(0.0);
        self.wet_relief.fill(0.0);
        self.has_relief = false;
        self.wet_count = 0;
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

    /// Copy everything in `region` out, row by row.
    pub fn read_region(&self, region: Region) -> Buffers {
        Buffers {
            dry: read_rows(&self.dry, self.width, region),
            wet: read_rows(&self.wet, self.width, region),
            relief: read_rows(&self.relief, self.width, region),
            wet_relief: read_rows(&self.wet_relief, self.width, region),
        }
    }

    /// Put pixels (as returned by [`Canvas::read_region`]) back into `region`.
    pub fn write_region(&mut self, region: Region, b: &Buffers) {
        // Adjust the wet count for just the pixels being replaced.
        let before = read_rows(&self.wet, self.width, region).iter().filter(|p| p[3] > 0.0).count();
        let after = b.wet.iter().filter(|p| p[3] > 0.0).count();
        self.wet_count = self.wet_count - before + after;
        write_rows(&mut self.dry, self.width, region, &b.dry);
        write_rows(&mut self.wet, self.width, region, &b.wet);
        write_rows(&mut self.relief, self.width, region, &b.relief);
        write_rows(&mut self.wet_relief, self.width, region, &b.wet_relief);
        self.has_relief |= b.relief.iter().any(|&h| h > 0.0);
        self.mark_changed(region);
    }

    /// Replace every pixel at once (used when loading a file).
    pub fn set_pixels(&mut self, dry: Vec<[f32; 4]>, wet: Option<Vec<[f32; 4]>>) {
        let n = self.width * self.height;
        assert_eq!(dry.len(), n, "pixel count must match canvas size");
        self.dry = dry;
        self.wet = wet.unwrap_or_else(|| vec![[0.0; 4]; n]);
        assert_eq!(self.wet.len(), n, "wet pixel count must match canvas size");
        self.recount_wet();
        self.mark_changed(self.bounds());
    }

    /// Swap the whole layer with `b` (drying, rewetting, clearing, merging,
    /// transforming, and their undo).
    pub fn swap_buffers(&mut self, b: &mut Buffers) {
        assert_eq!(b.dry.len(), self.dry.len());
        assert_eq!(b.wet.len(), self.wet.len());
        assert_eq!(b.relief.len(), self.relief.len());
        assert_eq!(b.wet_relief.len(), self.wet_relief.len());
        std::mem::swap(&mut self.wet_relief, &mut b.wet_relief);
        std::mem::swap(&mut self.dry, &mut b.dry);
        std::mem::swap(&mut self.wet, &mut b.wet);
        std::mem::swap(&mut self.relief, &mut b.relief);
        self.has_relief = self.relief.iter().any(|&h| h > 0.0);
        self.recount_wet();
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
        let style = DabStyle {
            color: brush.color,
            opacity: brush.opacity,
            mode,
            wetness: Wetness::Dry,
            texture: None,
            reach: 1.0,
            pigment: None,
            relief: None,
            bands: None,
            flow: None,
        };
        self.dab_with(cx, cy, brush, &style);
    }

    /// Like [`Canvas::dab`], but with a specific color, opacity, wetness, and
    /// tip texture (wet strokes change these from dab to dab).
    pub fn dab_with(&mut self, cx: f32, cy: f32, brush: &Brush, style: &DabStyle) {
        let Some(Region { x0, y0, x1, y1 }) = self.dab_bounds(cx, cy, brush) else { return };
        let src = premultiply(style.color);
        // MultiColored brush: which band a pixel is in depends on where it is across the tip.
        let (band_sin, band_cos) = brush.angle.to_radians().sin_cos();
        let band_span = 0.5 / brush.radius.max(0.5);
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
                // A selection limits where this dab can work.
                let coverage = match &self.clip {
                    Some(clip) => coverage * clip[y * self.width + x],
                    None => coverage,
                };
                if coverage <= 0.0 {
                    continue;
                }
                let mut strength = match style.flow {
                    Some(Flow { share, firm }) => {
                        let total = style.opacity.clamp(0.0, 0.995);
                        let built = 1.0 - (1.0 - total).powf(share * (coverage * 3.0).min(1.0));
                        // A fully loaded brush at full opacity just covers, dab
                        // by dab, as a solid brush does (built up in thin
                        // layers, wet paint underneath would mix into every one).
                        let solid = (coverage * style.opacity).clamp(0.0, 1.0);
                        let k = ((firm - 0.85) / 0.15).clamp(0.0, 1.0);
                        built + (solid.max(built) - built) * k * k
                    }
                    None => (coverage * style.opacity).clamp(0.0, 1.0),
                };
                if self.preserve_alpha {
                    // Lock transparency: no erasing, and paint only as much as
                    // there's already paint here.
                    if style.mode == DabMode::Erase {
                        continue;
                    }
                    strength *= self.combined(y * self.width + x)[3];
                    if strength <= 0.0 {
                        continue;
                    }
                }
                if let Some(paper) = &paper {
                    strength *= surface_acceptance(paper.height_at(x, y), brush.grain, style.reach);
                }
                let i = y * self.width + x;
                let (color, pigment) = match style.bands {
                    Some(bands) => {
                        let t = 0.5 + (dx * band_cos + dy * band_sin) * band_span;
                        let (c, n) = crate::multicolor::band_at(bands.colors, t);
                        (c, bands.pigments.map(|p| &p[n]))
                    }
                    None => (style.color, style.pigment),
                };
                let paint = if style.bands.is_some() { scale(premultiply(color), strength) } else { scale(src, strength) };
                let was_wet = self.wet[i][3] > 0.0;
                match style.mode {
                    DabMode::Paint => match style.wetness {
                        // Wet paint goes into the wet buffer, on top, mixing
                        // with the wet paint already there.
                        Wetness::Wet => self.wet[i] = deposit_wet(paint, self.wet[i], color, pigment),
                        // Dry paint on top of wet paint: whatever wet paint is
                        // here dries underneath it, then the dry paint covers it.
                        Wetness::Dry => {
                            if self.wet[i][3] > 0.0 {
                                self.dry[i] = over(self.wet[i], self.dry[i]);
                                self.wet[i] = [0.0; 4];
                                self.wet_relief[i] = 0.0; // its thickness dries too
                            }
                            self.dry[i] = over(paint, self.dry[i]);
                        }
                        // Smudging: moved paint stays wet where it's wet, dry where it's dry.
                        Wetness::Keep => {
                            if self.wet[i][3] > 0.01 {
                                self.wet[i] = deposit_wet(paint, self.wet[i], color, pigment);
                            } else {
                                self.dry[i] = over(paint, self.dry[i]);
                            }
                        }
                    },
                    DabMode::Erase => {
                        self.dry[i] = scale(self.dry[i], 1.0 - strength);
                        self.wet[i] = scale(self.wet[i], 1.0 - strength);
                        self.relief[i] *= 1.0 - strength;
                        self.wet_relief[i] *= 1.0 - strength;
                    }
                }
                // Impasto: dragged paint levels the surface, fresh paint piles up.
                // Only the wet part moves; dried thickness is a base the new paint sits on.
                if let (DabMode::Paint, Some(r)) = (style.mode, style.relief) {
                    let mut dry_h = (self.relief[i] - self.wet_relief[i]).max(0.0);
                    let mut h = self.wet_relief[i];
                    let c = coverage.clamp(0.0, 1.0);
                    if r.plow > 0.0 {
                        // Plowing: scrape wet paint from under the brush's middle...
                        h -= h * (r.plow * strength).min(1.0);
                        // ...and shove it up along the brush's edges.
                        h += r.push * r.plow * 4.0 * c * (1.0 - c);
                    }
                    if r.smear > 0.0 {
                        h += (r.smear_to - h) * (r.smear * strength).min(1.0);
                    }
                    // A full body of paint with softer grooves where the bristles part
                    // (the square root lifts the grooves without filling them).
                    if r.scrape > 0.0 {
                        // Scraping: wet paint comes off the canvas (dried paint stays).
                        // (By where the blade touches, not by how much paint it lays down.)
                        let k = (r.scrape * c).min(0.95);
                        self.wet[i] = scale(self.wet[i], 1.0 - k);
                        h *= 1.0 - k;
                    }
                    let target = if r.flat { r.build * (c / 0.35).min(1.0) } else { r.build * c.sqrt() };
                    if target > h {
                        h += (target - h) * strength;
                    }
                    if r.level {
                        // Flat paint: whatever was here sinks flat where the brush covers.
                        let k = strength.min(1.0);
                        h *= 1.0 - k;
                        dry_h *= 1.0 - k;
                    }
                    let h = h.clamp(0.0, (MAX_RELIEF - dry_h).max(0.0));
                    self.wet_relief[i] = h;
                    self.relief[i] = dry_h + h;
                    self.has_relief |= self.relief[i] > 0.0;
                }
                match (was_wet, self.wet[i][3] > 0.0) {
                    (false, true) => self.wet_count += 1,
                    (true, false) => self.wet_count -= 1,
                    _ => {}
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

impl Canvas {
    /// The average *wet* paint thickness under a dab, weighted by the brush
    /// shape: what the brush can move.
    pub fn sample_relief(&self, cx: f32, cy: f32, brush: &Brush) -> f32 {
        let Some(Region { x0, y0, x1, y1 }) = self.dab_bounds(cx, cy, brush) else { return 0.0 };
        let step = ((x1 - x0).max(y1 - y0) / 6).max(1);
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        for y in (y0..y1).step_by(step) {
            for x in (x0..x1).step_by(step) {
                let w = brush.coverage(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if w > 0.0 {
                    sum += self.wet_relief[y * self.width + x] * w;
                    weight += w;
                }
            }
        }
        if weight > 0.0 { sum / weight } else { 0.0 }
    }

    /// Like [`Canvas::sample_under`], but the colors under the brush are
    /// averaged **like paint** (pigment mixing) instead of like light, so
    /// blending blue against brown gives a dark neutral, not purple.
    /// Uses a grid of up to ~25 sample points to stay fast.
    /// Returns (straight color, average alpha).
    pub fn sample_under_pigment(&self, cx: f32, cy: f32, brush: &Brush, include_dry: bool) -> ([f32; 3], f32) {
        let Some(Region { x0, y0, x1, y1 }) = self.dab_bounds(cx, cy, brush) else { return ([0.0; 3], 0.0) };
        let step = ((x1 - x0).max(y1 - y0) / 5).max(1);
        let mut samples: Vec<(Pigment, f32)> = Vec::with_capacity(36);
        let (mut alpha_sum, mut weight_sum) = (0.0f32, 0.0f32);
        for y in (y0..y1).step_by(step) {
            for x in (x0..x1).step_by(step) {
                let w = brush.coverage(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if w <= 0.0 {
                    continue;
                }
                let i = y * self.width + x;
                let p = if include_dry { self.combined(i) } else { self.wet[i] };
                weight_sum += w;
                alpha_sum += p[3] * w;
                if p[3] > 0.01 {
                    let color = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
                    samples.push((Pigment::from_srgb(color), w * p[3]));
                }
            }
        }
        if weight_sum <= 0.0 {
            return ([0.0; 3], 0.0);
        }
        let alpha = alpha_sum / weight_sum;
        let color = crate::spectral::mix_many(&samples).unwrap_or([0.0; 3]);
        (color, alpha)
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

/// Put `paint` (premultiplied) onto wet paint `dst`. With a pigment, where the
/// two colors really differ, the colors mix like paint (Kubelka-Munk);
/// otherwise (or where they're the same color) it's a plain "over".
#[inline]
fn deposit_wet(paint: [f32; 4], dst: [f32; 4], src: Color, pigment: Option<&Pigment>) -> [f32; 4] {
    let (a_new, a_old) = (paint[3], dst[3]);
    let plain = over(paint, dst);
    let Some(pigment) = pigment else { return plain };
    // Nothing (much) underneath, or the new paint fully covers it: no mixing needed.
    if a_old < 0.01 || a_new >= 0.98 || a_new <= 0.0 {
        return plain;
    }
    // Same color underneath (the usual case inside a stroke): mixing would
    // give the same color, so skip the expensive part.
    let old = [dst[0] / a_old, dst[1] / a_old, dst[2] / a_old];
    let diff = (old[0] - src.r).abs() + (old[1] - src.g).abs() + (old[2] - src.b).abs();
    if diff < 0.02 {
        return plain;
    }
    // How much of the paint in this pixel is the new paint.
    let alpha = plain[3];
    let t = a_new / alpha;
    let mixed = mix_pigments(&Pigment::from_srgb(old), pigment, t);
    [mixed[0] * alpha, mixed[1] * alpha, mixed[2] * alpha, alpha]
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
        assert_eq!(saved.dry.len(), 4 * 5);
        canvas.dab(4.0, 5.0, &Brush { radius: 2.0, ..Brush::default() }, DabMode::Paint);
        assert_eq!(canvas.pixel(4, 5), Some(Color::BLACK));
        canvas.write_region(region, &saved);
        assert_eq!(canvas.pixel(4, 5), Some(Color::WHITE));
    }

    #[test]
    fn wet_paint_sits_on_top_and_only_it_is_picked_up() {
        let mut canvas = Canvas::new(20, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        // Dry red underneath, wet blue on top.
        canvas.dab(10.0, 10.0, &Brush { color: Color::rgb(1.0, 0.0, 0.0), ..brush.clone() }, DabMode::Paint);
        let wet_blue = DabStyle {
            color: Color::rgb(0.0, 0.0, 1.0),
            opacity: 0.5,
            mode: DabMode::Paint,
            wetness: Wetness::Wet,
            texture: None,
            reach: 1.0,
            pigment: None,
            relief: None,
            bands: None,
            flow: None,
        };
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
    fn wet_count_stays_exact() {
        let mut canvas = Canvas::new(40, 40, Color::TRANSPARENT);
        let brush = Brush { radius: 6.0, ..Brush::default() };
        let wet = DabStyle {
            color: Color::BLACK,
            opacity: 0.7,
            mode: DabMode::Paint,
            wetness: Wetness::Wet,
            texture: None,
            reach: 1.0,
            pigment: None,
            relief: None,
            bands: None,
            flow: None,
        };
        let exact = |c: &Canvas| c.wet_pixels().iter().filter(|p| p[3] > 0.0).count();
        canvas.dab_with(10.0, 10.0, &brush, &wet);
        canvas.dab_with(14.0, 12.0, &brush, &wet);
        assert_eq!(canvas.wet_count, exact(&canvas));
        canvas.dab(12.0, 11.0, &brush, DabMode::Paint); // dry paint dries what it covers
        assert_eq!(canvas.wet_count, exact(&canvas));
        canvas.dab(12.0, 11.0, &Brush { radius: 3.0, ..brush.clone() }, DabMode::Erase);
        assert_eq!(canvas.wet_count, exact(&canvas));
        let saved = canvas.read_region(canvas.bounds());
        canvas.clear(Color::TRANSPARENT);
        assert!(!canvas.has_wet_paint());
        canvas.write_region(canvas.bounds(), &saved);
        assert_eq!(canvas.wet_count, exact(&canvas));
        assert!(canvas.has_wet_paint());
    }

    #[test]
    fn lock_transparency_paints_only_on_existing_paint() {
        let mut canvas = Canvas::new(40, 20, Color::TRANSPARENT);
        canvas.dab(10.0, 10.0, &Brush { radius: 6.0, ..Brush::default() }, DabMode::Paint); // black blob on the left
        canvas.set_preserve_alpha(true);
        let red = Brush { color: Color::rgb(1.0, 0.0, 0.0), radius: 30.0, ..Brush::default() };
        canvas.dab(20.0, 10.0, &red, DabMode::Paint);
        assert_eq!(canvas.pixel(10, 10), Some(Color::rgb(1.0, 0.0, 0.0)), "existing paint recolored");
        assert_eq!(canvas.pixel(35, 10).unwrap().a, 0.0, "transparent area stays transparent");
        canvas.dab(10.0, 10.0, &red, DabMode::Erase);
        assert_eq!(canvas.pixel(10, 10).unwrap().a, 1.0, "erasing is blocked");
    }

    #[test]
    fn dry_media_dries_the_wet_paint_it_covers() {
        let mut canvas = Canvas::new(20, 20, Color::TRANSPARENT);
        let brush = Brush { radius: 5.0, ..Brush::default() };
        let wet = DabStyle {
            color: Color::BLACK,
            opacity: 1.0,
            mode: DabMode::Paint,
            wetness: Wetness::Wet,
            texture: None,
            reach: 1.0,
            pigment: None,
            relief: None,
            bands: None,
            flow: None,
        };
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
