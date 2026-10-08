//! Plain-text clipboard ("Copy link", copying and pasting song links), straight through Win32.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
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

/// The clipboard's text, "" when it holds none.
pub fn get_text() -> Result<String, String> {
    unsafe {
        OpenClipboard(None).map_err(|e| format!("Clipboard busy: {e}"))?;
        let text = (|| {
            let Ok(handle) = GetClipboardData(CF_UNICODETEXT) else { return String::new() };
            let memory = windows::Win32::Foundation::HGLOBAL(handle.0);
            let source = GlobalLock(memory) as *const u16;
            if source.is_null() {
                return String::new();
            }
            let len = (0..).take_while(|&i| *source.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(source, len));
            let _ = GlobalUnlock(memory);
            text
        })();
        let _ = CloseClipboard();
        Ok(text)
    }
}

/// Track URIs in pasted text: `open.spotify.com/track/ID` links (any query string) or
/// `spotify:track:ID`, one or more per line. Anything else (albums, playlists) is skipped.
pub fn track_uris(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|word| {
            let id = word
                .strip_prefix("spotify:track:")
                .or_else(|| word.split_once("open.spotify.com/track/").map(|(_, rest)| rest))?;
            let id: String = id.chars().take_while(char::is_ascii_alphanumeric).collect();
            (id.len() == SPOTIFY_ID_LEN).then(|| format!("spotify:track:{id}"))
        })
        .collect()
}

/// Spotify IDs are 22 base-62 characters.
const SPOTIFY_ID_LEN: usize = 22;

/// `spotify:track:ID` → `https://open.spotify.com/track/ID`; `None` for things without a web page.
pub fn web_link(uri: &str) -> Option<String> {
    let mut parts = uri.strip_prefix("spotify:")?.split(':');
    let (kind, id) = (parts.next()?, parts.next()?);
    (parts.next().is_none() && ["track", "album", "artist", "playlist"].contains(&kind))
        .then(|| format!("https://open.spotify.com/{kind}/{id}"))
}

#[cfg(test)]
mod tests {
    use super::{track_uris, web_link};

    #[test]
    fn finds_pasted_track_links() {
        let text = "https://open.spotify.com/track/0DiWol3AO6WpXZgp0goxAV?si=abc\nspotify:track:4uLU6hMCjMI75M1A2tKUQC \
                    https://open.spotify.com/album/0DiWol3AO6WpXZgp0goxAV spotify:track:short";
        assert_eq!(track_uris(text), ["spotify:track:0DiWol3AO6WpXZgp0goxAV", "spotify:track:4uLU6hMCjMI75M1A2tKUQC"]);
        assert!(track_uris("").is_empty());
    }

    #[test]
    fn builds_open_spotify_links() {
        assert_eq!(web_link("spotify:track:abc").as_deref(), Some("https://open.spotify.com/track/abc"));
        assert_eq!(web_link("spotify:playlist:p1").as_deref(), Some("https://open.spotify.com/playlist/p1"));
        assert_eq!(web_link("liked"), None);
        assert_eq!(web_link("spotify:user:me:collection"), None);
    }
}
