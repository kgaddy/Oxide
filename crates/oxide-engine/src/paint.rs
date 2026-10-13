//! How two paints combine.
//!
//! Everything that mixes colors (wet brushes picking up paint, smudging,
//! later the mixing palette) goes through [`mix_paint`], so upgrading from
//! simple RGB blending to real pigment mixing (Kubelka-Munk, milestone 5,
//! where blue + yellow = green) only needs to change this one function.

use crate::Color;

/// Mix `t` (0..=1) of paint `b` into paint `a`.
///
/// For now this is a straight RGB blend. Real paint mixes differently:
/// blue + yellow here gives gray, not green. That's milestone 5.
pub fn mix_paint(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_ends_are_the_inputs() {
        let red = Color::rgb(1.0, 0.0, 0.0);
        let blue = Color::rgb(0.0, 0.0, 1.0);
        assert_eq!(mix_paint(red, blue, 0.0), red);
        assert_eq!(mix_paint(red, blue, 1.0), blue);
    }
}
