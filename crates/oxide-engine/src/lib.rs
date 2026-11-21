//! Oxide's painting engine: documents, layers, brushes, strokes, undo, and files.
//!
//! This crate knows nothing about windows or UI. The app crate feeds it
//! pointer positions and displays the pixels it produces.

mod blend;
mod brush;
mod canvas;
mod document;
mod file;
mod history;
pub mod multicolor;
mod paint;
mod paper;
mod psd;
mod region;
pub mod selection;
pub use selection::{SelectMode, SelectShape};
mod spectral;
mod stroke;
mod tip;
mod tips;
mod transform;

pub use blend::{BlendMode, blend};
pub use brush::{Bristles, Brush, PaintMode, Rotation, TaperEnds};
pub use canvas::{Buffers, Canvas, Color, DabMode, DabStyle, Flow, MAX_RELIEF, ReliefDab, Wetness};
pub use document::{Clip, DEFAULT_DPI, Document, Layer, LayerGroup, LayerMask};
pub use file::FileError;
pub use paint::{Mixing, mix_paint};
/// Exposed for benchmarks (examples/bench_mix.rs).
pub mod spectral_bench {
    pub use crate::spectral::{Pigment, mix_pigments};
}
pub use paper::{Paper, PaperKind, PaperSettings, surface_acceptance};
pub use psd::PsdOptions;
pub use region::Region;
pub use stroke::{Preview, Stroke, StrokePoint};
pub use tip::DabTexture;
pub use tips::{Tip, TipImage, image_tips, register_tip, unregister_tip};
pub use transform::Transform;
