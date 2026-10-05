//! Backend thread: Spotify login, the librespot player behind a Spotify Connect device (Spirc),
//! and the command/event loop.
//!
//! Spirc owns the queue, shuffle, repeat and track advancing, so the phone and this window drive
//! the same state; local commands are translated into Spirc calls.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot::{
    connect::{ConnectConfig, LoadContextOptions, LoadRequest, LoadRequestOptions, Options, PlayingTrack, Spirc},
    core::{
        SpotifyUri, authentication::Credentials, cache::Cache, config::DeviceType, config::SessionConfig,
        session::Session,
    },
    metadata::{Artist, Metadata, Show, Track, audio::{AudioItem, UniqueFields}},
    playback::{
        audio_backend,
        config::{AudioFormat, PlayerConfig},
        mixer::{Mixer, MixerConfig, softmixer::SoftMixer},
        player::{Player, PlayerEvent},
    },
};
use librespot_oauth::OAuthClientBuilder;
use tokio::sync::mpsc;

use crate::covers::{self, CoverRequest};
use crate::nav::History;
use crate::settings::{self, LastSession, Quality, Repeat, Settings};
use crate::ui::{App, Row, clear_lyrics, set_liked_rows, set_current, set_lyrics, set_page_header, set_playing, set_position, set_rows, set_status};
use crate::web::{Item, LIKED_SONGS, PAGE_LIMIT, SEARCH_LIMIT, SEARCH_TYPES, WebApi, YOUR_EPISODES};

// Spotify desktop client id; same as librespot's internal KEYMASTER_CLIENT_ID.
const SPOTIFY_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const OAUTH_SCOPES: &[&str] = &["streaming"];
const DEVICE_NAME: &str = "SlimSpot";
// Last row of a full search page; clicking it appends the next page of tracks.
const MORE_URI: &str = "slimspot:more";
// Home tiles: Spotify shows 8 (two rows of four).
const QUICK_TILES: usize = 8;
// "New release": followed artists checked per run (one request each) and how recent counts.
const NEW_RELEASE_ARTISTS: usize = 10;
const NEW_RELEASE_DAYS: i64 = 60;
// The now-playing panel shows the start of the artist's biography.
const NOW_ABOUT_CHARS: usize = 400;
// Spotify took a few seconds to serve an uploaded cover (2026-10-05); poll for up to ~30 s.
const COVER_POLL_INTERVAL: Duration = Duration::from_secs(3);
const COVER_POLL_TRIES: u32 = 10;
// A track asked for this recently is retried after a reconnect: Spotify sometimes closes the
// session just as a track loads (audio key timeout, then "end of stream"; seen in the log 2026-10-04).
const RETRY_LOAD_WINDOW: Duration = Duration::from_secs(30);
// Pauses between reconnect attempts after the Connect device drops; the last one repeats.
const RECONNECT_BACKOFF: [Duration; 4] =
    [Duration::from_secs(2), Duration::from_secs(5), Duration::from_secs(15), Duration::from_secs(30)];

pub enum Command {
    Submit(String),
    OpenList(String),
    PlayUri(String),
    Toggle,
    Prev,
    Next,
    Seek(u32),
    /// Percent 0..=100; `save` is set when the slider is released.
    Volume { percent: f32, save: bool },
    ToggleShuffle,
    CycleRepeat,
    Quality(Quality),
    Normalize(bool),
    /// Settings → Theme: remember the palette.
    Theme(i32),
    /// Settings → Spotify app: the user's own Web API Client ID ("" = shared default).
    WebClientId(String),
    /// Settings → GPU renderer; applies on the next start.
    Gpu(bool),
    /// Winamp skin: the chosen .wsz path and whether the skin is showing (remembered for the next start).
    Skin { path: String, mode: bool },
    /// Winamp EQ window: the sink already uses it (`eq::set`); this remembers it.
    Eq(crate::eq::EqState),
    /// Save the session, disconnect the Connect device and end the UI event loop.
    Quit,
    ToggleLike,
    LoadDevices,
    /// Move playback to this Spotify Connect device id.
    Transfer(String),
    /// Start a radio from this track, or from the playing one.
    Radio(Option<String>),
    /// Row menu entry: radio/artist/album/like/open/copy on the row with this URI.
    RowAction { action: String, uri: String },
    Back,
    Forward,
    /// Show what the active device plays next.
    Queue,
    /// Open the page the playing track came from and scroll to it.
    GoToPlaying,
    /// Recently played tracks.
    Home,
    /// Play the current page from its first track (the header's play button).
    PlayPage,
    /// Sidebar filter: 0 all, 1 playlists, 2 albums, 3 artists; plus a name filter and A-Z order.
    /// sort: 0 = library order, 1 = recently opened, 2 = A-Z.
    FilterLibrary { kind: i32, text: String, sort: i32 },
    /// Find in page: rows of the current page matching `text`, in `sort` order (see `page-sort`).
    FilterPage { text: String, sort: i32 },
    /// Saves the open radio page as a new private playlist with its tracks.
    SaveRadio,
    /// "Add songs" popup on an own playlist: search tracks.
    AddSearch(String),
    /// Adds a track from the "Add songs" results to the open own playlist.
    AddToPage(String),
    /// Search page tab: 0 = All, then Songs/Albums/Artists/Playlists (SEARCH_TYPES).
    SearchTab(i32),
    /// Drag and drop in an own playlist: the row at `from` now goes to index `to`.
    MoveRow { from: usize, to: usize },
    /// Pause after this many minutes; -1 = at the end of the current track, 0 = off.
    SleepTimer(i32),
    /// From the create popup; an empty name falls back to Spotify's "My Playlist #N".
    CreatePlaylist { name: String, description: String },
    /// Rename the playlist on screen.
    /// Own playlist's new name and description (the header's edit popup).
    EditPlaylist { name: String, description: String },
    /// Own playlist's new cover, from an image file the user picked.
    PlaylistCover(PathBuf),
    /// Polls an uploaded cover until Spotify serves a new URL (`old`), `tries` more times.
    RefreshCover { list: String, old: String, tries: u32 },
    /// The track list scrolled close to the last row with a liked mark: check the next batch.
    CheckLikedMore,
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SlimSpot")
}

async fn browser_login(ui: &slint::Weak<App>) -> Result<Credentials, String> {
    set_status(ui, "Opening browser for Spotify login...");
    let client = OAuthClientBuilder::new(SPOTIFY_CLIENT_ID, REDIRECT_URI, OAUTH_SCOPES.to_vec())
        .open_in_browser()
        .build()
        .map_err(|e| format!("OAuth setup failed: {e}"))?;
    let token = client
        .get_access_token_async()
        .await
        .map_err(|e| format!("OAuth failed: {e}"))?;
    Ok(Credentials::with_access_token(token.access_token))
}

/// A connected session with its Connect device. Spirc must get a *fresh* session: it registers
/// its listeners first and then connects it itself (passing a connected one fails with
/// "Session is not connected"). Spirc also takes the player, so each attempt builds its own.
///
/// When the Spirc loop ends, `(generation, session_invalid)` is sent on `ended`, so the caller can
/// tell an unexpected stop of the current device from the shutdown of a replaced one.
async fn start_connect(
    cache: &Cache,
    credentials: Credentials,
    settings: &Settings,
    ended: &mpsc::UnboundedSender<(u64, bool)>,
    generation: u64,
) -> Result<Connect, String> {
    let session = Session::new(SessionConfig::default(), Some(cache.clone()));
    let backend = audio_backend::find(None).ok_or("No audio backend available")?;
    let mixer: Arc<dyn Mixer> = Arc::new(SoftMixer::open(MixerConfig::default()).map_err(|e| format!("Mixer error: {e}"))?);
    // Bitrate and normalisation are fixed per Player, which is why changing them restarts Connect.
    let player_config = PlayerConfig {
        bitrate: settings.quality.bitrate(),
        normalisation: settings.normalize,
        ..Default::default()
    };
    let player = Player::new(player_config, session.clone(), mixer.get_soft_volume(), move || {
        Box::new(crate::eq::EqSink::new(backend(None, AudioFormat::default()))) as Box<dyn audio_backend::Sink>
    });
    let events = player.get_player_event_channel();
    let config = ConnectConfig {
        name: DEVICE_NAME.into(),
        device_type: DeviceType::Computer,
        initial_volume: settings.mixer_volume(),
        ..Default::default()
    };
    let (spirc, task) = Spirc::new(config, session.clone(), credentials, player, mixer.clone())
        .await
        .map_err(|e| format!("Spotify Connect failed: {e}"))?;
    let local_mixer = mixer.clone();
    let (ended, watched) = (ended.clone(), session.clone());
    tokio::spawn(async move {
        task.await;
        let _ = ended.send((generation, watched.is_invalid()));
    });
    Ok((session, spirc, events, local_mixer))
}

/// The mixer is kept alongside Spirc so slider drags can change the volume locally: every
/// `Spirc::set_volume` also pushes the Connect state to Spotify, and doing that per drag step
/// got rate-limited (429), stalled the Spirc loop and made the slider stutter.
type Connect = (Session, Spirc, librespot::playback::player::PlayerEventChannel, Arc<dyn Mixer>);

/// Retries `start_connect` with growing pauses until it works; network outages are the usual
/// reason it fails, and there's nothing useful to do without a connection anyway.
async fn reconnect(
    ui: &slint::Weak<App>,
    cache: &Cache,
    settings: &Settings,
    ended: &mpsc::UnboundedSender<(u64, bool)>,
    generation: u64,
) -> Connect {
    let mut attempt = 0;
    loop {
        let result = match cache.credentials() {
            Some(creds) => start_connect(cache, creds, settings, ended, generation).await,
            None => Err("no cached credentials".into()),
        };
        match result {
            Ok(connect) => return connect,
            Err(e) => {
                let wait = RECONNECT_BACKOFF[attempt.min(RECONNECT_BACKOFF.len() - 1)];
                log::warn!("reconnect attempt {} failed: {e}", attempt + 1);
                set_status(ui, format!("Reconnect failed ({e}); retrying in {} s...", wait.as_secs()));
                tokio::time::sleep(wait).await;
                attempt += 1;
            }
        }
    }
}

/// Reloads what was playing on a freshly started device, at the same spot and play/pause state.
fn resume(
    ui: &slint::Weak<App>,
    spirc: &Spirc,
    shown: Option<&Shown>,
    settings: &Settings,
    now_uri: Option<&str>,
    position: (u32, Instant),
    was_playing: bool,
) {
    let Some(uri) = now_uri else { return };
    let elapsed = if was_playing { position.1.elapsed().as_millis() as u32 } else { 0 };
    let request = load_request(shown, uri, settings, position.0 + elapsed, was_playing);
    spirc_result(ui, spirc.activate().and_then(|()| spirc.load(request)));
}

/// The Web API's top-tracks endpoint returns 403 for Development Mode apps, so popular tracks
/// come from librespot's own metadata (the protocol the official client uses) instead.
/// The artist's name and popular tracks.
async fn artist_top_tracks(session: &Session, artist_uri: &str) -> Result<(String, Vec<Item>), String> {
    let id = SpotifyUri::from_uri(artist_uri).map_err(|e| e.to_string())?;
    let artist = Artist::get(session, &id).await.map_err(|e| format!("Artist lookup failed: {e}"))?;
    let rows = track_items(session, artist.top_tracks.for_country(&session.country()).to_vec()).await;
    Ok((artist.name, rows))
}

/// Titles, artists and covers for track URIs via librespot metadata, fetched concurrently and
/// returned in input order. Tracks that fail to load are left out rather than failing the list.
// With all 50 radio lookups in flight at once private RAM rose ~10 MB and stayed (15.9 -> 26 MB,
// 2026-10-04, a plain track change cost 1 MB), so lookups run a few at a time.
const TRACK_LOOKUPS_IN_FLIGHT: usize = 8;

async fn track_items(session: &Session, uris: Vec<SpotifyUri>) -> Vec<Item> {
    let limit = std::sync::Arc::new(tokio::sync::Semaphore::new(TRACK_LOOKUPS_IN_FLIGHT));
    let mut tasks = tokio::task::JoinSet::new();
    for (index, uri) in uris.into_iter().enumerate() {
        let (session, limit) = (session.clone(), limit.clone());
        tasks.spawn(async move {
            let _permit = limit.acquire().await;
            (index, Track::get(&session, &uri).await.ok().map(|track| track_item(&uri, track)))
        });
    }
    let mut found: Vec<(usize, Item)> = tasks.join_all().await.into_iter().filter_map(|(i, item)| Some((i, item?))).collect();
    found.sort_by_key(|(i, _)| *i);
    found.into_iter().map(|(_, item)| item).collect()
}

fn track_item(uri: &SpotifyUri, track: Track) -> Item {
    Item {
        title: track.name,
        artist: track.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
        uri: uri.to_uri().unwrap_or_default(),
        cover_url: track
            .album
            .covers
            .iter()
            .min_by_key(|c| c.width)
            .map(|c| format!("https://i.scdn.co/image/{}", c.id))
            .unwrap_or_default(),
        artist_uri: track.artists.first().and_then(|a| a.id.to_uri().ok()).unwrap_or_default(),
        album_uri: track.album.id.to_uri().unwrap_or_default(),
        duration_ms: track.duration.max(0) as u32,
        liked: false,
    }
}

/// A track's radio, resolved once: Spotify reshuffles a station every time it is resolved, so
/// these exact tracks are both shown and played (as a track list, not the station context).
/// Radio for a track, playlist, album or artist: Spotify's station context for it
/// (`spotify:station:<kind>:<id>`).
async fn radio_tracks(session: &Session, seed_uri: &str) -> Result<Vec<Item>, String> {
    let station = ["track:", "playlist:", "album:", "artist:"]
        .iter()
        .find_map(|kind| seed_uri.strip_prefix("spotify:").filter(|rest| rest.starts_with(kind)))
        .ok_or("Radio needs a track, playlist, album or artist")?;
    let rows = context_tracks(session, &format!("spotify:station:{station}"))
        .await
        .map_err(|e| format!("Radio unavailable: {e}"))?;
    if rows.is_empty() {
        return Err("Spotify has no radio for this".into());
    }
    Ok(rows)
}

/// A context's tracks through librespot (the official client's protocol). Spotify-made playlists
/// (Daily Mix, Discover Weekly, ...) answer 403/404 on the Web API but resolve here.
async fn context_tracks(session: &Session, context_uri: &str) -> Result<Vec<Item>, String> {
    let context = session.spclient().get_context(context_uri).await.map_err(|e| e.to_string())?;
    let mut seen = std::collections::HashSet::new();
    let uris: Vec<SpotifyUri> = context
        .pages
        .iter()
        .flat_map(|page| &page.tracks)
        .filter_map(|t| t.uri.as_deref().and_then(|u| SpotifyUri::from_uri(u).ok()))
        .filter(|u| matches!(u, SpotifyUri::Track { .. }) && seen.insert(u.to_uri().unwrap_or_default()))
        .collect();
    Ok(track_items(session, uris).await)
}

/// Header label of a page opened from `uri`.
fn kind_of(uri: &str) -> &'static str {
    if uri.starts_with("spotify:album:") {
        "Album"
    } else if uri.starts_with("spotify:artist:") {
        "Artist"
    } else if uri.starts_with("spotify:show:") {
        "Podcast"
    } else {
        "Playlist"
    }
}

/// Rows that open a page instead of playing.
fn is_page(uri: &str) -> bool {
    uri == LIKED_SONGS || uri == YOUR_EPISODES || ["spotify:playlist:", "spotify:album:", "spotify:artist:", "spotify:show:"].iter().any(|p| uri.starts_with(p))
}

/// A podcast's publisher from librespot metadata; "" if the lookup fails (the header just omits it).
async fn show_publisher(session: &Session, show_uri: &str) -> String {
    let Ok(id) = SpotifyUri::from_uri(show_uri) else { return String::new() };
    match Show::get(session, &id).await {
        Ok(show) => show.publisher,
        Err(e) => {
            log::warn!("show lookup for {show_uri} failed: {e}");
            String::new()
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Rows that play when clicked: tracks and podcast episodes.
fn is_playable(uri: &str) -> bool {
    uri.starts_with("spotify:track:") || uri.starts_with("spotify:episode:")
}

/// Accepts `spotify:track:ID` or `https://open.spotify.com/[intl-xx/]track/ID?si=...`.
pub fn parse_track(input: &str) -> Option<SpotifyUri> {
    let input = input.trim();
    let uri = match input.split_once("open.spotify.com/") {
        Some((_, rest)) => {
            let mut parts = rest.split(['/', '?']);
            parts.find(|p| *p == "track")?;
            format!("spotify:track:{}", parts.next()?)
        }
        None => input.to_string(),
    };
    match SpotifyUri::from_uri(&uri).ok()? {
        t @ SpotifyUri::Track { .. } => Some(t),
        _ => None,
    }
}

/// What the track list on screen came from; decides how a click is handed to Spirc.
#[derive(Clone)]
enum Shown {
    /// A playlist or Liked Songs: played as a real Spotify context, so the phone sees the playlist.
    Context(String),
    /// Search results: played as an ad-hoc track list.
    Tracks(Vec<String>),
}

fn load_request(shown: Option<&Shown>, track: &str, s: &Settings, seek_to: u32, start_playing: bool) -> LoadRequest {
    let options = LoadRequestOptions {
        start_playing,
        seek_to,
        context_options: Some(LoadContextOptions::Options(Options {
            shuffle: s.shuffle,
            repeat: s.repeat == Repeat::All,
            repeat_track: s.repeat == Repeat::One,
        })),
        playing_track: Some(PlayingTrack::Uri(track.into())),
    };
    match shown {
        Some(Shown::Context(uri)) => LoadRequest::from_context_uri(uri.clone(), options),
        Some(Shown::Tracks(uris)) if uris.iter().any(|u| u == track) => LoadRequest::from_tracks(uris.clone(), options),
        _ => LoadRequest::from_tracks(vec![track.into()], options),
    }
}

/// Now-playing data straight from librespot, so tracks started from the phone show up too.
fn now_playing(item: &AudioItem) -> Item {
    let artist_uri = match &item.unique_fields {
        UniqueFields::Track { artists, .. } => artists.first().and_then(|a| a.id.to_uri().ok()).unwrap_or_default(),
        _ => String::new(),
    };
    let artist = match &item.unique_fields {
        UniqueFields::Track { artists, .. } => artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
        UniqueFields::Local { artists, .. } => artists.clone().unwrap_or_default(),
        UniqueFields::Episode { show_name, .. } => show_name.clone(),
    };
    Item {
        title: item.name.clone(),
        artist,
        uri: item.uri.clone(),
        cover_url: item.covers.iter().min_by_key(|c| c.width).map(|c| c.url.clone()).unwrap_or_default(),
        artist_uri,
        // AudioItem names the album but carries no album id.
        album_uri: String::new(),
        duration_ms: item.duration_ms,
        liked: false,
    }
}

fn repeat_from(context: bool, track: bool) -> Repeat {
    match (context, track) {
        (_, true) => Repeat::One,
        (true, false) => Repeat::All,
        _ => Repeat::Off,
    }
}

/// One screen of the main panel, kept whole in the back/forward history.
struct Page {
    title: String,
    rows: Vec<Item>,
    shown: Shown,
    /// Sidebar entry to highlight (playlists and Liked Songs only), or "".
    list: String,
    /// Search query (or show URI), result type ("episode" for shows) and next offset while
    /// "Show more" is offered.
    more: Option<(String, &'static str, u32)>,
    /// Tabbed pages: the search query or artist URI and the chosen tab (search: 0 = All, then
    /// SEARCH_TYPES; artist: ARTIST_TABS).
    search: Option<(String, i32)>,
    /// Artist "About" tab: the biography, shown in place of the list.
    about: String,
    /// One of the user's own playlists: rows can be removed.
    editable: bool,
    /// Small label above the title ("Playlist", "Album", ...).
    kind: &'static str,
    /// Shown after the kind label: an album's "year · label", or "".
    info: String,
    /// Album saved / artist followed (header button); only meaningful on those pages.
    saved: bool,
    /// Header cover (playlists and albums), or "".
    cover_url: String,
    /// Rows already checked for liked marks (`mark_liked` continues from here).
    liked_upto: usize,
}

/// Keeps the header's Follow / Save button in step after a library change from anywhere.
fn set_page_saved(ui: &slint::Weak<App>, nav: &mut History<Page>, uri: &str, saved: bool) {
    let Some(page) = nav.current_mut() else { return };
    if matches!(&page.shown, Shown::Context(c) if c == uri) {
        page.saved = saved;
        let _ = ui.upgrade_in_event_loop(move |app| app.set_page_saved(saved));
    }
}

/// A search results page for one tab: "All" mixes artists, albums and tracks; the other tabs
/// list one type. Both page with "Show more".
async fn search_page(w: &mut WebApi, session: &Session, query: &str, tab: i32) -> Result<Page, String> {
    let kind = usize::try_from(tab - 1).ok().and_then(|i| SEARCH_TYPES.get(i)).copied();
    let (mut rows, has_next) = match kind {
        Some(kind) => w.search_type(session, query, kind, 0).await?,
        None => (w.search(session, query).await?, false),
    };
    // Only the track rows form the play queue; album/artist/playlist rows open pages.
    let tracks: Vec<String> = rows.iter().filter(|r| r.uri.starts_with("spotify:track:")).map(|r| r.uri.clone()).collect();
    // "All" pages its tracks, as before the tabs.
    let (paged, more_left) = match kind {
        Some(kind) => (kind, has_next),
        None => ("track", tracks.len() == SEARCH_LIMIT as usize),
    };
    let more = more_left.then(|| (query.to_string(), paged, SEARCH_LIMIT));
    if more.is_some() {
        rows.push(more_row());
    }
    Ok(Page {
        title: format!("Search: {query}"),
        rows,
        shown: Shown::Tracks(tracks),
        list: String::new(),
        more,
        search: Some((query.to_string(), tab)),
        about: String::new(),
        editable: false,
        kind: "Search",
        info: String::new(),
        saved: false,
        cover_url: String::new(),
        liked_upto: 0,
    })
}

/// Artist page tabs; 1..=4 are Web API album groups.
const ARTIST_TABS: [&str; 6] = ["Popular", "Albums", "Singles", "Compilations", "Appears On", "About"];
const ARTIST_GROUPS: [&str; 4] = ["album", "single", "compilation", "appears_on"];
// Development-mode limit for /artists/{id}/albums (see CLAUDE.md).
const ARTIST_GROUP_LIMIT: u32 = 10;

/// Another tab of the open artist page; keeps its title, follow state and context.
async fn artist_tab(w: &mut WebApi, session: &Session, page: &Page, tab: i32) -> Result<Page, String> {
    let Some((uri, _)) = page.search.clone() else { return Err("Not an artist page".into()) };
    let mut about = String::new();
    let mut more = None;
    let rows = match tab {
        0 => artist_top_tracks(session, &uri).await?.1,
        5 => {
            about = artist_bio(session, &uri).await;
            if about.is_empty() {
                about = "Spotify has no biography for this artist.".into();
            }
            Vec::new()
        }
        t => {
            let group = ARTIST_GROUPS[(t - 1).clamp(0, 3) as usize];
            let (mut rows, has_next) = w.artist_group(session, &uri, group, 0).await?;
            if has_next {
                more = Some((uri.clone(), group, ARTIST_GROUP_LIMIT));
                rows.push(more_row());
            }
            rows
        }
    };
    Ok(Page {
        title: page.title.clone(),
        rows,
        shown: page.shown.clone(),
        list: String::new(),
        more,
        search: Some((uri, tab)),
        about,
        editable: false,
        kind: "Artist",
        info: String::new(),
        saved: page.saved,
        cover_url: String::new(),
        liked_upto: 0,
    })
}

/// The artist's biography as plain text, "" if Spotify has none or the lookup fails.
async fn artist_bio(session: &Session, artist_uri: &str) -> String {
    let Ok(id) = SpotifyUri::from_uri(artist_uri) else { return String::new() };
    match Artist::get(session, &id).await {
        Ok(a) => a.biographies.first().map(|b| crate::web::plain_text(&b.text)).unwrap_or_default(),
        Err(e) => {
            log::warn!("artist lookup for {artist_uri} failed: {e}");
            String::new()
        }
    }
}

/// "Writers: A, B" lines from Spotify's track credits (spclient, not the Web API; verified 2026-10-05).
async fn track_credits(session: &Session, track_uri: &str) -> Result<String, String> {
    let id = track_uri.strip_prefix("spotify:track:").ok_or("Credits need a Spotify track")?;
    let body = session
        .spclient()
        .request_as_json(&http::Method::GET, &format!("/track-credits-view/v0/experimental/{id}/credits"), None, None)
        .await
        .map_err(|e| format!("Credits unavailable: {e}"))?;
    let json: serde_json::Value = serde_json::from_slice(&body).map_err(|e| format!("Credits unreadable: {e}"))?;
    Ok(credit_lines(&json))
}

fn credit_lines(json: &serde_json::Value) -> String {
    json["roleCredits"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|role| {
            let names: Vec<&str> = role["artists"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect();
            (!names.is_empty()).then(|| format!("{}: {}", role["roleTitle"].as_str().unwrap_or("?"), names.join(", ")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Days since 1970-01-01 for "YYYY-MM-DD" (civil-from-days inverse); partial dates count from
/// the start of their year/month.
fn days_from_date(date: &str) -> Option<i64> {
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let y = parts.next()??;
    let m = parts.next().flatten().unwrap_or(1);
    let d = parts.next().flatten().unwrap_or(1);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn more_row() -> Item {
    Item { title: "Show more".into(), uri: MORE_URI.into(), ..Default::default() }
}

/// Opens `page` as a new history entry and shows it.
fn open_page(ui: &slint::Weak<App>, nav: &mut History<Page>, shown: &mut Option<Shown>, page: Page) {
    nav.push(page);
    show_current(ui, nav, shown);
    scroll_to_top(ui);
}

/// A different page starts at its top; pages that grow or shrink in place keep their scroll.
fn scroll_to_top(ui: &slint::Weak<App>) {
    let _ = ui.upgrade_in_event_loop(|app| app.set_list_y(0.0));
}

/// Puts the history's current page on screen (after open, back or forward).
fn show_current(ui: &slint::Weak<App>, nav: &History<Page>, shown: &mut Option<Shown>) {
    let Some(page) = nav.current() else { return };
    *shown = Some(page.shown.clone());
    set_current(ui, page.list.clone(), App::set_current_list);
    set_rows(ui, page.rows.clone(), App::set_tracks);
    set_page_header(ui, page.kind, page.title.clone(), nav.can_back(), nav.can_forward());
    set_status(ui, page_summary(nav));
    let editable = page.editable;
    // Spotify numbers the rows of track lists, not of mixed pages like an artist's or search.
    let numbered = matches!(page.kind, "Playlist" | "Album" | "Radio" | "Up next" | "Podcast");
    let playable = page.rows.iter().any(|r| is_playable(&r.uri));
    let cover = page.cover_url.clone();
    let liked_upto = page.liked_upto as i32;
    let info = page.info.clone();
    let saved = page.saved;
    let search_tab = page.search.as_ref().map_or(-1, |s| s.1);
    let tabs: Vec<slint::SharedString> = match page.kind {
        "Artist" => ARTIST_TABS.iter().map(|t| (*t).into()).collect(),
        "Search" => ["All", "Songs", "Albums", "Artists", "Playlists"].iter().map(|t| (*t).into()).collect(),
        _ => Vec::new(),
    };
    let about = page.about.clone();
    let page_uri = match &page.shown {
        Shown::Context(uri) => uri.clone(),
        Shown::Tracks(_) => String::new(),
    };
    let _ = ui.upgrade_in_event_loop(move |app| {
        app.set_page_info(info.into());
        app.set_page_saved(saved);
        app.set_search_tab(search_tab);
        app.set_page_tabs(slint::ModelRc::new(slint::VecModel::from(tabs)));
        app.set_page_about(about.into());
        app.set_page_uri(page_uri.into());
        app.set_liked_checked(liked_upto);
        if app.get_page_cover_url() != cover.as_str() {
            app.set_page_cover(Default::default());
            app.set_page_cover_url(cover.into());
        }
        app.set_editable(editable);
        app.set_page_filter_text("".into());
        app.set_page_sort(0);
        app.set_numbered(numbered);
        app.set_page_playable(playable);
    });
}

/// Pushes the sidebar and the "Add to playlist" targets after the library changed.
fn refresh_library(ui: &slint::Weak<App>, library: &[Item], own_lists: &[Item]) {
    set_rows(ui, library.to_vec(), App::set_lists);
    set_rows(ui, own_lists.to_vec(), App::set_targets);
    // The full list goes back in library order, so the filter and sort chips reset too.
    let _ = ui.upgrade_in_event_loop(|app| {
        app.set_library_kind(0);
        app.set_library_sort(0);
    });
}

/// Scrolls the track list to `uri` if it is on the current page.
fn reveal_track(ui: &slint::Weak<App>, nav: &History<Page>, uri: &str) {
    let Some(index) = nav.current().and_then(|p| p.rows.iter().position(|r| r.uri == uri)) else { return };
    let _ = ui.upgrade_in_event_loop(move |app| app.invoke_reveal(index as i32));
}

/// The resting status line: the page's size. Transient messages ("Loading...") fall back to it.
fn page_summary(nav: &History<Page>) -> String {
    let Some(page) = nav.current() else { return String::new() };
    let rows = page.rows.iter().filter(|r| r.uri != MORE_URI);
    let tracks = rows.clone().filter(|r| r.uri.starts_with("spotify:track:")).count();
    let episodes = rows.clone().filter(|r| r.uri.starts_with("spotify:episode:")).count();
    let total_ms: u64 = rows.clone().map(|r| r.duration_ms as u64).sum();
    match (tracks, episodes, total_ms) {
        (_, _, 0) | (0, 0, _) => format!("{} items", rows.count()),
        (0, n, ms) => format!("{n} episodes, {}", about(ms)),
        (n, _, ms) => format!("{n} songs, {}", about(ms)),
    }
}

/// Spotify-style rounded length: "about 17 hr", "1 hr 5 min", "42 min 10 sec".
fn about(ms: u64) -> String {
    let (h, m, s) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60);
    match h {
        h if h >= 10 => format!("about {h} hr"),
        h if h > 0 => format!("{h} hr {m} min"),
        _ => format!("{m} min {s} sec"),
    }
}

/// Rows checked for liked marks per batch (5 requests of 40); the next batch is asked for by the
/// UI when the list scrolls near the end of the checked rows.
const LIKED_CHECK_BATCH: usize = 200;

/// Marks liked tracks in the current page's next batch of unchecked rows.
async fn mark_liked(ui: &slint::Weak<App>, web: &mut WebApi, session: &Session, nav: &mut History<Page>) {
    let Some(page) = nav.current_mut() else { return };
    let (from, to) = (page.liked_upto, (page.liked_upto + LIKED_CHECK_BATCH).min(page.rows.len()));
    if from >= to {
        return;
    }
    page.liked_upto = to;
    set_liked_checked(ui, to);
    let uris: Vec<String> =
        page.rows[from..to].iter().filter(|r| r.uri.starts_with("spotify:track:")).map(|r| r.uri.clone()).collect();
    let flags = if page.list == LIKED_SONGS {
        vec![true; uris.len()]
    } else {
        match web.contains(session, &uris).await {
            Ok(f) => f,
            Err(e) => return log::warn!("Liked check failed: {e}"),
        }
    };
    let liked: std::collections::HashSet<String> = uris.into_iter().zip(flags).filter(|(_, l)| *l).map(|(u, _)| u).collect();
    for row in &mut page.rows[from..to] {
        row.liked = liked.contains(&row.uri);
    }
    set_liked_rows(ui, liked.into_iter().collect(), true);
}

fn set_liked_checked(ui: &slint::Weak<App>, rows: usize) {
    let _ = ui.upgrade_in_event_loop(move |app| app.set_liked_checked(rows as i32));
}

/// The item behind a URI on screen: the current page, the sidebar, or what is playing.
fn find_item<'a>(nav: &'a History<Page>, library: &'a [Item], now: Option<&'a Item>, uri: &str) -> Option<&'a Item> {
    nav.current()
        .map(|p| p.rows.as_slice())
        .unwrap_or_default()
        .iter()
        .chain(library)
        .chain(now)
        .find(|i| i.uri == uri)
}

fn push_settings(ui: &slint::Weak<App>, s: &Settings) {
    let (volume, shuffle, repeat, quality, normalize) = (s.volume, s.shuffle, s.repeat.as_ui(), s.quality.as_ui(), s.normalize);
    let theme = s.theme;
    let _ = ui.upgrade_in_event_loop(move |app| {
        use slint::ComponentHandle;
        if app.global::<crate::ui::Theme>().get_palette() != theme {
            app.global::<crate::ui::Theme>().set_palette(theme);
            #[cfg(windows)]
            crate::instance::dark_title_bar(&app);
        }
        // Don't yank the slider while the user is dragging it.
        if (app.get_volume() - volume).abs() >= 1.0 {
            app.set_volume(volume);
        }
        app.set_shuffle(shuffle);
        app.set_repeat(repeat);
        app.set_quality(quality);
        app.set_normalize(normalize);
    });
}

fn save_settings(ui: &slint::Weak<App>, s: &Settings) {
    if let Err(e) = s.save() {
        set_status(ui, e);
    }
}

/// Remembers the current track and position. ponytail: saved on track change, pause and quit;
/// a crash mid-song resumes from the song's start, a periodic save would fix that.
fn save_last(
    ui: &slint::Weak<App>,
    settings: &mut Settings,
    item: Option<&Item>,
    position: (u32, Instant),
    playing: bool,
    duration_ms: u32,
    context: &Option<String>,
) {
    let Some(item) = item else { return };
    let elapsed = if playing { position.1.elapsed().as_millis() as u32 } else { 0 };
    settings.last = Some(LastSession {
        item: item.clone(),
        position_ms: (position.0 + elapsed).min(duration_ms),
        duration_ms,
        context: context.clone(),
    });
    save_settings(ui, settings);
}

fn spirc_result(ui: &slint::Weak<App>, r: Result<(), librespot::core::Error>) {
    if let Err(e) = r {
        set_status(ui, format!("Playback command failed: {e}"));
    }
}

pub async fn run(
    ui: slint::Weak<App>,
    // Our own queue: row menu actions re-enter it as the commands they stand for.
    tx: mpsc::UnboundedSender<Command>,
    mut rx: mpsc::UnboundedReceiver<Command>,
    cover_rx: mpsc::UnboundedReceiver<CoverRequest>,
) {
    let dir = cache_dir();
    let cache = match Cache::new(Some(dir.clone()), None, Some(dir.join("audio")), None) {
        Ok(c) => c,
        Err(e) => return set_status(&ui, format!("Cache error: {e}")),
    };
    let mut settings = Settings::load(&dir);
    crate::eq::set(settings.eq);
    push_settings(&ui, &settings);
    // Once only: later settings pushes (Spotify volume/shuffle events) mustn't move EQ sliders mid-drag.
    let (gpu, eq) = (settings.gpu, settings.eq);
    let _ = ui.upgrade_in_event_loop(move |app| {
        app.set_gpu(gpu);
        crate::ui::set_eq(&app, eq);
    });

    set_status(&ui, "Connecting...");
    let (ended_tx, mut ended_rx) = mpsc::unbounded_channel();
    // Bumped whenever the Connect device is replaced; ends of older generations are expected.
    let mut generation = 0u64;
    // Cached credentials first; if they're missing or rejected, one browser login and retry.
    let started = match cache.credentials() {
        Some(creds) => start_connect(&cache, creds, &settings, &ended_tx, generation).await,
        None => Err("No cached credentials".into()),
    };
    let started = match started {
        Ok(s) => Ok(s),
        Err(_) => match browser_login(&ui).await {
            Ok(creds) => start_connect(&cache, creds, &settings, &ended_tx, generation).await,
            Err(e) => Err(e),
        },
    };
    let (mut session, mut spirc, mut events, mut mixer) = match started {
        Ok(s) => s,
        Err(e) => return set_status(&ui, e),
    };
    // Web API is optional: playback by link still works if it fails.
    let browser_ui = ui.clone();
    let client_id_text = settings.web_client_id.clone();
    let _ = ui.upgrade_in_event_loop(move |app| app.set_web_client_id(client_id_text.into()));
    let mut web = match WebApi::login(&dir, &settings.web_client_id, move || set_status(&browser_ui, "Opening browser for Web API login...")).await {
        Ok(w) => Some(w),
        Err(e) => {
            set_status(&ui, format!("{e} — search/library disabled, links still work."));
            None
        }
    };
    tokio::spawn(covers::worker(session.clone(), ui.clone(), cover_rx));
    // Sidebar rows, kept to title the pages they open.
    let mut library: Vec<Item> = Vec::new();
    // The user's own playlists: targets of "Add to playlist".
    let mut own_lists: Vec<Item> = Vec::new();
    // Last "Add songs" results, so a click can add the full row.
    let mut add_results: Vec<Item> = Vec::new();
    // Artist page tab to switch to once it has opened ("More by" opens Albums).
    let mut open_tab: Option<i32> = None;
    // Home's "New release" tile, looked up once per run (None = not yet).
    let mut new_release: Option<Option<Item>> = None;
    // Artist whose biography the now-playing panel shows.
    let mut about_artist = String::new();
    // Set by GoToPlaying: scroll to this track once its page has loaded.
    let mut reveal: Option<String> = None;
    // Sleep timer: pause at this instant, or when the playing track ends.
    let mut sleep_at: Option<tokio::time::Instant> = None;
    let mut sleep_after_track = false;
    // What this window last asked Spirc to load (when, track, start position): a dropped session
    // can kill the load before TrackChanged, so a reconnect or Play retries this, not `now_uri`.
    let mut last_load: Option<(Instant, String, u32)> = None;
    // Paused by a Paused event (the device is active and `play` works). After a reconnect it isn't,
    // and Spirc ignores `play` while inactive, so Play loads the track again instead.
    let mut paused_here = false;
    if let Some(w) = web.as_mut() {
        set_status(&ui, "Loading playlists...");
        let mut lists = vec![
            Item { title: "Liked Songs".into(), uri: LIKED_SONGS.into(), ..Default::default() },
            Item { title: "Your Episodes".into(), artist: "Saved episodes".into(), uri: YOUR_EPISODES.into(), ..Default::default() },
        ];
        match w.playlists(&session).await {
            Ok(p) => lists.extend(p),
            Err(e) => set_status(&ui, format!("Playlists failed: {e}")),
        }
        // Saved albums and followed artists follow the playlists; their rows open album/artist pages.
        match w.saved_albums(&session).await {
            Ok(a) => lists.extend(a),
            Err(e) => set_status(&ui, format!("Saved albums failed: {e}")),
        }
        match w.followed_artists(&session).await {
            Ok(a) => lists.extend(a),
            Err(e) => set_status(&ui, format!("Followed artists failed: {e}")),
        }
        match w.saved_shows(&session).await {
            Ok(s) => lists.extend(s),
            Err(e) => set_status(&ui, format!("Saved podcasts failed: {e}")),
        }
        library = lists.clone();
        let me = format!("spotify:user:{}", session.username());
        own_lists = lists.iter().filter(|l| l.uri.starts_with("spotify:playlist:") && l.artist_uri == me).cloned().collect();
        set_rows(&ui, own_lists.clone(), App::set_targets);
        set_rows(&ui, lists, App::set_lists);
    }
    set_status(&ui, format!("Logged in as {}. Visible in Spotify Connect as \"{DEVICE_NAME}\".", session.username()));
    if web.is_some() {
        let _ = tx.send(Command::Home);
    }

    let mut playing = false;
    let mut shown: Option<Shown> = None;
    let mut nav: History<Page> = History::default();
    // Current track and last reported position, so a quality change can resume where it was.
    let mut now_uri: Option<String> = None;
    let mut now_item: Option<Item> = None;
    let mut duration_ms = 0u32;
    let mut position = (0u32, Instant::now());
    // Context of the last track started from this window, saved with the session.
    let mut playing_context: Option<String> = None;
    // Last session from the previous run: shown paused, loaded on the first Play. Loading it
    // eagerly would make this the active device and pause whatever the phone is playing.
    let mut pending_resume = settings.last.clone();
    // Liked Songs membership of the current track, refreshed on every track change.
    let mut liked = false;
    if let Some(last) = &pending_resume {
        let (item, at, total) = (last.item.clone(), last.position_ms as f32, last.duration_ms as f32);
        let _ = ui.upgrade_in_event_loop(move |app| {
            app.set_now(Row::from(item));
            app.set_duration(total);
            app.set_position(at);
        });
    }
    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => match cmd {
                // Album/artist rows in search results and on artist pages open their page.
                Command::PlayUri(uri) | Command::OpenList(uri) if is_page(&uri) => {
                    let Some(w) = web.as_mut() else {
                        set_status(&ui, "Library unavailable (web login failed)");
                        continue;
                    };
                    set_status(&ui, "Loading...");
                    if library.iter().any(|i| i.uri == uri) {
                        settings.opened.insert(uri.clone(), unix_now());
                        save_settings(&ui, &settings);
                    }
                    // Shows page their episodes with "Show more".
                    let mut more = None;
                    // Album and artist pages bring their own name; lists are named by the clicked row.
                    let result = if uri.starts_with("spotify:album:") {
                        w.album_tracks(&session, &uri).await
                    } else if uri == YOUR_EPISODES {
                        w.saved_episodes(&session).await.map(|rows| (String::new(), String::new(), rows))
                    } else if uri.starts_with("spotify:show:") {
                        match (w.show_name(&session, &uri).await, w.show_episodes(&session, &uri, 0).await) {
                            (Ok(name), Ok((mut rows, has_next))) => {
                                if has_next {
                                    more = Some((uri.clone(), "episode", PAGE_LIMIT));
                                    rows.push(more_row());
                                }
                                Ok((name, show_publisher(&session, &uri).await, rows))
                            }
                            (Err(e), _) | (_, Err(e)) => Err(e),
                        }
                    } else if uri.starts_with("spotify:artist:") {
                        artist_top_tracks(&session, &uri).await.map(|(name, top)| (name, String::new(), top))
                    } else {
                        match w.list_tracks(&session, &uri).await {
                            Ok(rows) => Ok((String::new(), String::new(), rows)),
                            Err(e) if uri.starts_with("spotify:playlist:") => {
                                log::warn!("Web API refused {uri} ({e}); reading it through librespot");
                                context_tracks(&session, &uri).await.map(|rows| (String::new(), String::new(), rows))
                            }
                            Err(e) => Err(e),
                        }
                    };
                    match result {
                        Ok((name, info, rows)) => {
                            // Otherwise the clicked row (sidebar or the page it was on) knows the name.
                            let title = if name.is_empty() {
                                find_item(&nav, &library, None, &uri).map(|i| i.title.clone()).unwrap_or_default()
                            } else {
                                name
                            };
                            let context = if uri == LIKED_SONGS { format!("spotify:user:{}:collection", session.username()) } else { uri.clone() };
                            let editable = own_lists.iter().any(|l| l.uri == uri);
                            // Albums show their own cover; playlists the sidebar row's.
                            let cover_url = match kind_of(&uri) {
                                "Album" | "Podcast" => rows.first().map(|r| r.cover_url.clone()).unwrap_or_default(),
                                "Playlist" => find_item(&nav, &library, None, &uri).map(|i| i.cover_url.clone()).unwrap_or_default(),
                                _ => String::new(),
                            };
                            let kind = kind_of(&uri);
                            let saved = matches!(kind, "Album" | "Artist" | "Podcast") && w.is_saved(&session, &uri).await.unwrap_or(false);
                            // Playlists show their description where albums show year and label.
                            let info = if uri.starts_with("spotify:playlist:") {
                                w.playlist_description(&session, &uri).await.unwrap_or_default()
                            } else {
                                info
                            };
                            // Only sidebar entries get highlighted.
                            let sidebar = uri == LIKED_SONGS || uri == YOUR_EPISODES || uri.starts_with("spotify:playlist:");
                            // Saved episodes have no Spotify context to load; they play as a list.
                            let page_shown = if uri == YOUR_EPISODES {
                                Shown::Tracks(rows.iter().map(|r| r.uri.clone()).collect())
                            } else {
                                Shown::Context(context.clone())
                            };
                            // Artist pages have tabs (Popular, Albums, ...); they open on Popular.
                            let search = (kind == "Artist").then(|| (uri.clone(), 0));
                            let list = if sidebar { uri } else { String::new() };
                            let reveal_row = reveal.take().filter(|_| playing_context.as_ref() == Some(&context));
                            open_page(&ui, &mut nav, &mut shown, Page { title, rows, shown: page_shown, list, more, search, about: String::new(), editable, kind, info, saved, cover_url, liked_upto: 0 });
                            if let Some(track) = reveal_row {
                                reveal_track(&ui, &nav, &track);
                            }
                            mark_liked(&ui, w, &session, &mut nav).await;
                            if let Some(tab) = open_tab.take() {
                                let _ = tx.send(Command::SearchTab(tab));
                            }
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::PlayUri(uri) if uri == MORE_URI => {
                    let (Some(w), Some(page)) = (web.as_mut(), nav.current_mut()) else { continue };
                    let Some((query, kind, offset)) = page.more.take() else { continue };
                    set_status(&ui, "Loading more...");
                    let artist_page = page.kind == "Artist";
                    let next_page = if artist_page {
                        w.artist_group(&session, &query, kind, offset).await
                    } else if kind == "episode" {
                        w.show_episodes(&session, &query, offset).await
                    } else {
                        w.search_type(&session, &query, kind, offset).await
                    };
                    let step = if kind == "episode" { PAGE_LIMIT } else if artist_page { ARTIST_GROUP_LIMIT } else { SEARCH_LIMIT };
                    match next_page {
                        Ok((found, has_next)) => {
                            page.rows.pop(); // the "Show more" row
                            if has_next {
                                page.more = Some((query, kind, offset + step));
                            }
                            // Spotify's later search pages repeat earlier hits (seen 2026-10-03).
                            let fresh: Vec<Item> = found.into_iter().filter(|f| !page.rows.iter().any(|r| r.uri == f.uri)).collect();
                            if let Shown::Tracks(uris) = &mut page.shown {
                                uris.extend(fresh.iter().filter(|r| r.uri.starts_with("spotify:track:")).map(|r| r.uri.clone()));
                            }
                            page.rows.extend(fresh);
                            if page.more.is_some() {
                                page.rows.push(more_row());
                            }
                        }
                        // Keep the row so the click can be retried.
                        Err(e) => {
                            page.more = Some((query, kind, offset));
                            set_status(&ui, e);
                            continue;
                        }
                    }
                    show_current(&ui, &nav, &mut shown);
                }
                Command::Queue => {
                    let Some(w) = web.as_mut() else { continue };
                    set_status(&ui, "Loading queue...");
                    match w.queue(&session).await {
                        Ok(rows) => {
                            let uris = rows.iter().map(|r| r.uri.clone()).collect();
                            open_page(&ui, &mut nav, &mut shown, Page { title: "Queue".into(), rows, shown: Shown::Tracks(uris), list: String::new(), more: None, search: None, about: String::new(), editable: false, kind: "Up next", info: String::new(), saved: false, cover_url: String::new(), liked_upto: 0 });
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                // A queue row: skip ahead to it, so the playing context carries on afterwards.
                Command::PlayUri(uri) if nav.current().is_some_and(|p| p.kind == "Up next") => {
                    let ahead = nav.current().and_then(|p| p.rows.iter().position(|r| r.uri == uri)).unwrap_or(0);
                    for _ in 0..=ahead {
                        spirc_result(&ui, spirc.next());
                    }
                    // Spotify's queue endpoint lags behind the skip; drop the passed rows locally instead.
                    if let Some(page) = nav.current_mut() {
                        page.rows.drain(..=ahead.min(page.rows.len().saturating_sub(1)));
                    }
                    show_current(&ui, &nav, &mut shown);
                }
                Command::PlayUri(uri) => {
                    set_status(&ui, "Loading...");
                    pending_resume = None;
                    playing_context = match &shown {
                        Some(Shown::Context(c)) => Some(c.clone()),
                        _ => None,
                    };
                    let request = load_request(shown.as_ref(), &uri, &settings, 0, true);
                    last_load = Some((Instant::now(), uri.clone(), 0));
                    spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                }
                Command::Submit(s) if parse_track(&s).is_some() => {
                    let uri = parse_track(&s).expect("checked above").to_uri().unwrap_or_default();
                    set_status(&ui, "Loading...");
                    pending_resume = None;
                    playing_context = None;
                    let request = load_request(None, &uri, &settings, 0, true);
                    last_load = Some((Instant::now(), uri.clone(), 0));
                    spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                }
                Command::Submit(q) if q.trim().is_empty() => {}
                Command::Submit(q) | Command::OpenList(q) => {
                    let Some(w) = web.as_mut() else {
                        set_status(&ui, "Search unavailable (web login failed)");
                        continue;
                    };
                    set_status(&ui, "Searching...");
                    match search_page(w, &session, q.trim(), 0).await {
                        Ok(page) => {
                            open_page(&ui, &mut nav, &mut shown, page);
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::SearchTab(tab) => {
                    let query = nav.current().and_then(|p| p.search.clone()).map(|(q, _)| q);
                    let (Some(w), Some(query)) = (web.as_mut(), query) else { continue };
                    set_status(&ui, "Loading...");
                    let result = match nav.current() {
                        Some(page) if page.kind == "Artist" => artist_tab(w, &session, page, tab).await,
                        _ => search_page(w, &session, &query, tab).await,
                    };
                    match result {
                        // A tab switch replaces the search page instead of adding history.
                        Ok(page) => {
                            if let Some(current) = nav.current_mut() {
                                *current = page;
                            }
                            show_current(&ui, &nav, &mut shown);
                            scroll_to_top(&ui);
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::Toggle => match pending_resume.take() {
                    Some(last) => {
                        playing_context = last.context.clone();
                        let shown = last.context.map(Shown::Context);
                        let request = load_request(shown.as_ref(), &last.item.uri, &settings, last.position_ms, true);
                        last_load = Some((Instant::now(), last.item.uri.clone(), last.position_ms));
                        spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                    }
                    None if playing || paused_here => spirc_result(&ui, if playing { spirc.pause() } else { spirc.play() }),
                    None => {
                        // Nothing started yet (e.g. the session dropped during the first load):
                        // retry what was last asked for.
                        let target = now_uri.clone().map(|u| (u, position.0)).or_else(|| last_load.as_ref().map(|l| (l.1.clone(), l.2)));
                        let Some((uri, at)) = target else {
                            set_status(&ui, "Nothing to play yet: pick a song");
                            continue;
                        };
                        let context = playing_context.clone().map(Shown::Context);
                        let request = load_request(context.as_ref(), &uri, &settings, at, true);
                        last_load = Some((Instant::now(), uri.clone(), position.0));
                        spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                    }
                },
                Command::ToggleLike => {
                    let (Some(w), Some(uri)) = (web.as_mut(), now_uri.clone()) else { continue };
                    match w.set_saved(&session, &uri, !liked).await {
                        Ok(()) => {
                            liked = !liked;
                            set_liked_rows(&ui, vec![uri.clone()], liked);
                            set_status(&ui, if liked { "Added to Liked Songs" } else { "Removed from Liked Songs" });
                        }
                        Err(e) => set_status(&ui, e),
                    }
                    let now_liked = liked;
                    let _ = ui.upgrade_in_event_loop(move |app| app.set_liked(now_liked));
                }
                Command::LoadDevices => {
                    let Some(w) = web.as_mut() else { continue };
                    set_rows(&ui, Vec::new(), App::set_devices);
                    match w.devices(&session).await {
                        Ok(devices) => set_rows(&ui, devices, App::set_devices),
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::Radio(seed) => {
                    let Some(seed) = seed.or_else(|| now_uri.clone()) else {
                        set_status(&ui, "Play a track first to start its radio");
                        continue;
                    };
                    set_status(&ui, "Building radio...");
                    match radio_tracks(&session, &seed).await {
                        Ok(rows) => {
                            let uris: Vec<String> = rows.iter().map(|r| r.uri.clone()).collect();
                            set_status(&ui, format!("Radio: {} tracks", rows.len()));
                            // A page's own radio (header button) isn't one of its rows: use the page title.
                            let page_title = nav.current().filter(|p| matches!(&p.shown, Shown::Context(c) if *c == seed)).map(|p| p.title.clone());
                            let seed_title = find_item(&nav, &library, now_item.as_ref(), &seed).map(|i| i.title.clone()).or(page_title).unwrap_or_default();
                            open_page(&ui, &mut nav, &mut shown, Page {
                                title: format!("{seed_title} Radio"),
                                rows,
                                shown: Shown::Tracks(uris.clone()),
                                list: String::new(),
                                more: None, search: None, about: String::new(),
                                editable: false,
                                kind: "Radio",
                                info: String::new(),
                                saved: false,
                                cover_url: String::new(),
                                liked_upto: 0,
                            });
                            if let Some(w) = web.as_mut() {
                                mark_liked(&ui, w, &session, &mut nav).await;
                            }
                            playing_context = None;
                            pending_resume = None;
                            let first = uris[0].clone();
                            let request = load_request(shown.as_ref(), &first, &settings, 0, true);
                            last_load = Some((Instant::now(), first.clone(), 0));
                            spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::RowAction { action, uri } => {
                    // The header's Follow / Save button acts on the page itself, which isn't one of its rows.
                    let page_item = nav.current().filter(|p| matches!(&p.shown, Shown::Context(c) if *c == uri)).map(|p| Item {
                        title: p.title.clone(),
                        artist: p.kind.to_string(),
                        uri: uri.clone(),
                        cover_url: p.cover_url.clone(),
                        ..Default::default()
                    });
                    let item = find_item(&nav, &library, now_item.as_ref(), &uri).cloned().or(page_item).unwrap_or_default();
                    let follow_up = match action.as_str() {
                        "radio" => Some(Command::Radio(Some(uri.clone()))),
                        "open" => Some(Command::OpenList(uri.clone())),
                        "artist" if !item.artist_uri.is_empty() => Some(Command::OpenList(item.artist_uri)),
                        "album" if !item.album_uri.is_empty() => Some(Command::OpenList(item.album_uri)),
                        "artist" | "album" => {
                            set_status(&ui, format!("No {action} known for this track"));
                            None
                        }
                        "like" => {
                            if let Some(w) = web.as_mut() {
                                match w.set_saved(&session, &uri, true).await {
                                    Ok(()) => {
                                        set_liked_rows(&ui, vec![uri.clone()], true);
                                        set_status(&ui, format!("Added \"{}\" to Liked Songs", item.title));
                                        if now_uri.as_deref() == Some(uri.as_str()) {
                                            liked = true;
                                            let _ = ui.upgrade_in_event_loop(|app| app.set_liked(true));
                                        }
                                    }
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        "queue" => {
                            if let Some(w) = web.as_mut() {
                                match w.add_to_queue(&session, &uri).await {
                                    Ok(()) => set_status(&ui, format!("Queued \"{}\"", item.title)),
                                    // Idle answers 404 (verified 2026-10-03): there is no active device.
                                    Err(_) if !playing => set_status(&ui, "Start playback first, then add to queue"),
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        "remove" => {
                            let list = nav.current().filter(|p| p.editable).map(|p| p.list.clone());
                            if let (Some(w), Some(list)) = (web.as_mut(), list) {
                                match w.remove_from_playlist(&session, &list, &uri).await {
                                    Ok(()) => {
                                        if let Some(page) = nav.current_mut() {
                                            page.rows.retain(|r| r.uri != uri);
                                        }
                                        show_current(&ui, &nav, &mut shown);
                                        set_status(&ui, format!("Removed \"{}\"", item.title));
                                    }
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        add if add.starts_with("add:") => {
                            let list = &add["add:".len()..];
                            if let Some(w) = web.as_mut() {
                                let name = own_lists.iter().find(|l| l.uri == list).map(|l| l.title.clone()).unwrap_or_default();
                                match w.add_to_playlist(&session, list, &uri).await {
                                    Ok(()) => set_status(&ui, format!("Added \"{}\" to {name}", item.title)),
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        "save" => {
                            if let Some(w) = web.as_mut() {
                                match w.set_saved(&session, &uri, true).await {
                                    Ok(()) => {
                                        if !library.iter().any(|i| i.uri == uri) {
                                            library.push(item.clone());
                                            refresh_library(&ui, &library, &own_lists);
                                        }
                                        set_page_saved(&ui, &mut nav, &uri, true);
                                        set_status(&ui, format!("Added \"{}\" to Your Library", item.title));
                                    }
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        "unfollow" => {
                            if let Some(w) = web.as_mut() {
                                // Albums and artists leave through the unified library endpoint; a playlist
                                // is unfollowed (Spotify's delete for own playlists).
                                let result = if uri.starts_with("spotify:playlist:") {
                                    w.unfollow_playlist(&session, &uri).await
                                } else {
                                    w.set_saved(&session, &uri, false).await
                                };
                                match result {
                                    Ok(()) => {
                                        library.retain(|i| i.uri != uri);
                                        own_lists.retain(|i| i.uri != uri);
                                        set_page_saved(&ui, &mut nav, &uri, false);
                                        refresh_library(&ui, &library, &own_lists);
                                        // Its page would show a playlist that no longer exists.
                                        if nav.current().is_some_and(|p| p.list == uri) {
                                            let _ = tx.send(Command::Home);
                                        }
                                        set_status(&ui, format!("Removed \"{}\" from Your Library", item.title));
                                    }
                                    Err(e) => set_status(&ui, e),
                                }
                            }
                            None
                        }
                        "credits" => {
                            set_status(&ui, "Loading credits...");
                            match track_credits(&session, &uri).await {
                                Ok(lines) => {
                                    let (title, lines) = (item.title.clone(), if lines.is_empty() { "Spotify lists no credits for this song.".to_string() } else { lines });
                                    set_status(&ui, page_summary(&nav));
                                    let _ = ui.upgrade_in_event_loop(move |app| {
                                        app.set_credits_title(title.into());
                                        app.set_credits_text(lines.into());
                                        app.invoke_show_credits();
                                    });
                                }
                                Err(e) => set_status(&ui, e),
                            }
                            None
                        }
                        // Album page "More by": the album's (first) artist, on the Albums tab.
                        "more-by" => {
                            let artist = nav.current().and_then(|p| p.rows.first()).map(|r| r.artist_uri.clone()).filter(|a| !a.is_empty());
                            match artist {
                                Some(artist) => {
                                    open_tab = Some(1);
                                    Some(Command::OpenList(artist))
                                }
                                None => {
                                    set_status(&ui, "No artist known for this album");
                                    None
                                }
                            }
                        }
                        "copy" => {
                            #[cfg(windows)]
                            match crate::clipboard::web_link(&uri) {
                                Some(link) => match crate::clipboard::set_text(&link) {
                                    Ok(()) => set_status(&ui, format!("Copied {link}")),
                                    Err(e) => set_status(&ui, e),
                                },
                                None => set_status(&ui, "This has no Spotify link"),
                            }
                            None
                        }
                        other => {
                            log::warn!("unknown row action {other}");
                            None
                        }
                    };
                    if let Some(cmd) = follow_up {
                        let _ = tx.send(cmd);
                    }
                }
                Command::Home => {
                    let Some(w) = web.as_mut() else { continue };
                    match w.recently_played(&session).await {
                        Ok(rows) => {
                            let uris = rows.iter().map(|r| r.uri.clone()).collect();
                            open_page(&ui, &mut nav, &mut shown, Page {
                                title: "Recently played".into(),
                                rows,
                                shown: Shown::Tracks(uris),
                                list: String::new(),
                                more: None, search: None, about: String::new(),
                                editable: false,
                                kind: "Home",
                                info: String::new(),
                                saved: false,
                                cover_url: String::new(),
                                liked_upto: 0,
                            });
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                    // Spotify's "New release from <artist>": once per run, newest release of the
                    // most recently opened followed artists, if it is recent.
                    if new_release.is_none() {
                        let mut artists: Vec<&Item> = library.iter().filter(|i| i.uri.starts_with("spotify:artist:")).collect();
                        artists.sort_by_key(|i| std::cmp::Reverse(settings.opened.get(&i.uri).copied().unwrap_or(0)));
                        let artists: Vec<Item> = artists.into_iter().take(NEW_RELEASE_ARTISTS).cloned().collect();
                        let today = (unix_now() / 86_400) as i64;
                        let mut newest: Option<(Item, String)> = None;
                        for artist in &artists {
                            if let Ok(Some((album, date))) = w.newest_release(&session, &artist.uri).await {
                                let recent = days_from_date(&date).is_some_and(|d| today - d <= NEW_RELEASE_DAYS);
                                if recent && newest.as_ref().is_none_or(|n| date > n.1) {
                                    newest = Some((Item { artist: format!("New release · {}", artist.title), ..album }, date));
                                }
                            }
                        }
                        new_release = Some(newest.map(|n| n.0));
                    }
                    let mut tiles: Vec<Item> = new_release.clone().flatten().into_iter().collect();
                    let mut recent: Vec<&Item> = library.iter().filter(|i| settings.opened.contains_key(&i.uri)).collect();
                    recent.sort_by_key(|i| std::cmp::Reverse(settings.opened[&i.uri]));
                    tiles.extend(recent.into_iter().chain(library.iter()).filter(|i| i.uri != YOUR_EPISODES).cloned());
                    let mut seen = std::collections::HashSet::new();
                    tiles.retain(|i| seen.insert(i.uri.clone()));
                    tiles.truncate(QUICK_TILES);
                    let bottom = tiles.split_off(tiles.len().min(QUICK_TILES / 2));
                    set_rows(&ui, tiles, App::set_quick_top);
                    set_rows(&ui, bottom, App::set_quick_bottom);
                }
                Command::PlayPage => {
                    let first = nav.current().and_then(|p| p.rows.iter().find(|r| is_playable(&r.uri)));
                    if let Some(row) = first {
                        let _ = tx.send(Command::PlayUri(row.uri.clone()));
                    }
                }
                Command::FilterLibrary { kind, text, sort } => {
                    let text = text.trim().to_lowercase();
                    let mut rows: Vec<Item> = library
                        .iter()
                        .filter(|i| match kind {
                            1 => i.uri == LIKED_SONGS || i.uri.starts_with("spotify:playlist:"),
                            2 => i.uri.starts_with("spotify:album:"),
                            3 => i.uri.starts_with("spotify:artist:"),
                            4 => i.uri == YOUR_EPISODES || i.uri.starts_with("spotify:show:"),
                            _ => true,
                        })
                        .filter(|i| text.is_empty() || i.title.to_lowercase().contains(&text) || i.artist.to_lowercase().contains(&text))
                        .cloned()
                        .collect();
                    match sort {
                        // Most recent first; never-opened entries keep library order after them.
                        1 => rows.sort_by_key(|i| std::cmp::Reverse(settings.opened.get(&i.uri).copied().unwrap_or(0))),
                        2 => rows.sort_by_key(|i| i.title.to_lowercase()),
                        _ => {}
                    }
                    set_rows(&ui, rows, App::set_lists);
                }
                Command::FilterPage { text, sort } => {
                    let Some(page) = nav.current() else { continue };
                    let text = text.trim().to_lowercase();
                    let mut rows: Vec<Item> = page
                        .rows
                        .iter()
                        .filter(|i| text.is_empty() || i.title.to_lowercase().contains(&text) || i.artist.to_lowercase().contains(&text))
                        // "Show more" only makes sense at the end of the unfiltered list.
                        .filter(|i| i.uri != MORE_URI || (text.is_empty() && sort == 0))
                        .cloned()
                        .collect();
                    match sort {
                        1 => rows.sort_by_key(|i| i.title.to_lowercase()),
                        2 => rows.sort_by_key(|i| i.artist.to_lowercase()),
                        3 => rows.sort_by_key(|i| i.duration_ms),
                        _ => {}
                    }
                    set_rows(&ui, rows, App::set_tracks);
                }
                Command::MoveRow { from, to } => {
                    let list = nav.current().filter(|p| p.editable && from < p.rows.len() && to < p.rows.len()).map(|p| p.list.clone());
                    let (Some(w), Some(list)) = (web.as_mut(), list) else { continue };
                    if from == to {
                        continue;
                    }
                    // Spotify counts insert_before in the list before the move.
                    let insert_before = if to > from { to + 1 } else { to };
                    match w.move_in_playlist(&session, &list, from, insert_before).await {
                        Ok(()) => {
                            if let Some(page) = nav.current_mut() {
                                let row = page.rows.remove(from);
                                page.rows.insert(to, row);
                            }
                            show_current(&ui, &nav, &mut shown);
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::SleepTimer(minutes) => {
                    sleep_at = (minutes > 0).then(|| tokio::time::Instant::now() + Duration::from_secs(minutes as u64 * 60));
                    sleep_after_track = minutes < 0;
                    set_status(&ui, match minutes {
                        0 => "Sleep timer off".to_string(),
                        m if m < 0 => "Pausing after this track".to_string(),
                        m => format!("Pausing in {m} min"),
                    });
                }
                Command::CreatePlaylist { name, description } => {
                    let Some(w) = web.as_mut() else { continue };
                    let name = match name.trim() {
                        "" => format!("My Playlist #{}", own_lists.len() + 1),
                        name => name.to_string(),
                    };
                    match w.create_playlist(&session, &name, description.trim()).await {
                        Ok(item) => {
                            // New playlists come first, below Liked Songs and Your Episodes, as in Spotify.
                            library.insert(2.min(library.len()), item.clone());
                            own_lists.insert(0, item.clone());
                            refresh_library(&ui, &library, &own_lists);
                            let _ = tx.send(Command::OpenList(item.uri));
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::EditPlaylist { name, description } => {
                    let (name, description) = (name.trim().to_string(), description.trim().to_string());
                    let list = nav.current().filter(|p| p.editable).map(|p| p.list.clone());
                    let (Some(w), Some(list)) = (web.as_mut(), list) else { continue };
                    if name.is_empty() {
                        continue;
                    }
                    match w.edit_playlist(&session, &list, &name, &description).await {
                        Ok(()) => {
                            for item in library.iter_mut().chain(own_lists.iter_mut()).filter(|i| i.uri == list) {
                                item.title = name.clone();
                            }
                            if let Some(page) = nav.current_mut() {
                                page.title = name.clone();
                                page.info = description.clone();
                            }
                            let _ = ui.upgrade_in_event_loop(move |app| app.set_page_info(description.into()));
                            refresh_library(&ui, &library, &own_lists);
                            set_page_header(&ui, nav.current().map(|p| p.kind).unwrap_or(""), name, nav.can_back(), nav.can_forward());
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::PlaylistCover(path) => {
                    let list = nav.current().filter(|p| p.editable).map(|p| p.list.clone());
                    let (Some(w), Some(list)) = (web.as_mut(), list) else { continue };
                    set_status(&ui, "Uploading cover...");
                    let jpeg = std::fs::read(&path).map_err(|e| format!("Reading {} failed: {e}", path.display())).and_then(|f| crate::covers::upload_jpeg(&f));
                    match jpeg {
                        Ok(jpeg) => match w.upload_cover(&session, &list, &jpeg).await {
                            // Spotify processes the image asynchronously; the new URL shows up a bit later.
                            Ok(()) => {
                                set_status(&ui, "Cover uploaded; waiting for Spotify to process it...");
                                let old = library.iter().find(|i| i.uri == list).map(|i| i.cover_url.clone()).unwrap_or_default();
                                let _ = tx.send(Command::RefreshCover { list, old, tries: COVER_POLL_TRIES });
                            }
                            Err(e) => set_status(&ui, e),
                        },
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::RefreshCover { list, old, tries } => {
                    let Some(w) = web.as_mut() else { continue };
                    let url = w.playlist_cover(&session, &list).await.unwrap_or_default();
                    if url.is_empty() || url == old {
                        if tries > 0 {
                            // A timer task, so the command loop keeps running while Spotify works.
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(COVER_POLL_INTERVAL).await;
                                let _ = tx.send(Command::RefreshCover { list, old, tries: tries - 1 });
                            });
                        }
                        continue;
                    }
                    for item in library.iter_mut().chain(own_lists.iter_mut()).filter(|i| i.uri == list) {
                        item.cover_url = url.clone();
                    }
                    refresh_library(&ui, &library, &own_lists);
                    if let Some(page) = nav.current_mut().filter(|p| p.list == list) {
                        page.cover_url = url.clone();
                        let _ = ui.upgrade_in_event_loop(move |app| {
                            app.set_page_cover(Default::default());
                            app.set_page_cover_url(url.into());
                        });
                    }
                    set_status(&ui, "Cover updated");
                }
                Command::SaveRadio => {
                    let page = nav.current().filter(|p| p.kind == "Radio").map(|p| (p.title.clone(), p.rows.iter().map(|r| r.uri.clone()).collect::<Vec<_>>()));
                    let (Some(w), Some((title, uris))) = (web.as_mut(), page) else { continue };
                    set_status(&ui, "Saving radio...");
                    let saved = match w.create_playlist(&session, &title, "").await {
                        Ok(item) => w.add_tracks(&session, &item.uri, &uris).await.map(|()| item),
                        Err(e) => Err(e),
                    };
                    match saved {
                        Ok(item) => {
                            set_status(&ui, format!("Saved \"{}\" ({} songs)", item.title, uris.len()));
                            library.insert(2.min(library.len()), item.clone());
                            own_lists.insert(0, item);
                            refresh_library(&ui, &library, &own_lists);
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::AddSearch(query) => {
                    let Some(w) = web.as_mut() else { continue };
                    if query.trim().is_empty() {
                        continue;
                    }
                    match w.search_type(&session, query.trim(), "track", 0).await {
                        Ok((rows, _)) => {
                            add_results = rows.clone();
                            set_rows(&ui, rows, App::set_add_results);
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::AddToPage(uri) => {
                    let list = nav.current().filter(|p| p.editable).map(|p| p.list.clone());
                    let (Some(w), Some(list)) = (web.as_mut(), list) else { continue };
                    let Some(item) = add_results.iter().find(|i| i.uri == uri).cloned() else { continue };
                    match w.add_to_playlist(&session, &list, &uri).await {
                        Ok(()) => {
                            set_status(&ui, format!("Added \"{}\"", item.title));
                            if let Some(page) = nav.current_mut() {
                                page.rows.push(item);
                            }
                            show_current(&ui, &nav, &mut shown);
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::CheckLikedMore => {
                    if let Some(w) = web.as_mut() {
                        mark_liked(&ui, w, &session, &mut nav).await;
                    }
                }
                Command::GoToPlaying => {
                    let Some(track) = now_uri.clone() else { continue };
                    let Some(context) = playing_context.clone() else {
                        set_status(&ui, "Started elsewhere; its playlist isn't known here");
                        continue;
                    };
                    let on_screen = matches!(&shown, Some(Shown::Context(c)) if *c == context);
                    if on_screen {
                        reveal_track(&ui, &nav, &track);
                    } else {
                        reveal = Some(track);
                        // Liked Songs plays as the user's collection context but opens as the "liked" row.
                        let page = if context.ends_with(":collection") { LIKED_SONGS.to_string() } else { context };
                        let _ = tx.send(Command::OpenList(page));
                    }
                }
                Command::Back => {
                    if nav.back().is_some() {
                        show_current(&ui, &nav, &mut shown);
                        scroll_to_top(&ui);
                    }
                }
                Command::Forward => {
                    if nav.forward().is_some() {
                        show_current(&ui, &nav, &mut shown);
                        scroll_to_top(&ui);
                    }
                }
                Command::Transfer(device_id) => {
                    let Some(w) = web.as_mut() else { continue };
                    pending_resume = None;
                    match w.transfer(&session, &device_id).await {
                        Ok(()) => set_status(&ui, "Playback moved"),
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::Quit => {
                    save_last(&ui, &mut settings, now_item.as_ref(), position, playing, duration_ms, &playing_context);
                    let _ = spirc.shutdown();
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                    break;
                }
                Command::Next => spirc_result(&ui, spirc.next()),
                Command::Prev => spirc_result(&ui, spirc.prev()),
                Command::Seek(ms) => spirc_result(&ui, spirc.set_position_ms(ms)),
                Command::Volume { percent, save } => {
                    let volume = settings::volume_to_mixer(percent);
                    settings.volume = percent;
                    mixer.set_volume(volume);
                    if save {
                        // Released: one Connect update so the phone shows the new volume
                        // (Spirc ignores it while this isn't the active device; the mixer is set above).
                        spirc_result(&ui, spirc.set_volume(volume));
                        save_settings(&ui, &settings);
                    }
                }
                Command::ToggleShuffle => spirc_result(&ui, spirc.shuffle(!settings.shuffle)),
                Command::CycleRepeat => {
                    let next = settings.repeat.next();
                    spirc_result(&ui, spirc.repeat(next == Repeat::All).and_then(|()| spirc.repeat_track(next == Repeat::One)));
                }
                Command::WebClientId(id) => {
                    let id = id.trim().to_string();
                    if !id.is_empty() && !crate::web::is_client_id(&id) {
                        set_status(&ui, "That isn't a Client ID (32 letters and digits from developer.spotify.com)");
                        continue;
                    }
                    if id == settings.web_client_id {
                        continue;
                    }
                    settings.web_client_id = id.clone();
                    save_settings(&ui, &settings);
                    // Like Spotifast: removing the personal id also drops that app's grant, so the next
                    // start doesn't fall back to it through the saved token.
                    if id.is_empty() {
                        if let Err(e) = std::fs::remove_file(dir.join(crate::web::WEB_TOKEN_FILE)) {
                            if e.kind() != std::io::ErrorKind::NotFound {
                                log::warn!("removing the web token failed: {e}");
                            }
                        }
                    }
                    set_status(&ui, "Client ID saved. Restart SlimSpot to sign in with it (the browser asks once).");
                }
                Command::Theme(palette) => {
                    settings.theme = palette;
                    save_settings(&ui, &settings);
                }
                Command::Gpu(on) => {
                    settings.gpu = on;
                    save_settings(&ui, &settings);
                    set_status(&ui, "Renderer saved. Restart SlimSpot to switch.");
                }
                Command::Skin { path, mode } => {
                    (settings.skin, settings.skin_mode) = (path, mode);
                    save_settings(&ui, &settings);
                }
                Command::Eq(state) => {
                    crate::eq::set(state);
                    settings.eq = state;
                    save_settings(&ui, &settings);
                }
                Command::Quality(_) | Command::Normalize(_) => {
                    match cmd {
                        Command::Quality(q) if q != settings.quality => settings.quality = q,
                        Command::Normalize(n) if n != settings.normalize => settings.normalize = n,
                        _ => continue,
                    }
                    save_settings(&ui, &settings);
                    set_status(&ui, "Applying audio settings...");
                    let was_playing = playing;
                    generation += 1;
                    let _ = spirc.shutdown();
                    (session, spirc, events, mixer) = reconnect(&ui, &cache, &settings, &ended_tx, generation).await;
                    playing = false;
                    set_playing(&ui, false);
                    set_status(&ui, "Audio settings applied");
                    resume(&ui, &spirc, shown.as_ref(), &settings, now_uri.as_deref(), position, was_playing);
                }
            },
            Some((ended_generation, session_invalid)) = ended_rx.recv() => {
                if ended_generation != generation {
                    continue; // a device we replaced on purpose
                }
                let reason = if session_invalid { "connection to Spotify lost" } else { "Connect loop stopped (see slimspot.log)" };
                log::warn!("Spotify Connect ended unexpectedly: {reason}");
                set_status(&ui, format!("Spotify Connect stopped: {reason}. Reconnecting..."));
                // A load asked for just now whose track never started is retried as asked.
                let retry = last_load
                    .as_ref()
                    .filter(|l| l.0.elapsed() < RETRY_LOAD_WINDOW && now_uri.as_deref() != Some(l.1.as_str()))
                    .map(|l| (l.1.clone(), l.2));
                let was_playing = playing || last_load.as_ref().is_some_and(|l| l.0.elapsed() < RETRY_LOAD_WINDOW);
                generation += 1;
                (session, spirc, events, mixer) = reconnect(&ui, &cache, &settings, &ended_tx, generation).await;
                playing = false;
                paused_here = false;
                set_playing(&ui, false);
                set_status(&ui, "Reconnected");
                let context = playing_context.clone().map(Shown::Context);
                match retry {
                    Some((uri, at)) => {
                        let request = load_request(context.as_ref().or(shown.as_ref()), &uri, &settings, at, true);
                        spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                    }
                    None => resume(&ui, &spirc, context.as_ref().or(shown.as_ref()), &settings, now_uri.as_deref(), position, was_playing),
                }
            },
            _ = async { tokio::time::sleep_until(sleep_at.expect("guarded")).await }, if sleep_at.is_some() => {
                sleep_at = None;
                spirc_result(&ui, spirc.pause());
                set_status(&ui, "Sleep timer: paused");
            },
            Some(ev) = events.recv() => {
                match ev {
                    PlayerEvent::TrackChanged { audio_item } => {
                        let item = now_playing(&audio_item);
                        if sleep_after_track && now_uri.is_some() {
                            sleep_after_track = false;
                            spirc_result(&ui, spirc.pause());
                            set_status(&ui, "Sleep timer: paused");
                        }
                        // Without a cover the previous track's tint would linger.
                        if item.cover_url.is_empty() {
                            let _ = ui.upgrade_in_event_loop(|app| app.set_now_tint(slint::Color::from_rgb_u8(16, 16, 42)));
                        }
                        if item.artist_uri != about_artist {
                            about_artist = item.artist_uri.clone();
                            let (about_session, about_ui, artist) = (session.clone(), ui.clone(), item.artist_uri.clone());
                            tokio::spawn(async move {
                                let bio: String = artist_bio(&about_session, &artist).await.chars().take(NOW_ABOUT_CHARS).collect();
                                let _ = about_ui.upgrade_in_event_loop(move |app| app.set_now_about(bio.into()));
                            });
                        }
                        now_uri = Some(item.uri.clone());
                        now_item = Some(item.clone());
                        pending_resume = None;
                        duration_ms = audio_item.duration_ms;
                        position = (0, Instant::now());
                        save_last(&ui, &mut settings, now_item.as_ref(), position, false, duration_ms, &playing_context);
                        // One quick request; failures just show the track as not liked.
                        liked = match web.as_mut() {
                            Some(w) => w.is_saved(&session, &item.uri).await.unwrap_or(false),
                            None => false,
                        };
                        let now_liked = liked;
                        let _ = ui.upgrade_in_event_loop(move |app| app.set_liked(now_liked));
                        // Fetched off the event loop; set_lyrics drops it if the track changed meanwhile.
                        clear_lyrics(&ui, "Loading lyrics...");
                        let (lyrics_session, lyrics_ui, lyrics_uri) = (session.clone(), ui.clone(), item.uri.clone());
                        tokio::spawn(async move {
                            let result = crate::lyrics::fetch(&lyrics_session, &lyrics_uri).await;
                            set_lyrics(&lyrics_ui, lyrics_uri, result);
                        });
                        let duration = audio_item.duration_ms as f32;
                        set_current(&ui, item.uri.clone(), App::set_current_track);
                        let _ = ui.upgrade_in_event_loop(move |app| {
                            app.set_now(Row::from(item));
                            app.set_duration(duration);
                        });
                        continue;
                    }
                    PlayerEvent::Seeked { position_ms, .. }
                    | PlayerEvent::PositionCorrection { position_ms, .. }
                    | PlayerEvent::PositionChanged { position_ms, .. } => {
                        position = (position_ms, Instant::now());
                        set_position(&ui, position_ms);
                        continue;
                    }
                    // Spirc reports state changes from either side (this window or the phone);
                    // they're the source of truth for the buttons and the saved settings.
                    PlayerEvent::ShuffleChanged { shuffle } => {
                        settings.shuffle = shuffle;
                        save_settings(&ui, &settings);
                        push_settings(&ui, &settings);
                        continue;
                    }
                    PlayerEvent::RepeatChanged { context, track } => {
                        settings.repeat = repeat_from(context, track);
                        save_settings(&ui, &settings);
                        push_settings(&ui, &settings);
                        continue;
                    }
                    PlayerEvent::VolumeChanged { volume } => {
                        // ponytail: remote volume changes update the slider but are only saved on the next local release.
                        settings.volume = volume as f32 / u16::MAX as f32 * 100.0;
                        push_settings(&ui, &settings);
                        continue;
                    }
                    PlayerEvent::Playing { position_ms, .. } => {
                        playing = true;
                        position = (position_ms, Instant::now());
                        set_position(&ui, position_ms);
                    }
                    PlayerEvent::Paused { position_ms, .. } => {
                        playing = false;
                        paused_here = true;
                        position = (position_ms, Instant::now());
                        set_position(&ui, position_ms);
                        save_last(&ui, &mut settings, now_item.as_ref(), position, false, duration_ms, &playing_context);
                    }
                    PlayerEvent::Stopped { .. } => {
                        playing = false;
                        paused_here = false;
                    }
                    PlayerEvent::Unavailable { .. } => { set_status(&ui, "Track unavailable"); continue }
                    _ => continue,
                }
                set_playing(&ui, playing);
                set_status(&ui, page_summary(&nav));
            },
            else => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_inputs() {
        assert!(parse_track("spotify:track:2WUy2Uywcj5cP0IXQagO3z").is_some());
        assert!(parse_track("https://open.spotify.com/track/2WUy2Uywcj5cP0IXQagO3z?si=abc").is_some());
        assert!(parse_track("https://open.spotify.com/intl-tr/track/2WUy2Uywcj5cP0IXQagO3z").is_some());
        assert!(parse_track("https://open.spotify.com/album/2WUy2Uywcj5cP0IXQagO3z").is_none());
        assert!(parse_track("garbage").is_none());
    }

    #[test]
    fn rounds_page_length_like_spotify() {
        assert_eq!(about(17 * 3_600_000 + 25 * 60_000), "about 17 hr");
        assert_eq!(about(3_600_000 + 5 * 60_000 + 59_000), "1 hr 5 min");
        assert_eq!(about(42 * 60_000 + 10_000), "42 min 10 sec");
    }

    #[test]
    fn maps_repeat_flags() {
        assert_eq!(repeat_from(false, false), Repeat::Off);
        assert_eq!(repeat_from(true, false), Repeat::All);
        assert_eq!(repeat_from(true, true), Repeat::One);
        assert_eq!(repeat_from(false, true), Repeat::One);
    }

    #[test]
    fn load_request_picks_context() {
        let s = Settings::load(&std::env::temp_dir().join("slimspot-no-such-dir"));
        let ctx = |r: LoadRequest| format!("{r:?}");
        let list = Shown::Context("spotify:playlist:abc".into());
        assert!(ctx(load_request(Some(&list), "spotify:track:x", &s, 0, true)).contains("spotify:playlist:abc"));
        let found = Shown::Tracks(vec!["spotify:track:a".into(), "spotify:track:x".into()]);
        assert!(ctx(load_request(Some(&found), "spotify:track:x", &s, 0, true)).contains("spotify:track:a"));
        // A pasted link that isn't in the shown search results plays on its own.
        assert!(!ctx(load_request(Some(&found), "spotify:track:zz", &s, 0, true)).contains("spotify:track:a"));
    }

    #[test]
    fn days_from_date_matches_unix_days() {
        assert_eq!(days_from_date("1970-01-01"), Some(0));
        assert_eq!(days_from_date("2000-03-01"), Some(11_017));
        assert_eq!(days_from_date("2026-10-05"), Some(20_731));
        assert_eq!(days_from_date("2024"), days_from_date("2024-01-01"));
        assert_eq!(days_from_date("x"), None);
    }

    #[test]
    fn credit_lines_join_roles() {
        let json = serde_json::json!({"roleCredits": [
            {"roleTitle": "Writers", "artists": [{"name": "A"}, {"name": "B"}]},
            {"roleTitle": "Empty", "artists": []}
        ]});
        assert_eq!(credit_lines(&json), "Writers: A, B");
    }
}
