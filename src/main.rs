use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSansMono.ttf");
const GLYPH_PX: f32 = 32.0;
const TEST_CHAR: char = 'A';

struct Glyph {
    metrics: fontdue::Metrics,
    coverage: Vec<u8>,
}

fn rasterize_test_glyph() -> Glyph {
    let font = fontdue::Font::from_bytes(FONT_BYTES, fontdue::FontSettings::default())
        .expect("embedded font failed to parse");
    let (metrics, coverage) = font.rasterize(TEST_CHAR, GLYPH_PX);
    Glyph { metrics, coverage }
}

/// Blits a single-channel coverage glyph as opaque white onto an opaque
/// black `buf`, clipping at the buffer edges.
fn blit_glyph(buf: &mut [u32], buf_width: usize, buf_height: usize, glyph: &Glyph, x0: i32, y0: i32) {
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
            let shade = (255 * coverage) / 255;
            buf[y as usize * buf_width + x as usize] =
                (0xFFu32 << 24) | (shade << 16) | (shade << 8) | shade;
        }
    }
}

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    glyph: Glyph,
}

impl Default for App {
    fn default() -> Self {
        App {
            window: None,
            surface: None,
            glyph: rasterize_test_glyph(),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes().with_title("screencap");
        let window = Rc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let context = Context::new(window.clone()).expect("failed to create softbuffer context");
        let mut surface =
            Surface::new(&context, window.clone()).expect("failed to create softbuffer surface");

        let size = window.inner_size();
        if let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        {
            surface
                .resize(width, height)
                .expect("failed to size softbuffer surface");
        }

        self.window = Some(window);
        self.surface = Some(surface);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(surface), Some(width), Some(height)) = (
                    self.surface.as_mut(),
                    NonZeroU32::new(size.width),
                    NonZeroU32::new(size.height),
                ) {
                    surface
                        .resize(width, height)
                        .expect("failed to resize softbuffer surface");
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + FRAME_INTERVAL));
    }
}

impl App {
    fn redraw(&mut self) {
        let (Some(window), Some(surface)) = (&self.window, self.surface.as_mut()) else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        let (width, height) = (size.width as usize, size.height as usize);
        let mut buffer = surface
            .buffer_mut()
            .expect("failed to get softbuffer buffer");
        buffer.fill(0xFF000000);
        blit_glyph(&mut buffer, width, height, &self.glyph, 20, 20);
        buffer
            .present()
            .expect("failed to present softbuffer buffer");
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("event loop error");
}
