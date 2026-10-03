//! Windows media keys and the volume-flyout media overlay (SystemMediaTransportControls),
//! attached to our window via the documented Win32 interop.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use tokio::sync::mpsc;
use windows::{
    Foundation::{TypedEventHandler, Uri},
    Media::{
        MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
        SystemMediaTransportControlsButton as Button, SystemMediaTransportControlsButtonPressedEventArgs,
    },
    Storage::Streams::RandomAccessStreamReference,
    Win32::{Foundation::HWND, System::WinRT::ISystemMediaTransportControlsInterop},
    core::{HSTRING, Ref, Result, factory},
};

use crate::player::Command;
use crate::ui::App;

fn attach(hwnd: isize, on_button: impl Fn(Button) + Send + 'static) -> Result<SystemMediaTransportControls> {
    let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(HWND(hwnd as _)) }?;
    smtc.SetIsEnabled(true)?;
    smtc.SetIsPlayEnabled(true)?;
    smtc.SetIsPauseEnabled(true)?;
    smtc.SetIsNextEnabled(true)?;
    smtc.SetIsPreviousEnabled(true)?;
    smtc.ButtonPressed(&TypedEventHandler::new(
        move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            on_button(args.ok()?.Button()?);
            Ok(())
        },
    ))?;
    Ok(smtc)
}

fn update(smtc: &SystemMediaTransportControls, title: &str, artist: &str, cover_url: &str, playing: bool) -> Result<()> {
    smtc.SetPlaybackStatus(if playing { MediaPlaybackStatus::Playing } else { MediaPlaybackStatus::Paused })?;
    let display = smtc.DisplayUpdater()?;
    display.SetType(MediaPlaybackType::Music)?;
    let music = display.MusicProperties()?;
    music.SetTitle(&HSTRING::from(title))?;
    music.SetArtist(&HSTRING::from(artist))?;
    if !cover_url.is_empty() {
        let uri = Uri::CreateUri(&HSTRING::from(cover_url))?;
        display.SetThumbnail(&RandomAccessStreamReference::CreateFromUri(&uri)?)?;
    }
    display.Update()
}

/// Hooks media keys to the backend once the native window exists. Slint only hands out the
/// HWND after winit has created the window inside the event loop (before that it reports
/// NotSupported), so the caller retries; returns false while the handle isn't available yet.
pub fn setup(app: &App, tx: &mpsc::UnboundedSender<Command>) -> bool {
    let hwnd = match app.window().window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        Ok(other) => {
            app.set_status(format!("Media keys unavailable: unexpected window handle {other:?}").into());
            return true;
        }
        Err(_) => return false,
    };
    let tx = tx.clone();
    let on_button = move |b: Button| {
        let cmd = match b {
            Button::Play | Button::Pause => Command::Toggle,
            Button::Next => Command::Next,
            Button::Previous => Command::Prev,
            _ => return,
        };
        let _ = tx.send(cmd);
    };
    let controls = match attach(hwnd, on_button) {
        Ok(c) => c,
        Err(e) => {
            app.set_status(format!("Media keys unavailable: {e}").into());
            return true;
        }
    };
    let weak = app.as_weak();
    app.on_now_playing_changed(move || {
        let Some(app) = weak.upgrade() else { return };
        let now = app.get_now();
        // Overlay is best-effort; playback doesn't depend on it.
        let _ = update(&controls, &now.title, &now.artist, &now.cover_url, app.get_playing());
    });
    true
}
