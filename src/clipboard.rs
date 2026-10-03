//! Plain-text clipboard writes (for "Copy link"), straight through Win32.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

// CF_UNICODETEXT from WinUser.h; the windows crate only exports it behind the OLE feature.
const CF_UNICODETEXT: u32 = 13;

pub fn set_text(text: &str) -> Result<(), String> {
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = utf16.len() * size_of::<u16>();
    unsafe {
        OpenClipboard(None).map_err(|e| format!("Clipboard busy: {e}"))?;
        let result = (|| {
            EmptyClipboard()?;
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let target = GlobalLock(memory) as *mut u16;
            if target.is_null() {
                return Err(windows::core::Error::from_thread());
            }
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), target, utf16.len());
            let _ = GlobalUnlock(memory);
            // On success the clipboard owns the memory; it must not be freed here.
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result.map_err(|e| format!("Copying failed: {e}"))
    }
}

/// `spotify:track:ID` → `https://open.spotify.com/track/ID`; `None` for things without a web page.
pub fn web_link(uri: &str) -> Option<String> {
    let mut parts = uri.strip_prefix("spotify:")?.split(':');
    let (kind, id) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && ["track", "album", "artist", "playlist"].contains(&kind))
        .then(|| format!("https://open.spotify.com/{kind}/{id}"))
}

#[cfg(test)]
mod tests {
    use super::web_link;

    #[test]
    fn builds_open_spotify_links() {
        assert_eq!(web_link("spotify:track:abc").as_deref(), Some("https://open.spotify.com/track/abc"));
        assert_eq!(web_link("spotify:playlist:p1").as_deref(), Some("https://open.spotify.com/playlist/p1"));
        assert_eq!(web_link("liked"), None);
        assert_eq!(web_link("spotify:user:me:collection"), None);
    }
}
