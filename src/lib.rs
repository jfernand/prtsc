//! A standalone screen-capture tool: a `winit` + `softbuffer` window that
//! renders a `ratatui`-style monospace grid directly to its own pixel
//! surface, with no external terminal emulator involved.
//!
//! This crate is currently application-shaped rather than a general-purpose
//! library — [`run`] opens a window and drives its event loop until the
//! window is closed. The [`glyph`] and [`backend`] modules (glyph
//! rasterization/the monospace cell grid, and the `ratatui` `Backend` built
//! on top of it) are the reusable parts and are documented as such.
//!
//! # Examples
//!
//! ```no_run
//! prtsc::run();
//! ```
#![warn(missing_docs)]

/// The application window and its event loop.
pub mod app;
/// A `ratatui` `Backend` that renders into the window directly.
pub mod backend;
/// Glyph rasterization and the monospace cell grid.
pub mod glyph;

/// Opens the application window and runs its event loop until closed.
///
/// See [`app::run`] for details.
pub use app::run;
