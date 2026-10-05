//! Album/playlist thumbnails: fetched only for rows the ListView instantiates, kept bounded.

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use librespot::core::session::Session;
use slint::{Model, ModelRc, Rgba8Pixel, SharedPixelBuffer};
use tokio::sync::mpsc;

use crate::ui::{App, Row};

// Decoded at 64 px so 32 px thumbnails stay sharp at 200% scaling; 16 KiB each.
const COVER_PX: u32 = 64;
// ~4.7 MiB of pixels at most, far more than the rows visible at once.
const MAX_LOADED_COVERS: usize = 300;
const COVER_FETCHES_IN_FLIGHT: usize = 6;
// Now-playing panel and page header covers (kinds 3 and 4): 300 px is Spotify's middle size.
const BIG_COVER_PX: u32 = 300;
// Corner radius baked into non-artist covers, in thumbnail pixels (~4 px on a 48 px row at 1x).
const CORNER_PX: f32 = 6.0;

/// kind: 0 = track list, 1 = sidebar, 2 = now-playing bar, 3 = now-playing panel, 4 = page header,
/// 5 = "Add songs" results, 6/7 = Home tiles (top/bottom row)
/// (matches the Slint callback).
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

pub struct CoverRequest {
    pub kind: i32,
    pub index: usize,
    pub uri: String,
    pub url: String,
}

/// Fetches, decodes and downsizes covers for rows the ListView actually instantiated.
pub async fn worker(session: Session, ui: slint::Weak<App>, mut rx: mpsc::UnboundedReceiver<CoverRequest>) {
    let limit = Arc::new(tokio::sync::Semaphore::new(COVER_FETCHES_IN_FLIGHT));
    let in_flight = Arc::new(Mutex::new(HashSet::new()));
    while let Some(req) = rx.recv().await {
        // init and `changed data` can both fire for the same row.
        if !in_flight.lock().unwrap().insert((req.kind, req.uri.clone())) {
            continue;
        }
        let (session, ui, limit, in_flight) = (session.clone(), ui.clone(), limit.clone(), in_flight.clone());
        tokio::spawn(async move {
            let _permit = limit.acquire().await;
            let key = (req.kind, req.uri.clone());
            // A missing cover leaves the grey placeholder; nothing else depends on it.
            // Artists are shown round, like Spotify.
            let round = req.uri.starts_with("spotify:artist:");
            let (url, px) = if req.kind >= 3 { (larger_cover(&req.url), BIG_COVER_PX) } else { (req.url.clone(), COVER_PX) };
            if let Ok(pixels) = fetch_thumbnail(&session, &url, round, px).await {
                let _ = ui.upgrade_in_event_loop(move |app| set_cover(&app, req, pixels));
            }
            in_flight.lock().unwrap().remove(&key);
        });
    }
}

async fn fetch_thumbnail(session: &Session, url: &str, round: bool, px: u32) -> Result<SharedPixelBuffer<Rgba8Pixel>, String> {
    let req = http::Request::get(url).body(bytes::Bytes::new()).map_err(|e| e.to_string())?;
    let resp = session.http_client().request_fut(req).map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body = http_body_util::BodyExt::collect(resp.into_body()).await.map_err(|e| e.to_string())?.to_bytes();
    let img = image::load_from_memory(&body).map_err(|e| e.to_string())?;
    let mut thumb = image::imageops::thumbnail(&img.to_rgba8(), px, px);
    let (w, h) = thumb.dimensions();
    let radius = if round { w.min(h) as f32 / 2.0 } else { CORNER_PX };
    for (x, y, px) in thumb.enumerate_pixels_mut() {
        px[3] = (px[3] as f32 * corner_coverage(x, y, w, h, radius)).round() as u8;
    }
    Ok(SharedPixelBuffer::clone_from_slice(thumb.as_raw(), w, h))
}

/// How much of pixel (x, y) lies inside a w×h rectangle with rounded corners of `radius`, 0..=1,
/// with a one-pixel soft edge. Slint's software renderer ignores border-radius when clipping
/// images, so rounded/round covers are baked into the pixels instead.
fn corner_coverage(x: u32, y: u32, w: u32, h: u32, radius: f32) -> f32 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    // Distance from the pixel to the nearest corner circle centre, only inside the corner zones.
    let cx = px.clamp(radius, w as f32 - radius);
    let cy = py.clamp(radius, h as f32 - radius);
    let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    (radius + 0.5 - dist).clamp(0.0, 1.0)
}

/// The 300 px variant of a Spotify cover URL; rows only keep the smallest one. Album covers encode
/// the size in the image id, mosaics in the path. Unknown forms are returned as they are.
pub fn larger_cover(url: &str) -> String {
    url.replace("ab67616d00004851", "ab67616d00001e02").replace("mosaic.scdn.co/60/", "mosaic.scdn.co/300/")
}

// Share of the cover color kept for the panel tint, so white text stays readable (looked right at 0.45).
const TINT_BRIGHTNESS: f32 = 0.45;

fn dim(channel: u8) -> u8 {
    (channel as f32 * TINT_BRIGHTNESS) as u8
}

/// Alpha-weighted average of RGBA bytes; the transparent rounded corners don't pull it to black.
fn average_rgb(rgba: &[u8]) -> [u8; 3] {
    let (mut sum, mut weight) = ([0u64; 3], 0u64);
    for px in rgba.chunks_exact(4) {
        let a = px[3] as u64;
        for c in 0..3 {
            sum[c] += px[c] as u64 * a;
        }
        weight += a;
    }
    sum.map(|s| s.checked_div(weight).unwrap_or(0) as u8)
}

#[cfg(test)]
mod tests {
    use super::{average_rgb, corner_coverage, larger_cover};

    #[test]
    fn picks_the_300px_cover() {
        assert_eq!(larger_cover("https://i.scdn.co/image/ab67616d00004851abc"), "https://i.scdn.co/image/ab67616d00001e02abc");
        assert_eq!(larger_cover("https://mosaic.scdn.co/60/a"), "https://mosaic.scdn.co/300/a");
        assert_eq!(larger_cover("https://other/x"), "https://other/x");
    }

    #[test]
    fn average_ignores_transparent_pixels() {
        assert_eq!(average_rgb(&[200, 100, 0, 255, 0, 0, 0, 0]), [200, 100, 0]);
        assert_eq!(average_rgb(&[]), [0, 0, 0]);
    }

    #[test]
    fn corners_are_transparent_and_centre_opaque() {
        // Circle on 64 px: corners out, centre and edge midpoints in.
        assert_eq!(corner_coverage(0, 0, 64, 64, 32.0), 0.0);
        assert_eq!(corner_coverage(32, 32, 64, 64, 32.0), 1.0);
        assert!(corner_coverage(32, 0, 64, 64, 32.0) > 0.99); // on the rim: anti-aliased, ~opaque
        // Small radius: only the very corner is cut.
        assert_eq!(corner_coverage(0, 0, 64, 64, 6.0), 0.0);
        assert_eq!(corner_coverage(10, 0, 64, 64, 6.0), 1.0);
        let edge = corner_coverage(1, 1, 64, 64, 6.0);
        assert!(edge > 0.0 && edge < 1.0, "soft edge, got {edge}");
    }
}

thread_local! {
    /// Rows currently holding a decoded cover, oldest first (UI thread only).
    static LOADED_COVERS: RefCell<VecDeque<(i32, String)>> = RefCell::default();
}

fn row_model(app: &App, kind: i32) -> ModelRc<Row> {
    match kind {
        1 => app.get_lists(),
        5 => app.get_add_results(),
        6 => app.get_quick_top(),
        7 => app.get_quick_bottom(),
        _ => app.get_tracks(),
    }
}

/// Applies a cover if the row is still the one that asked (the list may have changed meanwhile),
/// then evicts the oldest covers beyond MAX_LOADED_COVERS so RAM stays bounded on huge lists.
fn set_cover(app: &App, req: CoverRequest, pixels: SharedPixelBuffer<Rgba8Pixel>) {
    if req.kind == 3 {
        if app.get_now().uri == req.uri.as_str() {
            app.set_now_big(slint::Image::from_rgba8(pixels));
        }
        return;
    }
    if req.kind == 4 {
        if app.get_page_cover_url() == req.url.as_str() {
            app.set_page_cover(slint::Image::from_rgba8(pixels));
        }
        return;
    }
    if req.kind == 2 {
        let mut now = app.get_now();
        if now.uri == req.uri.as_str() {
            let [r, g, b] = average_rgb(pixels.as_bytes());
            app.set_now_tint(slint::Color::from_rgb_u8(dim(r), dim(g), dim(b)));
            now.cover = slint::Image::from_rgba8(pixels);
            app.set_now(now);
        }
        return;
    }
    let model = row_model(app, req.kind);
    let Some(mut row) = model.row_data(req.index) else { return };
    if row.uri != req.uri.as_str() || row.cover_url != req.url.as_str() {
        return;
    }
    row.cover = slint::Image::from_rgba8(pixels);
    model.set_row_data(req.index, row);
    LOADED_COVERS.with_borrow_mut(|loaded| {
        loaded.push_back((req.kind, req.uri));
        while loaded.len() > MAX_LOADED_COVERS {
            let Some((kind, uri)) = loaded.pop_front() else { break };
            let model = row_model(app, kind);
            // ponytail: linear scan per eviction; fine for a few thousand rows, index map if lists get huge.
            if let Some(i) = (0..model.row_count()).find(|&i| model.row_data(i).is_some_and(|r| r.uri == uri.as_str())) {
                let mut row = model.row_data(i).expect("index from row_count");
                row.cover = Default::default();
                model.set_row_data(i, row);
            }
        }
    });
}
