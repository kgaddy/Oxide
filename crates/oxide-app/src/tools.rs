//! Tools, brush presets, and the starter color swatches.

use oxide_engine::{Brush, Color};

/// The tool that the left mouse button / pen uses on the canvas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Eraser,
    Picker,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Brush, Tool::Eraser, Tool::Picker];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Picker => "Picker",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Brush => "B",
            Tool::Eraser => "E",
            Tool::Picker => "I",
        }
    }
}

/// Tools shown in the panel but not built yet, so the roadmap is visible.
pub const FUTURE_TOOLS: [&str; 5] = ["Knife", "Blend", "Smudge", "Fill", "Select"];

/// The shape of a brush, without its color (presets keep your current color).
pub struct BrushPreset {
    pub name: &'static str,
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub spacing: f32,
}

impl BrushPreset {
    pub fn apply(&self, brush: &mut Brush) {
        brush.radius = self.radius;
        brush.hardness = self.hardness;
        brush.opacity = self.opacity;
        brush.spacing = self.spacing;
    }

    pub fn matches(&self, brush: &Brush) -> bool {
        self.radius == brush.radius
            && self.hardness == brush.hardness
            && self.opacity == brush.opacity
            && self.spacing == brush.spacing
    }
}

pub const PRESETS: [BrushPreset; 8] = [
    BrushPreset { name: "Round Hard", radius: 10.0, hardness: 0.95, opacity: 1.0, spacing: 0.1 },
    BrushPreset { name: "Round Soft", radius: 18.0, hardness: 0.2, opacity: 0.8, spacing: 0.08 },
    BrushPreset { name: "Ink Fine", radius: 2.0, hardness: 0.95, opacity: 1.0, spacing: 0.05 },
    BrushPreset { name: "Ink Bold", radius: 5.0, hardness: 0.9, opacity: 1.0, spacing: 0.05 },
    BrushPreset { name: "Pencil", radius: 1.5, hardness: 0.5, opacity: 0.6, spacing: 0.1 },
    BrushPreset { name: "Marker", radius: 12.0, hardness: 0.85, opacity: 0.35, spacing: 0.05 },
    BrushPreset { name: "Airbrush", radius: 40.0, hardness: 0.0, opacity: 0.06, spacing: 0.05 },
    BrushPreset { name: "Dotted", radius: 4.0, hardness: 0.9, opacity: 1.0, spacing: 1.0 },
];

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
