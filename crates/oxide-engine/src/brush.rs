use crate::Color;

/// Settings for a simple round brush.
#[derive(Clone, Debug)]
pub struct Brush {
    pub color: Color,
    /// Radius in canvas pixels.
    pub radius: f32,
    /// 0.0 = very soft, fuzzy edge; 1.0 = crisp, hard edge.
    pub hardness: f32,
    /// How strongly each dab covers what's underneath (0..=1).
    pub opacity: f32,
    /// Distance between dabs, as a fraction of the diameter.
    /// Smaller = smoother strokes but more work.
    pub spacing: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Brush {
            color: Color::BLACK,
            radius: 8.0,
            hardness: 0.9,
            opacity: 1.0,
            spacing: 0.1,
        }
    }
}

impl Brush {
    /// How much paint a pixel gets at `dist` pixels from the dab center:
    /// 1.0 in the solid core, fading to 0.0 at the edge.
    pub fn coverage(&self, dist: f32) -> f32 {
        let r = self.radius.max(0.5);
        if dist >= r {
            return 0.0;
        }
        // The solid core is `hardness` of the radius; the rest is the soft edge.
        // Keep at least ~1px of fade so even hard brushes are antialiased.
        let core = (r * self.hardness.clamp(0.0, 1.0)).min(r - 1.0).max(0.0);
        if dist <= core {
            return 1.0;
        }
        let t = (dist - core) / (r - core);
        // Smoothstep gives a gentle, natural-looking falloff.
        1.0 - t * t * (3.0 - 2.0 * t)
    }

    /// Distance in pixels between dabs along a stroke.
    pub fn step(&self) -> f32 {
        (self.radius * 2.0 * self.spacing).max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_full_at_center_and_zero_outside() {
        let brush = Brush { radius: 10.0, hardness: 0.5, ..Brush::default() };
        assert_eq!(brush.coverage(0.0), 1.0);
        assert_eq!(brush.coverage(10.0), 0.0);
        let mid = brush.coverage(7.5);
        assert!(mid > 0.0 && mid < 1.0);
    }
}
