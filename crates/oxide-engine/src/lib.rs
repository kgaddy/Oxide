//! Oxide's painting engine: the canvas, brushes, and strokes.
//!
//! This crate knows nothing about windows or UI. The app crate feeds it
//! pointer positions and displays the pixels it produces.

mod brush;
mod canvas;
mod stroke;

pub use brush::Brush;
pub use canvas::{Canvas, Color};
pub use stroke::Stroke;
