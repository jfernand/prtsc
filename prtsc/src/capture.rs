//! Triggers the XDG Desktop Portal's screenshot picker.
//!
//! The portal call is async (`ashpd`/`zbus`) and its interactive dialog can
//! take arbitrarily long for the user to complete, so it runs on a
//! background thread with its own throwaway `tokio` runtime rather than
//! blocking the winit event loop. The result comes back over a channel that
//! [`crate::app`] polls without blocking.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use ashpd::desktop::screenshot::Screenshot;

/// The saved screenshot's location on success, or a human-readable error.
pub type CaptureResult = Result<String, String>;

/// Spawns the portal request in the background and returns a receiver that
/// yields its result once the user completes, cancels, or the portal call
/// otherwise fails.
pub fn spawn_capture() -> Receiver<CaptureResult> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
            .block_on(request_screenshot());
        // The receiver may already be gone if the window closed while this
        // capture was still in flight; there's nothing to do about that.
        let _ = tx.send(result);
    });
    rx
}

async fn request_screenshot() -> CaptureResult {
    let request = Screenshot::request()
        .interactive(true)
        .modal(true)
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let response = request.response().map_err(|err| err.to_string())?;
    Ok(response.uri().to_string())
}
