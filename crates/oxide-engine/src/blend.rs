//! Layer blend modes (Multiply, Screen, Overlay, ...), with the standard
//! formulas from the W3C compositing spec (as used by Photoshop and others).
//!
//! A layer's color `Cs` meets the color below `Cb`; the mode's `B(Cb, Cs)`
//! is used where both have paint, and plain paint elsewhere:
//! `result = (1 − αb)·Cs·αs + (1 − αs)·Cb·αb + αs·αb·B(Cb, Cs)`.

/// How a layer's colors combine with what's below it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    // Darken
    Multiply,
    Darken,
    ColorBurn,
    // Lighten
    Screen,
    Lighten,
    ColorDodge,
    // Contrast
    Overlay,
    SoftLight,
    HardLight,
    // Comparison
    Difference,
    Exclusion,
    // Color components
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    pub const ALL: [BlendMode; 16] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Darken,
        BlendMode::ColorBurn,
        BlendMode::Screen,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Darken => "Darken",
            BlendMode::ColorBurn => "Color Burn",
            BlendMode::Screen => "Screen",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "Color Dodge",
            BlendMode::Overlay => "Overlay",
            BlendMode::SoftLight => "Soft Light",
            BlendMode::HardLight => "Hard Light",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            BlendMode::Hue => "Hue",
            BlendMode::Saturation => "Saturation",
            BlendMode::Color => "Color",
            BlendMode::Luminosity => "Luminosity",
        }
    }

    /// A short note for the menu.
    pub fn hint(self) -> &'static str {
        match self {
            BlendMode::Normal => "Paint covers what's below",
            BlendMode::Multiply => "Darkens, like a transparent glaze (white = no change)",
            BlendMode::Darken => "Keeps whichever is darker",
            BlendMode::ColorBurn => "Darkens with more contrast",
            BlendMode::Screen => "Lightens, like light projected (black = no change)",
            BlendMode::Lighten => "Keeps whichever is lighter",
            BlendMode::ColorDodge => "Brightens strongly, for glows",
            BlendMode::Overlay => "Darks darker, lights lighter (by what's below)",
            BlendMode::SoftLight => "A gentle Overlay",
            BlendMode::HardLight => "A strong Overlay (by this layer)",
            BlendMode::Difference => "Subtracts colors",
            BlendMode::Exclusion => "A softer Difference",
            BlendMode::Hue => "This layer's hue, below's saturation and lightness",
            BlendMode::Saturation => "This layer's saturation",
            BlendMode::Color => "This layer's hue and saturation (colorize)",
            BlendMode::Luminosity => "This layer's lightness",
        }
    }

    /// Photoshop's four-letter code (for PSD files).
    pub fn psd_key(self) -> &'static [u8; 4] {
        match self {
            BlendMode::Normal => b"norm",
            BlendMode::Multiply => b"mul ",
            BlendMode::Darken => b"dark",
            BlendMode::ColorBurn => b"idiv",
            BlendMode::Screen => b"scrn",
            BlendMode::Lighten => b"lite",
            BlendMode::ColorDodge => b"div ",
            BlendMode::Overlay => b"over",
            BlendMode::SoftLight => b"sLit",
            BlendMode::HardLight => b"hLit",
            BlendMode::Difference => b"diff",
            BlendMode::Exclusion => b"smud",
            BlendMode::Hue => b"hue ",
            BlendMode::Saturation => b"sat ",
            BlendMode::Color => b"colr",
            BlendMode::Luminosity => b"lum ",
        }
    }

    /// A key for files and agents ("multiply", "soft_light", ...).
    pub fn key(self) -> String {
        self.label().to_lowercase().replace(' ', "_")
    }

    pub fn from_key(key: &str) -> Option<BlendMode> {
        let key = key.trim().to_lowercase().replace([' ', '-'], "_");
        BlendMode::ALL.into_iter().find(|m| m.key() == key)
    }
}

/// Composite `src` (this layer, premultiplied) onto `dst` (below, premultiplied).
pub fn blend(src: [f32; 4], dst: [f32; 4], mode: BlendMode) -> [f32; 4] {
    let (sa, da) = (src[3], dst[3]);
    if mode == BlendMode::Normal || sa <= 0.0 || da <= 0.0 {
        let k = 1.0 - sa;
        return [src[0] + dst[0] * k, src[1] + dst[1] * k, src[2] + dst[2] * k, sa + da * k];
    }
    // Straight colors for the formula.
    let cs = [src[0] / sa, src[1] / sa, src[2] / sa];
    let cb = [dst[0] / da, dst[1] / da, dst[2] / da];
    let mixed = mix(cb, cs, mode);
    let both = sa * da;
    let mut out = [0.0; 4];
    for c in 0..3 {
        out[c] = src[c] * (1.0 - da) + dst[c] * (1.0 - sa) + both * mixed[c].clamp(0.0, 1.0);
    }
    out[3] = sa + da - both;
    out
}

/// The mode's color where both layers have paint (straight colors).
fn mix(cb: [f32; 3], cs: [f32; 3], mode: BlendMode) -> [f32; 3] {
    let each = |f: fn(f32, f32) -> f32| [f(cb[0], cs[0]), f(cb[1], cs[1]), f(cb[2], cs[2])];
    match mode {
        BlendMode::Normal => cs,
        BlendMode::Multiply => each(|b, s| b * s),
        BlendMode::Screen => each(|b, s| b + s - b * s),
        BlendMode::Overlay => each(|b, s| hard_light(s, b)),
        BlendMode::Darken => each(f32::min),
        BlendMode::Lighten => each(f32::max),
        BlendMode::ColorDodge => each(|b, s| if b <= 0.0 { 0.0 } else if s >= 1.0 { 1.0 } else { (b / (1.0 - s)).min(1.0) }),
        BlendMode::ColorBurn => each(|b, s| if b >= 1.0 { 1.0 } else if s <= 0.0 { 0.0 } else { 1.0 - ((1.0 - b) / s).min(1.0) }),
        BlendMode::HardLight => each(hard_light),
        BlendMode::SoftLight => each(soft_light),
        BlendMode::Difference => each(|b, s| (b - s).abs()),
        BlendMode::Exclusion => each(|b, s| b + s - 2.0 * b * s),
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
    }
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { let t = 2.0 * s - 1.0; b + t - b * t }
}

fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

// Non-separable modes (from the W3C compositing spec).

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let (n, x) = (c[0].min(c[1]).min(c[2]), c[0].max(c[1]).max(c[2]));
    let mut c = c;
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let (max, min) = (c[0].max(c[1]).max(c[2]), c[0].min(c[1]).min(c[2]));
    if max <= min {
        return [0.0; 3];
    }
    c.map(|v| (v - min) / (max - min) * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(r: f32, g: f32, b: f32) -> [f32; 4] {
        [r, g, b, 1.0]
    }

    fn close(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn classic_modes_on_opaque_colors() {
        let below = opaque(0.5, 0.25, 1.0);
        let gray = opaque(0.5, 0.5, 0.5);
        assert!(close(blend(gray, below, BlendMode::Multiply), opaque(0.25, 0.125, 0.5)));
        assert!(close(blend(gray, below, BlendMode::Screen), opaque(0.75, 0.625, 1.0)));
        assert!(close(blend(opaque(1.0, 1.0, 1.0), below, BlendMode::Multiply), below), "white multiplies to no change");
        assert!(close(blend(opaque(0.0, 0.0, 0.0), below, BlendMode::Screen), below), "black screens to no change");
        assert!(close(blend(gray, below, BlendMode::Darken), opaque(0.5, 0.25, 0.5)));
        assert!(close(blend(gray, below, BlendMode::Difference), opaque(0.0, 0.25, 0.5)));
        // Overlay with 50% gray changes nothing.
        assert!(close(blend(gray, below, BlendMode::Overlay), below));
    }

    #[test]
    fn color_and_luminosity_swap_parts() {
        let red = opaque(1.0, 0.0, 0.0);
        let gray = opaque(0.4, 0.4, 0.4);
        let colored = blend(red, gray, BlendMode::Color);
        assert!(colored[0] > colored[1] && (lum([colored[0], colored[1], colored[2]]) - 0.4).abs() < 1e-3, "red at gray's lightness");
        let lit = blend(gray, red, BlendMode::Luminosity);
        assert!((lum([lit[0], lit[1], lit[2]]) - 0.4).abs() < 1e-3 && lit[0] > lit[1], "red hue at gray's lightness");
    }

    #[test]
    fn where_only_one_has_paint_it_shows_as_is() {
        let paint = [0.3, 0.1, 0.0, 0.5];
        assert!(close(blend(paint, [0.0; 4], BlendMode::Multiply), paint), "over nothing");
        assert!(close(blend([0.0; 4], opaque(0.2, 0.4, 0.6), BlendMode::Screen), opaque(0.2, 0.4, 0.6)), "nothing on top");
    }

    #[test]
    fn keys_round_trip() {
        for m in BlendMode::ALL {
            assert_eq!(BlendMode::from_key(&m.key()), Some(m));
        }
        assert_eq!(BlendMode::from_key("Soft Light"), Some(BlendMode::SoftLight));
    }
}
