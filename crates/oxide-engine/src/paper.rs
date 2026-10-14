//! The painting surface: paper, canvas, panel.
//!
//! The surface is not a picture laid over the painting. It's a **height
//! field** (the hills and valleys of the paper's tooth or the canvas weave)
//! that the brush interacts with: dry or light strokes catch on the peaks
//! and skip the valleys, loaded strokes fill them. The surface is also
//! **lit**, so its relief shows on screen.
//!
//! Every surface is generated procedurally (our own math, no scanned
//! textures) as a seamless tile that repeats across the canvas.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::tip::{hash, hash2};

/// Side length of the repeating surface tile, in canvas pixels.
const TILE: usize = 512;

/// The kinds of surface you can paint on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperKind {
    /// Hot-pressed paper or bristol: almost no texture.
    Smooth,
    /// Cold-pressed paper: a medium, bumpy tooth.
    ColdPress,
    /// Rough paper: big, deep bumps.
    Rough,
    /// Handmade paper: irregular, blotchy surface with scattered fibers.
    Handmade,
    /// Washi (Japanese paper): soft, with long visible fibers.
    Washi,
    /// Cotton canvas: a regular over-under weave.
    Canvas,
    /// Linen: a finer, more irregular weave.
    Linen,
    /// Gesso panel: nearly flat with faint brush marks in the primer.
    Panel,
}

impl PaperKind {
    pub const ALL: [PaperKind; 8] = [
        PaperKind::Smooth,
        PaperKind::ColdPress,
        PaperKind::Rough,
        PaperKind::Handmade,
        PaperKind::Washi,
        PaperKind::Canvas,
        PaperKind::Linen,
        PaperKind::Panel,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaperKind::Smooth => "Smooth (hot press)",
            PaperKind::ColdPress => "Cold press",
            PaperKind::Rough => "Rough",
            PaperKind::Handmade => "Handmade",
            PaperKind::Washi => "Washi",
            PaperKind::Canvas => "Canvas",
            PaperKind::Linen => "Linen",
            PaperKind::Panel => "Gesso panel",
        }
    }
}

/// The settings that define a surface. Saved in `.oxide` files.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperSettings {
    pub kind: PaperKind,
    /// Size of the texture features (1 = normal; 2 = twice as coarse).
    pub scale: f32,
    /// How deep the texture is (0 = perfectly flat, 1 = deep).
    pub depth: f32,
    /// Direction the light comes from, in degrees (0 = right, 90 = down,
    /// 135 = down-left... 225 = top-left).
    pub light_angle: f32,
    /// How visible the texture's light and shadow are on **bare paper**
    /// (0..=1; shown as 0–10, "Paper Texture").
    #[serde(default = "default_paper_visibility")]
    pub paper_visibility: f32,
    /// How visible the texture is **through paint** (0..=1;
    /// "Paint Texture"). Opaque paint mostly hides the weave; thin paint shows it.
    #[serde(default = "default_paint_visibility")]
    pub paint_visibility: f32,
    /// How strongly thick paint (impasto) is lit, 0..=1 (shown 0–10 as
    /// Impasto Depth). 0 = paint looks flat.
    #[serde(default = "default_impasto_depth")]
    pub impasto_depth: f32,
    /// How shiny thick paint is, 0..=1 (shown 0–10): highlights on the
    /// ridges that face the light.
    #[serde(default = "default_gloss")]
    pub gloss: f32,
    /// How dark the shadows thick paint casts are, 0..=1 (shown 0–10).
    #[serde(default = "default_shadows")]
    pub shadows: f32,
    /// How high the light is above the canvas, in degrees (10 = low, raking
    /// light with long shadows; 80 = nearly overhead).
    #[serde(default = "default_light_height")]
    pub light_height: f32,
}

fn default_shadows() -> f32 {
    0.5
}

fn default_light_height() -> f32 {
    40.0
}

fn default_impasto_depth() -> f32 {
    0.5
}

fn default_gloss() -> f32 {
    0.3
}

fn default_paper_visibility() -> f32 {
    0.8
}

fn default_paint_visibility() -> f32 {
    0.3
}

impl Default for PaperSettings {
    fn default() -> Self {
        PaperSettings {
            kind: PaperKind::ColdPress,
            scale: 1.0,
            depth: 0.5,
            light_angle: 225.0,
            paper_visibility: default_paper_visibility(),
            paint_visibility: default_paint_visibility(),
            impasto_depth: default_impasto_depth(),
            gloss: default_gloss(),
            shadows: default_shadows(),
            light_height: default_light_height(),
        }
    }
}

/// A generated surface: height and lighting for one repeating tile.
pub struct Paper {
    settings: PaperSettings,
    /// 0 (valley) ..= 1 (peak), already scaled by depth (flat paper = all 1).
    height: Vec<f32>,
    /// Brightness multiplier from lighting the relief (around 1.0).
    shade: Vec<f32>,
}

impl Paper {
    pub fn new(settings: PaperSettings) -> Self {
        if settings.depth <= 0.0 {
            // Perfectly flat: skip generating anything.
            return Paper { settings, height: vec![1.0; TILE * TILE], shade: vec![1.0; TILE * TILE] };
        }
        let raw = generate_height(&settings);
        // Deeper texture = valleys go lower. Flat (depth 0) = everything at 1.
        let depth = settings.depth.clamp(0.0, 1.0);
        let height: Vec<f32> = raw.iter().map(|h| 1.0 - depth * (1.0 - h)).collect();
        let shade = light(&height, depth, settings.light_angle);
        Paper { settings, height, shade }
    }

    pub fn settings(&self) -> PaperSettings {
        self.settings
    }

    /// Surface height at canvas pixel (x, y): 0 = deepest valley, 1 = top.
    pub fn height_at(&self, x: usize, y: usize) -> f32 {
        self.height[(y % TILE) * TILE + (x % TILE)]
    }

    /// How much lighting brightens or darkens pixel (x, y) (around 1.0), at
    /// full visibility.
    pub fn shade_at(&self, x: usize, y: usize) -> f32 {
        self.shade[(y % TILE) * TILE + (x % TILE)]
    }

    /// The shading actually shown at (x, y), given how much paint covers it
    /// (`paint_alpha` 0..=1): bare paper uses the paper visibility, painted
    /// areas the paint visibility, and partly covered pixels blend the two.
    pub fn visible_shade_at(&self, x: usize, y: usize, paint_alpha: f32) -> f32 {
        let s = &self.settings;
        let a = paint_alpha.clamp(0.0, 1.0);
        let visibility = s.paper_visibility.clamp(0.0, 1.0) * (1.0 - a) + s.paint_visibility.clamp(0.0, 1.0) * a;
        1.0 + (self.shade_at(x, y) - 1.0) * visibility
    }
}

impl PaperSettings {
    /// A perfectly smooth surface (no texture at all).
    pub fn flat() -> Self {
        PaperSettings { kind: PaperKind::Smooth, depth: 0.0, ..PaperSettings::default() }
    }
}

/// How much paint a pixel accepts given the surface height there.
///
/// `grain` (0..=1) is how strongly this brush/medium feels the texture;
/// `reach` (0..=1) is how deep into the valleys the paint gets (a loaded,
/// wet brush reaches far; a dry, light touch only grazes the peaks).
pub fn surface_acceptance(height: f32, grain: f32, reach: f32) -> f32 {
    let grain = grain.clamp(0.0, 1.0);
    if grain <= 0.0 {
        return 1.0;
    }
    // Paint lands where the surface is higher than this level.
    let level = (1.0 - reach.clamp(0.0, 1.0)) * grain;
    let t = ((height - level + 0.12) / 0.24).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ---------- Generating the height field ----------

/// A raw height tile, 0..=1, before depth is applied.
fn generate_height(s: &PaperSettings) -> Vec<f32> {
    let scale = s.scale.clamp(0.25, 4.0);
    let mut out = vec![0.0; TILE * TILE];
    // Each row is independent, so rows are generated on all CPU cores at once.
    out.par_chunks_mut(TILE).enumerate().for_each(|(y, row)| {
        for (x, value) in row.iter_mut().enumerate() {
            let (fx, fy) = (x as f32, y as f32);
            *value = match s.kind {
                PaperKind::Smooth => 0.5 + 0.5 * fbm(fx, fy, 4.0 * scale, 3, 11),
                PaperKind::ColdPress => {
                    let bumps = fbm(fx, fy, 14.0 * scale, 4, 21);
                    let fibers = fbm(fx, fy, 3.0 * scale, 2, 22);
                    0.75 * bumps + 0.25 * fibers
                }
                PaperKind::Rough => {
                    // Rounded, deep bumps: emphasize the high parts.
                    let b = fbm(fx, fy, 30.0 * scale, 5, 31);
                    (b * b * (3.0 - 2.0 * b)) * 0.85 + 0.15 * fbm(fx, fy, 4.0 * scale, 2, 32)
                }
                PaperKind::Handmade => {
                    // Big soft blotches, medium lumps, and fine grit: uneven, like pulp pressed by hand.
                    let blotches = fbm(fx, fy, 60.0 * scale, 3, 71);
                    let lumps = fbm(fx, fy, 12.0 * scale, 3, 72);
                    let grit = fbm(fx, fy, 2.0 * scale, 2, 73);
                    0.45 * blotches + 0.4 * lumps + 0.15 * grit
                }
                PaperKind::Washi => {
                    // Long fibers in several directions: noise stretched along each direction.
                    // (Integer combinations of x and y keep the tile seamless.)
                    let along = |u: f32, v: f32, seed| fbm(u * 0.08, v, 2.5 * scale, 2, seed);
                    let fibers = [
                        along(fx, fy, 81),
                        along(fy, fx, 82),
                        along(fx + fy, fx - fy, 83),
                        along(fx - fy, fx + fy, 84),
                    ];
                    let strongest = fibers.iter().cloned().fold(0.0_f32, f32::max);
                    0.7 * strongest + 0.3 * fbm(fx, fy, 20.0 * scale, 2, 85)
                }
                PaperKind::Canvas => weave(fx, fy, 8.0 * scale, 0.25, 41),
                PaperKind::Linen => weave(fx, fy, 5.0 * scale, 0.55, 51),
                PaperKind::Panel => {
                    // Faint horizontal brush marks from the gesso, plus a little grit.
                    let marks = fbm(fx * 0.15, fy * 2.0, 6.0 * scale, 3, 61);
                    0.6 + 0.3 * marks + 0.1 * fbm(fx, fy, 2.0 * scale, 2, 62)
                }
            };
        }
    });
    normalize(&mut out);
    out
}

/// Plain over-under weave: vertical and horizontal threads alternate which
/// is on top. `irregular` adds lumpy, uneven threads (linen).
fn weave(x: f32, y: f32, thread: f32, irregular: f32, seed: u32) -> f32 {
    // Make the thread width divide the tile exactly so it repeats seamlessly.
    let count = (TILE as f32 / thread).round().max(2.0);
    let count = if count as u32 % 2 == 1 { count + 1.0 } else { count };
    let period = TILE as f32 / count;
    let (u, v) = (x / period, y / period);
    let (iu, iv) = (u.floor(), v.floor());
    let (fu, fv) = (u - iu, v - iv);
    let bump = |f: f32| (f * std::f32::consts::PI).sin();

    // Each thread's thickness varies a little along its length.
    let warp_lumps = 1.0 - irregular * 0.5 * value_noise_1d(y / (period * 6.0), hash2(iu as u32, seed), count as u32);
    let weft_lumps = 1.0 - irregular * 0.5 * value_noise_1d(x / (period * 6.0), hash2(iv as u32, seed + 1), count as u32);

    let warp_on_top = (iu as i64 + iv as i64) % 2 == 0;
    let h = if warp_on_top {
        bump(fu) * warp_lumps * (0.7 + 0.3 * bump(fv))
    } else {
        bump(fv) * weft_lumps * (0.7 + 0.3 * bump(fu))
    };
    // A little fiber fuzz.
    0.85 * h + 0.15 * fbm(x, y, 2.0, 2, seed + 7)
}

/// Fractal noise: several octaves of seamless value noise. 0..=1.
fn fbm(x: f32, y: f32, cell: f32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut total = 0.0;
    let mut cell = cell.max(1.0);
    for o in 0..octaves {
        sum += amp * value_noise_2d(x, y, cell, seed.wrapping_mul(31).wrapping_add(o));
        total += amp;
        amp *= 0.5;
        cell = (cell * 0.5).max(1.0);
    }
    sum / total
}

/// Smooth 2-D value noise with lattice spacing `cell`, wrapping at the tile
/// edge so the texture repeats seamlessly. 0..=1.
fn value_noise_2d(x: f32, y: f32, cell: f32, seed: u32) -> f32 {
    let period = ((TILE as f32 / cell).round() as u32).max(1);
    let cell = TILE as f32 / period as f32;
    let (u, v) = (x / cell, y / cell);
    let (iu, iv) = (u.floor() as u32, v.floor() as u32);
    let (fu, fv) = (smooth(u - u.floor()), smooth(v - v.floor()));
    let corner = |i: u32, j: u32| hash01(hash2(hash2(i % period, j % period), seed));
    let top = lerp(corner(iu, iv), corner(iu + 1, iv), fu);
    let bottom = lerp(corner(iu, iv + 1), corner(iu + 1, iv + 1), fu);
    lerp(top, bottom, fv)
}

fn value_noise_1d(t: f32, seed: u32, period: u32) -> f32 {
    let i = t.floor() as i64 as u32;
    let f = smooth(t - t.floor());
    let a = hash01(hash2(i % period.max(1), seed));
    let b = hash01(hash2((i + 1) % period.max(1), seed));
    lerp(a, b, f)
}

/// Stretch values to fill 0..=1.
fn normalize(values: &mut [f32]) {
    let (min, max) = values.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let range = (max - min).max(1e-6);
    for v in values {
        *v = (*v - min) / range;
    }
}

/// Light the relief: brightness from the slope facing toward or away from the light.
fn light(height: &[f32], depth: f32, light_angle: f32) -> Vec<f32> {
    let (ly, lx) = light_angle.to_radians().sin_cos();
    // Steeper relief for deeper paper; the factor sets how pronounced the shading is.
    let relief = 6.0 * depth;
    let at = |x: usize, y: usize| height[(y % TILE) * TILE + (x % TILE)];
    let mut shade = vec![1.0; TILE * TILE];
    shade.par_chunks_mut(TILE).enumerate().for_each(|(y, row)| {
        for (x, value) in row.iter_mut().enumerate() {
            // Slope from neighbors (wrapping at the tile edge).
            let dx = at(x + 1, y) - at(x + TILE - 1, y);
            let dy = at(x, y + 1) - at(x, y + TILE - 1);
            // Surfaces sloping *toward* the light get brighter.
            let facing = -(dx * lx + dy * ly) * relief;
            *value = (1.0 + facing * 0.5).clamp(0.6, 1.25);
        }
    });
    shade
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn hash01(h: u32) -> f32 {
    (hash(h) >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(paper: &Paper) -> (f32, f32) {
        let min = paper.height.iter().cloned().fold(1.0, f32::min);
        let max = paper.height.iter().cloned().fold(0.0, f32::max);
        (min, max)
    }

    #[test]
    fn every_kind_generates_valid_heights() {
        for kind in PaperKind::ALL {
            let paper = Paper::new(PaperSettings { kind, ..PaperSettings::default() });
            let (min, max) = stats(&paper);
            assert!((0.0..=1.0).contains(&min) && (0.0..=1.0).contains(&max), "{kind:?}: {min}..{max}");
            assert!(max - min > 0.2, "{kind:?} should have visible texture");
        }
    }

    #[test]
    fn zero_depth_is_flat() {
        let paper = Paper::new(PaperSettings { depth: 0.0, ..PaperSettings::default() });
        assert_eq!(stats(&paper), (1.0, 1.0));
        assert!(paper.shade.iter().all(|&s| (s - 1.0).abs() < 1e-6));
    }

    #[test]
    fn texture_tiles_seamlessly() {
        let paper = Paper::new(PaperSettings { kind: PaperKind::Rough, ..PaperSettings::default() });
        // The pixel after the tile edge is the first pixel again: no seam jump.
        for y in [0, 100, 300] {
            let edge = paper.height_at(TILE - 1, y);
            let wrapped = paper.height_at(TILE, y);
            let next = paper.height_at(1, y);
            assert_eq!(wrapped, paper.height_at(0, y));
            // Neighbors across the seam are about as close as neighbors elsewhere.
            assert!((edge - wrapped).abs() < 0.3 && (wrapped - next).abs() < 0.3);
        }
    }

    #[test]
    fn paint_hides_the_texture_more_than_bare_paper() {
        let paper = Paper::new(PaperSettings { kind: PaperKind::Canvas, depth: 0.6, paper_visibility: 1.0, paint_visibility: 0.2, ..PaperSettings::default() });
        // Find a strongly lit or shadowed spot.
        let (x, y) = (0..TILE * TILE).map(|i| (i % TILE, i / TILE)).max_by(|a, b| {
            (paper.shade_at(a.0, a.1) - 1.0).abs().total_cmp(&(paper.shade_at(b.0, b.1) - 1.0).abs())
        }).unwrap();
        let bare = (paper.visible_shade_at(x, y, 0.0) - 1.0).abs();
        let painted = (paper.visible_shade_at(x, y, 1.0) - 1.0).abs();
        assert!(bare > 0.05, "texture shows on bare paper: {bare}");
        assert!((painted - bare * 0.2).abs() < 1e-5, "paint shows 20% of it: {painted} vs {bare}");
        // Zero visibility = no shading at all.
        let flat = Paper::new(PaperSettings { paper_visibility: 0.0, paint_visibility: 0.0, ..paper.settings() });
        assert_eq!(flat.visible_shade_at(x, y, 0.0), 1.0);
    }

    #[test]
    fn old_files_get_default_visibility() {
        let old = r#"kind = "Canvas"
scale = 1.0
depth = 0.4
light_angle = 225.0"#;
        let s: PaperSettings = toml::from_str(old).unwrap();
        assert_eq!((s.paper_visibility, s.paint_visibility), (0.8, 0.3));
    }

    #[test]
    fn dry_touch_catches_peaks_loaded_brush_fills_valleys() {
        let (peak, valley) = (0.9, 0.2);
        // Light, dry touch on a textured brush: peaks yes, valleys no.
        assert!(surface_acceptance(peak, 1.0, 0.3) > 0.9);
        assert!(surface_acceptance(valley, 1.0, 0.3) < 0.1);
        // Fully loaded: everything.
        assert!(surface_acceptance(valley, 1.0, 1.0) > 0.9);
        // A brush that ignores grain paints everywhere.
        assert_eq!(surface_acceptance(valley, 0.0, 0.0), 1.0);
    }
}
