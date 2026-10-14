//! Oxide's command layer: every action on a painting as plain data
//! ([`Command`]), carried out by a [`Session`]. The same commands can be driven
//! by the app, a CLI script, a JSON control channel, or an MCP server (so AI
//! agents like Claude can paint).

pub mod command;
pub mod export;
pub mod hatch;
pub mod import;
pub mod palette;
pub mod session;
pub mod tip_import;
pub mod tools;

pub use command::Command;
pub use session::{Output, Session};
