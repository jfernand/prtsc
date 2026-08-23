//! A screen-capture CLI tool built on the XDG Desktop Portal's screenshot
//! picker: `prtsc` with no arguments captures once and prints the saved
//! location, and `prtsc mcp` exposes the same capture action as an MCP tool
//! over stdio.
#![warn(missing_docs)]

mod capture;
mod mcp;
mod recording;
mod screencast;

/// Runs `prtsc` according to the first CLI argument: a one-shot capture with
/// no arguments, or the MCP server for `mcp`.
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
    match std::env::args().nth(1).as_deref() {
        None => capture_once().await,
        Some("mcp") => {
            if let Err(err) = mcp::run().await {
                eprintln!("mcp server error: {err}");
                std::process::exit(1);
            }
        }
        Some(other) => {
            eprintln!("unknown argument: {other} (expected no arguments, or `mcp`)");
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
