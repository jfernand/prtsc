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
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::backend::WinitBackend;
use crate::glyph::GlyphCache;
use crate::input::{self, Input};

/// Target frame interval when the FPS cap is enabled (~62.5fps).
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
    /// Toggled by pressing `c`. `Some(interval)` paces redraws to roughly
    /// one every `interval`; `None` redraws as fast as the platform allows.
    /// Capped by default - seeing the *uncapped* number is the reason this
    /// is a toggle rather than always on.
    fps_cap: Option<Duration>,
    last_redraw: Instant,
}

impl Default for App {
    fn default() -> Self {
        let now = Instant::now();
        App {
            window: None,
            terminal: None,
            show_fps: false,
            frames_since_sample: 0,
            sample_window_start: now,
            current_fps: 0.0,
            fps_history: VecDeque::with_capacity(FPS_HISTORY_LEN),
            fps_cap: Some(FRAME_INTERVAL),
            last_redraw: now,
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
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(input) = input::map_key(&event) {
                    self.handle_input(input, event_loop);
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(terminal) = &mut self.terminal {
                    terminal
                        .backend_mut()
                        .resize_surface(new_size.width, new_size.height)
                        .expect("failed to resize softbuffer surface");
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();

        // `winit`/X11 apparently services a pending `request_redraw()`
        // almost immediately regardless of `ControlFlow::WaitUntil` -
        // confirmed by instrumenting this function: with an unconditional
        // `request_redraw()` call here, it was re-entered every ~2-4ms
        // instead of the ~16ms `FRAME_INTERVAL` asked for, because each
        // redraw re-arms another one for the very next loop iteration.
        // Gating the request on an explicit elapsed-time check (rather
        // than trusting `WaitUntil` alone) sidesteps that: pacing becomes
        // "did enough time pass" rather than "did the platform wake us at
        // the requested moment", which is what makes the cap toggleable
        // and correct either way.
        let due = match self.fps_cap {
            Some(interval) => now.duration_since(self.last_redraw) >= interval,
            None => true,
        };
        if due {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            self.last_redraw = now;
        }

        event_loop.set_control_flow(match self.fps_cap {
            Some(interval) => ControlFlow::WaitUntil(self.last_redraw + interval),
            None => ControlFlow::Poll,
        });
    }
}

impl App {
    fn handle_input(&mut self, input: Input, event_loop: &ActiveEventLoop) {
        match input {
            Input::Quit => event_loop.exit(),
            Input::ToggleFps => {
                self.show_fps = !self.show_fps;
                if self.show_fps {
                    // Otherwise the first sample after enabling measures
                    // against a `sample_window_start` that's however long
                    // it's been since the counter was last on (or since
                    // startup), producing one misleadingly-low bar.
                    self.frames_since_sample = 0;
                    self.sample_window_start = Instant::now();
                }
            }
            Input::ToggleFpsCap => {
                self.fps_cap = match self.fps_cap {
                    Some(_) => None,
                    None => Some(FRAME_INTERVAL),
                };
            }
            // Wired up once the window-picker list exists (implementation
            // plan step 7); mapped now so the translation layer is in place
            // ahead of that.
            Input::Up | Input::Down | Input::Enter => {}
        }
    }

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
        let cap_label = if self.fps_cap.is_some() {
            "capped"
        } else {
            "uncapped"
        };

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
                        .block(
                            Block::bordered()
                                .title(format!("FPS: {current_fps:.1} ({cap_label}, c to toggle)")),
                        )
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
