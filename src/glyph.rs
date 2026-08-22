const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
const GLYPH_PX: f32 = 32.0;
const TEST_CHAR: char = 'A';

pub struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

pub fn rasterize_test_glyph() -> Glyph {
    let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
        .expect("embedded font failed to parse");
    let (metrics, coverage) = font.rasterize(TEST_CHAR, GLYPH_PX);
    Glyph { metrics, coverage }
}

/// Blits a single-channel coverage glyph as opaque white onto an opaque
/// black `buf`, clipping at the buffer edges.
pub fn blit_glyph(
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
