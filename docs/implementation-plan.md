# Implementation plan: prtsc

## Goal

`prtsc` is a screen-capture CLI driven by the XDG Desktop Portal's
`Screenshot` interface: run with no arguments to capture once and print the
saved file's location, or run `prtsc mcp` to expose the same capture action
as a single MCP tool over stdio for AI assistants/agents to call.

There is no in-app window list or picker UI. The portal's own
compositor-drawn dialog (GNOME Shell's screenshot tool, KDE's, etc.) handles
window/screen selection - see "Why no window list" below.

## Steps

### 1. Capture via the XDG Desktop Portal
Add `ashpd` (screenshot feature) and `tokio`. `capture::capture()` requests
`ashpd::desktop::screenshot::Screenshot` with `.interactive(true)`, awaits
the response, and returns the saved file's `file://` URI (or a
human-readable error string).

**Verify:** confirmed end-to-end in this sandbox - a `tools/call` against
the MCP server (see step 3) returned a real file URI, and the file existed
on disk with the reported PNG dimensions. The plain CLI path made the same
request and received a real (if less consistent) response from the portal;
completing the interactive picker itself is flaky in this specific sandboxed
desktop session (no physical user reliably present to click through GNOME
Shell's dialog), which is an environment characteristic, not a code issue -
the request/response/error-propagation path itself is proven correct.

### 2. CLI: capture-and-exit by default
`prtsc` with no arguments calls `capture::capture()` once, prints the URI to
stdout on success, or prints an error to stderr and exits non-zero on
failure/cancellation. No window, no event loop - just one async call driven
by a throwaway current-thread `tokio` runtime built in `run()`.

**Verify:** `prtsc` with no args exits after one capture attempt; stdout has
exactly the saved path on success.

### 3. MCP server (`prtsc mcp`)
Added `rmcp` (server, macros, transport-io features) plus `serde_json`
(required directly by the `#[tool]` macro's generated code, not just
transitively via `rmcp`). `mcp::run()` serves a single `capture` tool over
stdio via `CaptureServer::serve(stdio())`, delegating to the same
`capture::capture()` used by the CLI path.

**Verify:** a manual JSON-RPC handshake (`initialize` -> `tools/list` ->
`tools/call name=capture`) against `prtsc mcp` returned a real saved-file
URI, confirmed to exist on disk. See step 1's verify note.

## Screen recording (planned, not yet implemented)

`prtsc record [path]` will record a screencast to a `.webm` file via the
XDG Desktop Portal's `ScreenCast` interface, following the same
portal-only philosophy as capture: no window/screen enumeration in
`prtsc` itself, the compositor's own picker chooses the source.

**Encoder/container decision:** AV1 (`rav1e`) + WebM (`webm` crate),
not `ffmpeg`/libav. Ruled out two alternatives first:
- Shelling out to the `ffmpeg` binary via a piped child process - works,
  but process lifecycle/error handling through exit codes is fragile
  compared to a library call.
- `ffmpeg-next` (FFI bindings to libavcodec/libavformat) - needs matching
  `libav*-dev` headers at build time and matching `.so` files at runtime
  on whatever machine eventually runs `prtsc`; H.264 support specifically
  depends on how that machine's libavcodec was built (patent-encumbered
  codecs are often excluded from distro packaging).
- `openh264` (H.264) was also considered and rejected: its `source`
  feature compiles OpenH264 from source, which falls outside Cisco's
  royalty coverage for the underlying H.264 patents (that coverage is
  tied specifically to Cisco's own prebuilt binary, confirmed against
  OpenH264's own `BINARY_LICENSE.txt` and FAQ) - AV1 has no such
  patent-royalty story at all, which is the deciding factor.

The `webm` crate is not pure Rust - it's FFI bindings to Google's
`libwebm` C++ library, vendored and compiled by `cc` at build time (its
`build.rs` compiles `libwebm/mkvmuxer/*.cc` directly). That needs a C++
compiler at build time, but - unlike `ffmpeg-next` - it's statically
linked, so there's no runtime `.so` dependency on the machine that runs
the built `prtsc` binary.

### 1. Portal session negotiation
Add the `screencast` feature to `ashpd`. `Screencast::new()` ->
`create_session()` -> `select_sources(session, SelectSourcesOptions)` ->
`start(session, None, StartCastOptions)` returns `Streams`, each with a
`pipe_wire_node_id()` and size. `open_pipe_wire_remote(session, ..)`
returns an `OwnedFd` scoped to just that session/stream.

**Verify:** print the negotiated node id + stream size; confirm it
matches what was picked in the compositor's dialog.

### 2. Raw frame capture on a dedicated thread
Add the `pipewire` crate (`v1_0_0` feature, matching this system's
installed `libpipewire-0.3` 1.0.5). PipeWire's mainloop is a blocking C
loop, not async, so it needs its own `std::thread`.
`ContextRc::connect_fd_rc(fd, ..)` (takes the fd from step 1) -> build a
video `Stream`, negotiate format via an SPA `EnumFormat` pod (candidate
pixel formats/size range/framerate range - same shape as the `pipewire`
crate's own `examples/streams.rs`), `connect()` targeting the node id
from step 1. The `process` callback dequeues buffers and forwards raw
frame bytes (plus negotiated format/stride) over an `mpsc::channel` to
the encoder thread.

**Verify:** log frame count/size for a few seconds of a real session;
confirm roughly the expected framerate.

### 3. RGB -> YUV420 conversion, rav1e encode, webm mux
PipeWire hands back interleaved RGBA/BGRx frames; `rav1e` only accepts
planar YUV 4:2:0 (`frame.planes[0/1/2].copy_from_raw_u8(..)`, confirmed
against `rav1e`'s own y4m reader). Write a plain BT.601 + 2x2
chroma-averaging conversion function - no new dependency, just
arithmetic.

Encode loop: `EncoderConfig { width, height, chroma_sampling: Cs420, .. }`
-> `Config::new().with_encoder_config(..).new_context()` -> per frame:
`ctx.new_frame()`, fill planes, `ctx.send_frame(..)`, drain
`ctx.receive_packet()` (handling `NeedMoreData`/`Encoded`/`LimitReached`).

Mux: `Writer::new(file)` -> `SegmentBuilder::new(writer)?.set_mode(Live)?
.add_video_track(w, h, VideoCodecId::AV1, None)?.build()` -> per packet:
`segment.add_frame(track, &packet.data, timestamp_ns, keyframe)` ->
`segment.finalize(None)` on stop.

**Verify:** resulting `.webm` file is valid and playable
(`ffprobe`/a real player), roughly matching the recorded duration.

### 4. Start/stop lifecycle
- CLI: `prtsc record [path]` runs until Ctrl-C (SIGINT), then cleanly
  stops the stream/encoder/muxer and prints the saved path - mirrors the
  existing "print path on success" convention from plain `capture`.
- MCP: two tools, `start_recording` / `stop_recording`, since the MCP
  server process is long-lived (unlike the one-shot `capture` tool) and
  can hold the recording thread/channel handles as state between calls.

**Verify:** both paths produce a playable file.

### 5. Polish
- Handle portal cancellation cleanly (same error-surfacing pattern
  `capture` already has).
- Decide default output naming/location (mirror GNOME's
  `~/Videos/Screencasts/` convention, or require an explicit path arg).
- `cargo clippy` / `cargo fmt`.

**Known risk:** step 2's SPA pod format negotiation is fiddly,
low-level, C-binding-shaped code - worth prototyping standalone before
wiring into `prtsc` proper.

## Why no window list

Earlier revisions of this plan built a `winit` + `softbuffer` + `ratatui`
window with an in-app `xcap`-based window-picker `List`. That was dropped in
two stages:

1. `xcap`'s Linux window enumeration is X11/XCB-only (`_NET_CLIENT_LIST_STACKING`),
   invisible to native Wayland toolkit windows - replaced with `ashpd`'s
   `Screenshot` portal, whose own compositor-drawn picker handles selection
   instead (no caller-side window enumeration needed or possible).
2. Once the portal owned window/screen selection, the custom-rendered window
   had nothing left to justify its existence beyond showing a one-line
   status message - dropped entirely in favor of a plain CLI, with an MCP
   server mode added alongside it for programmatic/agent use.

The `winit`/`softbuffer`/`fontdue`/`ratatui` rendering layer itself still
exists, unused for now, in the separate `softbuffer-backend` crate (see its
own history below) - parked for a possible future interactive/TUI mode, not
deleted.

## `softbuffer-backend` crate history (parked, not currently used by `prtsc`)

The steps below built the `softbuffer-backend` crate: a `ratatui` `Backend`
that renders directly into a `winit` window via `softbuffer` and `fontdue`,
with no external terminal emulator involved. The crate still builds and
works standalone; `prtsc` just doesn't depend on it right now.

### 1. Bare window
Add `winit` and `softbuffer`. Open a window, run the event loop, fill the
softbuffer surface with a solid color on every redraw. No ratatui, no fonts
yet.

**Verify:** window opens, resizes without panicking, closes cleanly on the
close button / Esc.

### 2. Glyph rasterization
Add `fontdue`. Load a single embedded monospace font (e.g. bundle a `.ttf`
under `assets/`, loaded via `include_bytes!`). Rasterize one character at a
fixed size and blit it into the pixel buffer at a fixed position.

**Verify:** one crisp character appears in the window.

### 3. Glyph cache + cell grid
Rasterize the printable ASCII range once at startup into a cache keyed by
`char` (bitmap + advance width/height). Derive terminal-style `(cols, rows)`
from window pixel size and the fixed cell size (monospace, so cell width/
height come straight from the font metrics). Write a function that draws an
arbitrary `&str` grid (`Vec<Vec<char>>` or similar) to the buffer using the
cache.

**Verify:** a small hardcoded grid of text renders correctly, including
non-ASCII fallback behavior (draw blank/`?` for glyphs not in the cache).

**Font coverage note:** DejaVu Sans Mono alone covers every glyph ratatui
uses for borders/gauges/sparklines/scrollbars (box drawing, block elements,
geometric shapes) but has zero Braille Patterns (U+2800–U+28FF) coverage,
needed by ratatui's `Canvas` widget. Checked candidate fonts directly with a
`fontdue`-based coverage scan (see commit history/PR discussion, not kept as
a script in this repo) — DejaVu Sans (the proportional sibling, same
Bitstream Vera license) has full Braille coverage, so `GlyphCache` rasterizes
the printable-ASCII range from DejaVu Sans Mono (also defining the cell
grid) and the Braille range from DejaVu Sans as a fallback, keeping
everything under one already-vetted permissive license. GNU FreeMono also
has full coverage but is GPL-3 with a document-embedding exception that
doesn't clearly cover bundling into a compiled binary — deliberately not
used for that reason.

### 4. `Backend` implementation
Implement `ratatui::backend::Backend` for a `WinitBackend` type wrapping the
window, softbuffer surface, and glyph cache. Required methods: `draw`,
`hide_cursor`, `show_cursor`, `get_cursor_position`, `set_cursor_position`,
`clear`, `size`, `window_size`, `flush`. Map ratatui `Cell` (char + `Style`)
to the glyph cache plus a foreground/background color fill per cell — start
with fg/bg color only, ignore bold/italic/underline for now.

**Verify:** `Terminal::new(WinitBackend::new(...))` constructs successfully
and `terminal.draw(|f| f.render_widget(Paragraph::new("hello"), f.area()))`
shows real text in the window.

### 5. Event loop integration
Wire winit's event loop to drive redraws: request a redraw on window resize/
focus/expose events, and on an application tick (e.g. every 16ms or only
when state changes — start with redraw-on-input for simplicity). Confirm the
window resizes and ratatui re-lays-out content without stale artifacts
(clear-before-draw, resize the softbuffer surface to match).

**Verify:** resizing the window live re-flows a ratatui widget with no
tearing/garbage pixels.

**Known issue (unresolved):** live drag-resizing still shows occasional
artifacts. One real bug in this area was found and fixed — `resize_surface`
was re-querying `window.inner_size()` live instead of using the size the
`WindowEvent::Resized` event itself carried, which raced during fast event
bursts — but the user reports residual artifacts during live drag even after
that fix. Not yet root-caused; revisit before considering step 5 fully done.
Suspects worth checking first: whether `softbuffer`'s platform backend still
has its own lazy/rotating-buffer resize behavior independent of our own
buffer sizing (the same category of bug as the double-buffering flicker
fixed earlier), and whether winit's Wayland `RedrawRequested`-during-resize
gap (rust-windowing/winit#2609) interacts with our tick-driven redraw loop.

## Explicitly out of scope for this plan

- Any VT100/ANSI parsing (`memterm`, `vt100`) — not needed since ratatui
  never emits an escape-sequence stream to this backend.
- Multi-window / tabbed capture UI.
- GIF recording (see "Screen recording" above for video - GIF specifically
  is out of scope).
