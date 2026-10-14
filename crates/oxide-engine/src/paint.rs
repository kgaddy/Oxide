//! How two paints combine.
//!
//! Everything that mixes colors (wet brushes picking up paint, smudging,
//! the dirty brush, later the mixing palette) goes through [`mix_paint`].

use crate::Color;
use crate::spectral::mix_srgb;

/// How colors mix (the Edit → "Natural Colors" switch).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mixing {
    /// Like real paint (Kubelka-Munk pigment mixing): blue + yellow = green.
    #[default]
    Pigment,
    /// Like light on a screen (straight RGB blend): blue + yellow = gray.
    Rgb,
}

/// Mix `t` (0..=1) of paint `b` into paint `a`.
pub fn mix_paint(a: Color, b: Color, t: f32, mixing: Mixing) -> Color {
    let t = t.clamp(0.0, 1.0);
    let [r, g, b2] = match mixing {
        Mixing::Pigment => mix_srgb([a.r, a.g, a.b], [b.r, b.g, b.b], t),
        Mixing::Rgb => [a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t],
    };
    Color { r, g, b: b2, a: 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_ends_are_the_inputs() {
        let red = Color::rgb(1.0, 0.0, 0.0);
        let blue = Color::rgb(0.0, 0.0, 1.0);
        for mixing in [Mixing::Pigment, Mixing::Rgb] {
            assert_eq!(mix_paint(red, blue, 0.0, mixing), red);
            assert_eq!(mix_paint(red, blue, 1.0, mixing), blue);
        }
    }

    #[test]
    fn natural_colors_switch_changes_the_result() {
        let blue = Color::rgb(0.0, 0.129, 0.522);
        let yellow = Color::rgb(0.988, 0.827, 0.0);
        let paint = mix_paint(blue, yellow, 0.5, Mixing::Pigment);
        let light = mix_paint(blue, yellow, 0.5, Mixing::Rgb);
        assert!(paint.g > paint.r && paint.g > paint.b, "pigment: green {paint:?}");
        assert!(light.r > light.b, "rgb: a grayish olive/beige, not green {light:?}");
    }
}
