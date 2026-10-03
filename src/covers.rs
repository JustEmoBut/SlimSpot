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

/// kind: 0 = track list, 1 = sidebar, 2 = now-playing bar (matches the Slint callback).
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
            if let Ok(pixels) = fetch_thumbnail(&session, &req.url).await {
                let _ = ui.upgrade_in_event_loop(move |app| set_cover(&app, req, pixels));
            }
            in_flight.lock().unwrap().remove(&key);
        });
    }
}

async fn fetch_thumbnail(session: &Session, url: &str) -> Result<SharedPixelBuffer<Rgba8Pixel>, String> {
    let req = http::Request::get(url).body(bytes::Bytes::new()).map_err(|e| e.to_string())?;
    let resp = session.http_client().request_fut(req).map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body = http_body_util::BodyExt::collect(resp.into_body()).await.map_err(|e| e.to_string())?.to_bytes();
    let img = image::load_from_memory(&body).map_err(|e| e.to_string())?;
    let thumb = image::imageops::thumbnail(&img.to_rgba8(), COVER_PX, COVER_PX);
    Ok(SharedPixelBuffer::clone_from_slice(thumb.as_raw(), thumb.width(), thumb.height()))
}

thread_local! {
    /// Rows currently holding a decoded cover, oldest first (UI thread only).
    static LOADED_COVERS: RefCell<VecDeque<(bool, String)>> = RefCell::default();
}

fn row_model(app: &App, is_list: bool) -> ModelRc<Row> {
    if is_list { app.get_lists() } else { app.get_tracks() }
}

/// Applies a cover if the row is still the one that asked (the list may have changed meanwhile),
/// then evicts the oldest covers beyond MAX_LOADED_COVERS so RAM stays bounded on huge lists.
fn set_cover(app: &App, req: CoverRequest, pixels: SharedPixelBuffer<Rgba8Pixel>) {
    if req.kind == 2 {
        let mut now = app.get_now();
        if now.uri == req.uri.as_str() {
            now.cover = slint::Image::from_rgba8(pixels);
            app.set_now(now);
        }
        return;
    }
    let is_list = req.kind == 1;
    let model = row_model(app, is_list);
    let Some(mut row) = model.row_data(req.index) else { return };
    if row.uri != req.uri.as_str() || row.cover_url != req.url.as_str() {
        return;
    }
    row.cover = slint::Image::from_rgba8(pixels);
    model.set_row_data(req.index, row);
    LOADED_COVERS.with_borrow_mut(|loaded| {
        loaded.push_back((is_list, req.uri));
        while loaded.len() > MAX_LOADED_COVERS {
            let Some((is_list, uri)) = loaded.pop_front() else { break };
            let model = row_model(app, is_list);
            // ponytail: linear scan per eviction; fine for a few thousand rows, index map if lists get huge.
            if let Some(i) = (0..model.row_count()).find(|&i| model.row_data(i).is_some_and(|r| r.uri == uri.as_str())) {
                let mut row = model.row_data(i).expect("index from row_count");
                row.cover = Default::default();
                model.set_row_data(i, row);
            }
        }
    });
}
