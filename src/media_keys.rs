//! Windows media keys and the volume-flyout media overlay (SystemMediaTransportControls),
//! attached to our window via the documented Win32 interop.

use std::cell::RefCell;

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

thread_local! {
    /// The controls attached to our window (UI thread only).
    static CONTROLS: RefCell<Option<SystemMediaTransportControls>> = const { RefCell::new(None) };
}

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

/// Hooks the media keys to the backend; call once the native window exists.
pub fn setup(hwnd: isize, tx: mpsc::UnboundedSender<Command>) -> std::result::Result<(), String> {
    let on_button = move |b: Button| {
        let cmd = match b {
            Button::Play | Button::Pause => Command::Toggle,
            Button::Next => Command::Next,
            Button::Previous => Command::Prev,
            _ => return,
        };
        let _ = tx.send(cmd);
    };
    let controls = attach(hwnd, on_button).map_err(|e| format!("Media keys unavailable: {e}"))?;
    CONTROLS.set(Some(controls));
    Ok(())
}

/// Shows the playing track in the Windows media overlay. Best-effort; playback doesn't depend on it.
pub fn now_playing(title: &str, artist: &str, cover_url: &str, playing: bool) {
    CONTROLS.with_borrow(|controls| {
        if let Some(c) = controls {
            let _ = update(c, title, artist, cover_url, playing);
        }
    });
}
