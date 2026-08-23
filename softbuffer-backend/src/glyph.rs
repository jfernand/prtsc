//! Glyph rasterization and the monospace cell grid used to draw text into a
//! raw pixel buffer.
//!
//! [`crate::glyph::GlyphCache`] rasterizes a fixed, known set of characters
//! once up front — printable ASCII, Box Drawing, and Block Elements from the
//! primary font, plus Braille Patterns from a fallback font — rather than
//! rasterizing on demand, so drawing a frame never touches the font
//! rasterizer. Any character requested outside those ranges falls back to
//! `?` rather than panicking; see `PRIMARY_RANGES`/`FALLBACK_RANGES` for
//! exactly what's covered, and extend them if a widget starts needing more
//! (e.g. Geometric Shapes for scrollbar arrows).

use std::collections::HashMap;
use std::ops::RangeInclusive;

/// Primary font: defines the monospace cell grid. See `PRIMARY_RANGES` for
/// which of its glyphs are actually rasterized into the cache.
const PRIMARY_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// Fallback font, consulted only for ranges the primary font doesn't cover.
/// Same Bitstream Vera license/family as the primary font, so no new
/// license to track. Not monospace, but only used for glyphs (Braille dot
/// patterns) that are inherently small relative to a terminal cell.
const FALLBACK_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");

/// Ranges rasterized from `PRIMARY_FONT_BYTES`. Printable ASCII, plus Box
/// Drawing and Block Elements — used pervasively by ratatui's borders
/// (`Block`), gauges, and sparklines/bar charts. Confirmed at
/// `GlyphCache::new` time that the font actually has these (see the
/// `lookup_glyph_index` check in the loop below); this list should stay in
/// sync with whatever ranges the font-coverage check in the implementation
/// plan verified.
const PRIMARY_RANGES: &[RangeInclusive<u32>] = &[
    0x20..=0x7E,     // Printable ASCII
    0x2500..=0x257F, // Box Drawing
    0x2580..=0x259F, // Block Elements
];

/// Ranges rasterized from `FALLBACK_FONT_BYTES`. Currently just Braille
/// Patterns, used by ratatui's `Canvas` widget's Braille marker. Deliberately
/// a slice of ranges (not a single range) since more may be added later;
/// clippy's suggestion to collect it into a `Vec<u32>` doesn't fit that.
#[allow(clippy::single_range_in_vec_init)]
const FALLBACK_RANGES: &[RangeInclusive<u32>] = &[0x2800..=0x28FF];

/// Drawn in place of any character outside the cached ranges above.
const FALLBACK_CHAR: char = '?';

struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

/// Rasterizes every character in `ranges` that `font` actually has a glyph
/// for at `px`, inserting into `glyphs`. Code points the font doesn't cover
/// are silently skipped (rather than inserting `.notdef` boxes), so a range
/// can be a little optimistic about coverage without corrupting the cache.
fn rasterize_ranges(
    font: &fontdue::Font,
    ranges: &[RangeInclusive<u32>],
    px: f32,
    glyphs: &mut HashMap<char, Glyph>,
) {
    for range in ranges {
        for cp in range.clone() {
            let ch = char::from_u32(cp).expect("range is valid UTF-32");
            if font.lookup_glyph_index(ch) == 0 {
                continue;
            }
            let (metrics, coverage) = font.rasterize(ch, px);
            glyphs.insert(ch, Glyph { metrics, coverage });
        }
    }
}

/// A row-major `0xAARRGGBB` pixel buffer, borrowed from the caller, with
/// bounds-checked writes. Bundles a raw `&mut [u32]` with its width/height
/// so drawing functions don't need three separate parameters for it.
pub struct PixelBuf<'a> {
    pixels: &'a mut [u32],
    width: usize,
    height: usize,
}

impl<'a> PixelBuf<'a> {
    /// Wraps `pixels` (expected to hold exactly `width * height` elements)
    /// as a drawable buffer.
    pub fn new(pixels: &'a mut [u32], width: usize, height: usize) -> Self {
        PixelBuf {
            pixels,
            width,
            height,
        }
    }

    fn set(&mut self, x: i32, y: i32, color: (u8, u8, u8)) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        self.pixels[y as usize * self.width + x as usize] = pack(color);
    }
}

/// Printable-ASCII and Braille glyphs rasterized once at a fixed pixel
/// size, plus the monospace cell geometry derived from the primary font's
/// own metrics.
///
/// # Examples
///
/// ```
/// use softbuffer_backend::glyph::GlyphCache;
///
/// let cache = GlyphCache::new(20.0);
/// assert!(cache.cell_width > 0);
/// assert!(cache.cell_height > 0);
/// ```
pub struct GlyphCache {
    glyphs: HashMap<char, Glyph>,
    baseline: i32,
    /// Fixed pixel width of one monospace cell, derived from the primary
    /// font's advance width at the pixel size passed to [`GlyphCache::new`].
    pub cell_width: usize,
    /// Fixed pixel height of one monospace cell (line height), derived
    /// from the primary font's line metrics.
    pub cell_height: usize,
}

impl GlyphCache {
    /// Rasterizes the cache's glyph set at `px` pixels per em.
    ///
    /// This parses both embedded fonts and rasterizes every cached
    /// character up front, so it's meant to be called once at startup
    /// (or once per font-size change), not per frame.
    ///
    /// # Panics
    ///
    /// Panics if the embedded font data fails to parse, or if the primary
    /// font is missing horizontal line metrics — both would indicate a
    /// corrupt embedded font file rather than a caller error.
    ///
    /// # Examples
    ///
    /// ```
    /// use softbuffer_backend::glyph::GlyphCache;
    ///
    /// let cache = GlyphCache::new(20.0);
    /// ```
    pub fn new(px: f32) -> Self {
        let primary =
            fontdue::Font::from_bytes(PRIMARY_FONT_BYTES, fontdue::FontSettings::default())
                .expect("embedded primary font failed to parse");
        let fallback =
            fontdue::Font::from_bytes(FALLBACK_FONT_BYTES, fontdue::FontSettings::default())
                .expect("embedded fallback font failed to parse");

        let mut glyphs = HashMap::new();
        rasterize_ranges(&primary, PRIMARY_RANGES, px, &mut glyphs);
        rasterize_ranges(&fallback, FALLBACK_RANGES, px, &mut glyphs);

        let cell_width = glyphs[&FALLBACK_CHAR].metrics.advance_width.round() as usize;

        let line_metrics = primary
            .horizontal_line_metrics(px)
            .expect("embedded primary font is missing horizontal line metrics");
        let cell_height = line_metrics.new_line_size.round() as usize;
        let baseline = line_metrics.ascent.round() as i32;

        GlyphCache {
            glyphs,
            baseline,
            cell_width,
            cell_height,
        }
    }

    fn glyph_for(&self, ch: char) -> &Glyph {
        self.glyphs
            .get(&ch)
            .unwrap_or_else(|| &self.glyphs[&FALLBACK_CHAR])
    }

    /// Draws one monospace cell at grid position `(col, row)`: fills its
    /// pixel rectangle with `bg`, then blends `ch`'s rasterized glyph over
    /// it in `fg`, using the glyph's coverage as an alpha value per pixel.
    ///
    /// Characters outside the cached ranges (e.g. non-ASCII characters
    /// other than Braille Patterns) are drawn as `?`.
    ///
    /// # Examples
    ///
    /// ```
    /// use softbuffer_backend::glyph::{GlyphCache, PixelBuf};
    ///
    /// let cache = GlyphCache::new(20.0);
    /// let (width, height) = (200, 100);
    /// let mut pixels = vec![0xFF000000u32; width * height];
    /// let mut buf = PixelBuf::new(&mut pixels, width, height);
    /// cache.draw_cell(&mut buf, 0, 0, 'A', (255, 255, 0), (0, 0, 128));
    /// ```
    pub fn draw_cell(
        &self,
        buf: &mut PixelBuf<'_>,
        col: usize,
        row: usize,
        ch: char,
        fg: (u8, u8, u8),
        bg: (u8, u8, u8),
    ) {
        let x0 = (col * self.cell_width) as i32;
        let y0 = (row * self.cell_height) as i32;
        fill_rect(buf, x0, y0, self.cell_width, self.cell_height, bg);

        let glyph = self.glyph_for(ch);
        self.blit_glyph(buf, glyph, x0, y0, fg, bg);
    }

    /// Blits `glyph` into the cell whose top-left pixel is `(cell_x0,
    /// cell_y0)`, blending `fg` over `bg` by the glyph's per-pixel coverage.
    ///
    /// Clips to the cell's own `cell_width` x `cell_height` rectangle, not
    /// just the overall buffer's edges. Box-drawing and block-element
    /// glyphs in particular are deliberately rasterized a couple of pixels
    /// wider/taller than the nominal cell size in most fonts (so that,
    /// drawn edge-to-edge without clipping, lines join seamlessly the way a
    /// real terminal's glyph compositor expects) - left unclipped here,
    /// that overdraw bleeds into whichever neighboring cell didn't happen
    /// to get redrawn this frame, which cell wins depending on redraw
    /// order. That showed up as gaps between sparkline bars intermittently
    /// vanishing rather than as an obvious rendering bug.
    fn blit_glyph(
        &self,
        buf: &mut PixelBuf<'_>,
        glyph: &Glyph,
        cell_x0: i32,
        cell_y0: i32,
        fg: (u8, u8, u8),
        bg: (u8, u8, u8),
    ) {
        let glyph_x0 = cell_x0 + glyph.metrics.xmin;
        let glyph_y0 = cell_y0 + self.baseline - glyph.metrics.ymin - glyph.metrics.height as i32;
        let cell_x_max = cell_x0 + self.cell_width as i32;
        let cell_y_max = cell_y0 + self.cell_height as i32;

        for row in 0..glyph.metrics.height {
            let y = glyph_y0 + row as i32;
            if y < cell_y0 || y >= cell_y_max {
                continue;
            }
            for col in 0..glyph.metrics.width {
                let x = glyph_x0 + col as i32;
                if x < cell_x0 || x >= cell_x_max {
                    continue;
                }
                let coverage = glyph.coverage[row * glyph.metrics.width + col] as u32;
                if coverage == 0 {
                    continue;
                }
                buf.set(x, y, lerp(bg, fg, coverage));
            }
        }
    }
}

/// Fills a `width` x `height` rectangle at `(x0, y0)` with opaque `color`,
/// clipping at the buffer edges.
fn fill_rect(
    buf: &mut PixelBuf<'_>,
    x0: i32,
    y0: i32,
    width: usize,
    height: usize,
    color: (u8, u8, u8),
) {
    for row in 0..height {
        for col in 0..width {
            buf.set(x0 + col as i32, y0 + row as i32, color);
        }
    }
}

/// Linearly interpolates from `bg` to `fg` by `coverage / 255`.
fn lerp(bg: (u8, u8, u8), fg: (u8, u8, u8), coverage: u32) -> (u8, u8, u8) {
    let mix = |b: u8, f: u8| -> u8 {
        let (b, f) = (b as i32, f as i32);
        (b + (f - b) * coverage as i32 / 255) as u8
    };
    (mix(bg.0, fg.0), mix(bg.1, fg.1), mix(bg.2, fg.2))
}

/// Packs an opaque RGB color into the buffer's `0xAARRGGBB` pixel format.
fn pack((r, g, b): (u8, u8, u8)) -> u32 {
    (0xFFu32 << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}
