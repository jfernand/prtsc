//! A screen capture/recording CLI tool built on the XDG Desktop Portal:
//! `prtsc` with no arguments captures a screenshot once and prints the
//! saved location, `prtsc record [path]` records a screencast until
//! Ctrl-C, and `prtsc mcp` exposes the capture action as an MCP tool over
//! stdio.
#![warn(missing_docs)]

mod capture;
mod mcp;
mod recording;
mod screencast;

/// Runs `prtsc` according to the first CLI argument: a one-shot capture
/// with no arguments, `record [path]` to record a screencast until
/// Ctrl-C, or the MCP server for `mcp`.
///
/// # Examples
///
/// ```no_run
/// // Reads real CLI args and may block on a capture or an MCP session, so
/// // this is `no_run` rather than an executed doctest.
/// prtsc::run();
/// ```
pub fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    runtime.block_on(run_async());
}

async fn run_async() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => capture_once().await,
        Some("mcp") => {
            if let Err(err) = mcp::run().await {
                eprintln!("mcp server error: {err}");
                std::process::exit(1);
            }
        }
        Some("record") => record(args.next()).await,
        Some(other) => {
            eprintln!("unknown argument: {other} (expected no arguments, `mcp`, or `record`)");
            std::process::exit(2);
        }
    }
}

async fn capture_once() {
    match capture::capture().await {
        Ok(uri) => println!("{uri}"),
        Err(err) => {
            eprintln!("capture failed: {err}");
            std::process::exit(1);
        }
    }
}

async fn record(output: Option<String>) {
    let output = output.unwrap_or_else(|| "recording.mp4".to_string());

    let session = match screencast::negotiate().await {
        Ok(session) => session,
        Err(err) => {
            eprintln!("recording setup failed: {err}");
            std::process::exit(1);
        }
    };

    // Block SIGINT/SIGTERM on this thread *before* spawning the recording
    // thread below, so the new thread inherits the blocked mask too and
    // `recording::record`'s own signal handling (via pipewire's signalfd
    // mechanism, which needs the signal blocked at the OS level to work at
    // all) is the only thing that ever sees them - otherwise the kernel is
    // just as free to deliver the signal to this thread instead, whose
    // default disposition (terminate immediately) can race with and beat
    // pipewire's handling, skipping the clean-shutdown/`write_end` path
    // entirely. Found this the hard way: an unpatched Ctrl-C left a
    // 48-byte MP4 with no track and no video data, `write_end` never
    // called.
    block_interrupt_signals();

    let path = std::path::PathBuf::from(output);
    let thread_path = path.clone();
    // `pipewire-rs` asserts its mainloop is created on a thread literally
    // named "main" (see `utils::assert_main_thread`), which a
    // `tokio::task::spawn_blocking` worker (named "tokio-rt-worker") isn't -
    // discovered the hard way when this panicked on a real recording
    // attempt. A plain `std::thread` explicitly named "main" satisfies it;
    // blocking on `.join()` here is fine since this `current_thread`
    // runtime has nothing else to do concurrently anyway.
    let handle = std::thread::Builder::new()
        .name("main".to_string())
        .spawn(move || {
            // `session` (specifically its ashpd `Session`) must stay alive
            // for the whole recording - dropping it ends the portal-side
            // cast - so it's moved into this closure whole rather than
            // just its fields.
            let session = session;
            recording::record(session.fd, session.node_id, session.size, &thread_path)
        })
        .expect("failed to spawn recording thread");
    let result = handle.join().expect("recording thread panicked");

    match result {
        Ok(()) => println!("{}", path.display()),
        Err(err) => {
            eprintln!("recording failed: {err}");
            std::process::exit(1);
        }
    }
}

/// Blocks `SIGINT`/`SIGTERM` on the calling thread (and, by inheritance,
/// any thread spawned afterward) at the OS level.
fn block_interrupt_signals() {
    // SAFETY: `set` is a plain POD struct fully initialized by
    // `sigemptyset` before any other field is read, and the pointers
    // passed to `pthread_sigmask` are valid for the duration of the call.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}
