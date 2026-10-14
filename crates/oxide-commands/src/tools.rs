//! Tools (each painting tool is a *medium*), the brush shape each tool
//! uses, and the starter color swatches.
//!
//! A **brush shape** (size, hardness, roundness, angle, spacing) says what
//! the tip looks like. A **medium** (oils & acrylics, ink, pencil, ...) says
//! how the paint behaves. Any medium can use any brush preset.

use oxide_engine::{Bristles, Brush, Color, PaintMode, Rotation, TaperEnds, Tip};

/// The tool that the left mouse button / pen uses on the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    OilsAcrylics,
    Ink,
    Pencil,
    Marker,
    Airbrush,
    Eraser,
    Smudge,
    /// Palette knife: spreads thick, smooth paint, or scrapes wet paint off.
    Knife,
    /// Engraving-style hatching: draw two guide curves and evenly spaced,
    /// tapered lines fill the space between them (ink, dry).
    Hatch,
    /// Make selections (rectangle, ellipse, lasso); painting stays inside them.
    Select,
    Picker,
}

impl Tool {
    pub const COUNT: usize = 11;
    pub const ALL: [Tool; Tool::COUNT] = [
        Tool::OilsAcrylics,
        Tool::Ink,
        Tool::Pencil,
        Tool::Marker,
        Tool::Airbrush,
        Tool::Eraser,
        Tool::Smudge,
        Tool::Knife,
        Tool::Hatch,
        Tool::Select,
        Tool::Picker,
    ];

    /// Position in [`Tool::ALL`], used to index per-tool settings.
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::OilsAcrylics => "Oils & Acrylics",
            Tool::Ink => "Ink",
            Tool::Pencil => "Pencil",
            Tool::Marker => "Marker",
            Tool::Airbrush => "Airbrush",
            Tool::Eraser => "Eraser",
            Tool::Smudge => "Smudge",
            Tool::Knife => "Knife",
            Tool::Hatch => "Hatch",
            Tool::Select => "Select",
            Tool::Picker => "Picker",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::OilsAcrylics => "B",
            Tool::Ink => "N",
            Tool::Pencil => "P",
            Tool::Marker => "M",
            // (A toggles Erase mode for oils.)
            Tool::Airbrush => "U",
            Tool::Eraser => "E",
            Tool::Smudge => "S",
            Tool::Knife => "K",
            Tool::Hatch => "H",
            Tool::Select => "L",
            Tool::Picker => "I",
        }
    }

    /// Tools that put the current color on the canvas.
    pub fn paints(self) -> bool {
        matches!(self, Tool::OilsAcrylics | Tool::Ink | Tool::Pencil | Tool::Marker | Tool::Airbrush | Tool::Knife | Tool::Hatch)
    }

    /// Tools whose paint is wet (picks up and drags paint, can run dry).
    pub fn is_wet_medium(self) -> bool {
        matches!(self, Tool::OilsAcrylics | Tool::Knife)
    }

    /// What each tool starts with.
    pub fn default_state(self) -> ToolState {
        // `grain` = how much the medium feels the paper texture.
        let shape = |radius, hardness, spacing, grain| BrushShape { radius, hardness, spacing, grain, ..BrushShape::default() };
        let (shape, opacity, loading, oiliness) = match self {
            // Oils start with a natural, slightly worn brush.
            Tool::OilsAcrylics => {
                let natural = BrushShape { roughness: 0.35, streaks: 0.45, lift_off: 0.6, ..shape(14.0, 0.6, 0.04, 0.6) };
                (natural, 1.0, 0.85, 0.4)
            }
            Tool::Ink => (shape(3.0, 0.95, 0.05, 0.2), 1.0, 1.0, 0.0),
            Tool::Pencil => (shape(1.5, 0.5, 0.1, 0.9), 0.6, 1.0, 0.0),
            Tool::Marker => (shape(12.0, 0.85, 0.05, 0.3), 0.35, 1.0, 0.0),
            Tool::Airbrush => (shape(40.0, 0.0, 0.05, 0.1), 0.06, 1.0, 0.0),
            Tool::Eraser => (shape(16.0, 0.8, 0.08, 0.0), 1.0, 1.0, 0.0),
            // Smudge keeps its strength in `oiliness`.
            Tool::Smudge => (shape(14.0, 0.5, 0.05, 0.0), 1.0, 0.0, 0.8),
            // A long, thin, hard blade that stays square to the stroke; oily so it drags paint.
            Tool::Knife => {
                let blade = BrushShape { roundness: 0.15, rotation: Rotation::FollowStroke, ..shape(22.0, 1.0, 0.03, 0.5) };
                (blade, 1.0, 0.9, 0.7)
            }
            // Hatch lines: crisp engraver lines that taper at both ends.
            Tool::Hatch => (BrushShape { taper: 0.6, ..shape(2.0, 1.0, 0.05, 0.2) }, 1.0, 1.0, 0.0),
            Tool::Picker | Tool::Select => (shape(1.0, 1.0, 0.1, 0.0), 1.0, 1.0, 0.0),
        };
        // How each medium responds to pen pressure (size, opacity), like the
        // real thing: ink and oils thicken, pencil and airbrush darken.
        let (size_pressure, opacity_pressure) = match self {
            Tool::OilsAcrylics => (0.6, 0.3),
            // Ink: a light touch thins the line right down (to 10%), for tapers.
            Tool::Ink => (1.0, 0.0),
            // Hatch: pressure carries the Fade toward the second rail.
            Tool::Hatch => (1.0, 0.0),
            Tool::Pencil => (0.3, 0.8),
            Tool::Marker => (0.2, 0.4),
            Tool::Airbrush => (0.0, 1.0),
            Tool::Eraser => (0.6, 0.3),
            Tool::Smudge => (0.4, 0.6),
            Tool::Knife => (0.2, 0.0),
            Tool::Picker | Tool::Select => (0.0, 0.0),
        };
        let shape = BrushShape { size_pressure, opacity_pressure, ..shape };
        // Oils hold paint for a while; everything else never runs out.
        let length = match self {
            Tool::OilsAcrylics => 0.75,
            Tool::Knife => 0.6,
            _ => 1.0,
        };
        // Oils go on with some body, knife paint thick; other media are flat.
        let thickness = match self {
            Tool::OilsAcrylics => 0.5,
            Tool::Knife => 0.9,
            _ => 0.0,
        };
        ToolState { shape, opacity, loading, oiliness, length, paint_mode: PaintMode::PaintMix, erase: false, thickness, scrape: false }
    }
}

/// Tools shown in the panel but not built yet, so the roadmap is visible.
pub const FUTURE_TOOLS: [&str; 3] = ["Pastel", "Blend", "Fill"];

/// What the brush tip looks like. Brush presets are shapes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushShape {
    /// Radius in canvas pixels.
    pub radius: f32,
    /// 0 = soft, fuzzy edge; 1 = crisp edge.
    pub hardness: f32,
    /// Distance between dabs, as a fraction of the brush width.
    pub spacing: f32,
    /// 1 = round; lower = flat, like a chisel or calligraphy nib.
    pub roundness: f32,
    /// Rotation of the tip in degrees.
    pub angle: f32,
    /// Ragged, grainy edges (0 = perfect outline).
    pub roughness: f32,
    /// Bristle streaks along the stroke (0 = smooth).
    pub streaks: f32,
    /// How much the stroke end thins and breaks up as the brush lifts (0 = off).
    pub lift_off: f32,
    /// Both ends of every stroke come to a clean point, like an engraved line (0 = off).
    pub taper: f32,
    /// Which ends the taper points (both, or just the start or the end).
    pub taper_ends: TaperEnds,
    /// How much the paper texture shows in the stroke (0 = ignores the paper).
    pub grain: f32,
    /// How the tip turns while painting (fixed, follow the stroke, pen...).
    pub rotation: Rotation,
    /// How much pen pressure changes the size (0 = not at all).
    pub size_pressure: f32,
    /// How much pen pressure changes the opacity (0 = not at all).
    pub opacity_pressure: f32,
    /// Some = a bristle brush (separate hairs); None = a stamped tip.
    pub bristles: Option<Bristles>,
    /// The stamped tip: a smooth ellipse or a texture (sponge, chalk, ...).
    pub tip: Tip,
    /// Per-dab randomness 0..=1: rotation, size, scatter off the path.
    pub jitter_angle: f32,
    pub jitter_size: f32,
    pub scatter: f32,
}

impl Default for BrushShape {
    fn default() -> Self {
        BrushShape {
            radius: 12.0,
            hardness: 0.7,
            spacing: 0.05,
            roundness: 1.0,
            angle: 0.0,
            roughness: 0.0,
            streaks: 0.0,
            lift_off: 0.0,
            taper: 0.0,
            taper_ends: TaperEnds::Both,
            grain: 0.0,
            rotation: Rotation::None,
            size_pressure: 0.0,
            opacity_pressure: 0.0,
            bristles: None,
            tip: Tip::Round,
            jitter_angle: 0.0,
            jitter_size: 0.0,
            scatter: 0.0,
        }
    }
}

/// Everything a tool remembers: its brush shape and medium settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolState {
    pub shape: BrushShape,
    pub opacity: f32,
    /// Wet media only: how much paint the brush lays down.
    pub loading: f32,
    /// Wet media: how much paint gets picked up and dragged.
    /// Smudge: how strongly it drags.
    pub oiliness: f32,
    /// Wet media: how long the paint lasts (1 = never runs out).
    pub length: f32,
    /// Wet media: Paint / Paint & Mix / Paint & Blend / Blend (keys 1–4).
    pub paint_mode: PaintMode,
    /// Wet media: Erase mode (key 5, or A).
    pub erase: bool,
    /// Wet media: how thick the paint goes on (impasto), 0 = flat.
    pub thickness: f32,
    /// Knife: scrape wet paint off instead of spreading paint.
    pub scrape: bool,
}

impl ToolState {
    /// Combine this tool's settings with a color into an engine brush.
    pub fn brush(&self, tool: Tool, color: Color) -> Brush {
        let s = self.shape;
        let mut brush = Brush {
            color,
            radius: s.radius,
            hardness: s.hardness,
            spacing: s.spacing,
            roundness: s.roundness,
            angle: s.angle,
            rotation: s.rotation,
            size_pressure: s.size_pressure,
            opacity_pressure: s.opacity_pressure,
            roughness: s.roughness,
            streaks: s.streaks,
            lift_off: s.lift_off,
            taper: s.taper,
            taper_ends: s.taper_ends,
            grain: s.grain,
            bristles: s.bristles,
            tip: s.tip,
            jitter_angle: s.jitter_angle,
            jitter_size: s.jitter_size,
            scatter: s.scatter,
            opacity: self.opacity,
            ..Brush::default()
        };
        if tool.is_wet_medium() {
            brush.loading = self.loading;
            brush.oiliness = self.oiliness;
            brush.length = self.length;
            brush.paint_mode = self.paint_mode;
            brush.wet_medium = true;
            brush.impasto = self.thickness;
        }
        if tool == Tool::Knife {
            brush.knife = true;
            if self.scrape {
                // An empty knife: no paint of its own; it scrapes and drags what's wet.
                brush.loading = 0.0;
                brush.paint_mode = PaintMode::Blend;
            }
        }
        if tool == Tool::Smudge {
            // No paint of its own, just drags what's there (wet or dry).
            brush.loading = 0.0;
            brush.oiliness = self.oiliness;
            brush.pickup_dry = true;
        }
        brush
    }
}

/// A starter palette of classic artist pigments (approximate screen colors).
pub const SWATCHES: [(&str, Color); 18] = [
    ("Titanium White", Color::rgb(0.97, 0.97, 0.95)),
    ("Ivory Black", Color::rgb(0.09, 0.09, 0.09)),
    ("Payne's Grey", Color::rgb(0.22, 0.26, 0.31)),
    ("Lemon Yellow", Color::rgb(0.98, 0.92, 0.25)),
    ("Cadmium Yellow", Color::rgb(0.99, 0.78, 0.0)),
    ("Yellow Ochre", Color::rgb(0.80, 0.60, 0.22)),
    ("Raw Sienna", Color::rgb(0.78, 0.50, 0.24)),
    ("Burnt Sienna", Color::rgb(0.55, 0.25, 0.14)),
    ("Burnt Umber", Color::rgb(0.34, 0.22, 0.15)),
    ("Cadmium Orange", Color::rgb(0.96, 0.47, 0.10)),
    ("Cadmium Red", Color::rgb(0.86, 0.13, 0.12)),
    ("Alizarin Crimson", Color::rgb(0.62, 0.08, 0.16)),
    ("Dioxazine Purple", Color::rgb(0.33, 0.16, 0.42)),
    ("Ultramarine Blue", Color::rgb(0.16, 0.20, 0.62)),
    ("Cerulean Blue", Color::rgb(0.16, 0.48, 0.73)),
    ("Phthalo Green", Color::rgb(0.0, 0.36, 0.30)),
    ("Sap Green", Color::rgb(0.31, 0.45, 0.16)),
    ("Viridian", Color::rgb(0.25, 0.51, 0.43)),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_indexes_match_all() {
        for (i, tool) in Tool::ALL.iter().enumerate() {
            assert_eq!(tool.index(), i);
        }
    }

    #[test]
    fn only_oils_are_wet_and_smudge_adds_no_paint() {
        let red = Color::rgb(1.0, 0.0, 0.0);
        let oils = Tool::OilsAcrylics.default_state().brush(Tool::OilsAcrylics, red);
        assert!(oils.is_wet());
        // Same settings on Ink stay dry: wetness comes from the medium.
        let ink = Tool::OilsAcrylics.default_state().brush(Tool::Ink, red);
        assert!(!ink.is_wet());
        let smudge = Tool::Smudge.default_state().brush(Tool::Smudge, red);
        assert_eq!(smudge.loading, 0.0);
    }
}
