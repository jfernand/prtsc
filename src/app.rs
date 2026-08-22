//! The application window: a `winit` [`winit::application::ApplicationHandler`]
//! that owns a `ratatui` [`ratatui::Terminal`] backed by
//! [`crate::backend::WinitBackend`] and redraws it on a fixed frame interval.

use std::rc::Rc;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::widgets::Paragraph;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::backend::WinitBackend;
use crate::glyph::GlyphCache;

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const FONT_PX: f32 = 20.0;

#[derive(Default)]
struct App {
    window: Option<Rc<Window>>,
    terminal: Option<Terminal<WinitBackend>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes().with_title("prtsc");
        let window = Rc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );

        let backend = WinitBackend::new(window.clone(), GlyphCache::new(FONT_PX))
            .expect("failed to create winit backend");
        let terminal = Terminal::new(backend).expect("failed to create terminal");

        self.window = Some(window);
        self.terminal = Some(terminal);
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
            WindowEvent::Resized(_) => {
                if let Some(terminal) = &mut self.terminal {
                    terminal
                        .backend_mut()
                        .resize_surface()
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
        let Some(terminal) = &mut self.terminal else {
            return;
        };
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("Hello, prtsc!"), frame.area());
            })
            .expect("failed to draw frame");
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
/// window/backend/terminal creation fails.
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
