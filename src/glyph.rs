use std::collections::HashMap;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");

/// Drawn in place of any character outside the cached printable-ASCII range.
const FALLBACK_CHAR: char = '?';

struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

/// Printable-ASCII glyphs rasterized once at a fixed pixel size, plus the
/// monospace cell geometry derived from the font's own metrics.
pub struct GlyphCache {
    glyphs: HashMap<char, Glyph>,
    baseline: i32,
    pub cell_width: usize,
    pub cell_height: usize,
}

impl GlyphCache {
    pub fn new(px: f32) -> Self {
        let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
            .expect("embedded font failed to parse");

        let mut glyphs = HashMap::new();
        for byte in 0x20u8..=0x7E {
            let ch = byte as char;
            let (metrics, coverage) = font.rasterize(ch, px);
            glyphs.insert(ch, Glyph { metrics, coverage });
        }

        let cell_width = glyphs[&FALLBACK_CHAR].metrics.advance_width.round() as usize;

        let line_metrics = font
            .horizontal_line_metrics(px)
            .expect("embedded font is missing horizontal line metrics");
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
