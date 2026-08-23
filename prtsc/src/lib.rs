//! A screen-capture CLI tool built on the XDG Desktop Portal's screenshot
//! picker: `prtsc` captures once and prints the saved location.
#![warn(missing_docs)]

mod capture;

/// Captures once via the XDG Desktop Portal's screenshot picker, printing
/// the saved location to stdout on success or an error to stderr on
/// failure/cancellation (with a non-zero exit code).
///
/// # Examples
///
/// ```no_run
/// // Blocks on a real portal request, so this is `no_run` rather than an
/// // executed doctest.
/// prtsc::run();
/// ```
pub fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    runtime.block_on(capture_once());
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
