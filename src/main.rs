#![windows_subsystem = "windows"]

#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod clipboard;
mod covers;
#[cfg(windows)]
mod dialog;
#[cfg(windows)]
mod instance;
mod logger;
mod lyrics;
mod nav;
#[cfg(windows)]
mod media_keys;
mod player;
mod settings;
#[cfg(windows)]
mod taskbar;
mod ui;
mod web;

use std::time::Duration;

use slint::{ComponentHandle, Model};
use tokio::sync::mpsc;

use crate::covers::CoverRequest;

// Mini player window: just wide enough for the bar's three sections.
const MINI_WIDTH: f32 = 860.0;
const MINI_HEIGHT: f32 = 96.0;
use crate::player::Command;
use crate::ui::{App, Tray};

const POSITION_TICK: Duration = Duration::from_millis(500);
// NOTIFYICONDATAW.szTip holds 128 UTF-16 units including the terminator.
const TOOLTIP_MAX_CHARS: usize = 120;
// Polls for the native window handle after startup; gives up after ~5 s.
#[cfg(windows)]
const MEDIA_KEYS_RETRY: Duration = Duration::from_millis(100);
#[cfg(windows)]
const MEDIA_KEYS_MAX_TRIES: u32 = 50;
// ponytail: polling because winit's minimize/restore events need Slint's unstable winit API.
const RESTORE_POLL: Duration = Duration::from_millis(100);
// If the backend hasn't finished quitting by then (e.g. still logging in), quit anyway.
const QUIT_GRACE: Duration = Duration::from_secs(3);

/// Tray icon, decoded from the PNG rendered off assets/icon.svg (no runtime SVG renderer).
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
    // Launched by the Windows Run entry: stay in the tray.
    #[cfg(windows)]
    let start_in_tray = std::env::args().any(|a| a == autostart::TRAY_ARG);
    #[cfg(windows)]
    {
        app.set_start_with_windows(autostart::is_enabled());
        let weak = app.as_weak();
        app.on_set_autostart(move |on| {
            let Some(app) = weak.upgrade() else { return };
            if let Err(e) = autostart::set(on) {
                app.set_status(e.into());
            }
            // Show what Windows will actually do, even if the change failed.
            app.set_start_with_windows(autostart::is_enabled());
        });
    }

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
    app.on_transfer(send_str(Command::Transfer));
    app.on_toggle_like(send(|| Command::ToggleLike));
    app.on_load_devices(send(|| Command::LoadDevices));
    app.on_start_radio(send(|| Command::Radio(None)));
    app.on_show_queue(send(|| Command::Queue));
    app.on_go_to_playing(send(|| Command::GoToPlaying));
    app.on_go_home(send(|| Command::Home));
    app.on_check_liked_more(send(|| Command::CheckLikedMore));
    app.on_play_page(send(|| Command::PlayPage));
    app.on_add_search(send_str(Command::AddSearch));
    app.on_add_to_page(send_str(Command::AddToPage));
    {
        let tx = tx.clone();
        app.on_choose_search_tab(move |tab| {
            let _ = tx.send(Command::SearchTab(tab));
        });
    }
    {
        let tx = tx.clone();
        app.on_filter_library(move |kind, text, sort| {
            let _ = tx.send(Command::FilterLibrary { kind, text: text.to_string(), sort });
        });
    }
    {
        let filter_tx = tx.clone();
        app.on_filter_page(move |text, sort| {
            let _ = filter_tx.send(Command::FilterPage { text: text.to_string(), sort });
        });
        let tx = tx.clone();
        app.on_move_row(move |from, to| {
            let _ = tx.send(Command::MoveRow { from: from as usize, to: to as usize });
        });
    }
    {
        let tx = tx.clone();
        app.on_set_sleep(move |minutes| {
            let _ = tx.send(Command::SleepTimer(minutes));
        });
    }
    {
        let tx = tx.clone();
        app.on_create_playlist(move |name, description| {
            let _ = tx.send(Command::CreatePlaylist { name: name.into(), description: description.into() });
        });
    }
    {
        let tx = tx.clone();
        app.on_edit_playlist(move |name, description| {
            let _ = tx.send(Command::EditPlaylist { name: name.into(), description: description.into() });
        });
    }
    #[cfg(windows)]
    {
        let (weak, tx) = (app.as_weak(), tx.clone());
        app.on_change_cover(move || {
            let (weak, tx) = (weak.clone(), tx.clone());
            // Called from the popup's click handler, which closes the popup right after: the
            // modal dialog opens on the next turn of the event loop, once the popup is gone.
            slint::Timer::single_shot(Duration::ZERO, move || {
                let Some(app) = weak.upgrade() else { return };
                if let Some(path) = dialog::pick_image(&app) {
                    let _ = tx.send(Command::PlaylistCover(path));
                }
            });
        });
    }
    {
        // Mini player: remember the full size and shrink to the player bar (and back).
        let weak = app.as_weak();
        let full_size = std::cell::Cell::new(None);
        app.on_toggle_mini(move || {
            let Some(app) = weak.upgrade() else { return };
            let window = app.window();
            let mini = !app.get_mini();
            if mini {
                full_size.set(Some(window.size()));
            }
            app.set_mini(mini);
            match (mini, full_size.get()) {
                (true, _) => window.set_size(slint::LogicalSize::new(MINI_WIDTH, MINI_HEIGHT)),
                (false, Some(size)) => window.set_size(size),
                (false, None) => {}
            }
        });
    }
    let action_tx = tx.clone();
    app.on_row_action(move |action, uri| {
        let _ = action_tx.send(Command::RowAction { action: action.into(), uri: uri.into() });
    });
    app.on_go_back(send(|| Command::Back));
    app.on_go_forward(send(|| Command::Forward));
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
            let now = a.get_now();
            let tip: String = match now.title.as_str() {
                "" => "SlimSpot".into(),
                title => format!("{title} - {}", now.artist).chars().take(TOOLTIP_MAX_CHARS).collect(),
            };
            if t.get_tip() != tip.as_str() {
                t.set_tip(tip.into());
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
            let done = if media_keys::setup(&app, &tx) {
                true
            } else if tries >= MEDIA_KEYS_MAX_TRIES {
                app.set_status("Media keys unavailable (no window handle)".into());
                true
            } else {
                false
            };
            if done {
                timer.stop();
                taskbar::setup(&app, &tx);
                instance::dark_title_bar(&app);
                // Tray start: the window was only shown so its HWND exists for the media keys.
                if start_in_tray {
                    let _ = app.hide();
                }
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
        // Highlight the sung line; at a 500 ms tick a line lights up at most half a second late.
        if app.get_show_lyrics() {
            let times: Vec<u32> = app.get_lyric_times().iter().map(|t| t as u32).collect();
            let index = lyrics::current_line(&times, app.get_position() as u32).map_or(-1, |i| i as i32);
            if index != app.get_lyric_index() {
                app.set_lyric_index(index);
            }
        }
    });

    let (cover_tx, cover_rx) = mpsc::unbounded_channel();
    app.on_need_cover(move |kind, index, uri, url| {
        let _ = cover_tx.send(CoverRequest { kind, index: index as usize, uri: uri.into(), url: url.into() });
    });

    let backend_tx = tx.clone();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(player::run(weak, backend_tx, rx, cover_rx));
    });

    // Shown even when starting in the tray: the media keys need the native window, which
    // exists only after a first show; it is hidden again as soon as they are attached.
    app.show()?;
    // Not `app.run()`: that ends when the last window closes, but a hidden window must keep playing.
    slint::run_event_loop_until_quit()
}
