//! A standalone screen-capture tool: a `winit` + `softbuffer` window that
//! renders a `ratatui`-style monospace grid directly to its own pixel
//! surface, with no external terminal emulator involved.
//!
//! This crate is currently application-shaped rather than a general-purpose
//! library — [`run`] opens a window and drives its event loop until the
//! window is closed. The [`glyph`] module (glyph rasterization and the
//! monospace cell grid) is the reusable part and is documented as such.
//!
//! # Examples
//!
//! ```no_run
//! prtsc::run();
//! ```
#![warn(missing_docs)]

/// The application window and its event loop.
pub mod app;
/// Glyph rasterization and the monospace cell grid.
pub mod glyph;

/// Opens the application window and runs its event loop until closed.
///
/// See [`app::run`] for details.
pub use app::run;
