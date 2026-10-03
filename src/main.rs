#![windows_subsystem = "windows"]

mod covers;
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
use crate::ui::App;

const POSITION_TICK: Duration = Duration::from_millis(500);
// Polls for the native window handle after startup; gives up after ~5 s.
#[cfg(windows)]
const MEDIA_KEYS_RETRY: Duration = Duration::from_millis(100);
#[cfg(windows)]
const MEDIA_KEYS_MAX_TRIES: u32 = 50;
// ponytail: polling because winit's minimize/restore events need Slint's unstable winit API.
const RESTORE_POLL: Duration = Duration::from_millis(100);

fn main() -> Result<(), slint::PlatformError> {
    logger::init(&player::cache_dir());
    let app = App::new()?;
    let (tx, rx) = mpsc::unbounded_channel();

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

    app.run()
}
