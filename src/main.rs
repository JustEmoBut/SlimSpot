#![windows_subsystem = "windows"]

#[cfg(windows)]
mod autostart;
#[cfg(target_os = "linux")]
#[path = "autostart_linux.rs"]
mod autostart;
mod clipboard;
mod covers;
#[cfg(windows)]
mod dialog;
#[cfg(target_os = "linux")]
#[path = "dialog_linux.rs"]
mod dialog;
mod eq;
#[cfg(windows)]
mod instance;
#[cfg(target_os = "linux")]
#[path = "instance_linux.rs"]
mod instance;
mod logger;
mod lyrics;
#[cfg(windows)]
mod media_keys;
#[cfg(target_os = "linux")]
#[path = "media_keys_linux.rs"]
mod media_keys;
mod nav;
mod player;
mod settings;
#[cfg(windows)]
mod taskbar;
#[cfg(not(windows))]
mod targz;
mod ui;
mod update;
mod web;
#[cfg(windows)]
mod zip;

use std::time::Duration;

use tokio::sync::mpsc;

// An update's new exe waits this long for the old one to quit.
const UPDATE_WAIT: Duration = Duration::from_secs(10);

fn main() {
    logger::init(&player::cache_dir());
    // Started by an update: let the old instance quit before taking the single-instance guard.
    #[cfg(any(windows, target_os = "linux"))]
    update::wait_for_previous(UPDATE_WAIT);
    #[cfg(any(windows, target_os = "linux"))]
    let instance = match instance::acquire() {
        instance::Instance::Secondary => return,
        primary => primary,
    };
    update::remove_old();

    let ui = ui::channel();
    #[cfg(any(windows, target_os = "linux"))]
    {
        let ui = ui.clone();
        instance.listen(move || ui.send(ui::UiUpdate::Show));
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let (cover_tx, cover_rx) = mpsc::unbounded_channel();
    // "Update to vX" clicks, served by the update task.
    let (update_tx, update_rx) = mpsc::unbounded_channel();
    let backend_tx = tx.clone();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let backend = rt.handle().clone();
    std::thread::spawn(move || backend.block_on(player::run(ui, backend_tx, rx, cover_rx, update_rx)));
    // The UI thread uses tokio timers too (Freya's own executor polls them).
    let _rt = rt.enter();
    #[cfg(any(windows, target_os = "linux"))]
    let start_in_tray = std::env::args().any(|a| a == autostart::TRAY_ARG);
    #[cfg(not(any(windows, target_os = "linux")))]
    let start_in_tray = false;
    ui::run(tx, cover_tx, update_tx, start_in_tray);
}
