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
use crate::ui::{App, Row, clear_lyrics, set_current, set_lyrics, set_page_header, set_playing, set_position, set_rows, set_status};
use crate::web::{Item, LIKED_SONGS, WebApi};

// Spotify desktop client id; same as librespot's internal KEYMASTER_CLIENT_ID.
const SPOTIFY_CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
const OAUTH_SCOPES: &[&str] = &["streaming"];
const DEVICE_NAME: &str = "SlimSpot";
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
// ponytail: all requests in flight at once (50 for a radio). After a radio the private RAM sat at
// ~25 MB instead of ~17 MB (measured 2026-10-03, cause unconfirmed); fetch in batches if it matters.
async fn track_items(session: &Session, uris: Vec<SpotifyUri>) -> Vec<Item> {
    let mut tasks = tokio::task::JoinSet::new();
    for (index, uri) in uris.into_iter().enumerate() {
        let session = session.clone();
        tasks.spawn(async move { (index, Track::get(&session, &uri).await.ok().map(|track| track_item(&uri, track))) });
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
    }
}

/// A track's radio, resolved once: Spotify reshuffles a station every time it is resolved, so
/// these exact tracks are both shown and played (as a track list, not the station context).
async fn radio_tracks(session: &Session, track_uri: &str) -> Result<Vec<Item>, String> {
    let id = track_uri.strip_prefix("spotify:track:").ok_or("Radio needs a Spotify track")?;
    let context = session
        .spclient()
        .get_context(&format!("spotify:station:track:{id}"))
        .await
        .map_err(|e| format!("Radio unavailable: {e}"))?;
    let mut seen = std::collections::HashSet::new();
    let uris: Vec<SpotifyUri> = context
        .pages
        .iter()
        .flat_map(|page| &page.tracks)
        .filter_map(|t| t.uri.as_deref().and_then(|u| SpotifyUri::from_uri(u).ok()))
        .filter(|u| matches!(u, SpotifyUri::Track { .. }) && seen.insert(u.to_uri().unwrap_or_default()))
        .collect();
    if uris.is_empty() {
        return Err("Spotify has no radio for this track".into());
    }
    Ok(track_items(session, uris).await)
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
}

/// Opens `page` as a new history entry and shows it.
fn open_page(ui: &slint::Weak<App>, nav: &mut History<Page>, shown: &mut Option<Shown>, page: Page) {
    nav.push(page);
    show_current(ui, nav, shown);
}

/// Puts the history's current page on screen (after open, back or forward).
fn show_current(ui: &slint::Weak<App>, nav: &History<Page>, shown: &mut Option<Shown>) {
    let Some(page) = nav.current() else { return };
    *shown = Some(page.shown.clone());
    set_current(ui, page.list.clone(), App::set_current_list);
    set_rows(ui, page.rows.clone(), App::set_tracks);
    set_page_header(ui, page.title.clone(), nav.can_back(), nav.can_forward());
    set_status(ui, format!("{} items", page.rows.len()));
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
        set_rows(&ui, lists, App::set_lists);
    }
    set_status(&ui, format!("Logged in as {}. Visible in Spotify Connect as \"{DEVICE_NAME}\".", session.username()));

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
                        w.list_tracks(&session, &uri).await.map(|rows| (String::new(), rows))
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
                            // Only sidebar entries get highlighted.
                            let sidebar = uri == LIKED_SONGS || uri.starts_with("spotify:playlist:");
                            let list = if sidebar { uri } else { String::new() };
                            open_page(&ui, &mut nav, &mut shown, Page { title, rows, shown: Shown::Context(context), list });
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::PlayUri(uri) => {
                    set_status(&ui, "Loading...");
                    pending_resume = None;
                    playing_context = match &shown {
                        Some(Shown::Context(c)) => Some(c.clone()),
                        _ => None,
                    };
                    let request = load_request(shown.as_ref(), &uri, &settings, 0, true);
                    spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                }
                Command::Submit(s) if parse_track(&s).is_some() => {
                    let uri = parse_track(&s).expect("checked above").to_uri().unwrap_or_default();
                    set_status(&ui, "Loading...");
                    pending_resume = None;
                    playing_context = None;
                    let request = load_request(None, &uri, &settings, 0, true);
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
                        Ok(rows) => {
                            set_status(&ui, format!("{} results", rows.len()));
                            // Only the track rows form the play queue; album/artist rows open pages.
                            let tracks = rows.iter().filter(|r| r.uri.starts_with("spotify:track:")).map(|r| r.uri.clone()).collect();
                            let title = format!("Search: {}", q.trim());
                            open_page(&ui, &mut nav, &mut shown, Page { title, rows, shown: Shown::Tracks(tracks), list: String::new() });
                        }
                        Err(e) => set_status(&ui, e),
                    }
                }
                Command::Toggle => match pending_resume.take() {
                    Some(last) => {
                        playing_context = last.context.clone();
                        let shown = last.context.map(Shown::Context);
                        let request = load_request(shown.as_ref(), &last.item.uri, &settings, last.position_ms, true);
                        spirc_result(&ui, spirc.activate().and_then(|()| spirc.load(request)));
                    }
                    None => spirc_result(&ui, if playing { spirc.pause() } else { spirc.play() }),
                },
                Command::ToggleLike => {
                    let (Some(w), Some(uri)) = (web.as_mut(), now_uri.clone()) else { continue };
                    match w.set_saved(&session, &uri, !liked).await {
                        Ok(()) => {
                            liked = !liked;
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
                            });
                            playing_context = None;
                            pending_resume = None;
                            let first = uris[0].clone();
                            let request = load_request(shown.as_ref(), &first, &settings, 0, true);
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
                Command::Back => {
                    if nav.back().is_some() {
                        show_current(&ui, &nav, &mut shown);
                    }
                }
                Command::Forward => {
                    if nav.forward().is_some() {
                        show_current(&ui, &nav, &mut shown);
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
                let was_playing = playing;
                generation += 1;
                (session, spirc, events, mixer) = reconnect(&ui, &cache, &settings, &ended_tx, generation).await;
                playing = false;
                set_playing(&ui, false);
                set_status(&ui, "Reconnected");
                resume(&ui, &spirc, shown.as_ref(), &settings, now_uri.as_deref(), position, was_playing);
            },
            Some(ev) = events.recv() => {
                match ev {
                    PlayerEvent::TrackChanged { audio_item } => {
                        let item = now_playing(&audio_item);
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
                        position = (position_ms, Instant::now());
                        set_position(&ui, position_ms);
                        save_last(&ui, &mut settings, now_item.as_ref(), position, false, duration_ms, &playing_context);
                    }
                    PlayerEvent::Stopped { .. } => playing = false,
                    PlayerEvent::Unavailable { .. } => { set_status(&ui, "Track unavailable"); continue }
                    _ => continue,
                }
                set_playing(&ui, playing);
                if playing { set_status(&ui, "Playing") } else { set_status(&ui, "Paused / stopped") }
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
