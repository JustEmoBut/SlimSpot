//! Album/playlist covers: fetched only for rows the UI instantiates. The UI decodes them at the
//! size they're drawn (Freya's ImageViewer) and keeps a bounded number of them.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use librespot::core::session::Session;
use tokio::sync::mpsc;

use crate::ui::{Ui, UiUpdate};

const COVER_FETCHES_IN_FLIGHT: usize = 6;
/// Cover uploads are scaled to this square; a JPEG at JPEG_QUALITY stays far below Spotify's
/// 256 KB (base64) limit.
const UPLOAD_PX: u32 = 500;
const JPEG_QUALITY: u8 = 85;

/// Any JPEG/PNG file as a square JPEG for `PUT /playlists/{id}/images`.
pub fn upload_jpeg(file: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(file).map_err(|e| format!("Not a usable image: {e}"))?;
    let square = img.resize_to_fill(UPLOAD_PX, UPLOAD_PX, image::imageops::FilterType::Triangle).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&square)
        .map_err(|e| format!("JPEG encoding failed: {e}"))?;
    Ok(out)
}

/// Downloads cover URLs the UI asks for and hands back the encoded image.
pub async fn worker(session: Session, ui: Ui, mut rx: mpsc::UnboundedReceiver<String>) {
    let limit = Arc::new(tokio::sync::Semaphore::new(COVER_FETCHES_IN_FLIGHT));
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    while let Some(url) = rx.recv().await {
        if !in_flight.lock().unwrap().insert(url.clone()) {
            continue;
        }
        let (session, ui, limit, in_flight) = (session.clone(), ui.clone(), limit.clone(), in_flight.clone());
        tokio::spawn(async move {
            let _permit = limit.acquire().await;
            // A missing cover leaves the grey placeholder; nothing else depends on it.
            match fetch(&session, &url).await {
                Ok(bytes) => ui.send(UiUpdate::Cover(url.clone(), bytes)),
                Err(e) => log::warn!("Cover {url}: {e}"),
            }
            in_flight.lock().unwrap().remove(&url);
        });
    }
}

async fn fetch(session: &Session, url: &str) -> Result<bytes::Bytes, String> {
    let req = http::Request::get(url).body(bytes::Bytes::new()).map_err(|e| e.to_string())?;
    let resp = session.http_client().request_fut(req).map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(http_body_util::BodyExt::collect(resp.into_body()).await.map_err(|e| e.to_string())?.to_bytes())
}

/// The 300 px variant of a Spotify cover URL; rows only keep the smallest one. Album covers encode
/// the size in the image id, mosaics in the path. Unknown forms are returned as they are.
pub fn larger_cover(url: &str) -> String {
    url.replace("ab67616d00004851", "ab67616d00001e02").replace("mosaic.scdn.co/60/", "mosaic.scdn.co/300/")
}

#[cfg(test)]
mod tests {
    use super::larger_cover;

    #[test]
    fn picks_the_300px_cover() {
        assert_eq!(larger_cover("https://i.scdn.co/image/ab67616d00004851abc"), "https://i.scdn.co/image/ab67616d00001e02abc");
        assert_eq!(larger_cover("https://mosaic.scdn.co/60/a"), "https://mosaic.scdn.co/300/a");
        assert_eq!(larger_cover("https://other/x"), "https://other/x");
    }
}
