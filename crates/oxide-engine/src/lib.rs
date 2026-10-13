//! Oxide's painting engine: documents, layers, brushes, strokes, undo, and files.
//!
//! This crate knows nothing about windows or UI. The app crate feeds it
//! pointer positions and displays the pixels it produces.

mod brush;
mod canvas;
mod document;
mod file;
mod history;
mod paint;
pub mod multicolor;
mod paper;
mod psd;
mod region;
mod spectral;
mod stroke;
mod tip;
mod tips;
mod transform;

pub use brush::{Bristles, Brush, PaintMode, Rotation};
pub use canvas::{Buffers, Canvas, Color, DabMode, DabStyle, MAX_RELIEF, ReliefDab, Wetness};
pub use document::{DEFAULT_DPI, Document, Layer, LayerGroup};
pub use file::FileError;
pub use paint::{Mixing, mix_paint};
/// Exposed for benchmarks (examples/bench_mix.rs).
pub mod spectral_bench {
    pub use crate::spectral::{Pigment, mix_pigments};
}
pub use psd::PsdOptions;
pub use paper::{Paper, PaperKind, PaperSettings, surface_acceptance};
pub use region::Region;
pub use stroke::{Stroke, StrokePoint};
pub use tip::DabTexture;
pub use tips::{Tip, TipImage, image_tips, register_tip, unregister_tip};
pub use transform::Transform;
