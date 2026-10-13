use crate::Color;

/// How the brush tip turns while you paint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Rotation {
    /// The tip keeps its angle (e.g. a calligraphy nib).
    #[default]
    None,
    /// The tip turns to follow the direction of the stroke (e.g. a flat brush).
    FollowStroke,
    /// The tip turns toward the direction the pen is tilted.
    PenTilt,
    /// The tip turns with the pen's barrel rotation (needs a pen that reports it).
    PenRotation,
}

impl Rotation {
    pub const ALL: [Rotation; 4] = [Rotation::None, Rotation::FollowStroke, Rotation::PenTilt, Rotation::PenRotation];

    pub fn label(self) -> &'static str {
        match self {
            Rotation::None => "None",
            Rotation::FollowStroke => "Follow stroke",
            Rotation::PenTilt => "Pen tilt",
            Rotation::PenRotation => "Pen rotation",
        }
    }

    /// Does this mode use pen data (and act like `None` with a mouse)?
    pub fn needs_pen(self) -> bool {
        matches!(self, Rotation::PenTilt | Rotation::PenRotation)
    }
}

/// Settings for a brush tip.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub color: Color,
    /// Radius in canvas pixels (along the tip's long axis).
    pub radius: f32,
    /// 0.0 = very soft, fuzzy edge; 1.0 = crisp, hard edge.
    pub hardness: f32,
    /// How strongly each dab covers what's underneath (0..=1).
    pub opacity: f32,
    /// Distance between dabs, as a fraction of the diameter.
    /// Smaller = smoother strokes but more work.
    pub spacing: f32,
    /// 1.0 = round tip; smaller = flatter ellipse, like a chisel or
    /// calligraphy nib.
    pub roundness: f32,
    /// Rotation of the tip in degrees (only visible when roundness < 1).
    /// With [`Rotation::FollowStroke`], this is relative to the stroke:
    /// 0 = broad side across the stroke (a wide mark), 90 = edge-on (thin).
    pub angle: f32,
    /// How the tip turns while painting.
    pub rotation: Rotation,
    /// How much paint the brush holds (0..=1). 1 = never runs out; lower
    /// values make the stroke fade and run dry, like a real brush.
    pub loading: f32,
    /// How much the brush picks up and drags the paint already on the
    /// canvas (0..=1). 0 = plain paint; higher = wet, blending, oily paint.
    pub oiliness: f32,
    /// Ragged, grainy edges instead of a perfect outline (0..=1).
    pub roughness: f32,
    /// Bristle streaks running along the stroke (0..=1).
    pub streaks: f32,
    /// How much the end of a stroke thins, fades, and breaks up when the
    /// brush lifts off the canvas (0..=1).
    pub lift_off: f32,
    /// Pick up dry paint as well as wet (the Smudge tool works on dry paint).
    pub pickup_dry: bool,
    /// How much pen pressure controls the size (0 = not at all, 1 = a light
    /// touch makes a tiny mark).
    pub size_pressure: f32,
    /// How much pen pressure controls the opacity.
    pub opacity_pressure: f32,
    /// How strongly the paper texture affects this brush (0..=1): high =
    /// catches the tooth and skips valleys (pencil, dry brush); 0 = ignores it.
    pub grain: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Brush {
            color: Color::BLACK,
            radius: 8.0,
            hardness: 0.9,
            opacity: 1.0,
            spacing: 0.1,
            roundness: 1.0,
            angle: 0.0,
            rotation: Rotation::None,
            loading: 1.0,
            oiliness: 0.0,
            roughness: 0.0,
            streaks: 0.0,
            lift_off: 0.0,
            pickup_dry: false,
            size_pressure: 0.0,
            opacity_pressure: 0.0,
            grain: 0.0,
        }
    }
}

impl Brush {
    /// How much paint a pixel gets at offset (dx, dy) from the dab center:
    /// 1.0 in the solid core, fading to 0.0 at the edge.
    pub fn coverage(&self, dx: f32, dy: f32) -> f32 {
        let r = self.radius.max(0.5);

        // Rotate the offset into the tip's own frame, then stretch the short
        // axis so the ellipse becomes a circle we can measure distance in.
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let u = dx * cos + dy * sin;
        let v = (-dx * sin + dy * cos) / self.roundness.clamp(0.05, 1.0);
        let dist = (u * u + v * v).sqrt();
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

    /// The tip's actual angle (degrees) for a stroke moving in direction
    /// `dir` (a unit vector), or `None` if the direction isn't known yet.
    pub fn tip_angle(&self, dir: Option<(f32, f32)>) -> f32 {
        match (self.rotation, dir) {
            // Broad side across the stroke: the tip's long axis is
            // perpendicular to the direction of travel.
            (Rotation::FollowStroke, Some((dx, dy))) => self.angle + dy.atan2(dx).to_degrees() + 90.0,
            // Pen modes get their angle from the pen (see `StrokePoint`);
            // without pen data they behave like a fixed tip.
            _ => self.angle,
        }
    }

    /// Size multiplier for a given pen pressure (0..=1).
    pub fn pressure_size(&self, pressure: f32) -> f32 {
        // Never vanish completely: the lightest touch still leaves a fine line.
        1.0 - self.size_pressure.clamp(0.0, 1.0) * (1.0 - pressure.clamp(0.0, 1.0)) * 0.9
    }

    /// Opacity multiplier for a given pen pressure (0..=1).
    pub fn pressure_opacity(&self, pressure: f32) -> f32 {
        1.0 - self.opacity_pressure.clamp(0.0, 1.0) * (1.0 - pressure.clamp(0.0, 1.0))
    }

    /// Wet brushes interact with the paint already on the canvas.
    pub fn is_wet(&self) -> bool {
        self.oiliness > 0.0 || self.loading < 1.0
    }

    /// Distance in pixels between dabs along a stroke.
    pub fn step(&self) -> f32 {
        // Flat tips are thinner across, so space dabs by the thin side too.
        let size = self.radius * 2.0 * self.roundness.clamp(0.05, 1.0).max(0.25);
        (size * self.spacing).max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_is_full_at_center_and_zero_outside() {
        let brush = Brush { radius: 10.0, hardness: 0.5, ..Brush::default() };
        assert_eq!(brush.coverage(0.0, 0.0), 1.0);
        assert_eq!(brush.coverage(10.0, 0.0), 0.0);
        let mid = brush.coverage(7.5, 0.0);
        assert!(mid > 0.0 && mid < 1.0);
    }

    #[test]
    fn flat_tip_is_thin_across_and_wide_along() {
        // Roundness 0.2, angle 0: wide horizontally, thin vertically.
        let brush = Brush { radius: 10.0, roundness: 0.2, hardness: 1.0, ..Brush::default() };
        assert!(brush.coverage(8.0, 0.0) > 0.0);
        assert_eq!(brush.coverage(0.0, 4.0), 0.0);

        // Rotated 90°: now it's tall and thin.
        let rotated = Brush { angle: 90.0, ..brush };
        assert!(rotated.coverage(0.0, 8.0) > 0.0);
        assert_eq!(rotated.coverage(4.0, 0.0), 0.0);
    }
}
