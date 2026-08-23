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

## Screen recording (implemented, portal interaction unverified in this sandbox)

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

### 1. Portal session negotiation - done (`prtsc/src/screencast.rs`)
`Screencast::new()` -> `create_session()` -> `select_sources(session,
SelectSourcesOptions)` -> `start(session, None, ..)` returns `Streams`,
each with a `pipe_wire_node_id()` and size. `open_pipe_wire_remote(session,
..)` returns an `OwnedFd` scoped to just that session/stream. The ashpd
`Session` is kept alive (moved whole into the recording thread's closure)
for the entire recording - dropping it early would end the cast portal-side.

**Verify:** reaches the real portal (`xdg-desktop-portal-gnome`, installed
earlier in this session) and triggers its native screen-share picker -
confirmed via `journalctl` activity - but completing that picker
interactively isn't possible in this sandbox: unlike some GTK dialogs,
GNOME Shell's screen-share picker has no X11/XWayland-visible surface at
all (confirmed - no new window appeared under `xdotool search`), so it
can't be clicked through here. Same class of limitation already accepted
for `capture`'s `Screenshot` portal earlier in this project.

### 2. Raw frame capture + encode on one dedicated thread - done (`prtsc/src/recording.rs`)
Simplified from the original plan: encoding happens directly inside
PipeWire's `process` callback, on the same thread as its mainloop,
rather than forwarding frames over a channel to a separate encoder
thread - one thread doing dequeue -> convert -> encode -> mux keeps the
first working version simple; revisit only if profiling ever shows the
encoder is slow enough to drop PipeWire buffers.

`ContextRc::connect_fd_rc(fd, ..)` (the fd from step 1) -> a video
`Stream`, format negotiated via an SPA `EnumFormat` pod offering the
pixel layouts `openh264` can consume directly (BGRx/BGRA/RGBx/RGBA -
same pod-building shape as the `pipewire` crate's own
`examples/streams.rs`) at the portal's reported size. Ctrl-C/SIGTERM
handling is PipeWire's own (`main_loop.loop_().add_signal_local(Signal::INT/TERM,
..)`, the same idiom as the crate's `pw-mon.rs` example) rather than
routed through `tokio` - simpler, since the whole capture/encode loop is
already isolated on its own thread.

**Verify:** not exercisable end-to-end here - blocked on step 1's portal
interaction. See step 3 for how the encode/mux logic (the actual
hand-written, error-prone part) was verified independently instead.

### 3. H.264 encode via openh264, mux to MP4 - done, verified independently of PipeWire
Convert each raw frame with `YUVBuffer::from_bgra8_source(BgraSliceU8::new(bytes, (w, h)))`
(or the RGBA equivalent) - `openh264` ships this conversion built in, no
hand-rolled color-space math needed. `Encoder::encode(&yuv_buffer)` ->
`EncodedBitStream`, whose `layer(i)`/`nal_unit(n)` accessors expose
individual Annex-B NAL units directly; the *first* IDR frame's SPS (NAL
type 7) and PPS (NAL type 8) get pulled out to build
`mp4::AvcConfig { .. }` before `Mp4Writer::add_track` - the one place NAL
type bytes get inspected by hand. Other NALs are converted from Annex-B
to AVCC (4-byte length prefix) and written via `Mp4Writer::write_sample`.
PipeWire buffers can have row padding (`stride > width * 4`); a
`repack_rows` helper strips it before handing data to `openh264`'s slice
wrappers, which assert on exactly `width * height * 4` bytes.

**Verify:** since step 1 blocks any real capture, added a `#[cfg(test)]`
unit test (`recording::tests::encodes_synthetic_frames_to_a_readable_mp4`)
that calls `encode_frame` directly with synthetic pixel data, bypassing
PipeWire/the portal entirely, then reads the result back with the `mp4`
crate's own reader (track count, track type, sample count). Additionally
confirmed with tools outside this codebase: `ffprobe` identifies the
output as valid `h264 (Constrained Baseline) yuv420p`, and `ffmpeg -i ...
-f null -` decodes every frame without error. This isolates and verifies
exactly the hand-written, highest-risk logic (NAL parsing, SPS/PPS
extraction, AVCC framing, MP4 sample writing) independently of whether a
real PipeWire session is available.

### 4. Start/stop lifecycle
- CLI: done - `prtsc record [path]` runs until Ctrl-C/SIGTERM, then
  cleanly finalizes and prints the saved path, mirroring `capture`'s
  "print path on success" convention.
- MCP `start_recording`/`stop_recording`: not yet done. Needs the
  recording thread's handle/stop-signal to live in `CaptureServer`'s
  state across two separate tool calls (unlike the one-shot `capture`
  tool) - deferred as a follow-up.

**Verify:** CLI path type-checks and the encode/mux half is verified per
step 3; full run blocked on step 1's portal interaction, same as above.

### 5. Polish - not yet done
- Handle portal cancellation cleanly (same error-surfacing pattern
  `capture` already has) - `screencast::negotiate` does return `Err` on
  failure already, but the "user closed the picker without choosing
  anything" case specifically hasn't been exercised.
- Decide default output naming/location - currently just `recording.mp4`
  in the working directory when no path is given; GNOME's
  `~/Videos/Screencasts/` convention not yet adopted.
- `cargo clippy` / `cargo fmt` - clean as of this pass.

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
