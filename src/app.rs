//! The application window: a `winit` [`winit::application::ApplicationHandler`]
//! that owns a `ratatui` [`ratatui::Terminal`] backed by
//! [`crate::backend::WinitBackend`] and redraws it on a fixed frame interval.

use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, Paragraph, Sparkline};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::backend::WinitBackend;
use crate::glyph::GlyphCache;

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const FONT_PX: f32 = 20.0;

/// How often one FPS sample is taken and pushed into the history graph.
/// Sampling faster than once a second keeps the sparkline responsive to
/// recent dips instead of only updating once every full second.
const FPS_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// How many samples the FPS history graph keeps, oldest dropped first.
/// At [`FPS_SAMPLE_INTERVAL`] = 250ms, 60 samples covers the last 15s.
const FPS_HISTORY_LEN: usize = 60;

/// Height, in cells, of the FPS graph panel (including its border).
const FPS_PANEL_HEIGHT: u16 = 7;

struct App {
    window: Option<Rc<Window>>,
    terminal: Option<Terminal<WinitBackend>>,
    /// Toggled by pressing `f`; off by default so the graph doesn't
    /// clutter the window unless asked for.
    show_fps: bool,
    frames_since_sample: u32,
    sample_window_start: Instant,
    current_fps: f64,
    fps_history: VecDeque<u64>,
}

impl Default for App {
    fn default() -> Self {
        App {
            window: None,
            terminal: None,
            show_fps: false,
            frames_since_sample: 0,
            sample_window_start: Instant::now(),
            current_fps: 0.0,
            fps_history: VecDeque::with_capacity(FPS_HISTORY_LEN),
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Character(key),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if key.eq_ignore_ascii_case("f") => {
                self.show_fps = !self.show_fps;
            }
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
    /// Takes one FPS sample if [`FPS_SAMPLE_INTERVAL`] has elapsed since
    /// the last one, updating `current_fps` and pushing into the history
    /// ring buffer used by the sparkline.
    fn sample_fps(&mut self) {
        self.frames_since_sample += 1;
        let elapsed = self.sample_window_start.elapsed();
        if elapsed < FPS_SAMPLE_INTERVAL {
            return;
        }

        self.current_fps = self.frames_since_sample as f64 / elapsed.as_secs_f64();
        self.frames_since_sample = 0;
        self.sample_window_start = Instant::now();

        if self.fps_history.len() >= FPS_HISTORY_LEN {
            self.fps_history.pop_front();
        }
        self.fps_history.push_back(self.current_fps.round() as u64);
    }

    fn redraw(&mut self) {
        if self.show_fps {
            self.sample_fps();
        }

        let Some(terminal) = &mut self.terminal else {
            return;
        };
        let show_fps = self.show_fps;
        let current_fps = self.current_fps;
        let fps_history = &self.fps_history;

        terminal
            .draw(|frame| {
                let area = frame.area();
                let hello_area = if show_fps {
                    let [fps_area, rest] = Layout::vertical([
                        Constraint::Length(FPS_PANEL_HEIGHT),
                        Constraint::Min(0),
                    ])
                    .areas(area);
                    let sparkline = Sparkline::default()
                        .block(Block::bordered().title(format!("FPS: {current_fps:.1}")))
                        .data(fps_history.iter());
                    frame.render_widget(sparkline, fps_area);
                    rest
                } else {
                    area
                };
                frame.render_widget(Paragraph::new("Hello, prtsc!"), hello_area);
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
