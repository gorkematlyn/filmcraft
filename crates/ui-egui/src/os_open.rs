//! Files the operating system asks the running app to open: a double-click or Open With in the
//! Finder on macOS, whether FilmCraft is closed (the request waits here until the first frame) or
//! already running. The host's OS integration calls [`request`] from any thread; the app takes the
//! paths once per frame and opens them as it opens a dropped file: a project (`.fcproj`) through
//! `file.open`, like File ▸ Open and the command line, and anything else is imported.
//!
//! Windows and Linux hand such files to the new process as arguments (`apps/filmcraft/src/args.rs`);
//! macOS sends an "open documents" event instead, which only a host that listens for it can turn
//! into a call to [`request`].

use std::sync::{Mutex, OnceLock, PoisonError};

static QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WAKE: OnceLock<egui::Context> = OnceLock::new();

/// Ask the app to open `paths` (absolute file paths, in the order the OS gave them). Wakes the app
/// when it is idle.
pub fn request(paths: impl IntoIterator<Item = String>) {
    let mut queue = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
    let before = queue.len();
    queue.extend(paths);
    let added = queue.len() > before;
    drop(queue);
    if added && let Some(ctx) = WAKE.get() {
        ctx.request_repaint();
    }
}

/// The requested paths so far, oldest first; the queue is empty afterwards.
pub(crate) fn take() -> Vec<String> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Let [`request`] wake the app's event loop.
pub(crate) fn follow(ctx: &egui::Context) {
    WAKE.get_or_init(|| ctx.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test: the queue is process-wide.
    #[test]
    fn requests_are_taken_once_in_order() {
        let _ = take();
        request(["/a/one.fcproj".to_string()]);
        request(["/a/two.mov".to_string(), "/a/three.png".to_string()]);
        request(Vec::new());
        assert_eq!(take(), ["/a/one.fcproj", "/a/two.mov", "/a/three.png"]);
        assert!(take().is_empty(), "taken paths are not opened twice");
    }
}
