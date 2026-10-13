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
mod paper;
mod region;
mod stroke;
mod tip;

pub use brush::{Brush, Rotation};
pub use canvas::{Canvas, Color, DabMode, DabStyle, Wetness};
pub use document::{DEFAULT_DPI, Document, Layer};
pub use file::FileError;
pub use paint::mix_paint;
pub use paper::{Paper, PaperKind, PaperSettings, surface_acceptance};
pub use region::Region;
pub use stroke::{Stroke, StrokePoint};
pub use tip::DabTexture;
