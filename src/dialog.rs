//! Native "Open" dialog for choosing a playlist cover image.

use std::path::PathBuf;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
use windows::core::{PCWSTR, w};

/// Blocks (modal to `owner`) until the user picks a JPEG/PNG or cancels; `None` on cancel or
/// failure. Meant for a thread of its own, so the UI keeps drawing meanwhile.
pub fn pick_image(owner: Option<isize>) -> Option<PathBuf> {
    pick(owner, w!("Images"), w!("*.jpg;*.jpeg;*.png"), w!("Choose a playlist cover"))
}

fn pick(owner: Option<isize>, kind: PCWSTR, spec: PCWSTR, title: PCWSTR) -> Option<PathBuf> {
    let owner = owner.map(|h| HWND(h as _));
    unsafe {
        // The dialog needs COM in an STA on the calling thread.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
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
