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

    let path = std::path::PathBuf::from(output);
    let result = {
        let path = path.clone();
        tokio::task::spawn_blocking(move || {
            // `session` (specifically its ashpd `Session`) must stay alive
            // for the whole recording - dropping it ends the portal-side
            // cast - so it's moved into this closure whole rather than
            // just its fields.
            let session = session;
            recording::record(session.fd, session.node_id, session.size, &path)
        })
        .await
        .expect("recording thread panicked")
    };

    match result {
        Ok(()) => println!("{}", path.display()),
        Err(err) => {
            eprintln!("recording failed: {err}");
            std::process::exit(1);
        }
    }
}
