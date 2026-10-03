#![windows_subsystem = "windows"]

mod covers;
#[cfg(windows)]
mod instance;
mod logger;
#[cfg(windows)]
mod media_keys;
mod player;
mod settings;
mod ui;
mod web;

use std::time::Duration;

use slint::ComponentHandle;
use tokio::sync::mpsc;

use crate::covers::CoverRequest;
use crate::player::Command;
use crate::ui::{App, Tray};

const POSITION_TICK: Duration = Duration::from_millis(500);
// Polls for the native window handle after startup; gives up after ~5 s.
#[cfg(windows)]
const MEDIA_KEYS_RETRY: Duration = Duration::from_millis(100);
#[cfg(windows)]
const MEDIA_KEYS_MAX_TRIES: u32 = 50;
// ponytail: polling because winit's minimize/restore events need Slint's unstable winit API.
const RESTORE_POLL: Duration = Duration::from_millis(100);
// If the backend hasn't finished quitting by then (e.g. still logging in), quit anyway.
const QUIT_GRACE: Duration = Duration::from_secs(3);

/// Window/tray icon, decoded from the PNG rendered off assets/icon.svg (no runtime SVG renderer).
fn app_icon() -> slint::Image {
    let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon-64.png")) else {
        return slint::Image::default();
    };
    let rgba = img.to_rgba8();
    slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height()))
}

fn main() -> Result<(), slint::PlatformError> {
    logger::init(&player::cache_dir());
    #[cfg(windows)]
    let instance = match instance::acquire() {
        instance::Instance::Secondary => return Ok(()),
        primary => primary,
    };
    let app = App::new()?;
    let (tx, rx) = mpsc::unbounded_channel();
    let icon = app_icon();
    app.set_app_icon(icon.clone());

    // Closing the window hides it to the tray; playback continues. Quit from the tray or Ctrl+Q.
    app.window().on_close_requested(|| slint::CloseRequestResponse::HideWindow);
    let quit = {
        let tx = tx.clone();
        move || {
            let _ = tx.send(Command::Quit);
            slint::Timer::single_shot(QUIT_GRACE, || {
                let _ = slint::quit_event_loop();
            });
        }
    };
    app.on_quit(quit.clone());
    let tray = Tray::new()?;
    tray.set_tray_icon(icon);
    let show = {
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                #[cfg(windows)]
                instance::bring_to_front(&app);
                #[cfg(not(windows))]
                let _ = app.show();
            }
        }
    };
    tray.on_show_window(show.clone());
    tray.on_quit(quit);
    #[cfg(windows)]
    {
        let weak = app.as_weak();
        instance.listen(move || {
            let _ = weak.upgrade_in_event_loop(|app| instance::bring_to_front(&app));
        });
    }

    let send = |cmd: fn() -> Command| {
        let tx = tx.clone();
        move || {
            let _ = tx.send(cmd());
        }
    };
    let send_str = |cmd: fn(String) -> Command| {
        let tx = tx.clone();
        move |s: slint::SharedString| {
            let _ = tx.send(cmd(s.into()));
        }
    };
    app.on_submit(send_str(Command::Submit));
    app.on_open_list(send_str(Command::OpenList));
    app.on_play_uri(send_str(Command::PlayUri));
    app.on_toggle(send(|| Command::Toggle));
    app.on_prev(send(|| Command::Prev));
    app.on_next(send(|| Command::Next));
    tray.on_toggle(send(|| Command::Toggle));
    tray.on_prev(send(|| Command::Prev));
    tray.on_next(send(|| Command::Next));
    // The tray menu says Play or Pause to match the window.
    let tray_weak = tray.as_weak();
    let ui_for_tray = app.as_weak();
    let tray_sync = slint::Timer::default();
    tray_sync.start(slint::TimerMode::Repeated, POSITION_TICK, move || {
        if let (Some(t), Some(a)) = (tray_weak.upgrade(), ui_for_tray.upgrade()) {
            if t.get_playing() != a.get_playing() {
                t.set_playing(a.get_playing());
            }
        }
    });
    app.on_toggle_shuffle(send(|| Command::ToggleShuffle));
    app.on_cycle_repeat(send(|| Command::CycleRepeat));
    let seek_tx = tx.clone();
    app.on_seek(move |ms| {
        let _ = seek_tx.send(Command::Seek(ms as u32));
    });
    let quality_tx = tx.clone();
    app.on_set_quality(move |index| {
        let _ = quality_tx.send(Command::Quality(settings::Quality::from_ui(index as i64)));
    });
    let normalize_tx = tx.clone();
    app.on_set_normalize(move |on| {
        let _ = normalize_tx.send(Command::Normalize(on));
    });
    let volume_tx = tx.clone();
    app.on_set_volume(move |percent, save| {
        let _ = volume_tx.send(Command::Volume { percent, save });
    });

    #[cfg(windows)]
    let media_keys_timer = std::rc::Rc::new(slint::Timer::default());
    #[cfg(windows)]
    {
        let (weak, tx, timer) = (app.as_weak(), tx.clone(), media_keys_timer.clone());
        let mut tries = 0;
        media_keys_timer.start(slint::TimerMode::Repeated, MEDIA_KEYS_RETRY, move || {
            let Some(app) = weak.upgrade() else { return timer.stop() };
            tries += 1;
            if media_keys::setup(&app, &tx) {
                timer.stop();
            } else if tries >= MEDIA_KEYS_MAX_TRIES {
                app.set_status("Media keys unavailable (no window handle)".into());
                timer.stop();
            }
        });
    }

    // See `repaint-flip` in the Slint markup.
    let restore_watch = slint::Timer::default();
    let restore_ui = app.as_weak();
    let mut was_minimized = false;
    restore_watch.start(slint::TimerMode::Repeated, RESTORE_POLL, move || {
        let Some(app) = restore_ui.upgrade() else { return };
        let minimized = app.window().is_minimized();
        if was_minimized && !minimized {
            app.set_repaint_flip(!app.get_repaint_flip());
        }
        was_minimized = minimized;
    });

    // Interpolates the seek bar between player events.
    let ticker = slint::Timer::default();
    let tick_ui = app.as_weak();
    ticker.start(slint::TimerMode::Repeated, POSITION_TICK, move || {
        let Some(app) = tick_ui.upgrade() else { return };
        if app.get_playing() && !app.get_seeking() {
            let next = app.get_position() + POSITION_TICK.as_millis() as f32;
            app.set_position(next.min(app.get_duration()));
        }
    });

    let (cover_tx, cover_rx) = mpsc::unbounded_channel();
    app.on_need_cover(move |kind, index, uri, url| {
        let _ = cover_tx.send(CoverRequest { kind, index: index as usize, uri: uri.into(), url: url.into() });
    });

    let weak = app.as_weak();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(player::run(weak, rx, cover_rx));
    });

    app.show()?;
    // Not `app.run()`: that ends when the last window closes, but a hidden window must keep playing.
    slint::run_event_loop_until_quit()
}
