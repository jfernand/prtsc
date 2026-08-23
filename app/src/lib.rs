//! A standalone screen-capture tool: a `winit` + `softbuffer` window that
//! renders a `ratatui`-style monospace grid directly to its own pixel
//! surface, with no external terminal emulator involved.
//!
//! This crate is the application shell — [`run`] opens a window and drives
//! its event loop until the window is closed. The reusable rendering layer
//! (glyph rasterization and the `ratatui` `Backend` built on top of it)
//! lives in the separate `prtsc-backend` crate this one depends on.
//!
//! # Examples
//!
//! ```no_run
//! prtsc::run();
//! ```
#![warn(missing_docs)]

/// The application window and its event loop.
pub mod app;
// Key-event-to-app-action mapping; internal to `app`, not part of the
// reusable public API.
mod input;

/// Opens the application window and runs its event loop until closed.
///
/// See [`app::run`] for details.
pub use app::run;
