//! A ratatui [`ratatui::backend::Backend`] that renders directly into a
//! `softbuffer` pixel surface via [`crate::glyph::GlyphCache`] — no terminal
//! emulator, PTY, or ANSI escape sequences involved.

use std::error::Error as StdError;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use ratatui::style::Color;
use softbuffer::{Context, Surface};
use winit::window::Window;

use crate::glyph::{GlyphCache, PixelBuf};

/// Error type for [`WinitBackend`] operations.
///
/// Every fallible operation in this backend bottoms out in a `softbuffer`
/// surface call, so this just wraps [`softbuffer::SoftBufferError`].
#[derive(Debug)]
pub struct BackendError(softbuffer::SoftBufferError);

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "softbuffer error: {}", self.0)
    }
}

impl StdError for BackendError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

impl From<softbuffer::SoftBufferError> for BackendError {
    fn from(err: softbuffer::SoftBufferError) -> Self {
        BackendError(err)
    }
}

/// Renders a ratatui UI directly into a `winit` window's pixel surface.
///
/// Unlike ratatui's built-in backends (crossterm, termion, ...), this one
/// owns the window itself rather than talking to a terminal emulator over a
/// PTY: [`Backend::draw`] translates each [`Cell`] into a filled background
/// rectangle plus a rasterized glyph via [`GlyphCache`].
pub struct WinitBackend {
    window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    glyph_cache: GlyphCache,
    cursor_position: Position,
    cursor_visible: bool,
}

impl WinitBackend {
    /// Wraps `window` in a new backend, creating its `softbuffer` surface
    /// and sizing it to the window's current inner size.
    ///
    /// # Errors
    ///
    /// Returns an error if the `softbuffer` context/surface can't be
    /// created for `window`, or if the initial surface sizing fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # // Needs a live winit window, so this doesn't run as a doctest.
    /// use std::rc::Rc;
    /// use prtsc::backend::WinitBackend;
    /// use prtsc::glyph::GlyphCache;
    ///
    /// # fn example(window: Rc<winit::window::Window>) -> Result<(), Box<dyn std::error::Error>> {
    /// let backend = WinitBackend::new(window, GlyphCache::new(20.0))?;
    /// let terminal = ratatui::Terminal::new(backend)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(window: Rc<Window>, glyph_cache: GlyphCache) -> Result<Self, BackendError> {
        let context = Context::new(window.clone())?;
        let mut surface = Surface::new(&context, window.clone())?;

        let size = window.inner_size();
        if let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            surface.resize(width, height)?;
        }

        Ok(WinitBackend {
            window,
            surface,
            glyph_cache,
            cursor_position: Position::ORIGIN,
            cursor_visible: true,
        })
    }

    /// Resizes the underlying pixel surface to match the window's current
    /// inner size.
    ///
    /// Call this from a `WindowEvent::Resized` handler. It only resizes the
    /// pixel surface itself; ratatui's `Terminal` picks up the resulting
    /// change in [`Backend::size`] automatically on the next `draw` call, so
    /// there's no need to call `Terminal::resize` separately.
    ///
    /// # Errors
    ///
    /// Returns an error if the surface can't be resized.
    pub fn resize_surface(&mut self) -> Result<(), BackendError> {
        let size = self.window.inner_size();
        if let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            self.surface.resize(width, height)?;
        }
        Ok(())
    }

    fn cols_rows(&self) -> (u16, u16) {
        let size = self.window.inner_size();
        let cols = size.width as usize / self.glyph_cache.cell_width;
        let rows = size.height as usize / self.glyph_cache.cell_height;
        (
            cols.min(u16::MAX as usize) as u16,
            rows.min(u16::MAX as usize) as u16,
        )
    }
}

/// Maps a ratatui [`Color`] to opaque RGB, using `default` for
/// [`Color::Reset`]. Named colors follow the standard xterm 16-color
/// palette; [`Color::Indexed`] follows the standard xterm 256-color cube.
fn color_to_rgb(color: Color, default: (u8, u8, u8)) -> (u8, u8, u8) {
    match color {
        Color::Reset => default,
        Color::Black => (0, 0, 0),
        Color::Red => (205, 0, 0),
        Color::Green => (0, 205, 0),
        Color::Yellow => (205, 205, 0),
        Color::Blue => (0, 0, 238),
        Color::Magenta => (205, 0, 205),
        Color::Cyan => (0, 205, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (127, 127, 127),
        Color::LightRed => (255, 0, 0),
        Color::LightGreen => (0, 255, 0),
        Color::LightYellow => (255, 255, 0),
        Color::LightBlue => (92, 92, 255),
        Color::LightMagenta => (255, 0, 255),
        Color::LightCyan => (0, 255, 255),
        Color::White => (255, 255, 255),
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed_to_rgb(i),
    }
}

/// The standard xterm 256-color palette: 0-15 are the named ANSI colors,
/// 16-231 are a 6x6x6 RGB cube, 232-255 are a grayscale ramp.
fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    const NAMED: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    const RAMP: [u8; 6] = [0, 95, 135, 175, 215, 255];

    match index {
        0..=15 => NAMED[index as usize],
        16..=231 => {
            let i = index - 16;
            let r = RAMP[(i / 36) as usize];
            let g = RAMP[((i / 6) % 6) as usize];
            let b = RAMP[(i % 6) as usize];
            (r, g, b)
        }
        232..=255 => {
            let level = 8 + (index - 232) * 10;
            (level, level, level)
        }
    }
}

impl Backend for WinitBackend {
    type Error = BackendError;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let size = self.window.inner_size();
        let (width, height) = (size.width as usize, size.height as usize);
        let mut buffer = self.surface.buffer_mut()?;
        let mut pixels = PixelBuf::new(&mut buffer, width, height);

        for (x, y, cell) in content {
            let fg = color_to_rgb(cell.fg, (255, 255, 255));
            let bg = color_to_rgb(cell.bg, (0, 0, 0));
            let ch = cell.symbol().chars().next().unwrap_or(' ');
            self.glyph_cache.draw_cell(
                &mut pixels,
                x as usize,
                y as usize,
                ch,
                fg,
                bg,
            );
        }

        // softbuffer only shows a buffer on screen once presented; there's
        // no separate "swap" step, so this has to happen here rather than
        // in `flush` (whose default no-op assumption was wrong - this was
        // caught by the window staying black until this was added).
        buffer.present()?;

        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor_position)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.cursor_position = position.into();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        let mut buffer = self.surface.buffer_mut()?;
        buffer.fill(0xFF000000);
        buffer.present()?;
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        // `All` is the only variant this app currently exercises (nothing
        // yet uses partial-screen clearing); the rest are harmless no-ops
        // rather than a hard error, since leaving stale pixels in place
        // just means the next full `draw` overwrites them anyway.
        match clear_type {
            ClearType::All => self.clear(),
            _ => Ok(()),
        }
    }

    fn size(&self) -> Result<Size, Self::Error> {
        let (cols, rows) = self.cols_rows();
        Ok(Size::new(cols, rows))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let (cols, rows) = self.cols_rows();
        let size = self.window.inner_size();
        Ok(WindowSize {
            columns_rows: Size::new(cols, rows),
            pixels: Size::new(size.width as u16, size.height as u16),
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // `draw` and `clear` already present a fully up-to-date frame
        // every time they run, so there's nothing left to flush here.
        Ok(())
    }
}
