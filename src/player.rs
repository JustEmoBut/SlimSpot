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
    metadata::{Artist, Metadata, Track, audio::{AudioItem, UniqueFields}},
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
use crate::web::{Item, LIKED_SONGS, SEARCH_LIMIT, WebApi};

// Spotify desktop client id; same as librespot's internal KEYMASTER_CLIENT_ID.
const SPOTIFY_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const OAUTH_SCOPES: &[&str] = &["streaming"];
const DEVICE_NAME: &str = "SlimSpot";
// Last row of a full search page; clicking it appends the next page of tracks.
const MORE_URI: &str = "slimspot:more";
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
    FilterLibrary { kind: i32, text: String, az: bool },
    /// Pause after this many minutes; -1 = at the end of the current track, 0 = off.
    SleepTimer(i32),
    CreatePlaylist,
    /// Rename the playlist on screen.
    RenamePlaylist(String),
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
        backend(None, AudioFormat::default())
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
async fn radio_tracks(session: &Session, track_uri: &str) -> Result<Vec<Item>, String> {
    let id = track_uri.strip_prefix("spotify:track:").ok_or("Radio needs a Spotify track")?;
    let rows = context_tracks(session, &format!("spotify:station:track:{id}"))
        .await
        .map_err(|e| format!("Radio unavailable: {e}"))?;
    if rows.is_empty() {
        return Err("Spotify has no radio for this track".into());
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
    } else {
        "Playlist"
    }
}

/// Rows that open a page instead of playing.
fn is_page(uri: &str) -> bool {
    uri == LIKED_SONGS || ["spotify:playlist:", "spotify:album:", "spotify:artist:"].iter().any(|p| uri.starts_with(p))
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
    /// Search query and next track offset while "Show more" is offered.
    more: Option<(String, u32)>,
    /// One of the user's own playlists: rows can be removed.
    editable: bool,
    /// Small label above the title ("Playlist", "Album", ...).
    kind: &'static str,
    /// Header cover (playlists and albums), or "".
    cover_url: String,
    /// Rows already checked for liked marks (`mark_liked` continues from here).
    liked_upto: usize,
}

fn more_row() -> Item {
    Item { title: "Show more tracks".into(), uri: MORE_URI.into(), ..Default::default() }
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
    let numbered = matches!(page.kind, "Playlist" | "Album" | "Radio" | "Up next");
    let playable = page.rows.iter().any(|r| r.uri.starts_with("spotify:track:"));
    let cover = page.cover_url.clone();
    let liked_upto = page.liked_upto as i32;
    let _ = ui.upgrade_in_event_loop(move |app| {
        app.set_liked_checked(liked_upto);
        if app.get_page_cover_url() != cover.as_str() {
            app.set_page_cover(Default::default());
            app.set_page_cover_url(cover.into());
        }
        app.set_editable(editable);
        app.set_numbered(numbered);
        app.set_page_playable(playable);
    });
}

/// Pushes the sidebar and the "Add to playlist" targets after the library changed.
fn refresh_library(ui: &slint::Weak<App>, library: &[Item], own_lists: &[Item]) {
    set_rows(ui, library.to_vec(), App::set_lists);
    set_rows(ui, own_lists.to_vec(), App::set_targets);
    let _ = ui.upgrade_in_event_loop(|app| app.set_library_kind(0));
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
    let total_ms: u64 = rows.clone().map(|r| r.duration_ms as u64).sum();
    match (tracks, total_ms) {
        (0, _) | (_, 0) => format!("{} items", rows.count()),
        (n, ms) => format!("{n} songs, {}", about(ms)),
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
    let _ = ui.upgrade_in_event_loop(move |app| {
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
    push_settings(&ui, &settings);

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
    let mut web = match WebApi::login(&dir, move || set_status(&browser_ui, "Opening browser for Web API login...")).await {
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
    // Set by GoToPlaying: scroll to this track once its page has loaded.
    let mut reveal: Option<String> = None;
    // Sleep timer: pause at this instant, or when the playing track ends.
    let mut sleep_at: Option<tokio::time::Instant> = None;
    let mut sleep_after_track = false;
    // When this window last asked Spirc to load something.
    let mut last_load: Option<Instant> = None;
    // Paused by a Paused event (the device is active and `play` works). After a reconnect it isn't,
    // and Spirc ignores `play` while inactive, so Play loads the track again instead.
    let mut paused_here = false;
    if let Some(w) = web.as_mut() {
        set_status(&ui, "Loading playlists...");
        let mut lists = vec![Item { title: "Liked Songs".into(), uri: LIKED_SONGS.into(), ..Default::default() }];
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
                    // Album and artist pages bring their own name; lists are named by the clicked row.
                    let result = if uri.starts_with("spotify:album:") {
                        w.album_tracks(&session, &uri).await
                    } else if uri.starts_with("spotify:artist:") {
                        match artist_top_tracks(&session, &uri).await {
                            Ok((name, mut top)) => w.artist_albums(&session, &uri).await.map(|albums| {
                                top.extend(albums);
                                (name, top)
                            }),
                            Err(e) => Err(e),
                        }
                    } else {
                        match w.list_tracks(&session, &uri).await {
                            Ok(rows) => Ok((String::new(), rows)),
                            Err(e) if uri.starts_with("spotify:playlist:") => {
                                log::warn!("Web API refused {uri} ({e}); reading it through librespot");
                                context_tracks(&session, &uri).await.map(|rows| (String::new(), rows))
                            }
                            Err(e) => Err(e),
                        }
                    };
                    match result {
                        Ok((name, rows)) => {
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
                                "Album" => rows.first().map(|r| r.cover_url.clone()).unwrap_or_default(),
                                "Playlist" => find_item(&nav, &library, None, &uri).map(|i| i.cover_url.clone()).unwrap_or_default(),
                                _ => String::new(),
                            };
                            let kind = kind_of(&uri);
                            // Only sidebar entries get highlighted.
                            let sidebar = uri == LIKED_SONGS || uri.starts_with("spotify:playlist:");
                            let list = if sidebar { uri } else { String::new() };
                            let reveal_row = reveal.take().filter(|_| playing_context.as_ref() == Some(&context));
                            open_page(&ui, &mut nav, &mut shown, Page { title, rows, shown: Shown::Context(context), list, more: None, editable, kind, cover_url, liked_upto: 0 });
                            if let Some(track) = reveal_row {
                                reveal_track(&ui, &nav, &track);
                            }
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::PlayUri(uri) if uri == MORE_URI => {
                    let (Some(w), Some(page)) = (web.as_mut(), nav.current_mut()) else { continue };
                    let Some((query, offset)) = page.more.take() else { continue };
                    set_status(&ui, "Loading more...");
                    match w.search_tracks(&session, &query, offset).await {
                        Ok(found) => {
                            page.rows.pop(); // the "Show more" row
                            if found.len() == SEARCH_LIMIT as usize {
                                page.more = Some((query, offset + SEARCH_LIMIT));
                            }
                            // Spotify's later search pages repeat earlier hits (seen 2026-10-03).
                            let fresh: Vec<Item> = found.into_iter().filter(|f| !page.rows.iter().any(|r| r.uri == f.uri)).collect();
                            if let Shown::Tracks(uris) = &mut page.shown {
                                uris.extend(fresh.iter().map(|r| r.uri.clone()));
                            }
                            page.rows.extend(fresh);
                            if page.more.is_some() {
                                page.rows.push(more_row());
                            }
                        }
                        // Keep the row so the click can be retried.
                        Err(e) => {
                            page.more = Some((query, offset));
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
                            open_page(&ui, &mut nav, &mut shown, Page { title: "Queue".into(), rows, shown: Shown::Tracks(uris), list: String::new(), more: None, editable: false, kind: "Up next", cover_url: String::new(), liked_upto: 0 });
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
                    last_load = Some(Instant::now());
                    spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                }
                Command::Submit(s) if parse_track(&s).is_some() => {
                    let uri = parse_track(&s).expect("checked above").to_uri().unwrap_or_default();
                    set_status(&ui, "Loading...");
                    pending_resume = None;
                    playing_context = None;
                    let request = load_request(None, &uri, &settings, 0, true);
                    last_load = Some(Instant::now());
                    spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                }
                Command::Submit(q) if q.trim().is_empty() => {}
                Command::Submit(q) | Command::OpenList(q) => {
                    let Some(w) = web.as_mut() else {
                        set_status(&ui, "Search unavailable (web login failed)");
                        continue;
                    };
                    set_status(&ui, "Searching...");
                    match w.search(&session, q.trim()).await {
                        Ok(mut rows) => {
                            // Only the track rows form the play queue; album/artist rows open pages.
                            let tracks: Vec<String> = rows.iter().filter(|r| r.uri.starts_with("spotify:track:")).map(|r| r.uri.clone()).collect();
                            let more = (tracks.len() == SEARCH_LIMIT as usize).then(|| (q.trim().to_string(), SEARCH_LIMIT));
                            if more.is_some() {
                                rows.push(more_row());
                            }
                            let title = format!("Search: {}", q.trim());
                            open_page(&ui, &mut nav, &mut shown, Page { title, rows, shown: Shown::Tracks(tracks), list: String::new(), more, editable: false, kind: "Search", cover_url: String::new(), liked_upto: 0 });
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
                        last_load = Some(Instant::now());
                        spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                    }
                    None if playing || paused_here => spirc_result(&ui, if playing { spirc.pause() } else { spirc.play() }),
                    None => {
                        let Some(uri) = now_uri.clone() else { continue };
                        let context = playing_context.clone().map(Shown::Context);
                        let request = load_request(context.as_ref(), &uri, &settings, position.0, true);
                        last_load = Some(Instant::now());
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
                            let seed_title = find_item(&nav, &library, now_item.as_ref(), &seed).map(|i| i.title.clone()).unwrap_or_default();
                            open_page(&ui, &mut nav, &mut shown, Page {
                                title: format!("{seed_title} Radio"),
                                rows,
                                shown: Shown::Tracks(uris.clone()),
                                list: String::new(),
                                more: None,
                                editable: false,
                                kind: "Radio",
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
                            last_load = Some(Instant::now());
                            spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::RowAction { action, uri } => {
                    let item = find_item(&nav, &library, now_item.as_ref(), &uri).cloned().unwrap_or_default();
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
                        "unfollow" => {
                            if let Some(w) = web.as_mut() {
                                match w.unfollow_playlist(&session, &uri).await {
                                    Ok(()) => {
                                        library.retain(|i| i.uri != uri);
                                        own_lists.retain(|i| i.uri != uri);
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
                                more: None,
                                editable: false,
                                kind: "Home",
                                cover_url: String::new(),
                                liked_upto: 0,
                            });
                            mark_liked(&ui, w, &session, &mut nav).await;
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::PlayPage => {
                    let first = nav.current().and_then(|p| p.rows.iter().find(|r| r.uri.starts_with("spotify:track:")));
                    if let Some(row) = first {
                        let _ = tx.send(Command::PlayUri(row.uri.clone()));
                    }
                }
                Command::FilterLibrary { kind, text, az } => {
                    let text = text.trim().to_lowercase();
                    let mut rows: Vec<Item> = library
                        .iter()
                        .filter(|i| match kind {
                            1 => i.uri == LIKED_SONGS || i.uri.starts_with("spotify:playlist:"),
                            2 => i.uri.starts_with("spotify:album:"),
                            3 => i.uri.starts_with("spotify:artist:"),
                            _ => true,
                        })
                        .filter(|i| text.is_empty() || i.title.to_lowercase().contains(&text) || i.artist.to_lowercase().contains(&text))
                        .cloned()
                        .collect();
                    if az {
                        rows.sort_by_key(|i| i.title.to_lowercase());
                    }
                    set_rows(&ui, rows, App::set_lists);
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
                Command::CreatePlaylist => {
                    let Some(w) = web.as_mut() else { continue };
                    let name = format!("My Playlist #{}", own_lists.len() + 1);
                    match w.create_playlist(&session, &name).await {
                        Ok(item) => {
                            // New playlists come first, below Liked Songs, as in Spotify.
                            library.insert(1.min(library.len()), item.clone());
                            own_lists.insert(0, item.clone());
                            refresh_library(&ui, &library, &own_lists);
                            let _ = tx.send(Command::OpenList(item.uri));
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::RenamePlaylist(name) => {
                    let name = name.trim().to_string();
                    let list = nav.current().filter(|p| p.editable).map(|p| p.list.clone());
                    let (Some(w), Some(list)) = (web.as_mut(), list) else { continue };
                    if name.is_empty() {
                        continue;
                    }
                    match w.rename_playlist(&session, &list, &name).await {
                        Ok(()) => {
                            for item in library.iter_mut().chain(own_lists.iter_mut()).filter(|i| i.uri == list) {
                                item.title = name.clone();
                            }
                            if let Some(page) = nav.current_mut() {
                                page.title = name.clone();
                            }
                            refresh_library(&ui, &library, &own_lists);
                            set_page_header(&ui, nav.current().map(|p| p.kind).unwrap_or(""), name, nav.can_back(), nav.can_forward());
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
                let was_playing = playing || last_load.is_some_and(|t| t.elapsed() < RETRY_LOAD_WINDOW);
                generation += 1;
                (session, spirc, events, mixer) = reconnect(&ui, &cache, &settings, &ended_tx, generation).await;
                playing = false;
                paused_here = false;
                set_playing(&ui, false);
                set_status(&ui, "Reconnected");
                let context = playing_context.clone().map(Shown::Context);
                resume(&ui, &spirc, context.as_ref().or(shown.as_ref()), &settings, now_uri.as_deref(), position, was_playing);
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
}
