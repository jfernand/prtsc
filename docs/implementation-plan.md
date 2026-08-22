# Implementation plan: standalone ratatui window via winit + softbuffer

## Goal

Turn `screencap` into a self-contained binary that, when launched, opens its
own native window (no external terminal emulator required), shows a ratatui
TUI for picking a window from `xcap::Window::all()`, and captures the
selected window on demand.

## Architecture recap

- `winit` owns the OS window and the event loop (input, resize, close).
- `softbuffer` gives us a raw RGB pixel buffer tied to that window — no GPU
  API, no shaders.
- A small font rasterizer (`fontdue`) turns a monospace font into per-glyph
  bitmaps we blit into the pixel buffer ourselves.
- A custom type implements ratatui's `Backend` trait on top of the above, so
  the rest of the app (widgets, layout, state) is ordinary ratatui code and
  doesn't know it isn't running in a real terminal.
- `xcap` (already a dependency) supplies window enumeration and capture.

No terminal emulator, no VT100/ANSI parsing, no `memterm`/`vt100` — ratatui
hands the backend a structured `Buffer` of cells directly.

## Steps

Each step should compile, run, and be manually verified before moving to the
next. Keep commits small and scoped to one step (see `AGENTS.md` — no commits
without human review regardless).

### 1. Bare window
Add `winit` and `softbuffer`. Open a window, run the event loop, fill the
softbuffer surface with a solid color on every redraw. No ratatui, no fonts
yet.

**Verify:** window opens, resizes without panicking, closes cleanly on the
close button / Esc.

### 2. Glyph rasterization
Add `fontdue`. Load a single embedded monospace font (e.g. bundle a `.ttf`
under `assets/`, loaded via `include_bytes!`). Rasterize one character at a
fixed size and blit it into the pixel buffer at a fixed position.

**Verify:** one crisp character appears in the window.

### 3. Glyph cache + cell grid
Rasterize the printable ASCII range once at startup into a cache keyed by
`char` (bitmap + advance width/height). Derive terminal-style `(cols, rows)`
from window pixel size and the fixed cell size (monospace, so cell width/
height come straight from the font metrics). Write a function that draws an
arbitrary `&str` grid (`Vec<Vec<char>>` or similar) to the buffer using the
cache.

**Verify:** a small hardcoded grid of text renders correctly, including
non-ASCII fallback behavior (draw blank/`?` for glyphs not in the cache).

### 4. `Backend` implementation
Implement `ratatui::backend::Backend` for a `WinitBackend` type wrapping the
window, softbuffer surface, and glyph cache. Required methods: `draw`,
`hide_cursor`, `show_cursor`, `get_cursor_position`, `set_cursor_position`,
`clear`, `size`, `window_size`, `flush`. Map ratatui `Cell` (char + `Style`)
to the glyph cache plus a foreground/background color fill per cell — start
with fg/bg color only, ignore bold/italic/underline for now.

**Verify:** `Terminal::new(WinitBackend::new(...))` constructs successfully
and `terminal.draw(|f| f.render_widget(Paragraph::new("hello"), f.area()))`
shows real text in the window.

### 5. Event loop integration
Wire winit's event loop to drive redraws: request a redraw on window resize/
focus/expose events, and on an application tick (e.g. every 16ms or only
when state changes — start with redraw-on-input for simplicity). Confirm the
window resizes and ratatui re-lays-out content without stale artifacts
(clear-before-draw, resize the softbuffer surface to match).

**Verify:** resizing the window live re-flows a ratatui widget with no
tearing/garbage pixels.

### 6. Keyboard input plumbing
Map winit `KeyEvent`s to a small internal input enum (`Up`, `Down`, `Enter`,
`Quit`, ...). Do not attempt to reuse `crossterm::event::KeyEvent` — winit's
key model is different enough that a thin translation layer is clearer than
forcing compatibility.

**Verify:** pressing keys logs/prints the expected mapped variant.

### 7. Window list state + UI
Add app state: `windows: Vec<xcap::Window>`, `selected: usize`. On startup
(and optionally on an explicit refresh key), populate from
`xcap::Window::all()`. Render as a ratatui `List` with the selected item
highlighted. Wire Up/Down (and j/k) to move selection, wrapping or clamping
at the ends.

**Verify:** the real list of open windows appears and keyboard navigation
selects each one visibly.

### 8. Capture action
On Enter, call `capture_image()` on the selected window and save a PNG
(reuse the existing `sav_{i:03}.png` naming or make it configurable). Show
a status line/toast in the UI confirming the saved path.

**Verify:** pressing Enter on a real window produces a correct PNG on disk.

**Open decision (deferred):** whether post-selection capture is single-shot,
a continuous recording loop with a stop key, or something else — resolve
this before or during this step, not before.

### 9. Polish
- Handle DPI/`scale_factor` changes (winit can report these independently of
  pixel resize).
- Graceful error handling for `xcap` failures (window closed between listing
  and capture, permission errors, etc.) — surface in the UI, don't panic.
- Esc/`q` to quit from the picker.
- `cargo clippy` clean, `cargo fmt` applied.

## Explicitly out of scope for this plan

- Any VT100/ANSI parsing (`memterm`, `vt100`) — not needed since ratatui
  never emits an escape-sequence stream to this backend.
- Multi-window / tabbed capture UI.
- Video/GIF recording (only still-frame PNG capture per the steps above).
