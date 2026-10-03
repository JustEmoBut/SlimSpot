//! Spotify Web API: its own OAuth grant, search, playlists and Liked Songs.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use librespot::core::session::Session;
use librespot_oauth::{OAuthClient, OAuthClientBuilder, OAuthToken};

// The desktop client id is rejected/rate-limited on api.spotify.com (keymaster 403, login5 429),
// so the Web API uses a separate app: your own via SLIMSPOT_WEB_CLIENT_ID (its redirect URI must
// be WEB_REDIRECT_URI), else the one the saved token was granted to, else the public app shared
// by ncspot / spotify-player / Spotifast.
const DEFAULT_WEB_CLIENT_ID: &str = "d420a117a32841c2b3474932e49fb54b";
const WEB_CLIENT_ID_ENV: &str = "SLIMSPOT_WEB_CLIENT_ID";
const WEB_REDIRECT_URI: &str = "http://127.0.0.1:8989/login";
const WEB_SCOPES: &[&str] = &[
    "user-read-private",
    "user-library-read",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-modify",
    "user-follow-read",
    "user-read-playback-state",
    "user-modify-playback-state",
    "playlist-modify-private",
    "playlist-modify-public",
    "user-read-recently-played",
];
const WEB_TOKEN_FILE: &str = "web_refresh_token";
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);
// Same limits Spotifast uses for Web API 429s.
const RATE_LIMIT_RETRIES: u32 = 3;
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);
const API: &str = "https://api.spotify.com/v1";
// Spotify rejects limit > 10 ("Invalid limit") on search and artist albums for Development Mode
// apps (verified 2026-10-03 with a personal client id); playlists and saved tracks still take 50.
pub const SEARCH_LIMIT: u32 = 10;
const ARTIST_ALBUMS_LIMIT: u32 = 10;
// Artist albums are paged 10 at a time; stop after this many pages.
const ARTIST_ALBUM_PAGES: usize = 5;
// Search shows a few artists and albums above the tracks; tracks still get SEARCH_LIMIT.
const SEARCH_ARTISTS: usize = 3;
const SEARCH_ALBUMS: usize = 6;
const CONTAINS_LIMIT: usize = 40;
const PAGE_LIMIT: u32 = 50;
pub const LIKED_SONGS: &str = "liked";

/// Thread-safe row data; `ui::Row` holds a `slint::Image`, which must stay on the UI thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub title: String,
    pub artist: String,
    pub uri: String,
    pub cover_url: String,
    /// First artist and the album of a track (for the row menu); empty when unknown.
    pub artist_uri: String,
    pub album_uri: String,
    /// Track length; 0 when unknown (non-track rows).
    pub duration_ms: u32,
    /// In Liked Songs; filled in after a page opens (`WebApi::contains`).
    pub liked: bool,
}

pub struct WebApi {
    client: OAuthClient,
    token: OAuthToken,
    token_path: PathBuf,
    /// First line of the token file; a saved token is only reused when client id and scopes match.
    grant_key: String,
}

impl WebApi {
    /// Reuses the saved refresh token when it was granted to the same app with the same scopes;
    /// otherwise calls `on_browser` and opens a browser login.
    pub async fn login(dir: &Path, on_browser: impl FnOnce()) -> Result<Self, String> {
        let token_path = dir.join(WEB_TOKEN_FILE);
        let saved = std::fs::read_to_string(&token_path).unwrap_or_default();
        let saved = saved.split_once('\n');
        let client_id = pick_client_id(std::env::var(WEB_CLIENT_ID_ENV).ok(), saved.map(|(key, _)| key));
        let client = OAuthClientBuilder::new(&client_id, WEB_REDIRECT_URI, WEB_SCOPES.to_vec())
            .open_in_browser()
            .build()
            .map_err(|e| format!("Web OAuth setup failed: {e}"))?;
        let grant_key = format!("{client_id} {}", WEB_SCOPES.join(","));
        let refreshed = match saved {
            Some((key, rt)) if key == grant_key => refresh(&client, rt.trim()).await.ok(),
            _ => None,
        };
        let token = match refreshed {
            Some(t) => t,
            None => {
                on_browser();
                client
                    .get_access_token_async()
                    .await
                    .map_err(|e| format!("Web login failed: {e}"))?
            }
        };
        let api = Self { client, token, token_path, grant_key };
        api.save()?;
        Ok(api)
    }

    fn save(&self) -> Result<(), String> {
        std::fs::write(&self.token_path, format!("{}\n{}", self.grant_key, self.token.refresh_token))
            .map_err(|e| format!("Saving web token failed: {e}"))
    }

    async fn access_token(&mut self) -> Result<String, String> {
        if Instant::now() + TOKEN_REFRESH_MARGIN >= self.token.expires_at {
            self.token = refresh(&self.client, &self.token.refresh_token)
                .await
                .map_err(|e| format!("Token refresh failed: {e}"))?;
            self.save()?;
        }
        Ok(self.token.access_token.clone())
    }

    /// Uses `request_fut`, not `request`: the latter turns every non-2xx into an opaque error
    /// and gives up on 429 when Retry-After exceeds librespot's 10 s cap (Spotify sends ~13 s).
    async fn get_json(&mut self, session: &Session, url: &str) -> Result<serde_json::Value, String> {
        self.send(session, http::Method::GET, url, None).await
    }

    /// Any Web API call. Write endpoints often answer 200/204 with an empty body: that is `Null`.
    async fn send(
        &mut self,
        session: &Session,
        method: http::Method,
        url: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let payload = bytes::Bytes::from(body.map(|b| b.to_string()).unwrap_or_default());
        for attempt in 0..=RATE_LIMIT_RETRIES {
            let token = self.access_token().await?;
            let req = http::Request::builder()
                .method(method.clone())
                .uri(url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                // Spotify's front end rejects body-less PUT/DELETE without it ("411 Length Required");
                // the HTTP client doesn't add it for an empty body.
                .header("Content-Length", payload.len())
                .body(payload.clone())
                .map_err(|e| e.to_string())?;
            let fut = session.http_client().request_fut(req).map_err(|e| format!("Request failed: {e}"))?;
            let resp = fut.await.map_err(|e| format!("Request failed: {e}"))?;
            let status = resp.status();
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok()?.parse::<u64>().ok())
                .map(Duration::from_secs);
            let body = http_body_util::BodyExt::collect(resp.into_body())
                .await
                .map_err(|e| format!("Response read failed: {e}"))?
                .to_bytes();
            match status.as_u16() {
                200..=299 if body.is_empty() => return Ok(serde_json::Value::Null),
                // Some writes answer 200 with a non-JSON body (POST /me/player/queue, verified 2026-10-03).
                200..=299 if method != http::Method::GET => return Ok(serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)),
                200..=299 => return serde_json::from_slice(&body).map_err(|e| format!("Response parse failed: {e}")),
                429 => match retry_after.filter(|d| *d <= MAX_RETRY_AFTER) {
                    Some(wait) if attempt < RATE_LIMIT_RETRIES => tokio::time::sleep(wait).await,
                    _ => return Err(format!("Spotify rate limit (retry after {retry_after:?}); the shared web app may be busy — set {WEB_CLIENT_ID_ENV}")),
                },
                // Spotify-owned/editorial playlists are often closed to third-party apps.
                403 | 404 => return Err(format!("Spotify doesn't expose this to third-party apps (HTTP {status})")),
                _ => return Err(format!("HTTP {status}: {}", String::from_utf8_lossy(&body))),
            }
        }
        unreachable!("the last attempt always returns")
    }

    /// Follows `next` links until the paging object ends, collecting `items`.
    async fn all_items(&mut self, session: &Session, first_url: String) -> Result<Vec<serde_json::Value>, String> {
        let mut items = Vec::new();
        let mut next = Some(first_url);
        while let Some(url) = next {
            let mut page = self.get_json(session, &url).await?;
            if let Some(arr) = page["items"].as_array_mut() {
                items.append(arr);
            }
            next = page["next"].as_str().map(String::from);
        }
        Ok(items)
    }

    /// Artists, then albums, then tracks, in one list; album/artist rows open their page.
    pub async fn search(&mut self, session: &Session, query: &str) -> Result<Vec<Item>, String> {
        let url = format!("{API}/search?type=artist,album,track&limit={SEARCH_LIMIT}&q={}", url_encode(query));
        let json = self.get_json(session, &url).await?;
        let items = |kind: &str| json[kind]["items"].as_array().cloned().unwrap_or_default();
        let artists = items("artists");
        let albums = items("albums");
        let tracks = items("tracks");
        Ok(artists.iter().filter_map(artist_row).take(SEARCH_ARTISTS)
            .chain(albums.iter().filter_map(album_row).take(SEARCH_ALBUMS))
            .chain(tracks.iter().filter_map(track_row))
            .collect())
    }

    /// The next page of track results for "Show more" (offset verified to work up to 990, 2026-10-03).
    pub async fn search_tracks(&mut self, session: &Session, query: &str, offset: u32) -> Result<Vec<Item>, String> {
        let url = format!("{API}/search?type=track&limit={SEARCH_LIMIT}&offset={offset}&q={}", url_encode(query));
        let json = self.get_json(session, &url).await?;
        Ok(json["tracks"]["items"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(track_row).collect())
    }

    /// Up next on the active device; `{"currently_playing":null,"queue":[]}` when nothing plays.
    pub async fn queue(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let json = self.get_json(session, &format!("{API}/me/player/queue")).await?;
        Ok(json["queue"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(track_row).collect())
    }

    /// Liked Songs membership for each URI, in order. `/me/library/contains` takes at most 40 URIs
    /// per request ("Too many uris requested" at 50, verified 2026-10-03).
    pub async fn contains(&mut self, session: &Session, uris: &[String]) -> Result<Vec<bool>, String> {
        let mut found = Vec::with_capacity(uris.len());
        for chunk in uris.chunks(CONTAINS_LIMIT) {
            let json = self.get_json(session, &format!("{API}/me/library/contains?uris={}", chunk.join(","))).await?;
            let flags = json.as_array().map(Vec::as_slice).unwrap_or_default();
            found.extend((0..chunk.len()).map(|i| flags.get(i).and_then(|f| f.as_bool()).unwrap_or(false)));
        }
        Ok(found)
    }

    /// Recently played tracks, newest first, each once.
    pub async fn recently_played(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let json = self.get_json(session, &format!("{API}/me/player/recently-played?limit={PAGE_LIMIT}")).await?;
        let mut seen = std::collections::HashSet::new();
        Ok(json["items"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|i| track_row(&i["track"]))
            .filter(|t| seen.insert(t.uri.clone()))
            .collect())
    }

    /// Plays `uri` after the current track on the active device.
    pub async fn add_to_queue(&mut self, session: &Session, uri: &str) -> Result<(), String> {
        self.send(session, http::Method::POST, &format!("{API}/me/player/queue?uri={uri}"), None).await.map(|_| ())
    }

    pub async fn add_to_playlist(&mut self, session: &Session, playlist: &str, uri: &str) -> Result<(), String> {
        let id = playlist.strip_prefix("spotify:playlist:").ok_or("Not a playlist")?;
        let body = serde_json::json!({ "uris": [uri] });
        self.send(session, http::Method::POST, &format!("{API}/playlists/{id}/items"), Some(body)).await.map(|_| ())
    }

    /// Removes every occurrence of `uri` from the playlist.
    pub async fn remove_from_playlist(&mut self, session: &Session, playlist: &str, uri: &str) -> Result<(), String> {
        let id = playlist.strip_prefix("spotify:playlist:").ok_or("Not a playlist")?;
        let body = serde_json::json!({ "items": [{ "uri": uri }] });
        self.send(session, http::Method::DELETE, &format!("{API}/playlists/{id}/items"), Some(body)).await.map(|_| ())
    }

    /// Albums saved to the library (`/me/albums` items wrap the album).
    pub async fn saved_albums(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let items = self.all_items(session, format!("{API}/me/albums?limit={PAGE_LIMIT}")).await?;
        Ok(items.iter().filter_map(|i| album_row(&i["album"])).collect())
    }

    /// Followed artists; this endpoint pages by cursor under `artists`, not by `items`/`next` at the top.
    pub async fn followed_artists(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let mut rows = Vec::new();
        let mut next = Some(format!("{API}/me/following?type=artist&limit={PAGE_LIMIT}"));
        while let Some(url) = next {
            let page = self.get_json(session, &url).await?;
            rows.extend(page["artists"]["items"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(artist_row));
            next = page["artists"]["next"].as_str().map(String::from);
        }
        Ok(rows)
    }

    /// Whether a track is in Liked Songs. `/me/tracks/contains` is 403 for this app; the unified
    /// `/me/library` endpoints take URIs and work (verified 2026-10-03).
    pub async fn is_saved(&mut self, session: &Session, uri: &str) -> Result<bool, String> {
        let json = self.get_json(session, &format!("{API}/me/library/contains?uris={uri}")).await?;
        Ok(json[0].as_bool().unwrap_or(false))
    }

    pub async fn set_saved(&mut self, session: &Session, uri: &str, saved: bool) -> Result<(), String> {
        let method = if saved { http::Method::PUT } else { http::Method::DELETE };
        self.send(session, method, &format!("{API}/me/library?uris={uri}"), None).await.map(|_| ())
    }

    /// Spotify Connect devices of this account, as rows: `uri` holds the device id.
    pub async fn devices(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let json = self.get_json(session, &format!("{API}/me/player/devices")).await?;
        Ok(json["devices"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(device_row).collect())
    }

    /// Moves playback to `device_id` and keeps it playing.
    pub async fn transfer(&mut self, session: &Session, device_id: &str) -> Result<(), String> {
        let body = serde_json::json!({ "device_ids": [device_id], "play": true });
        self.send(session, http::Method::PUT, &format!("{API}/me/player"), Some(body)).await.map(|_| ())
    }

    /// The album's name and tracks. Album tracks carry no images, so they all get the album's cover.
    pub async fn album_tracks(&mut self, session: &Session, album_uri: &str) -> Result<(String, Vec<Item>), String> {
        let id = album_uri.strip_prefix("spotify:album:").ok_or("Not an album URI")?;
        let album = self.get_json(session, &format!("{API}/albums/{id}")).await?;
        let cover = smallest_image(&album["images"]).to_string();
        let mut items = album["tracks"]["items"].as_array().cloned().unwrap_or_default();
        if let Some(next) = album["tracks"]["next"].as_str() {
            items.extend(self.all_items(session, next.to_string()).await?);
        }
        let rows = items
            .iter()
            .filter_map(track_row)
            .map(|t| Item { cover_url: cover.clone(), album_uri: album_uri.to_string(), ..t })
            .collect();
        Ok((album["name"].as_str().unwrap_or_default().to_string(), rows))
    }

    /// The artist's albums and singles, newest first as Spotify orders them, up to ARTIST_ALBUM_PAGES pages.
    pub async fn artist_albums(&mut self, session: &Session, artist_uri: &str) -> Result<Vec<Item>, String> {
        let id = artist_uri.strip_prefix("spotify:artist:").ok_or("Not an artist URI")?;
        let mut rows = Vec::new();
        let mut next = Some(format!("{API}/artists/{id}/albums?include_groups=album,single&limit={ARTIST_ALBUMS_LIMIT}"));
        for _ in 0..ARTIST_ALBUM_PAGES {
            let Some(url) = next.take() else { break };
            let page = self.get_json(session, &url).await?;
            rows.extend(page["items"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(album_row));
            next = page["next"].as_str().map(String::from);
        }
        Ok(rows)
    }

    pub async fn playlists(&mut self, session: &Session) -> Result<Vec<Item>, String> {
        let items = self.all_items(session, format!("{API}/me/playlists?limit={PAGE_LIMIT}")).await?;
        Ok(items.iter().filter_map(playlist_row).collect())
    }

    pub async fn list_tracks(&mut self, session: &Session, list: &str) -> Result<Vec<Item>, String> {
        let url = match list.strip_prefix("spotify:playlist:") {
            Some(id) => format!("{API}/playlists/{id}/items?limit={PAGE_LIMIT}&additional_types=track"),
            None => format!("{API}/me/tracks?limit={PAGE_LIMIT}"),
        };
        let items = self.all_items(session, url).await?;
        // Playlist items carry the track in `item` (older responses: `track`); saved tracks in `track`.
        Ok(items
            .iter()
            .filter_map(|i| track_row(if i["item"].is_object() { &i["item"] } else { &i["track"] }))
            .collect())
    }
}

/// Spotify doesn't always rotate the refresh token, and librespot-oauth then reports it as "".
/// Keep the old one instead, or the saved token becomes empty and every start needs the browser.
async fn refresh(client: &OAuthClient, refresh_token: &str) -> Result<OAuthToken, String> {
    if refresh_token.is_empty() {
        return Err("No refresh token saved".into());
    }
    let token = client.refresh_token_async(refresh_token).await.map_err(|e| e.to_string())?;
    Ok(keep_refresh_token(token, refresh_token))
}

fn keep_refresh_token(mut token: OAuthToken, previous: &str) -> OAuthToken {
    if token.refresh_token.is_empty() {
        token.refresh_token = previous.into();
    }
    token
}

/// Env var wins; otherwise stick with the app the saved token belongs to, so launching without
/// the env var (e.g. from a shortcut started before `setx`) doesn't force a new browser login.
fn pick_client_id(env: Option<String>, saved_key: Option<&str>) -> String {
    env.filter(|id| !id.trim().is_empty())
        .or_else(|| saved_key.and_then(|k| k.split(' ').next()).filter(|id| !id.is_empty()).map(String::from))
        .unwrap_or_else(|| DEFAULT_WEB_CLIENT_ID.into())
}

/// Web API pages may contain nulls, local files and episodes; only playable Spotify tracks pass.
fn track_row(t: &serde_json::Value) -> Option<Item> {
    let uri = t["uri"].as_str().filter(|u| u.starts_with("spotify:track:"))?;
    let artist = t["artists"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    Some(Item {
        title: t["name"].as_str().unwrap_or("?").into(),
        artist,
        uri: uri.into(),
        cover_url: smallest_image(&t["album"]["images"]).into(),
        artist_uri: t["artists"][0]["uri"].as_str().unwrap_or_default().into(),
        album_uri: t["album"]["uri"].as_str().unwrap_or_default().into(),
        duration_ms: t["duration_ms"].as_u64().unwrap_or(0) as u32,
        liked: false,
    })
}

fn artist_row(a: &serde_json::Value) -> Option<Item> {
    Some(Item {
        title: a["name"].as_str().unwrap_or("?").into(),
        artist: "Artist".into(),
        uri: a["uri"].as_str().filter(|u| u.starts_with("spotify:artist:"))?.into(),
        cover_url: smallest_image(&a["images"]).into(),
        ..Default::default()
    })
}

fn album_row(a: &serde_json::Value) -> Option<Item> {
    let artists = a["artists"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let kind = if a["album_type"].as_str() == Some("single") { "Single" } else { "Album" };
    Some(Item {
        title: a["name"].as_str().unwrap_or("?").into(),
        artist: format!("{kind} · {artists}"),
        uri: a["uri"].as_str().filter(|u| u.starts_with("spotify:album:"))?.into(),
        cover_url: smallest_image(&a["images"]).into(),
        ..Default::default()
    })
}

fn device_row(d: &serde_json::Value) -> Option<Item> {
    let active = if d["is_active"].as_bool() == Some(true) { " · playing here" } else { "" };
    Some(Item {
        title: d["name"].as_str().unwrap_or("?").into(),
        artist: format!("{}{active}", d["type"].as_str().unwrap_or("Device")),
        // Restricted devices can't be controlled through the Web API.
        uri: d["id"].as_str().filter(|_| d["is_restricted"].as_bool() != Some(true))?.into(),
        ..Default::default()
    })
}

fn playlist_row(p: &serde_json::Value) -> Option<Item> {
    let uri = p["uri"].as_str()?;
    let owner = p["owner"]["display_name"].as_str().unwrap_or_default();
    Some(Item {
        title: p["name"].as_str().unwrap_or("?").into(),
        artist: owner.into(),
        uri: uri.into(),
        cover_url: smallest_image(&p["images"]).into(),
        // A playlist has no artist; this holds the owner, so own (editable) playlists can be told apart.
        artist_uri: p["owner"]["uri"].as_str().unwrap_or_default().into(),
        ..Default::default()
    })
}

/// Spotify lists 640/300/64 px variants; the smallest is plenty for a 32 px thumbnail.
/// Missing widths (common on playlist mosaics) sort last.
fn smallest_image(images: &serde_json::Value) -> &str {
    images
        .as_array()
        .and_then(|imgs| imgs.iter().min_by_key(|i| i["width"].as_u64().unwrap_or(u64::MAX)))
        .and_then(|i| i["url"].as_str())
        .unwrap_or_default()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn track_row_filters_unplayable() {
        let t = json!({"name": "One More Time", "uri": "spotify:track:0DiWol3AO6WpXZgp0goxAV",
                       "artists": [{"name": "Daft Punk"}, {"name": "X"}]});
        assert_eq!(track_row(&t).unwrap().artist, "Daft Punk, X");
        assert!(track_row(&json!(null)).is_none());
        assert!(track_row(&json!({"name": "No uri"})).is_none());
        assert!(track_row(&json!({"uri": "spotify:local:a:b:c:1"})).is_none());
        assert!(track_row(&json!({"uri": "spotify:episode:0DiWol3AO6WpXZgp0goxAV"})).is_none());
    }

    #[test]
    fn album_and_artist_rows() {
        let album = json!({"name": "Discovery", "album_type": "album", "uri": "spotify:album:1",
                           "artists": [{"name": "Daft Punk"}], "images": [{"url": "s", "width": 64}]});
        let row = album_row(&album).unwrap();
        assert_eq!((row.artist.as_str(), row.cover_url.as_str()), ("Album · Daft Punk", "s"));
        assert_eq!(album_row(&json!({"album_type": "single", "uri": "spotify:album:2"})).unwrap().artist, "Single · ");
        assert!(album_row(&json!({"uri": "spotify:track:1"})).is_none());
        assert_eq!(artist_row(&json!({"name": "Daft Punk", "uri": "spotify:artist:1"})).unwrap().artist, "Artist");
        assert!(artist_row(&json!(null)).is_none());
    }

    #[test]
    fn device_rows() {
        let d = json!({"id": "abc", "name": "SlimSpot", "type": "Computer", "is_active": true, "is_restricted": false});
        let row = device_row(&d).unwrap();
        assert_eq!((row.uri.as_str(), row.artist.as_str()), ("abc", "Computer · playing here"));
        assert!(device_row(&json!({"id": "x", "is_restricted": true})).is_none());
        assert!(device_row(&json!({"name": "no id"})).is_none());
    }

    #[test]
    fn picks_smallest_cover() {
        let imgs = json!([{"url": "big", "width": 640}, {"url": "none", "width": null}, {"url": "small", "width": 64}]);
        assert_eq!(smallest_image(&imgs), "small");
        assert_eq!(smallest_image(&json!([{"url": "only", "width": null}])), "only");
        assert_eq!(smallest_image(&json!(null)), "");
    }

    #[test]
    fn url_encodes_query() {
        assert_eq!(url_encode("daft punk&ç"), "daft%20punk%26%C3%A7");
    }

    #[test]
    fn keeps_previous_refresh_token_when_not_rotated() {
        let token = |rt: &str| OAuthToken {
            access_token: "a".into(),
            refresh_token: rt.into(),
            expires_at: Instant::now(),
            token_type: "Bearer".into(),
            scopes: vec![],
        };
        assert_eq!(keep_refresh_token(token(""), "old").refresh_token, "old");
        assert_eq!(keep_refresh_token(token("new"), "old").refresh_token, "new");
    }

    #[test]
    fn client_id_prefers_env_then_saved_then_default() {
        let saved = Some("mine user-read-private,user-library-read");
        assert_eq!(pick_client_id(Some("env".into()), saved), "env");
        assert_eq!(pick_client_id(None, saved), "mine");
        assert_eq!(pick_client_id(Some("  ".into()), saved), "mine");
        assert_eq!(pick_client_id(None, None), DEFAULT_WEB_CLIENT_ID);
    }
}
