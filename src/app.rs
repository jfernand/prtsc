//! The application window: a `winit` [`winit::application::ApplicationHandler`]
//! that owns a `softbuffer` pixel surface and repaints it via
//! [`crate::glyph::GlyphCache`] on a fixed frame interval.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::glyph::GlyphCache;

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const FONT_PX: f32 = 20.0;

const TEST_ROWS: [&str; 4] = [
    "Hello, prtsc! ~!@#$%^&*()_+",
    "0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    "unicode fallback: 日本語 emoji: 🎉",
    "braille: ⠁⠃⠉⠙⠑⠋⠛⠓⠊⠚ ⣿⡿⢿⣟⣯⣷",
];

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    glyph_cache: GlyphCache,
}

impl Default for App {
    fn default() -> Self {
        App {
            window: None,
            surface: None,
            glyph_cache: GlyphCache::new(FONT_PX),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes().with_title("prtsc");
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
        self.glyph_cache
            .draw_grid(&mut buffer, width, height, &TEST_ROWS, 10, 10);
        buffer
            .present()
            .expect("failed to present softbuffer buffer");
    }
}

/// Opens the application window and runs its event loop.
///
/// This blocks the calling thread until the window is closed (via its
/// close button or the Escape key) and never returns before then, so it
/// should typically be the last thing called from `main`.
///
/// # Panics
///
/// Panics if the platform windowing backend can't be initialized, or if
/// window/surface creation fails.
///
/// # Examples
///
/// ```no_run
/// // Opens a window and blocks until it's closed, so this is `no_run`
/// // rather than an executed doctest.
/// prtsc::run();
/// ```
pub fn run() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("event loop error");
}
