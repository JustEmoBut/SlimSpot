//! Single instance on Linux: a Unix socket in the user's runtime directory. The first launch
//! listens on it; a second launch connects (which wakes the running window) and exits.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub enum Instance {
    /// We're the only one. `None` if the socket couldn't be created: run anyway, just unguarded.
    Primary(Option<UnixListener>),
    /// Another instance is running and has been asked to show itself.
    Secondary,
}

/// `$XDG_RUNTIME_DIR` is per user and cleared at logout; `/tmp` with the user id otherwise.
fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("slimspot.sock"),
        None => std::env::temp_dir().join(format!("slimspot-{}.sock", unsafe { libc::getuid() })),
    }
}

pub fn acquire() -> Instance {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Instance::Secondary;
    }
    // Nobody answers: a socket file left by a crash. Take it over.
    let _ = std::fs::remove_file(&path);
    match UnixListener::bind(&path) {
        Ok(listener) => Instance::Primary(Some(listener)),
        Err(e) => {
            log::warn!("single-instance socket {}: {e}; running without the guard", path.display());
            Instance::Primary(None)
        }
    }
}

/// Shows the window (from tray or minimized) and asks the window manager to focus it.
pub fn bring_to_front(window: &freya::winit::window::Window) {
    window.set_visible(true);
    window.set_minimized(false);
    window.focus_window();
}

impl Instance {
    /// Calls `on_wake` (from a background thread) every time another launch connects.
    pub fn listen(self, on_wake: impl Fn() + Send + 'static) {
        let Instance::Primary(Some(listener)) = self else { return };
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stream.is_ok() {
                    on_wake();
                }
            }
        });
    }
}
