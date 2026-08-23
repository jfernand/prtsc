//! Glyph rasterization and a `ratatui` [`backend::Backend`](ratatui::backend::Backend)
//! that renders directly into a `winit` window via `softbuffer` — no
//! terminal emulator, PTY, or ANSI escape sequences involved.
//!
//! [`glyph`] rasterizes an embedded monospace font into a fixed cell grid;
//! [`backend`] builds a `ratatui` backend on top of it. Together these are
//! the reusable rendering layer — the application shell (event loop, input
//! handling, window-picker UI) lives in the separate `prtsc` crate, which
//! depends on this one.

#![warn(missing_docs)]

/// A `ratatui` `Backend` that renders into a `winit` window directly.
pub mod backend;
/// Glyph rasterization and the monospace cell grid.
pub mod glyph;
