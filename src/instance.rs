//! Single instance: a second launch wakes the running window instead of starting another
//! player (two Connect devices and two token writers would fight each other).

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
use windows::core::w;

// ASFW_ANY from WinUser.h; not exported by the windows crate version we use.
const ASFW_ANY: u32 = u32::MAX;

pub enum Instance {
    /// We're the only one; the named event doubles as the "already running" marker and the
    /// wake-up signal. `None` if the event couldn't be created: run anyway, just unguarded.
    Primary(Option<isize>),
    /// Another instance is running and has been asked to show itself.
    Secondary,
}

pub fn acquire() -> Instance {
    // Auto-reset event in the session namespace, so each Windows user gets their own instance.
    let Ok(event) = (unsafe { CreateEventW(None, false, false, w!("Local\\SlimSpot.Show")) }) else {
        log::warn!("single-instance event unavailable; running without the guard");
        return Instance::Primary(None);
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // Let the running instance take the foreground, then wake it.
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
        }
        return Instance::Secondary;
    }
    Instance::Primary(Some(event.0 as isize))
}

/// Shows the window (from tray or minimized) and asks Windows to put it in front.
pub fn bring_to_front(app: &crate::ui::App) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::ComponentHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

    let _ = app.show();
    app.window().set_minimized(false);
    if let Ok(RawWindowHandle::Win32(h)) = app.window().window_handle().window_handle().map(|h| h.as_raw()) {
        // Only allowed when the caller holds the foreground right (a click, or AllowSetForegroundWindow
        // from the second launch); otherwise Windows flashes the taskbar button, which is fine.
        let _ = unsafe { SetForegroundWindow(HWND(h.hwnd.get() as _)) };
    }
}

/// Dark title bar to match the dark UI (Windows 10 20H1+; ignored where unsupported).
pub fn dark_title_bar(app: &crate::ui::App) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::ComponentHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE,
        DwmSetWindowAttribute,
    };

    let Ok(RawWindowHandle::Win32(h)) = app.window().window_handle().window_handle().map(|h| h.as_raw()) else { return };
    let hwnd = HWND(h.hwnd.get() as _);
    let set = |attribute: DWMWINDOWATTRIBUTE, value: u32| {
        // Windows 10 ignores the color attributes (Windows 11 only); the dark mode flag still applies.
        let _ = unsafe { DwmSetWindowAttribute(hwnd, attribute, (&value as *const u32).cast(), size_of::<u32>() as u32) };
    };
    set(DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    // The current palette's colors: the caption blends into Theme.base, the title is Theme.subdued.
    let theme = app.global::<crate::ui::Theme>();
    set(DWMWA_CAPTION_COLOR, colorref(theme.get_base()));
    set(DWMWA_TEXT_COLOR, colorref(theme.get_subdued()));
    set(DWMWA_BORDER_COLOR, colorref(theme.get_raised()));
}

/// COLORREF is 0x00BBGGRR.
fn colorref(c: slint::Color) -> u32 {
    u32::from(c.red()) | u32::from(c.green()) << 8 | u32::from(c.blue()) << 16
}

impl Instance {
    /// Calls `on_wake` (from a background thread) every time another launch signals us.
    pub fn listen(self, on_wake: impl Fn() + Send + 'static) {
        let Instance::Primary(Some(handle)) = self else { return };
        std::thread::spawn(move || loop {
            let event = HANDLE(handle as *mut core::ffi::c_void);
            unsafe { WaitForSingleObject(event, INFINITE) };
            on_wake();
        });
    }
}
