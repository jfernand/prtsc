//! Translates `winit` key events into small, app-level input actions.
//!
//! `winit`'s `Key`/`NamedKey` model doesn't line up with `crossterm`'s
//! `KeyCode` closely enough to make forcing compatibility worthwhile, so
//! [`crate::app`] matches on [`Input`] instead of winit's own key types
//! directly — this is the one place that translation happens.

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, NamedKey};

/// App-level input actions, decoupled from winit's own key representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Move the window-picker selection up (arrow up or `k`).
    Up,
    /// Move the window-picker selection down (arrow down or `j`).
    Down,
    /// Confirm the current selection.
    Enter,
    /// Close the application (Escape or `q`).
    Quit,
    /// Toggle the FPS history graph (`f`).
    ToggleFps,
    /// Toggle whether redraws are frame-rate capped (`c`).
    ToggleFpsCap,
}

/// Maps a `winit` keyboard event to an [`Input`], or `None` for keys/states
/// the app doesn't act on. Only key-down presses are mapped (`ElementState`
/// covers both first-press and OS key-repeat as `Pressed`); releases are
/// ignored.
pub fn map_key(event: &KeyEvent) -> Option<Input> {
    if event.state != ElementState::Pressed {
        return None;
    }
    match &event.logical_key {
        Key::Named(NamedKey::Escape) => Some(Input::Quit),
        Key::Named(NamedKey::ArrowUp) => Some(Input::Up),
        Key::Named(NamedKey::ArrowDown) => Some(Input::Down),
        Key::Named(NamedKey::Enter) => Some(Input::Enter),
        Key::Character(key) if key.eq_ignore_ascii_case("q") => Some(Input::Quit),
        Key::Character(key) if key.eq_ignore_ascii_case("k") => Some(Input::Up),
        Key::Character(key) if key.eq_ignore_ascii_case("j") => Some(Input::Down),
        Key::Character(key) if key.eq_ignore_ascii_case("f") => Some(Input::ToggleFps),
        Key::Character(key) if key.eq_ignore_ascii_case("c") => Some(Input::ToggleFpsCap),
        _ => None,
    }
}
