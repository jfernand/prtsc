use std::collections::HashMap;
use std::ops::RangeInclusive;

/// Primary font: defines the monospace cell grid and covers printable
/// ASCII, box drawing, block elements, and geometric shapes/arrows — every
/// glyph ratatui's borders, gauges, sparklines, and scrollbars use.
const PRIMARY_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// Fallback font, consulted only for ranges the primary font doesn't cover.
/// Same Bitstream Vera license/family as the primary font, so no new
/// license to track. Not monospace, but only used for glyphs (Braille dot
/// patterns) that are inherently small relative to a terminal cell.
const FALLBACK_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");

const PRIMARY_RANGE: RangeInclusive<u32> = 0x20..=0x7E;

/// Ranges rasterized from `FALLBACK_FONT_BYTES`. Currently just Braille
/// Patterns, used by ratatui's `Canvas` widget's Braille marker.
const FALLBACK_RANGES: &[RangeInclusive<u32>] = &[0x2800..=0x28FF];

/// Drawn in place of any character outside the cached ranges above.
const FALLBACK_CHAR: char = '?';

struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

/// Printable-ASCII and Braille glyphs rasterized once at a fixed pixel
/// size, plus the monospace cell geometry derived from the primary font's
/// own metrics.
pub struct GlyphCache {
    glyphs: HashMap<char, Glyph>,
    baseline: i32,
    pub cell_width: usize,
    pub cell_height: usize,
}

impl GlyphCache {
    pub fn new(px: f32) -> Self {
        let primary =
            fontdue::Font::from_bytes(PRIMARY_FONT_BYTES, fontdue::FontSettings::default())
                .expect("embedded primary font failed to parse");
        let fallback =
            fontdue::Font::from_bytes(FALLBACK_FONT_BYTES, fontdue::FontSettings::default())
                .expect("embedded fallback font failed to parse");

        let mut glyphs = HashMap::new();
        for cp in PRIMARY_RANGE {
            let ch = char::from_u32(cp).expect("primary range is valid UTF-32");
            let (metrics, coverage) = primary.rasterize(ch, px);
            glyphs.insert(ch, Glyph { metrics, coverage });
        }
        for range in FALLBACK_RANGES {
            for cp in range.clone() {
                let ch = char::from_u32(cp).expect("fallback ranges are valid UTF-32");
                if fallback.lookup_glyph_index(ch) == 0 {
                    continue;
                }
                let (metrics, coverage) = fallback.rasterize(ch, px);
                glyphs.insert(ch, Glyph { metrics, coverage });
            }
        }

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

    /// Draws each element of `rows` as one line of monospace cells, top-left
    /// corner at `(origin_x, origin_y)`. Characters not in the cache (e.g.
    /// non-ASCII) are drawn as `FALLBACK_CHAR`.
    pub fn draw_grid(
        &self,
        buf: &mut [u32],
        buf_width: usize,
        buf_height: usize,
        rows: &[&str],
        origin_x: i32,
        origin_y: i32,
    ) {
        for (row, line) in rows.iter().enumerate() {
            let cell_y = origin_y + (row * self.cell_height) as i32;
            for (col, ch) in line.chars().enumerate() {
                let glyph = self.glyph_for(ch);
                let cell_x = origin_x + (col * self.cell_width) as i32;
                let glyph_x = cell_x + glyph.metrics.xmin;
                let glyph_y =
                    cell_y + self.baseline - glyph.metrics.ymin - glyph.metrics.height as i32;
                blit_glyph(buf, buf_width, buf_height, glyph, glyph_x, glyph_y);
            }
        }
    }
}

/// Blits a single-channel coverage glyph as opaque white onto an opaque
/// black `buf`, clipping at the buffer edges.
fn blit_glyph(
    buf: &mut [u32],
    buf_width: usize,
    buf_height: usize,
    glyph: &Glyph,
    x0: i32,
    y0: i32,
) {
    for row in 0..glyph.metrics.height {
        for col in 0..glyph.metrics.width {
            let coverage = glyph.coverage[row * glyph.metrics.width + col] as u32;
            if coverage == 0 {
                continue;
            }
            let x = x0 + col as i32;
            let y = y0 + row as i32;
            if x < 0 || y < 0 || x as usize >= buf_width || y as usize >= buf_height {
                continue;
            }
            buf[y as usize * buf_width + x as usize] =
                (0xFFu32 << 24) | (coverage << 16) | (coverage << 8) | coverage;
        }
    }
}
