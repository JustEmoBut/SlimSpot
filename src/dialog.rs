//! Native "Open" dialog for choosing a playlist cover image.

use std::path::PathBuf;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::core::{PCWSTR, w};

use crate::ui::App;

/// Blocks (modal) until the user picks a JPEG/PNG or cancels; `None` on cancel or failure.
pub fn pick_image(app: &App) -> Option<PathBuf> {
    pick(app, w!("Images"), w!("*.jpg;*.jpeg;*.png"), w!("Choose a playlist cover"))
}

fn pick(app: &App, kind: PCWSTR, spec: PCWSTR, title: PCWSTR) -> Option<PathBuf> {
    let owner = match app.window().window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Some(HWND(h.hwnd.get() as _)),
        _ => None,
    };
    unsafe {
        // COM is already initialised as STA on the UI thread (winit; see taskbar.rs).
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        dialog.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: kind, pszSpec: spec }]).ok()?;
        dialog.SetTitle(title).ok()?;
        // Cancel comes back as an error.
        dialog.Show(owner).ok()?;
        let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(name.0 as *const _));
        path
    }
}
