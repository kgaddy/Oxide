//! Texture tips: the brush stamps a grayscale image instead of a smooth
//! ellipse (texture brushes). All tips are generated here in code
//! (our own, clean-room), once, the first time they're used.
//!
//! A mask is sampled in the tip's own frame, so size, roundness, angle and
//! "follow stroke" work exactly as for the round tip.
//!
//! Tips can also be **imported images** (PNG, Photoshop .abr, GIMP .gbr/.gih;
//! see `oxide_commands::tip_import`). They're registered here by name, and an
//! image tip can have several frames (variations): each dab picks one.

use std::sync::{Mutex, OnceLock};

use crate::tip::{hash, hash2};

/// The tip's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Tip {
    /// A smooth ellipse (hardness sets the edge).
    #[default]
    Round,
    /// Porous blobs, like a natural sponge.
    Sponge,
    /// Grainy, broken texture, like chalk or dry pastel.
    Chalk,
    /// Drops of paint flicked off a brush.
    Splatter,
    /// Separate streaks with ragged ends, like a dry, worn brush.
    DryBrush,
    /// Crumpled cloth.
    Rag,
    /// Fine, sparse dots.
    Speckle,
    /// An imported image (see [`register_tip`]).
    Image(&'static TipImage),
}

impl Tip {
    /// The built-in tips (imported ones: [`image_tips`]).
    pub const ALL: [Tip; 7] = [Tip::Round, Tip::Sponge, Tip::Chalk, Tip::Splatter, Tip::DryBrush, Tip::Rag, Tip::Speckle];

    /// The name shown in menus.
    pub fn name(self) -> &'static str {
        match self {
            Tip::Image(image) => &image.name,
            builtin => builtin.label(),
        }
    }

    /// How a tip is written in preset files and agent commands: "sponge",
    /// "dry_brush", ... or "image:<name>" for an imported tip.
    pub fn key(self) -> String {
        match self {
            Tip::Image(image) => format!("image:{}", image.name),
            builtin => builtin.label().to_lowercase().replace(' ', "_"),
        }
    }

    /// The tip for a key from [`Tip::key`] (or an imported tip's plain name).
    pub fn from_key(key: &str) -> Option<Tip> {
        let key = key.trim();
        if let Some(name) = key.strip_prefix("image:") {
            return find_image_tip(name);
        }
        Tip::ALL.into_iter().find(|t| t.key() == key.to_lowercase()).or_else(|| find_image_tip(key))
    }

    fn label(self) -> &'static str {
        match self {
            Tip::Round => "Round",
            Tip::Sponge => "Sponge",
            Tip::Chalk => "Chalk",
            Tip::Splatter => "Splatter",
            Tip::DryBrush => "Dry brush",
            Tip::Rag => "Rag",
            Tip::Speckle => "Speckle",
            Tip::Image(_) => "Image",
        }
    }

    /// The image for this tip and dab (`frame` picks among an imported tip's
    /// variations); None for the round tip.
    pub(crate) fn mask(self, frame: u32) -> Option<&'static Mask> {
        static MASKS: [OnceLock<Mask>; 6] = [const { OnceLock::new() }; 6];
        if let Tip::Image(image) = self {
            return image.frames.get(frame as usize % image.frames.len().max(1));
        }
        let index = Tip::ALL.iter().position(|&t| t == self)?.checked_sub(1)?;
        Some(MASKS[index].get_or_init(|| Mask::generate(self)))
    }
}

// Preset files store a tip as its key ("sponge", "image:My tip").
impl serde::Serialize for Tip {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> serde::Deserialize<'de> for Tip {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Tip, D::Error> {
        let key = String::deserialize(d)?;
        // An imported tip that has since been removed falls back to round.
        Ok(Tip::from_key(&key).unwrap_or_default())
    }
}

/// An imported tip: a name and one or more grayscale frames (1 = paint).
#[derive(Debug)]
pub struct TipImage {
    pub name: String,
    frames: Vec<Mask>,
}

impl PartialEq for TipImage {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl TipImage {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Imported tips. Each is leaked so tips can be plain `Copy` values; there
/// are only ever a few, and re-importing a name replaces it in the list.
static IMAGES: Mutex<Vec<&'static TipImage>> = Mutex::new(Vec::new());

/// Add (or replace) an imported tip. Each frame is (width, height, coverage
/// 0..=1 row by row, 1 = paint).
pub fn register_tip(name: &str, frames: Vec<(usize, usize, Vec<f32>)>) -> Tip {
    let frames = frames.into_iter().filter(|(w, h, d)| *w > 0 && *h > 0 && d.len() == w * h).map(|(w, h, data)| Mask { w, h, data }).collect();
    let image: &'static TipImage = Box::leak(Box::new(TipImage { name: name.to_string(), frames }));
    let mut images = IMAGES.lock().expect("tip registry");
    match images.iter_mut().find(|t| t.name == name) {
        Some(slot) => *slot = image,
        None => images.push(image),
    }
    Tip::Image(image)
}

/// Forget an imported tip (brushes already using it keep working).
pub fn unregister_tip(name: &str) {
    IMAGES.lock().expect("tip registry").retain(|t| t.name != name);
}

/// All imported tips, in the order they were added.
pub fn image_tips() -> Vec<Tip> {
    IMAGES.lock().expect("tip registry").iter().map(|&t| Tip::Image(t)).collect()
}

fn find_image_tip(name: &str) -> Option<Tip> {
    IMAGES.lock().expect("tip registry").iter().find(|t| t.name == name).map(|&t| Tip::Image(t))
}

const SIZE: usize = 256;

/// A grayscale image, 0..=1. Its longer side spans the tip from -1 to 1.
#[derive(Debug)]
pub(crate) struct Mask {
    w: usize,
    h: usize,
    data: Vec<f32>,
}

impl Mask {
    /// The mask at (u, v), -1..=1 across the tip (0 outside), smoothly interpolated.
    pub(crate) fn sample(&self, u: f32, v: f32) -> f32 {
        let half = self.w.max(self.h) as f32 * 0.5;
        let x = u * half + self.w as f32 * 0.5 - 0.5;
        let y = v * half + self.h as f32 * 0.5 - 0.5;
        if !(0.0..(self.w - 1) as f32).contains(&x) || !(0.0..(self.h - 1) as f32).contains(&y) {
            return 0.0;
        }
        let (ix, iy) = (x as usize, y as usize);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let at = |x: usize, y: usize| self.data[y * self.w + x];
        let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * fx;
        let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * fx;
        top + (bottom - top) * fy
    }

    fn generate(tip: Tip) -> Mask {
        let seed = hash(Tip::ALL.iter().position(|&t| t == tip).unwrap_or(0) as u32 + 1);
        // Splatter is a list of drops; the others are a formula per pixel.
        let drops: Vec<(f32, f32, f32)> = (0..90)
            .map(|i| {
                let h = hash2(seed, i);
                let (a, r) = (rand(h) * std::f32::consts::TAU, rand(h ^ 1).powf(0.8) * 0.95);
                (r * a.cos(), r * a.sin(), 0.015 + 0.09 * rand(h ^ 2).powi(3))
            })
            .collect();
        let data = (0..SIZE * SIZE)
            .map(|i| {
                let (u, v) = ((i % SIZE) as f32 / (SIZE - 1) as f32 * 2.0 - 1.0, (i / SIZE) as f32 / (SIZE - 1) as f32 * 2.0 - 1.0);
                let r = (u * u + v * v).sqrt();
                // A wobbly round outline, so no tip has a perfect circle edge.
                let wobble = 0.12 * fbm(u * 2.0 + 7.0, v * 2.0, seed ^ 9, 3);
                let disk = smoothstep(1.0, 0.82, r + wobble);
                let value = match tip {
                    Tip::Round | Tip::Image(_) => 1.0,
                    Tip::Sponge => {
                        let n = fbm(u * 5.0, v * 5.0, seed, 4);
                        disk * smoothstep(0.42, 0.56, n)
                    }
                    Tip::Chalk => {
                        let grain = rand(hash2(seed, i as u32));
                        let n = fbm(u * 14.0, v * 14.0, seed, 3);
                        disk * smoothstep(0.35, 0.75, 0.55 * n + 0.45 * grain)
                    }
                    Tip::Splatter => drops.iter().map(|&(x, y, s)| smoothstep(s, s * 0.6, ((u - x).powi(2) + (v - y).powi(2)).sqrt())).fold(0.0, f32::max),
                    Tip::DryBrush => {
                        // Streaks run along v (the direction of travel for a tip
                        // that follows the stroke); their ends are ragged.
                        let streak = value_noise_1d(u * 30.0, seed);
                        let ends = 0.9 - 0.3 * value_noise_1d(u * 9.0 + 3.0, seed ^ 5);
                        let along = smoothstep(ends, ends - 0.2, v.abs());
                        smoothstep(1.0, 0.9, u.abs()) * along * smoothstep(0.32, 0.52, streak)
                    }
                    Tip::Rag => {
                        let n = fbm(u * 4.0, v * 4.0, seed, 5);
                        let ridges = 1.0 - (2.0 * n - 1.0).abs();
                        disk * smoothstep(0.55, 0.85, ridges) * (0.6 + 0.4 * fbm(u * 12.0, v * 12.0, seed ^ 3, 2))
                    }
                    Tip::Speckle => {
                        // Dots on a jittered grid, fewer toward the edge.
                        let (cx, cy) = ((u * 18.0).floor(), (v * 18.0).floor());
                        let h = hash2(hash2(seed, cx as i32 as u32), cy as i32 as u32);
                        let (dx, dy) = (u * 18.0 - cx - 0.2 - 0.6 * rand(h), v * 18.0 - cy - 0.2 - 0.6 * rand(h ^ 1));
                        let dot = smoothstep(0.32, 0.18, (dx * dx + dy * dy).sqrt());
                        let keep = (rand(h ^ 2) < 0.45 * (1.0 - r)) as u8 as f32;
                        dot * keep
                    }
                };
                value.clamp(0.0, 1.0)
            })
            .collect();
        Mask { w: SIZE, h: SIZE, data }
    }
}

fn rand(h: u32) -> f32 {
    (hash(h) >> 8) as f32 / (1u32 << 24) as f32
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Smooth 2D value noise, 0..=1.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let corner = |dx: f32, dy: f32| rand(hash2(hash2(seed, (ix + dx) as i32 as u32), (iy + dy) as i32 as u32));
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * sx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

fn value_noise_1d(x: f32, seed: u32) -> f32 {
    value_noise(x, 0.5, seed)
}

/// Several octaves of value noise, 0..=1.
fn fbm(x: f32, y: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut total, mut f) = (0.0, 1.0, 0.0, 1.0);
    for o in 0..octaves {
        sum += value_noise(x * f, y * f, hash2(seed, o)) * amp;
        total += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_texture_tip_has_paint_and_gaps() {
        for tip in Tip::ALL.into_iter().skip(1) {
            let mask = tip.mask(0).unwrap();
            let painted = mask.data.iter().filter(|&&v| v > 0.5).count() as f32 / mask.data.len() as f32;
            assert!(painted > 0.02 && painted < 0.75, "{tip:?}: {painted:.2} of the tip is solid");
            assert_eq!(mask.sample(1.5, 0.0), 0.0, "nothing outside the tip");
        }
        assert!(Tip::Round.mask(0).is_none());
    }

    #[test]
    fn imported_tips_register_by_name_and_round_trip_through_keys() {
        // A 4×2 tip: paint on the left half only.
        let tip = register_tip("test half", vec![(4, 2, vec![1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0])]);
        assert_eq!(tip.name(), "test half");
        assert_eq!(Tip::from_key("image:test half"), Some(tip));
        assert_eq!(Tip::from_key("sponge"), Some(Tip::Sponge));
        assert_eq!(Tip::from_key(&Tip::DryBrush.key()), Some(Tip::DryBrush));
        let mask = tip.mask(7).unwrap();
        assert!(mask.sample(-0.6, 0.0) > 0.9 && mask.sample(0.6, 0.0) < 0.1, "left paints, right doesn't");
        assert_eq!(mask.sample(0.0, 0.9), 0.0, "the short side doesn't reach the tip's edge");
        unregister_tip("test half");
        assert_eq!(Tip::from_key("image:test half"), None);
    }
}
