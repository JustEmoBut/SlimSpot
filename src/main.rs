#![windows_subsystem = "windows"]

// ponytail: autostart, dialog, media_keys and taskbar still target the Slint window; they come back as the Freya UI grows (phase 2).
#[cfg(windows)]
mod clipboard;
mod covers;
mod eq;
#[cfg(windows)]
mod instance;
mod logger;
mod lyrics;
mod nav;
mod player;
mod settings;
mod ui;
mod update;
mod web;
mod zip;

use std::time::Duration;

use tokio::sync::mpsc;

// An update's new exe waits this long for the old one to quit.
const UPDATE_WAIT: Duration = Duration::from_secs(10);

fn main() {
    logger::init(&player::cache_dir());
    // Started by an update: let the old instance quit before taking the single-instance guard.
    #[cfg(windows)]
    update::wait_for_previous(UPDATE_WAIT);
    #[cfg(windows)]
    let instance = match instance::acquire() {
        instance::Instance::Secondary => return,
        primary => primary,
    };
    update::remove_old();

    let ui = ui::channel();
    #[cfg(windows)]
    {
        let ui = ui.clone();
        instance.listen(move || ui.send(ui::UiUpdate::Show));
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let (cover_tx, cover_rx) = mpsc::unbounded_channel();
    // "Update to vX" clicks; the chip returns with the settings screen.
    let (_update_tx, update_rx) = mpsc::unbounded_channel();
    let backend_tx = tx.clone();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let backend = rt.handle().clone();
    std::thread::spawn(move || backend.block_on(player::run(ui, backend_tx, rx, cover_rx, update_rx)));
    // The UI thread uses tokio timers too (Freya's own executor polls them).
    let _rt = rt.enter();
    ui::run(tx, cover_tx);
}
