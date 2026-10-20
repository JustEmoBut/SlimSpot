//! The window: a Freya UI driven by `UiUpdate`s from the backend. Commands go back over the
//! backend's `Command` channel.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use freya::animation::*;
use freya::prelude::*;
use freya::tray::menu::{Menu as TrayMenu, MenuEvent, MenuItem as TrayItem, PredefinedMenuItem};
use freya::tray::{Icon, MouseButton, MouseButtonState, TrayEvent, TrayIcon, TrayIconBuilder, TrayIconEvent};
use freya::winit::window::Theme as WindowTheme;
use tokio::sync::mpsc;

use crate::eq::EqState;
use crate::player::{Command, MORE_URI};
use crate::settings::Quality;
use crate::web::Item;

// ---- Messages from the backend ----

/// Track-list models the backend fills from `web::Item`s.
#[derive(Clone, Copy, Debug)]
pub enum Rows {
    Tracks,
    Lists,
    Targets,
    SideRows,
    Devices,
    Shelf,
    QuickTop,
    QuickBottom,
    AddResults,
}

/// Which "currently showing" URI a `UiUpdate::Current` sets.
#[derive(Clone, Copy, Debug)]
pub enum Current {
    List,
    Track,
}

/// Everything the backend shows. It sends these instead of touching the window, so the
/// backend doesn't depend on the UI toolkit.
pub enum UiUpdate {
    Status(String),
    Playing(bool),
    Position(u32),
    Rows(Rows, Vec<Item>),
    /// Appended to the track list in place, so loaded covers and the scroll position stay.
    AppendTracks(Vec<Item>),
    Lyrics(String, Result<Option<crate::lyrics::Lyrics>, String>),
    ClearLyrics(String),
    LikedRows(Vec<String>, bool),
    PageHeader { kind: &'static str, title: String, can_back: bool, can_forward: bool },
    Current(Current, String),
    Page(PageView),
    PageSaved(bool),
    PageInfo(String),
    PageCover(String),
    ScrollTop,
    Reveal(usize),
    LikedChecked(usize),
    ResetLibraryChips,
    Settings { volume: f32, shuffle: bool, repeat: i32, quality: i32, normalize: bool, theme: i32 },
    StartupSettings { gpu: bool, low_api: bool, compact: bool, eq: crate::eq::EqState },
    Now { item: Item, duration: f32, position: Option<f32> },
    NowTintDefault,
    NowAbout(String),
    Liked(bool),
    Credits { title: String, text: String },
    UpdateVersion(String),
    WebClientId(String),
    /// An encoded cover image for this URL (see `covers::worker`).
    Cover(String, Bytes),
    /// Another launch asked this window to show itself.
    Show,
    /// The app's own quit path (saves, shuts Spirc down), as after an update.
    InvokeQuit,
    /// Ends the event loop; sent by that quit path once the backend is done.
    QuitEventLoop,
}

/// The page-level state `show_current` puts on screen.
pub struct PageView {
    pub info: String,
    pub saved: bool,
    pub search_tab: i32,
    pub tabs: Vec<&'static str>,
    pub about: String,
    pub page_uri: String,
    pub liked_upto: i32,
    pub cover: String,
    pub editable: bool,
    pub numbered: bool,
    pub playable: bool,
}

/// The backend's handle to the UI: a sink for `UiUpdate`s, cheap to clone and `Send`.
#[derive(Clone)]
pub struct Ui(Arc<dyn Fn(UiUpdate) + Send + Sync>);

impl Ui {
    pub fn send(&self, update: UiUpdate) {
        (self.0)(update)
    }
}

pub fn set_liked_rows(ui: &Ui, uris: Vec<String>, liked: bool) {
    ui.send(UiUpdate::LikedRows(uris, liked));
}

pub fn set_status(ui: &Ui, text: impl Into<String>) {
    ui.send(UiUpdate::Status(text.into()));
}

pub fn set_playing(ui: &Ui, playing: bool) {
    ui.send(UiUpdate::Playing(playing));
}

pub fn set_position(ui: &Ui, ms: u32) {
    ui.send(UiUpdate::Position(ms));
}

pub fn set_rows(ui: &Ui, items: Vec<Item>, target: Rows) {
    ui.send(UiUpdate::Rows(target, items));
}

pub fn set_lyrics(ui: &Ui, uri: String, result: Result<Option<crate::lyrics::Lyrics>, String>) {
    ui.send(UiUpdate::Lyrics(uri, result));
}

pub fn clear_lyrics(ui: &Ui, note: &str) {
    ui.send(UiUpdate::ClearLyrics(note.to_string()));
}

pub fn set_page_header(ui: &Ui, kind: &'static str, title: String, can_back: bool, can_forward: bool) {
    ui.send(UiUpdate::PageHeader { kind, title, can_back, can_forward });
}

pub fn set_current(ui: &Ui, uri: String, which: Current) {
    ui.send(UiUpdate::Current(which, uri));
}

// ---- Wiring ----

static UPDATES: Mutex<Option<mpsc::UnboundedReceiver<UiUpdate>>> = Mutex::new(None);
static COMMANDS: OnceLock<mpsc::UnboundedSender<Command>> = OnceLock::new();
static COVER_REQUESTS: OnceLock<mpsc::UnboundedSender<String>> = OnceLock::new();
// "Update to vX" clicks go to the update task, not the command loop (see CLAUDE.md, Updates).
static UPDATE_CLICKS: OnceLock<mpsc::UnboundedSender<()>> = OnceLock::new();

/// The handle the backend sends through; `run` picks the updates up on the UI thread.
pub fn channel() -> Ui {
    let (tx, rx) = mpsc::unbounded_channel();
    *UPDATES.lock().unwrap() = Some(rx);
    Ui(Arc::new(move |update| {
        let _ = tx.send(update);
    }))
}

fn send(cmd: Command) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.send(cmd);
    }
}

// The backend saves the session and shuts Spirc down first; this bounds the wait (it may be stuck
// waiting for a browser login).
const QUIT_GRACE: Duration = Duration::from_secs(3);

fn quit() {
    send(Command::Quit);
    std::thread::spawn(|| {
        std::thread::sleep(QUIT_GRACE);
        std::process::exit(0);
    });
}

/// Opens the window and runs the event loop until the app quits.
pub fn run(commands: mpsc::UnboundedSender<Command>, cover_requests: mpsc::UnboundedSender<String>, update_clicks: mpsc::UnboundedSender<()>, start_in_tray: bool) {
    let _ = COMMANDS.set(commands);
    let _ = COVER_REQUESTS.set(cover_requests);
    let _ = UPDATE_CLICKS.set(update_clicks);
    launch(
        LaunchConfig::new()
            .with_font("Poppins", &include_bytes!("../assets/fonts/Poppins-Regular.ttf")[..])
            .with_font("Poppins", &include_bytes!("../assets/fonts/Poppins-SemiBold.ttf")[..])
            .with_default_font("Poppins")
            .with_window(
                WindowConfig::new(app)
                    .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
                    .with_title("SlimSpot")
                    .with_icon(LaunchConfig::window_icon(include_bytes!("../assets/icon-64.png")))
                    .with_renderer(RendererPreference::Software)
                    .with_window_attributes(|attributes, _| attributes.with_theme(Some(WindowTheme::Dark)))
                    .with_window_handle(move |window| {
                        let _ = MAIN_WINDOW.set(window.id());
                        #[cfg(windows)]
                        windows_setup(window);
                        // Started by the Windows Run entry: stay in the tray. Freya shows the window right
                        // after this hook, so the hide is queued for after that.
                        if start_in_tray {
                            Platform::get().with_window(window.id(), |w| w.set_visible(false));
                        }
                    })
                    // Close hides to the tray; playback continues. Quit from the tray or Ctrl+Q.
                    .with_on_close(|_, id| {
                        // Hiding inside the close event itself doesn't stick; queue it for after.
                        Platform::get().with_window(id, |w| w.set_visible(false));
                        CloseDecision::KeepOpen
                    }),
            )
            .with_tray(tray_icon, |event, _| {
                let show = || show_window();
                match event {
                    TrayEvent::Icon(TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) => show(),
                    TrayEvent::Menu(MenuEvent { id }) => match id.as_ref() {
                        TRAY_SHOW => show(),
                        TRAY_TOGGLE => send(Command::Toggle),
                        TRAY_PREV => send(Command::Prev),
                        TRAY_NEXT => send(Command::Next),
                        TRAY_QUIT => quit(),
                        _ => {}
                    },
                    _ => {}
                }
            }),
    );
}

static MAIN_WINDOW: OnceLock<freya::winit::window::WindowId> = OnceLock::new();
/// The window's HWND, as the owner of native dialogs.
static MAIN_HWND: OnceLock<isize> = OnceLock::new();

/// Title bar colors, media keys and taskbar buttons; they need the native window, which exists by now.
#[cfg(windows)]
fn windows_setup(window: &freya::winit::window::Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    crate::instance::dark_title_bar(window, BASE, SUB, HAIRLINE);
    let (Ok(RawWindowHandle::Win32(h)), Some(tx)) = (window.window_handle().map(|h| h.as_raw()), COMMANDS.get()) else {
        return log::warn!("Media keys and taskbar buttons unavailable: no Win32 window handle");
    };
    let hwnd = h.hwnd.get();
    let _ = MAIN_HWND.set(hwnd);
    if let Err(e) = crate::media_keys::setup(hwnd, tx.clone()) {
        log::warn!("{e}");
    }
    crate::taskbar::setup(hwnd, tx.clone());
}

/// Brings the window back from the tray (or from behind other windows).
fn show_window() {
    if let Some(id) = MAIN_WINDOW.get() {
        Platform::get().with_window(*id, |w| {
            #[cfg(windows)]
            crate::instance::bring_to_front(w);
        });
    }
}

const TRAY_SHOW: &str = "show";
const TRAY_TOGGLE: &str = "toggle";
const TRAY_PREV: &str = "prev";
const TRAY_NEXT: &str = "next";
const TRAY_QUIT: &str = "quit";

fn tray_icon() -> TrayIcon {
    let menu = TrayMenu::new();
    let items = [(TRAY_SHOW, "Show SlimSpot"), (TRAY_TOGGLE, "Play/Pause"), (TRAY_PREV, "Previous"), (TRAY_NEXT, "Next")];
    for (id, label) in items {
        let _ = menu.append(&TrayItem::with_id(id, label, true, None));
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&TrayItem::with_id(TRAY_QUIT, "Quit", true, None));
    let mut builder = TrayIconBuilder::new().with_tooltip("SlimSpot").with_menu(Box::new(menu)).with_menu_on_left_click(false);
    match image::load_from_memory(include_bytes!("../assets/icon-64.png")) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            match Icon::from_rgba(rgba.into_raw(), w, h) {
                Ok(icon) => builder = builder.with_icon(icon),
                Err(e) => log::warn!("Tray icon: {e}"),
            }
        }
        Err(e) => log::warn!("Tray icon: {e}"),
    }
    builder.build().expect("tray icon")
}

// ---- Design tokens (one dark theme) ----

const WINDOW_WIDTH: f64 = 1280.;
const WINDOW_HEIGHT: f64 = 800.;
const BASE: (u8, u8, u8) = (10, 10, 12);
const SURFACE: (u8, u8, u8) = (20, 20, 23);
const ELEVATED: (u8, u8, u8) = (30, 30, 35);
const HOVER: (u8, u8, u8) = (38, 38, 44);
const HAIRLINE: (u8, u8, u8) = (40, 40, 46);
const TEXT: (u8, u8, u8) = (245, 245, 247);
const SUB: (u8, u8, u8) = (161, 161, 170);
const ACCENT: (u8, u8, u8) = (30, 215, 96);
const ACCENT_HOVER: (u8, u8, u8) = (60, 235, 120);
const ON_ACCENT: (u8, u8, u8) = (8, 8, 10);
const RADIUS_PANEL: f32 = 12.;
const RADIUS_COVER: f32 = 6.;
const GAP: f32 = 8.;
const PLAYER_H: f32 = 88.;
const SIDEBAR_W: f32 = 300.;
const SIDE_PANEL_W: f32 = 320.;
const SIDE_TABS: [&str; 3] = ["Now playing", "Queue", "Recent"];
// Right panel tab indexes.
const TAB_NOW: i32 = 0;
const TAB_QUEUE: i32 = 1;
const TAB_RECENT: i32 = 2;
const ROW_H: f32 = 56.;
// Room for row numbers up to 4 digits (Liked Songs has hundreds).
const NUMBER_W: f32 = 36.;
// Compact track list: one-line rows without covers (Settings → Display).
const COMPACT_ROW_H: f32 = 36.;
// Mini player: the bar alone, always on top.
const MINI_WIDTH: f64 = 860.;
const MINI_HEIGHT: f64 = 96.;
// Share of the cover's average color kept for the page tint, so white text stays readable.
const TINT_BRIGHTNESS: f32 = 0.45;
const DEFAULT_TINT: (u8, u8, u8) = (88, 64, 128);
// Playing-row equalizer bars: a few steps a second keep the software renderer's CPU low.
const EQ_BARS_TICK: Duration = Duration::from_millis(300);
const SHORTCUTS: [(&str, &str); 23] = [
    ("Space", "Play / pause"),
    ("Ctrl+Right / Left", "Next / previous"),
    ("Shift+Right / Left", "Seek 10 s"),
    ("Ctrl+Up / Down", "Volume"),
    ("M", "Mute"),
    ("B", "Like the playing song"),
    ("Ctrl+S / Ctrl+R", "Shuffle / repeat"),
    ("Alt+Left / Right", "Back / forward"),
    ("Ctrl+H", "Home"),
    ("Ctrl+B", "Hide / show the sidebar"),
    ("/ or Ctrl+L", "Search"),
    ("Ctrl+F", "Find in page"),
    ("Up / Down", "Move the row cursor (Shift extends)"),
    ("Enter", "Play the row under the cursor"),
    ("Ctrl+A", "Select all songs"),
    ("Ctrl+C / X / V", "Copy / cut / paste song links"),
    ("Delete", "Remove the selection"),
    ("Q or Ctrl+U", "Queue"),
    ("Ctrl+Y", "Lyrics"),
    ("Ctrl+,", "Settings"),
    ("?", "This list"),
    ("Esc", "Close / leave the search box"),
    ("Ctrl+Q", "Quit"),
];
const HOVER_MS: u64 = 150;
const PAGE_ANIM_MS: u64 = 250;
// Covers kept as encoded bytes (a few KB each); Freya decodes and caches them at drawn size.
const MAX_COVERS: usize = 300;
const POSITION_TICK: Duration = Duration::from_millis(500);
// Keyboard steps: Shift+arrows seek, Ctrl+Up/Down change the volume.
const SEEK_STEP_MS: u32 = 10_000;
const VOLUME_STEP: f32 = 5.;
// Lyrics lines have a fixed height, so the sung line can be centered arithmetically.
const LYRIC_LINE_H: f32 = 52.;
// Roughly half the lyrics panel at the default window size, so the sung line sits mid-panel.
const LYRICS_TOP: f32 = 260.;
// Winamp's 10 EQ bands and its built-in presets (dB / MAX_DB).
const EQ_BANDS: [&str; 10] = ["60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K"];
const EQ_PRESETS: [(&str, [f32; 10]); 8] = [
    ("Flat", [0., 0., 0., 0., 0., 0., 0., 0., 0., 0.]),
    ("Classical", [0., 0., 0., 0., 0., 0., -0.36, -0.36, -0.36, -0.48]),
    ("Dance", [0.48, 0.36, 0.12, 0., 0., -0.28, -0.36, -0.36, 0., 0.]),
    ("Full Bass", [0.4, 0.4, 0.4, 0.24, 0.08, -0.2, -0.4, -0.52, -0.56, -0.56]),
    ("Full Treble", [-0.48, -0.48, -0.48, -0.2, 0.12, 0.56, 0.8, 0.8, 0.8, 0.84]),
    ("Pop", [-0.08, 0.24, 0.36, 0.4, 0.28, 0., -0.12, -0.12, -0.08, -0.08]),
    ("Rock", [0.4, 0.24, -0.28, -0.4, -0.16, 0.2, 0.44, 0.56, 0.56, 0.56]),
    ("Techno", [0.4, 0.28, 0., -0.28, -0.24, 0., 0.4, 0.48, 0.48, 0.44]),
];
// Library filter chips (kind 1..=4; 0 = none, like Spotify there is no "All" chip) and sort chip labels.
const LIBRARY_KINDS: [&str; 4] = ["Playlists", "Albums", "Artists", "Podcasts"];
const LIBRARY_SORTS: [&str; 3] = ["Sort", "Recents", "A-Z"];
// The next batch of liked marks is asked for when the list scrolls this close to the checked rows.
const LIKED_AHEAD: usize = 20;
const PAGE_SORTS: [&str; 4] = ["Custom order", "Title", "Artist", "Duration"];
const QUALITIES: [&str; 3] = ["96 kbps", "160 kbps", "320 kbps"];
const SLEEP_CHOICES: [(&str, i32); 5] = [("Off", 0), ("15 min", 15), ("30 min", 30), ("1 hr", 60), ("End of track", -1)];

// ---- Icons: 24x24 paths, stroked unless noted ----
const I_HOME: &str = "M4 11 L12 4 L20 11 V20 H14.5 V14 H9.5 V20 H4 Z";
const I_SEARCH: &str = "M17 10.5 C17 14.1 14.1 17 10.5 17 C6.9 17 4 14.1 4 10.5 C4 6.9 6.9 4 10.5 4 C14.1 4 17 6.9 17 10.5 Z M15.5 15.5 L20 20";
const I_LIBRARY: &str = "M4 4 V20 M8.5 4 V20 M13 5 L17.5 19.5";
const I_PLAY: &str = "M8 5 L19 12 L8 19 Z";
const I_PAUSE: &str = "M6.5 5 H10 V19 H6.5 Z M14 5 H17.5 V19 H14 Z";
const I_NEXT: &str = "M5.5 5.5 L15 12 L5.5 18.5 Z M16 5 H18.5 V19 H16 Z";
const I_PREV: &str = "M18.5 5.5 L9 12 L18.5 18.5 Z M5.5 5 H8 V19 H5.5 Z";
const I_SHUFFLE: &str = "M3 7 H7 L15 17 H20 M3 17 H7 L9.6 13.8 M14.4 10.2 L15 7 H20 M17 4 L20 7 L17 10 M17 14 L20 17 L17 20";
const I_REPEAT: &str = "M4 12 V10 C4 8 5.5 6.5 7.5 6.5 H18 M15 3.5 L18 6.5 L15 9.5 M20 12 V14 C20 16 18.5 17.5 16.5 17.5 H6 M9 20.5 L6 17.5 L9 14.5";
const I_HEART: &str = "M12 20 C12 20 3 14.5 3 8.8 C3 6 5.1 4 7.6 4 C9.4 4 11 5 12 6.6 C13 5 14.6 4 16.4 4 C18.9 4 21 6 21 8.8 C21 14.5 12 20 12 20 Z";
const I_VOLUME: &str = "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9 C16.8 10.5 16.8 13.5 15.5 15 M18 6.5 C20.8 9.5 20.8 14.5 18 17.5";
const I_BACK: &str = "M15 5 L8 12 L15 19";
const I_FORWARD: &str = "M9 5 L16 12 L9 19";
const I_RADIO: &str = "M8.8 8.8 C7 10.6 7 13.4 8.8 15.2 M15.2 8.8 C17 10.6 17 13.4 15.2 15.2 M5.8 5.8 C2.4 9.2 2.4 14.8 5.8 18.2 M18.2 5.8 C21.6 9.2 21.6 14.8 18.2 18.2 M12 10.6 C12.8 10.6 13.4 11.2 13.4 12 C13.4 12.8 12.8 13.4 12 13.4 C11.2 13.4 10.6 12.8 10.6 12 C10.6 11.2 11.2 10.6 12 10.6 Z";
const I_MORE: &str = "M5 10.5 H8 V13.5 H5 Z M10.5 10.5 H13.5 V13.5 H10.5 Z M16 10.5 H19 V13.5 H16 Z";
const I_LYRICS: &str = "M9.5 3.5 H14.5 V11.5 C14.5 14 9.5 14 9.5 11.5 Z M6 11 C6 18 18 18 18 11 M12 16.5 V21 M8.5 21 H15.5";
const I_SETTINGS: &str = "M12 9 C13.7 9 15 10.3 15 12 C15 13.7 13.7 15 12 15 C10.3 15 9 13.7 9 12 C9 10.3 10.3 9 12 9 Z M12 2.5 V5 M12 19 V21.5 M2.5 12 H5 M19 12 H21.5 M5.3 5.3 L7.1 7.1 M16.9 16.9 L18.7 18.7 M5.3 18.7 L7.1 16.9 M16.9 7.1 L18.7 5.3";
const I_PANEL: &str = "M3.5 5 H20.5 V19 H3.5 Z M14.5 5 V19";
const I_QUEUE: &str = "M4 6 H20 M4 11 H20 M4 16 H11 M15 14 V20 L20 17 Z";
const I_PENCIL: &str = "M5 19 L6 14.5 L15.5 5 L19 8.5 L9.5 18 Z M13.5 7 L17 10.5";
const I_PLUS: &str = "M12 5 V19 M5 12 H19";
const I_REFRESH: &str = "M19.5 12 A7.5 7.5 0 1 1 17 6.4 M17.5 2.5 V6.8 H13.2";
const I_MINI: &str = "M4 6 H20 V18 H4 Z M11 12 H18 V16 H11 Z";
const I_DEVICES: &str = "M3 5 H14 V15 H3 Z M6 19 H11 M8.5 15 V19 M17 7 H21 V19 H17 Z";
const I_CHECK: &str = "M6.5 12.5 L10.5 16.5 L17.5 8.5";
const I_FULLSCREEN: &str = "M4 9 V4 H9 M15 4 H20 V9 M20 15 V20 H15 M9 20 H4 V15";
const I_CLOCK: &str = "M12 3.5 C16.7 3.5 20.5 7.3 20.5 12 C20.5 16.7 16.7 20.5 12 20.5 C7.3 20.5 3.5 16.7 3.5 12 C3.5 7.3 7.3 3.5 12 3.5 Z M12 7.5 V12 L15 14";

// ---- State ----

/// Everything on screen, as one reactive value.
// ponytail: any update re-renders every reader of the model; split into channels (freya-radio) if profiling shows it.
#[derive(Default)]
struct Model {
    status: String,
    playing: bool,
    position_ms: u32,
    position_at: Option<Instant>,
    duration_ms: u32,
    now: Option<Item>,
    liked: bool,
    lists: Vec<Item>,
    tracks: Vec<Item>,
    quick: Vec<Item>,
    quick_top: Vec<Item>,
    quick_bottom: Vec<Item>,
    shelf: Vec<Item>,
    page_kind: &'static str,
    page_title: String,
    page_uri: String,
    page_info: String,
    page_cover: String,
    numbered: bool,
    playable: bool,
    can_back: bool,
    can_forward: bool,
    /// Bumped per page, so the page remounts (entry animation, scroll to top).
    page_gen: u32,
    current_list: String,
    current_track: String,
    volume: f32,
    shuffle: bool,
    repeat: i32,
    covers: HashMap<String, Bytes>,
    cover_order: VecDeque<String>,
    /// Search/artist page tabs and the chosen one.
    tabs: Vec<&'static str>,
    search_tab: i32,
    /// An artist's biography (About tab).
    about: String,
    /// An own playlist: rows can be removed from it.
    editable: bool,
    /// Own playlists, the targets of "Add to playlist".
    targets: Vec<Item>,
    credits: Option<(String, String)>,
    /// The volume before M muted it.
    muted_from: Option<f32>,
    quality: i32,
    normalize: bool,
    eq: EqState,
    low_api: bool,
    web_client_id: String,
    sleep_minutes: i32,
    /// A newer release to offer ("Update to vX"); empty when none.
    update_version: String,
    show_settings: bool,
    show_lyrics: bool,
    lyric_lines: Vec<String>,
    /// Start time per line; empty when the lyrics aren't synced.
    lyric_times: Vec<u32>,
    lyrics_note: String,
    /// The right panel's tab; `None` while it's closed.
    side_tab: Option<i32>,
    /// Queue or Recent rows, whichever tab asked last.
    side_rows: Vec<Item>,
    /// The playing artist's biography, first characters (Now playing tab).
    now_about: String,
    /// The playlist popup: `Some(true)` creates, `Some(false)` edits the open playlist.
    playlist_popup: Option<bool>,
    add_popup: bool,
    add_results: Vec<Item>,
    library_kind: i32,
    library_sort: i32,
    /// Find in page (Ctrl+F): open, filter text and sort; reset by a new page.
    find_open: bool,
    page_filter: String,
    page_sort: i32,
    /// Marked track rows (indexes into `tracks`) and the Shift anchor.
    selected: BTreeSet<usize>,
    anchor: usize,
    /// Rows whose liked mark is known; later ones are asked for while scrolling (`CheckLikedMore`).
    liked_checked: usize,
    liked_asked: usize,
    compact: bool,
    mini: bool,
    /// The window size before the mini player, restored when it closes.
    restore_size: Option<(f64, f64)>,
    devices_open: bool,
    devices: Vec<Item>,
    shortcuts_open: bool,
    /// Keyboard row cursor (Up/Down, Enter plays it).
    cursor: Option<usize>,
    /// A row to scroll into view ("Go to playing", the keyboard cursor).
    reveal: Option<usize>,
    /// The now-playing cover's average color, dimmed: the page header tint.
    tint: (u8, u8, u8),
    /// The open album/artist/podcast is in the library (header button).
    page_saved: bool,
    /// Full-screen lyrics: the window goes full screen with only lyrics and the player bar.
    lyrics_full: bool,
    /// Ctrl+B hides the sidebar.
    hide_sidebar: bool,
}

impl Model {
    /// Playback position now, interpolated between backend updates.
    fn position(&self) -> u32 {
        let elapsed = match (self.playing, self.position_at) {
            (true, Some(at)) => at.elapsed().as_millis() as u32,
            _ => 0,
        };
        (self.position_ms + elapsed).min(self.duration_ms.max(self.position_ms))
    }

    /// The encoded cover for `url`, asking the backend for it the first time.
    fn cover(&self, url: &str) -> Option<Bytes> {
        if url.is_empty() {
            return None;
        }
        let found = self.covers.get(url).cloned();
        if found.is_none() {
            request_cover(url);
        }
        found
    }

    fn apply(&mut self, update: UiUpdate) {
        match update {
            UiUpdate::Status(text) => self.status = text,
            UiUpdate::Playing(playing) => {
                self.position_ms = self.position();
                self.position_at = Some(Instant::now());
                self.playing = playing;
            }
            UiUpdate::Position(ms) => {
                self.position_ms = ms;
                self.position_at = Some(Instant::now());
            }
            UiUpdate::Rows(target, items) => match target {
                Rows::Tracks => {
                    self.tracks = items;
                    self.selected.clear();
                    self.cursor = None;
                }
                Rows::Lists => self.lists = items,
                Rows::Shelf => self.shelf = items,
                Rows::QuickTop => {
                    self.quick_top = items;
                    self.quick = interleave(&self.quick_top, &self.quick_bottom);
                }
                Rows::QuickBottom => {
                    self.quick_bottom = items;
                    self.quick = interleave(&self.quick_top, &self.quick_bottom);
                }
                Rows::Targets => self.targets = items,
                Rows::SideRows => self.side_rows = items,
                Rows::AddResults => self.add_results = items,
                Rows::Devices => self.devices = items,
            },
            UiUpdate::AppendTracks(items) => self.tracks.extend(items),
            UiUpdate::LikedRows(uris, liked) => {
                for row in self.tracks.iter_mut().filter(|r| uris.contains(&r.uri)) {
                    row.liked = liked;
                }
            }
            UiUpdate::PageHeader { kind, title, can_back, can_forward } => {
                if (kind, &title) != (self.page_kind, &self.page_title) {
                    self.page_gen += 1;
                    // Any page change closes the settings and lyrics and resets find.
                    self.show_settings = false;
                    self.show_lyrics = false;
                    if self.lyrics_full {
                        self.lyrics_full = false;
                        if let Some(id) = MAIN_WINDOW.get().copied() {
                            Platform::get().with_window(id, |w| w.set_fullscreen(None));
                        }
                    }
                    self.find_open = false;
                    self.page_filter.clear();
                    self.page_sort = 0;
                }
                self.page_kind = kind;
                self.page_title = title;
                self.can_back = can_back;
                self.can_forward = can_forward;
            }
            UiUpdate::Current(Current::List, uri) => self.current_list = uri,
            UiUpdate::Current(Current::Track, uri) => self.current_track = uri,
            UiUpdate::Page(p) => {
                self.page_info = p.info;
                self.page_uri = p.page_uri;
                self.page_cover = p.cover;
                self.numbered = p.numbered;
                self.playable = p.playable;
                self.tabs = p.tabs;
                self.search_tab = p.search_tab;
                self.about = p.about;
                self.editable = p.editable;
                self.page_saved = p.saved;
                self.liked_checked = p.liked_upto.max(0) as usize;
                self.liked_asked = self.liked_checked;
            }
            UiUpdate::LikedChecked(rows) => self.liked_checked = rows,
            UiUpdate::PageInfo(info) => self.page_info = info,
            UiUpdate::PageCover(url) => self.page_cover = url,
            UiUpdate::Settings { volume, shuffle, repeat, quality, normalize, .. } => {
                self.volume = volume;
                self.shuffle = shuffle;
                self.repeat = repeat;
                self.quality = quality;
                self.normalize = normalize;
            }
            UiUpdate::StartupSettings { low_api, eq, compact, .. } => {
                self.low_api = low_api;
                self.eq = eq;
                self.compact = compact;
            }
            UiUpdate::Reveal(index) => {
                self.reveal = Some(index);
                self.cursor = Some(index);
            }
            UiUpdate::NowTintDefault => self.tint = DEFAULT_TINT,
            UiUpdate::UpdateVersion(version) => self.update_version = version,
            UiUpdate::WebClientId(id) => self.web_client_id = id,
            // Dropped if another track has started meanwhile.
            UiUpdate::Lyrics(uri, result) => {
                if self.now.as_ref().is_some_and(|n| n.uri == uri) {
                    (self.lyric_lines, self.lyric_times, self.lyrics_note) = match result {
                        Ok(Some(l)) => (l.lines, l.times_ms.unwrap_or_default(), String::new()),
                        Ok(None) => (Vec::new(), Vec::new(), "No lyrics for this track".into()),
                        Err(e) => (Vec::new(), Vec::new(), e),
                    };
                }
            }
            UiUpdate::ClearLyrics(note) => {
                self.lyric_lines.clear();
                self.lyric_times.clear();
                self.lyrics_note = note;
            }
            UiUpdate::Now { item, duration, position } => {
                self.now = Some(item);
                self.duration_ms = duration as u32;
                if let Some(at) = position {
                    self.position_ms = at as u32;
                    self.position_at = Some(Instant::now());
                }
            }
            UiUpdate::Liked(liked) => self.liked = liked,
            UiUpdate::NowAbout(bio) => self.now_about = bio,
            // A library change (create, rename, remove) brings the full list back in library order.
            UiUpdate::ResetLibraryChips => {
                self.library_kind = 0;
                self.library_sort = 0;
            }
            UiUpdate::Credits { title, text } => self.credits = Some((title, text)),
            UiUpdate::Cover(url, bytes) => {
                if self.now.as_ref().is_some_and(|n| n.cover_url == url) {
                    if let Some(tint) = cover_tint(&bytes) {
                        self.tint = tint;
                    }
                }
                self.cover_order.push_back(url.clone());
                self.covers.insert(url, bytes);
                while self.cover_order.len() > MAX_COVERS {
                    if let Some(old) = self.cover_order.pop_front() {
                        self.covers.remove(&old);
                        REQUESTED.with_borrow_mut(|r| r.remove(&old));
                    }
                }
            }
            UiUpdate::Show => show_window(),
            UiUpdate::InvokeQuit => quit(),
            UiUpdate::QuitEventLoop => std::process::exit(0),
            // ponytail: lyrics, credits, settings, queue, page tabs and the update chip come with their screens (phase 2).
            UiUpdate::PageSaved(saved) => self.page_saved = saved,
            // Pages remount per page and start at the top already.
            UiUpdate::ScrollTop => {}
        }
    }
}

/// The average color of a cover, dimmed so white text stays readable on it.
fn cover_tint(encoded: &[u8]) -> Option<(u8, u8, u8)> {
    let img = image::load_from_memory(encoded).ok()?.to_rgb8();
    let (mut sum, n) = ([0u64; 3], (img.width() * img.height()).max(1) as u64);
    for px in img.pixels() {
        for c in 0..3 {
            sum[c] += px[c] as u64;
        }
    }
    let dim = |s: u64| ((s / n) as f32 * TINT_BRIGHTNESS) as u8;
    Some((dim(sum[0]), dim(sum[1]), dim(sum[2])))
}

/// Home's tiles in reading order: the backend fills a left and a right column.
fn interleave(left: &[Item], right: &[Item]) -> Vec<Item> {
    let mut out = Vec::with_capacity(left.len() + right.len());
    for i in 0..left.len().max(right.len()) {
        out.extend(left.get(i).cloned());
        out.extend(right.get(i).cloned());
    }
    out
}

thread_local! {
    /// Cover URLs already asked for (UI thread only); forgotten when evicted, so they can come back.
    static REQUESTED: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
}

fn request_cover(url: &str) {
    if REQUESTED.with_borrow_mut(|r| r.insert(url.to_string())) {
        if let Some(tx) = COVER_REQUESTS.get() {
            let _ = tx.send(url.to_string());
        }
    }
}

fn fmt_ms(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

// ---- Drawing helpers ----

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::from_rgb(c.0, c.1, c.2)
}

/// Mixes `a` toward `b` by `t` (0..1), for hover fades.
fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

fn icon(path: &str, size: f32, color: (u8, u8, u8), filled: bool) -> SvgViewer {
    let hex = format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2);
    let paint = if filled {
        format!(r##"fill="{hex}""##)
    } else {
        format!(r##"fill="none" stroke="{hex}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round""##)
    };
    let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{path}" {paint}/></svg>"##);
    SvgViewer::new(Bytes::from(svg.into_bytes())).width(Size::px(size)).height(Size::px(size))
}

/// A list's cover; Liked Songs and Your Episodes have none, so they get Spotify-like stand-ins.
fn list_cover(item: &Item, bytes: Option<Bytes>, size: f32, radius: f32) -> Rect {
    let stand_in = |bg: Fill, path: &str| rect().width(Size::px(size)).height(Size::px(size)).corner_radius(radius).background(bg).center().child(icon(path, size * 0.45, TEXT, path == I_HEART));
    match item.uri.as_str() {
        "liked" => stand_in(LinearGradient::new().angle(135.).stop(((69, 10, 245), 0.)).stop(((196, 239, 217), 100.)).into(), I_HEART),
        "episodes" => stand_in(rgb((0, 100, 80)).into(), I_RADIO),
        _ => cover(&item.cover_url, bytes, size, radius),
    }
}

fn cover(url: &str, bytes: Option<Bytes>, size: f32, radius: f32) -> Rect {
    rect()
        .width(Size::px(size))
        .height(Size::px(size))
        .corner_radius(radius)
        .overflow(Overflow::Clip)
        .background(rgb(ELEVATED))
        .maybe_child(bytes.map(|b| {
            ImageViewer::new((url.to_string(), b)).width(Size::px(size)).height(Size::px(size)).image_cover(ImageCover::Center)
        }))
}

fn one_line(text: impl Into<String>, size: f32, color: (u8, u8, u8)) -> Label {
    label().text(text.into()).font_size(size).color(rgb(color)).max_lines(1).text_overflow(TextOverflow::Ellipsis)
}

fn heading(text: impl Into<String>, size: f32) -> Label {
    one_line(text, size, TEXT).font_weight(FontWeight::BOLD)
}

fn use_hover() -> (UseAnimation<AnimNum>, f32) {
    let anim = use_animation(|_| AnimNum::new(0., 1.).time(HOVER_MS).ease(Ease::Out));
    let t = anim.get().value();
    (anim, t)
}

fn use_model() -> State<Model> {
    use_consume::<State<Model>>()
}

// ---- Components ----

#[derive(PartialEq)]
struct IconButton {
    path: &'static str,
    size: f32,
    active: bool,
    filled: bool,
    on_press: Option<fn()>,
}

impl Component for IconButton {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let color = if self.active { ACCENT } else { mix(SUB, TEXT, t) };
        let on_press = self.on_press;
        rect()
            .padding(6.)
            .corner_radius(8.)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| {
                if let Some(f) = on_press {
                    f();
                }
            })
            .child(icon(self.path, self.size, color, self.filled))
    }
}

#[derive(PartialEq)]
struct Chip {
    label: &'static str,
    active: bool,
    on_press: EventHandler<()>,
}

impl Component for Chip {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let (bg, fg) = if self.active { (TEXT, ON_ACCENT) } else { (mix(ELEVATED, HOVER, t), TEXT) };
        let on_press = self.on_press.clone();
        rect()
            .padding((6., 14.))
            .corner_radius(99.)
            .background(rgb(bg))
            .color(rgb(fg))
            .font_size(13.)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| on_press.call(()))
            .child(self.label)
    }
}

fn action(label: &'static str, name: &str, uri: &str) -> MenuButton {
    let (name, uri) = (name.to_string(), uri.to_string());
    MenuButton::new().on_press(move |_| send(Command::RowAction { action: name.clone(), uri: uri.clone() })).child(label)
}

/// The row menu (right-click or "..."): what applies depends on the row.
fn row_menu(item: &Item, sidebar: bool, editable: bool, targets: &[Item]) -> Menu {
    let uri = item.uri.as_str();
    let track = uri.starts_with("spotify:track:");
    let saveable = ["spotify:album:", "spotify:artist:", "spotify:show:"].iter().any(|p| uri.starts_with(p));
    let in_library = sidebar && (saveable || uri.starts_with("spotify:playlist:"));
    let radio = track || saveable || uri.starts_with("spotify:playlist:");
    let mut menu = Menu::new();
    if track {
        menu = menu.child(action("Add to queue", "queue", uri));
        if !targets.is_empty() {
            let lists: Vec<Element> = targets.iter().map(|t| action(leak(&t.title), &format!("add:{}", t.uri), uri).into_element()).collect();
            menu = menu.child(SubMenu::new().label("Add to playlist").children(lists));
        }
        menu = menu.child(action("Save to Liked Songs", "like", uri));
    } else {
        menu = menu.child(action("Open", "open", uri));
    }
    if radio {
        menu = menu.child(action("Start radio", "radio", uri));
    }
    if track {
        menu = menu.child(action("Go to artist", "artist", uri)).child(action("Go to album", "album", uri)).child(action("View credits", "credits", uri));
    }
    menu = menu.child(action("Copy link", "copy", uri));
    if track && editable && !sidebar {
        menu = menu.child(action("Remove from this playlist", "remove", uri));
    }
    if saveable && !sidebar {
        menu = menu.child(action("Save to Your Library", "save", uri));
    }
    if in_library {
        let (title, uri) = (item.title.clone(), uri.to_string());
        // Removing asks first; for an own playlist it is Spotify's delete.
        menu = menu.child(MenuButton::new().on_press(move |_| ContextMenu::open(confirm_remove(&title, &uri))).child("Remove from Your Library"));
    }
    menu
}

fn confirm_remove(title: &str, uri: &str) -> Menu {
    Menu::new()
        .child(rect().padding((8., 12.)).color(rgb(SUB)).font_size(12.).child(format!("Remove \"{title}\" from Your Library?")))
        .child(action("Remove", "unfollow", uri))
        .child(MenuButton::new().on_press(|_| ContextMenu::close()).child("Cancel"))
}

/// Menu labels must be `'static`; playlist names live as long as the menu is open, a handful per menu.
// ponytail: leaks each target's name once per open menu (a few bytes); intern them if menus get opened thousands of times.
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

#[derive(PartialEq)]
struct NavItem {
    path: &'static str,
    label: &'static str,
    active: bool,
    on_press: fn(),
}

impl Component for NavItem {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let on_press = self.on_press;
        let color = if self.active { TEXT } else { mix(SUB, TEXT, t) };
        rect()
            .horizontal()
            .width(Size::fill())
            .padding((10., 12.))
            .spacing(16.)
            .corner_radius(8.)
            .cross_align(Alignment::Center)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| on_press())
            .child(icon(self.path, 22., color, self.active && self.path == I_HOME))
            .child(rect().font_size(15.).font_weight(FontWeight::SEMI_BOLD).color(rgb(color)).child(self.label))
    }
}

#[derive(PartialEq)]
struct LibraryRow {
    item: Item,
    selected: bool,
    cover: Option<Bytes>,
    /// An own playlist or Liked Songs: dropped songs are added (or saved) there.
    drop_target: bool,
}

impl Component for LibraryRow {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let uri = self.item.uri.clone();
        let bg = if self.selected { HOVER } else { mix(SURFACE, ELEVATED, t) };
        let round = self.item.uri.starts_with("spotify:artist:");
        let row = rect()
            .horizontal()
            .width(Size::fill())
            .padding(8.)
            .spacing(12.)
            .corner_radius(8.)
            .background(rgb(bg))
            .cross_align(Alignment::Center)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(Command::OpenList(uri.clone())))
            .on_secondary_down({
                let item = self.item.clone();
                move |_| ContextMenu::open_from_down(row_menu(&item, true, false, &[]))
            })
            .child(list_cover(&self.item, self.cover.clone(), 48., if round { 24. } else { RADIUS_COVER }))
            .child(
                rect()
                    .width(Size::func(|p| Some(p.parent - 64.)))
                    .spacing(2.)
                    .child(one_line(self.item.title.clone(), 14., if self.selected { ACCENT } else { TEXT }))
                    .child(one_line(self.item.artist.clone(), 12., SUB)),
            );
        if !self.drop_target {
            return row.into_element();
        }
        let list = self.item.uri.clone();
        DropZone::new(move |index: usize| {
            let Some(model) = MODEL.get() else { return };
            let uris = model.peek().dragged_uris(index);
            if !uris.is_empty() {
                send(Command::AddUris { list: list.clone(), uris });
            }
        })
        .child(row)
        .into_element()
    }

    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.item.uri)
    }
}

/// Home quick-access tile: cover, name, and a play button that fades in on hover.
#[derive(PartialEq)]
struct QuickTile {
    item: Item,
    cover: Option<Bytes>,
}

impl Component for QuickTile {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let (open, play) = (self.item.uri.clone(), self.item.uri.clone());
        // Only the "New release · artist" tile has a subtitle, in `artist`.
        let subtitle = self.item.artist.starts_with("New release").then(|| one_line(self.item.artist.clone(), 12., ACCENT));
        rect()
            .horizontal()
            .width(Size::flex(1.))
            .height(Size::px(64.))
            .corner_radius(8.)
            .overflow(Overflow::Clip)
            .background(rgb(mix(ELEVATED, HOVER, t)))
            .cross_align(Alignment::Center)
            .content(Content::Flex)
            .spacing(14.)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(Command::OpenList(open.clone())))
            .child(list_cover(&self.item, self.cover.clone(), 64., 0.))
            .child(
                rect()
                    .width(Size::flex(1.))
                    .font_weight(FontWeight::SEMI_BOLD)
                    .child(one_line(self.item.title.clone(), 14., TEXT))
                    .maybe_child(subtitle),
            )
            .child(
                rect()
                    .width(Size::px(36.))
                    .height(Size::px(36.))
                    .corner_radius(18.)
                    .background(rgb(ACCENT))
                    .center()
                    .opacity(t)
                    .on_press(move |e: Event<PressEventData>| {
                        e.stop_propagation();
                        send(Command::PlayUri(play.clone()));
                    })
                    .child(icon(I_PLAY, 16., ON_ACCENT, true)),
            )
    }
}

/// Shelf card: highlights and shows a play button on hover.
#[derive(PartialEq)]
struct ShelfCard {
    item: Item,
    cover: Option<Bytes>,
}

impl Component for ShelfCard {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let uri = self.item.uri.clone();
        let url = crate::covers::larger_cover(&self.item.cover_url);
        rect()
            .width(Size::px(176.))
            .padding(12.)
            .spacing(10.)
            .corner_radius(10.)
            .background(rgb(mix(SURFACE, ELEVATED, t)))
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(Command::OpenList(uri.clone())))
            .child(
                cover(&url, self.cover.clone(), 152., 8.).child(
                    rect()
                        .position(Position::new_absolute().right(8.).bottom(8. + 8. * (1. - t)))
                        .width(Size::px(44.))
                        .height(Size::px(44.))
                        .corner_radius(22.)
                        .background(rgb(ACCENT))
                        .center()
                        .opacity(t)
                        .child(icon(I_PLAY, 18., ON_ACCENT, true)),
                ),
            )
            .child(rect().font_weight(FontWeight::SEMI_BOLD).child(one_line(self.item.title.clone(), 14., TEXT)))
            .child(one_line(self.item.artist.clone(), 12., SUB))
    }
}

#[derive(PartialEq)]
struct TrackRow {
    index: usize,
    item: Item,
    current: bool,
    playing: bool,
    numbered: bool,
    cover: Option<Bytes>,
    marked: bool,
    /// Rows of an own, unsorted, unfiltered playlist can be dragged into a new order.
    reorder: bool,
    compact: bool,
    /// The keyboard cursor is on this row.
    cursor: bool,
}

impl Component for TrackRow {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let mut hovered = use_state(|| false);
        let model = use_model();
        // "Show more" is hidden while filtering (the filter only covers the loaded rows).
        if self.item.uri == MORE_URI && !model.peek().page_filter.is_empty() {
            return rect().into_element();
        }
        if self.item.uri == MORE_URI {
            return rect()
                .width(Size::fill())
                .height(Size::px(ROW_H))
                .center()
                .child(
                    rect()
                        .padding((8., 20.))
                        .corner_radius(99.)
                        .background(rgb(mix(ELEVATED, HOVER, t)))
                        .font_size(13.)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .on_pointer_enter(move |_| anim.start())
                        .on_pointer_leave(move |_| anim.reverse())
                        .on_press(|_| send(Command::PlayUri(MORE_URI.to_string())))
                        .child("Show more"),
                )
                .into_element();
        }
        let open_menu = {
            let item = self.item.clone();
            move || {
                let m = model.read();
                row_menu(&item, false, m.editable, &m.targets)
            }
        };
        let menu_for_button = open_menu.clone();
        let round = self.item.uri.starts_with("spotify:artist:");
        let (uri, current) = (self.item.uri.clone(), self.current);
        let marker = if hovered() {
            icon(if current && self.playing { I_PAUSE } else { I_PLAY }, 14., TEXT, true).into_element()
        } else if current && self.playing {
            EqBars.into_element()
        } else if current {
            icon(I_VOLUME, 16., ACCENT, false).into_element()
        } else if self.numbered {
            rect().color(rgb(SUB)).font_size(14.).child(format!("{}", self.index + 1)).into_element()
        } else {
            rect().into_element()
        };
        let duration = if self.item.duration_ms > 0 { fmt_ms(self.item.duration_ms) } else { String::new() };
        let index = self.index;
        let bg = if self.marked { mix(ELEVATED, ACCENT, 0.18) } else { mix(SURFACE, ELEVATED, t) };
        let (row_h, compact) = (if self.compact { COMPACT_ROW_H } else { ROW_H }, self.compact);
        let title_line = if compact {
            // One line: "Title · Artist".
            rect().width(Size::flex(1.)).child(one_line(format!("{}  ·  {}", self.item.title, self.item.artist), 14., if current { ACCENT } else { TEXT }))
        } else {
            rect()
                .width(Size::flex(1.))
                .spacing(2.)
                .child(one_line(self.item.title.clone(), 15., if current { ACCENT } else { TEXT }))
                .child(one_line(self.item.artist.clone(), 13., SUB))
        };
        let row = rect()
            .horizontal()
            .width(Size::fill())
            .height(Size::px(row_h))
            .border(self.cursor.then(|| Border::new().fill(rgb(HAIRLINE)).width(1.).alignment(BorderAlignment::Inner)))
            .padding((0., 16.))
            .spacing(16.)
            .corner_radius(6.)
            .background(rgb(bg))
            .cross_align(Alignment::Center)
            .content(Content::Flex)
            .on_pointer_enter(move |_| {
                hovered.set(true);
                anim.start();
            })
            .on_pointer_leave(move |_| {
                hovered.set(false);
                anim.reverse();
            })
            // Ctrl/Shift mark rows; a plain click plays (the playing row pauses/resumes instead of restarting).
            .on_press(move |_| {
                let (ctrl, shift) = modifiers_held();
                if ctrl || shift {
                    if let Some(mut model) = MODEL.get() {
                        model.write().select(index, ctrl, shift);
                    }
                } else {
                    send(if current { Command::Toggle } else { Command::PlayUri(uri.clone()) });
                }
            })
            .on_secondary_down(move |_| ContextMenu::open_from_down(open_menu()))
            .child(rect().width(Size::px(NUMBER_W)).cross_align(Alignment::Center).child(marker))
            .maybe_child((!compact).then(|| cover(&self.item.cover_url, self.cover.clone(), 40., if round { 20. } else { 4. })))
            .child(title_line)
            .child(
                rect()
                    .width(Size::px(32.))
                    .opacity(if self.item.liked { 1. } else { 0. })
                    .child(icon(I_HEART, 16., ACCENT, true)),
            )
            .child(rect().width(Size::px(44.)).color(rgb(SUB)).font_size(13.).text_align(TextAlign::End).child(duration))
            .child(
                rect()
                    .width(Size::px(28.))
                    .opacity(t)
                    .on_press(move |e: Event<PressEventData>| {
                        e.stop_propagation();
                        ContextMenu::open(menu_for_button());
                    })
                    .child(icon(I_MORE, 18., SUB, true)),
            );
        let label = self.item.title.clone();
        let dragged = DragZone::new(index)
            .drag_element(rect().padding((8., 14.)).corner_radius(8.).background(rgb(ACCENT)).color(rgb(ON_ACCENT)).font_size(13.).child(label))
            .child(row);
        if !self.reorder {
            return dragged.into_element();
        }
        DropZone::new(move |from: usize| {
            if from != index {
                send(Command::MoveRow { from, to: index });
            }
        })
        .child(dragged)
        .into_element()
    }

    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.index)
    }
}

/// Three bars that bounce while the row's song plays.
#[derive(PartialEq)]
struct EqBars;

impl Component for EqBars {
    fn render(&self) -> impl IntoElement {
        let mut step = use_state(|| 0usize);
        use_future(move || async move {
            loop {
                tokio::time::sleep(EQ_BARS_TICK).await;
                *step.write() += 1;
            }
        });
        // Fixed height patterns per bar, out of phase, so it reads as music rather than a loop.
        const HEIGHTS: [[f32; 4]; 3] = [[6., 14., 9., 12.], [12., 5., 14., 8.], [9., 12., 5., 14.]];
        let i = step();
        rect()
            .horizontal()
            .height(Size::px(14.))
            .spacing(2.)
            .cross_align(Alignment::End)
            .children(HEIGHTS.iter().map(|h| rect().width(Size::px(3.)).height(Size::px(h[i % 4])).corner_radius(1.).background(rgb(ACCENT)).into_element()))
    }
}

#[derive(PartialEq)]
struct PlayFab {
    size: f32,
    playing: bool,
}

impl Component for PlayFab {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let s = self.size;
        let playing = self.playing;
        rect()
            .width(Size::px(s))
            .height(Size::px(s))
            .corner_radius(s / 2.)
            .background(rgb(mix(ACCENT, ACCENT_HOVER, t)))
            .scale((1. + 0.05 * t, 1. + 0.05 * t))
            .center()
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(if playing { Command::Toggle } else { Command::PlayPage }))
            .child(icon(if playing { I_PAUSE } else { I_PLAY }, s * 0.4, ON_ACCENT, true))
    }
}

// ---- Screens ----

#[derive(PartialEq)]
struct Sidebar;

impl Component for Sidebar {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let filter = use_state(String::new);
        let m = model.read();
        let home = m.page_kind == "Home";
        let nav = rect()
            .width(Size::fill())
            .padding(8.)
            .corner_radius(RADIUS_PANEL)
            .background(rgb(SURFACE))
            .child(NavItem { path: I_HOME, label: "Home", active: home, on_press: || send(Command::Home) })
            .child(NavItem { path: I_SEARCH, label: "Search", active: m.page_kind == "Search", on_press: focus_search });
        let rows: Vec<Element> = m
            .lists
            .iter()
            .map(|item| {
                let drop_target = item.uri == crate::web::LIKED_SONGS || m.targets.iter().any(|t| t.uri == item.uri);
                LibraryRow { item: item.clone(), selected: item.uri == m.current_list, cover: m.cover(&item.cover_url), drop_target }.into_element()
            })
            .collect();
        let (kind, sort) = (m.library_kind, m.library_sort);
        // Filtering happens in the backend on the loaded rows (no request), whenever a control changes.
        let mut sent = use_state(|| (0, String::new(), 0));
        let filter_key = (kind, filter.read().clone(), sort);
        if filter_key != *sent.peek() {
            send(Command::FilterLibrary { kind: filter_key.0, text: filter_key.1.clone(), sort: filter_key.2 });
            sent.set(filter_key);
        }
        // Wraps to a second line when the four chips don't fit the sidebar.
        let chips = rect().horizontal().width(Size::fill()).padding((0., 8.)).spacing(6.).content(Content::Wrap { wrap_spacing: Some(6.) }).children(LIBRARY_KINDS.iter().enumerate().map(move |(i, label)| {
            let k = i as i32 + 1;
            Chip {
                label,
                active: kind == k,
                on_press: EventHandler::new(move |_| {
                    if let Some(mut model) = MODEL.get() {
                        let mut m = model.write();
                        m.library_kind = if m.library_kind == k { 0 } else { k };
                    }
                }),
            }
            .into_element()
        }));
        let tools = rect()
            .horizontal()
            .width(Size::fill())
            .padding((0., 8.))
            .spacing(6.)
            .cross_align(Alignment::Center)
            .content(Content::Flex)
            .child(rect().width(Size::flex(1.)).child(Input::new(filter).placeholder("Search in Your Library").width(Size::fill())))
            .child(Chip {
                label: LIBRARY_SORTS[sort as usize],
                active: sort != 0,
                on_press: EventHandler::new(|_| {
                    if let Some(mut model) = MODEL.get() {
                        let mut m = model.write();
                        m.library_sort = (m.library_sort + 1) % LIBRARY_SORTS.len() as i32;
                    }
                }),
            });
        let library = rect()
            .width(Size::fill())
            .height(Size::func(|p| Some(p.parent - 128.)))
            .padding(8.)
            .spacing(GAP)
            .corner_radius(RADIUS_PANEL)
            .background(rgb(SURFACE))
            .child(
                rect()
                    .horizontal()
                    .padding((6., 8.))
                    .spacing(12.)
                    .cross_align(Alignment::Center)
                    .content(Content::Flex)
                    .child(icon(I_LIBRARY, 22., SUB, false))
                    .child(rect().width(Size::flex(1.)).font_size(15.).font_weight(FontWeight::SEMI_BOLD).color(rgb(TEXT)).child("Your Library"))
                    .child(IconButton { path: I_REFRESH, size: 18., active: false, filled: false, on_press: Some(|| send(Command::Refresh)) })
                    .child(IconButton { path: I_PLUS, size: 18., active: false, filled: false, on_press: Some(|| open_playlist_popup(true)) }),
            )
            .child(chips)
            .child(tools)
            .child(ScrollView::new().children(rows));
        rect().width(Size::px(SIDEBAR_W)).height(Size::fill()).spacing(GAP).child(nav).child(library)
    }
}

/// Back/forward, the search box and the status line, above every page.
#[derive(PartialEq)]
struct TopBar;

impl Component for TopBar {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let query = use_state(String::new);
        let m = model.read();
        rect()
            .horizontal()
            .width(Size::fill())
            .padding((12., 16.))
            .spacing(8.)
            .cross_align(Alignment::Center)
            .child(IconButton { path: I_BACK, size: 20., active: false, filled: false, on_press: m.can_back.then_some(|| send(Command::Back)) })
            .child(IconButton { path: I_FORWARD, size: 20., active: false, filled: false, on_press: m.can_forward.then_some(|| send(Command::Forward)) })
            .child(
                Input::new(query)
                    .a11y_id(use_consume::<SearchBox>().0)
                    .placeholder("What do you want to play?")
                    .width(Size::px(360.))
                    .on_submit(|q: String| {
                        if !q.trim().is_empty() {
                            send(Command::Submit(q));
                        }
                    }),
            )
            .child(rect().width(Size::func(|p| Some((p.parent - 640.).max(0.)))).padding((0., 8.)).child(one_line(m.status.clone(), 12., SUB)))
            .maybe_child((!m.update_version.is_empty()).then(|| {
                Chip {
                    label: leak(&format!("Update to v{}", m.update_version)),
                    active: true,
                    on_press: EventHandler::new(|_| {
                        if let Some(tx) = UPDATE_CLICKS.get() {
                            let _ = tx.send(());
                        }
                    }),
                }
            }))
            .child(IconButton { path: I_SETTINGS, size: 20., active: m.show_settings, filled: false, on_press: Some(toggle_settings) })
    }
}

thread_local! {
    /// The model handle for plain `fn()` handlers (icon buttons, shortcuts); set once by `app`.
    static MODEL: std::cell::Cell<Option<State<Model>>> = const { std::cell::Cell::new(None) };
}

fn toggle_settings() {
    if let Some(mut model) = MODEL.get() {
        let mut m = model.write();
        m.show_settings = !m.show_settings;
        m.show_lyrics = false;
    }
}

/// Whether Ctrl / Shift is held right now (pointer events carry no modifiers in Freya).
fn modifiers_held() -> (bool, bool) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
        // The high bit of GetKeyState is set while the key is down.
        let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
        (down(VK_CONTROL), down(VK_SHIFT))
    }
    #[cfg(not(windows))]
    (false, false)
}

impl Model {
    /// Ctrl toggles a row, Shift extends from the anchor, a plain mark selects one.
    fn select(&mut self, index: usize, ctrl: bool, shift: bool) {
        if index >= self.tracks.len() {
            return;
        }
        if shift {
            let from = self.anchor.min(self.tracks.len() - 1);
            self.selected = (from.min(index)..=from.max(index)).collect();
        } else {
            self.anchor = index;
            if ctrl {
                if !self.selected.remove(&index) {
                    self.selected.insert(index);
                }
            } else {
                self.selected = BTreeSet::from([index]);
            }
        }
    }

    /// Track URIs of the marked rows, in list order.
    fn selected_uris(&self) -> Vec<String> {
        self.selected.iter().filter_map(|&i| self.tracks.get(i)).filter(|t| t.uri.starts_with("spotify:track:")).map(|t| t.uri.clone()).collect()
    }

    /// The songs a drag from row `index` carries: the selection if the row is part of it, else that row.
    fn dragged_uris(&self, index: usize) -> Vec<String> {
        if self.selected.contains(&index) {
            self.selected_uris()
        } else {
            self.tracks.get(index).map(|t| vec![t.uri.clone()]).unwrap_or_default()
        }
    }
}

/// Copies `open.spotify.com` links of the selection (Ctrl+C, and Ctrl+X before removing).
fn copy_selection(model: &mut Model) -> Vec<String> {
    let uris = model.selected_uris();
    #[cfg(windows)]
    {
        let links: Vec<String> = uris.iter().filter_map(|u| crate::clipboard::web_link(u)).collect();
        if !links.is_empty() {
            model.status = match crate::clipboard::set_text(&links.join("\n")) {
                Ok(()) => format!("Copied {} link{}", links.len(), if links.len() == 1 { "" } else { "s" }),
                Err(e) => e,
            };
        }
    }
    uris
}

/// Adds song links from the clipboard to the open own playlist (Ctrl+V).
fn paste_links() {
    #[cfg(windows)]
    match crate::clipboard::get_text().map(|t| crate::clipboard::track_uris(&t)) {
        Ok(uris) if !uris.is_empty() => send(Command::AddUris { list: String::new(), uris }),
        Ok(_) => send(Command::Status("No song links on the clipboard".into())),
        Err(e) => send(Command::Status(e)),
    }
}

/// Opens the right panel on `tab`, or closes it when that tab is already showing. Queue and Recent
/// are fetched when their tab opens (Spirc keeps its queue private; see CLAUDE.md).
fn toggle_side(tab: i32) {
    let Some(mut model) = MODEL.get() else { return };
    let mut m = model.write();
    if m.side_tab == Some(tab) {
        m.side_tab = None;
        return;
    }
    m.side_tab = Some(tab);
    m.side_rows.clear();
    match tab {
        TAB_QUEUE => send(Command::Queue),
        TAB_RECENT => send(Command::Recent),
        _ => {}
    }
}

/// Mini player: the bar alone, small and always on top; the previous size comes back after.
fn toggle_mini() {
    let Some(mut model) = MODEL.get() else { return };
    let Some(id) = MAIN_WINDOW.get().copied() else { return };
    let mini = !model.peek().mini;
    let restore = model.peek().restore_size;
    model.write().mini = mini;
    Platform::get().with_window(id, move |w| {
        use freya::winit::dpi::LogicalSize;
        use freya::winit::window::WindowLevel;
        if mini {
            let size: LogicalSize<f64> = w.inner_size().to_logical(w.scale_factor());
            if let Some(mut model) = MODEL.get() {
                model.write().restore_size = Some((size.width, size.height));
            }
            w.set_window_level(WindowLevel::AlwaysOnTop);
            let _ = w.request_inner_size(LogicalSize::new(MINI_WIDTH, MINI_HEIGHT));
        } else {
            w.set_window_level(WindowLevel::Normal);
            let (width, height) = restore.unwrap_or((WINDOW_WIDTH, WINDOW_HEIGHT));
            let _ = w.request_inner_size(LogicalSize::new(width, height));
        }
    });
}

fn open_devices() {
    if let Some(mut model) = MODEL.get() {
        let mut m = model.write();
        m.devices_open = true;
        m.devices.clear();
    }
    send(Command::LoadDevices);
}

fn open_playlist_popup(creating: bool) {
    if let Some(mut model) = MODEL.get() {
        model.write().playlist_popup = Some(creating);
    }
}

fn open_add_popup() {
    if let Some(mut model) = MODEL.get() {
        let mut m = model.write();
        m.add_popup = true;
        m.add_results.clear();
    }
}

/// Native Open dialog on a thread of its own (it's modal and would stall the UI), then upload.
fn change_cover() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        if let Some(path) = crate::dialog::pick_image(MAIN_HWND.get().copied()) {
            send(Command::PlaylistCover(path));
        }
    });
}

/// Full-screen lyrics on or off; leaving lyrics also leaves full screen.
fn set_lyrics_full(on: bool) {
    let Some(mut model) = MODEL.get() else { return };
    {
        let mut m = model.write();
        if m.lyrics_full == on {
            return;
        }
        m.lyrics_full = on;
        if on {
            m.show_lyrics = true;
            m.show_settings = false;
        }
    }
    if let Some(id) = MAIN_WINDOW.get().copied() {
        Platform::get().with_window(id, move |w| w.set_fullscreen(on.then_some(freya::winit::window::Fullscreen::Borderless(None))));
    }
}

fn toggle_lyrics() {
    let leaving = MODEL.get().is_some_and(|m| m.peek().show_lyrics);
    if leaving {
        set_lyrics_full(false);
    }
    if let Some(mut model) = MODEL.get() {
        let mut m = model.write();
        m.show_lyrics = !m.show_lyrics;
        m.show_settings = false;
    }
}

#[derive(PartialEq)]
struct HomePage;

impl Component for HomePage {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let m = model.read();
        let tiles: Vec<Element> = m.quick.iter().map(|i| QuickTile { item: i.clone(), cover: m.cover(&i.cover_url) }.into_element()).collect();
        let mut tile_rows = Vec::new();
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            // Four per row; two while the right panel narrows the page, or the names don't fit.
            let row: Vec<Element> = tiles.by_ref().take(if m.side_tab.is_some() { 2 } else { 4 }).collect();
            tile_rows.push(rect().horizontal().width(Size::fill()).spacing(GAP).content(Content::Flex).children(row).into_element());
        }
        let shelf: Vec<Element> = m
            .shelf
            .iter()
            .map(|i| ShelfCard { item: i.clone(), cover: m.cover(&crate::covers::larger_cover(&i.cover_url)) }.into_element())
            .collect();
        let recent: Vec<Element> = m
            .tracks
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let current = item.uri == m.current_track;
                TrackRow { index, item: item.clone(), current, playing: m.playing, numbered: false, cover: m.cover(&item.cover_url), marked: m.selected.contains(&index), reorder: false, compact: false, cursor: false }.into_element()
            })
            .collect();
        ScrollView::new()
            .child(
                rect()
                    .width(Size::fill())
                    .padding((8., 24., 24., 24.))
                    .spacing(16.)
                    .background(LinearGradient::new().angle(180.).stop((m.tint, 0.)).stop((SURFACE, 100.)))
                    .child(heading(m.page_title.clone(), 28.))
                    .children(tile_rows),
            )
            .maybe_child((!shelf.is_empty()).then(|| {
                rect()
                    .padding((8., 24.))
                    .spacing(8.)
                    .child(heading("Recently played", 22.))
                    .child(ScrollView::new().direction(Direction::Horizontal).height(Size::px(240.)).child(rect().horizontal().children(shelf)))
            }))
            .maybe_child((!recent.is_empty()).then(|| rect().padding((8., 24., 24., 24.)).spacing(4.).child(heading("Recent tracks", 22.)).children(recent)))
    }
}

#[derive(PartialEq)]
struct ListPage;

impl Component for ListPage {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let find = use_state(String::new);
        let scroll = use_scroll_controller(ScrollConfig::default);
        let m = model.read();
        let header_cover = crate::covers::larger_cover(&m.page_cover);
        let has_cover = !header_cover.is_empty();
        let header = rect()
            .horizontal()
            .width(Size::fill())
            .padding((24., 24., 24., 24.))
            .spacing(24.)
            .cross_align(Alignment::End)
            .background(LinearGradient::new().angle(180.).stop((m.tint, 0.)).stop((mix(m.tint, SURFACE, 0.55), 100.)))
            .maybe_child(has_cover.then(|| cover(&header_cover, m.cover(&header_cover), 200., 8.).shadow((0., 12., 32., 0., (0, 0, 0, 160)))))
            .child(
                rect()
                    .width(Size::func(move |p| Some(p.parent - if has_cover { 224. } else { 0. })))
                    .spacing(6.)
                    .child(rect().font_size(13.).font_weight(FontWeight::SEMI_BOLD).color(rgb(TEXT)).child(m.page_kind))
                    .child(heading(m.page_title.clone(), 56.))
                    .maybe_child((!m.page_info.is_empty()).then(|| one_line(m.page_info.clone(), 13., SUB))),
            );
        let actions = rect()
            .horizontal()
            .width(Size::fill())
            .padding((20., 24.))
            .spacing(20.)
            .cross_align(Alignment::Center)
            .background(LinearGradient::new().angle(180.).stop((mix(m.tint, SURFACE, 0.55), 0.)).stop((SURFACE, 100.)))
            .maybe_child(m.playable.then(|| PlayFab { size: 56., playing: m.playing && !m.current_list.is_empty() && m.current_list == m.page_uri }))
            .child(IconButton { path: I_SHUFFLE, size: 26., active: m.shuffle, filled: false, on_press: Some(|| send(Command::ToggleShuffle)) })
            .maybe_child((m.page_kind == "Artist").then(|| {
                // Like Spotify: an outlined "Follow" pill on artists.
                let (uri, saved) = (m.page_uri.clone(), m.page_saved);
                rect()
                    .padding((6., 16.))
                    .corner_radius(99.)
                    .border(Border::new().fill(rgb(if saved { SUB } else { TEXT })).width(1.).alignment(BorderAlignment::Inner))
                    .font_size(13.)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .on_press(move |_| send(Command::RowAction { action: (if saved { "unfollow" } else { "save" }).into(), uri: uri.clone() }))
                    .child(if saved { "Following" } else { "Follow" })
            }))
            .maybe_child(matches!(m.page_kind, "Album" | "Podcast").then(|| {
                // ...and a round +/check on albums and podcasts.
                let (uri, saved) = (m.page_uri.clone(), m.page_saved);
                rect()
                    .width(Size::px(32.))
                    .height(Size::px(32.))
                    .corner_radius(16.)
                    .center()
                    .background(rgb(if saved { ACCENT } else { SURFACE }))
                    .border((!saved).then(|| Border::new().fill(rgb(SUB)).width(1.5).alignment(BorderAlignment::Inner)))
                    .on_press(move |_| send(Command::RowAction { action: (if saved { "unfollow" } else { "save" }).into(), uri: uri.clone() }))
                    .child(icon(if saved { I_CHECK } else { I_PLUS }, 18., if saved { ON_ACCENT } else { TEXT }, false))
            }))
            .maybe_child(["spotify:playlist:", "spotify:album:", "spotify:artist:"].iter().any(|p| m.page_uri.starts_with(p)).then(|| {
                let uri = m.page_uri.clone();
                rect()
                    .padding(6.)
                    .on_press(move |_| send(Command::RowAction { action: "radio".into(), uri: uri.clone() }))
                    .child(icon(I_RADIO, 24., SUB, false))
            }))
            .maybe_child((m.page_kind == "Album").then(|| {
                let uri = m.page_uri.clone();
                Chip { label: "More by this artist", active: false, on_press: EventHandler::new(move |_| send(Command::RowAction { action: "more-by".into(), uri: uri.clone() })) }
            }))
            .maybe_child((m.page_kind == "Radio").then(|| Chip { label: "Save as playlist", active: false, on_press: EventHandler::new(|_| send(Command::SaveRadio)) }))
            .maybe_child(m.editable.then(|| IconButton { path: I_PENCIL, size: 24., active: false, filled: false, on_press: Some(|| open_playlist_popup(false)) }))
            .maybe_child(m.editable.then(|| IconButton { path: I_PLUS, size: 24., active: false, filled: false, on_press: Some(open_add_popup) }));
        // Find in page (Ctrl+F): filter and sort run in the backend on the page's rows (no request).
        let (page_sort, find_open) = (m.page_sort, m.find_open);
        let typed = find.read().clone();
        let changed = typed != m.page_filter;
        drop(m);
        if changed {
            model.write().page_filter = typed.clone();
            send(Command::FilterPage { text: typed, sort: page_sort });
        }
        let find_bar = find_open.then(|| {
            rect()
                .horizontal()
                .padding((0., 24., 12., 24.))
                .spacing(GAP)
                .cross_align(Alignment::Center)
                .child(Input::new(find).placeholder("Find in page").width(Size::px(260.)).auto_focus(true))
                .child(Chip {
                    label: PAGE_SORTS[page_sort as usize],
                    active: page_sort != 0,
                    on_press: EventHandler::new(move |_| {
                        let mut model = model;
                        let (text, sort) = {
                            let mut m = model.write();
                            m.page_sort = (m.page_sort + 1) % PAGE_SORTS.len() as i32;
                            (m.page_filter.clone(), m.page_sort)
                        };
                        send(Command::FilterPage { text, sort });
                    }),
                })
        });
        let m = model.read();
        // An empty own playlist invites songs, like Spotify.
        let empty_own = (m.editable && m.tracks.is_empty()).then(|| {
            rect().padding((24., 24.)).child(Chip { label: "Let's find something for your playlist", active: true, on_press: EventHandler::new(|_| open_add_popup()) })
        });
        let columns = rect()
            .horizontal()
            .width(Size::fill())
            .padding((8., 40.))
            .spacing(16.)
            .content(Content::Flex)
            .color(rgb(SUB))
            .font_size(12.)
            .child(rect().width(Size::px(NUMBER_W)).cross_align(Alignment::Center).child(if m.numbered { "#" } else { "" }))
            .maybe_child((!m.compact).then(|| rect().width(Size::px(40.))))
            .child(rect().width(Size::flex(1.)).child("Title"))
            .child(rect().width(Size::px(32.)))
            .child(rect().width(Size::px(44.)).cross_align(Alignment::End).child(icon(I_CLOCK, 16., SUB, false)));
        let tabs = (!m.tabs.is_empty()).then(|| {
            rect().horizontal().padding((0., 24., 12., 24.)).spacing(GAP).children(m.tabs.iter().enumerate().map(|(i, label)| {
                Chip { label, active: m.search_tab == i as i32, on_press: EventHandler::new(move |_| send(Command::SearchTab(i as i32))) }.into_element()
            }))
        });
        // The artist About tab shows the biography instead of rows.
        let about = (!m.about.is_empty() && m.tracks.is_empty()).then(|| {
            ScrollView::new().child(paragraph().span(m.about.clone()).padding((8., 24.)).font_size(14.).color(rgb(SUB)).line_height(1.6).width(Size::fill()))
        });
        let len = m.tracks.len();
        let row_h = if m.compact { COMPACT_ROW_H } else { ROW_H };
        // "Go to playing" and the keyboard cursor bring their row into view, at the top of the list.
        if let Some(index) = m.reveal {
            let mut scroll = scroll;
            scroll.scroll_to_y(-((index as f32 * row_h) as i32));
        }
        let list = rect().width(Size::fill()).height(Size::fill()).padding((0., 24.)).child(
            VirtualScrollView::new(move |item, _| {
                let m = model.read();
                let Some(track) = m.tracks.get(item.index) else { return rect().into_element() };
                let current = track.uri == m.current_track;
                let ask_more = item.index + LIKED_AHEAD >= m.liked_checked && m.liked_checked < m.tracks.len() && m.liked_asked <= m.liked_checked;
                let checked = m.liked_checked;
                let row = TrackRow {
                    index: item.index,
                    item: track.clone(),
                    current,
                    playing: m.playing,
                    numbered: m.numbered,
                    // No cover is fetched in compact mode.
                    cover: if m.compact { None } else { m.cover(&track.cover_url) },
                    compact: m.compact,
                    cursor: m.cursor == Some(item.index),
                    marked: m.selected.contains(&item.index),
                    reorder: m.editable && m.page_sort == 0 && m.page_filter.is_empty(),
                }
                .into_element();
                drop(m);
                if ask_more {
                    // Once per batch: liked_asked moves past liked_checked until the answer raises it.
                    if let Some(mut model) = MODEL.get() {
                        model.write().liked_asked = checked + 1;
                    }
                    send(Command::CheckLikedMore);
                }
                row
            })
            .length(len)
            .item_size(row_h)
            .scroll_controller(scroll),
        );
        if m.reveal.is_some() {
            drop(m);
            model.write().reveal = None;
        }
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .child(header)
            .child(actions)
            .maybe_child(tabs)
            .maybe_child(find_bar)
            .maybe_child(empty_own)
            .maybe_child(about)
            .child(columns)
            .child(rect().width(Size::fill()).height(Size::px(1.)).margin((0., 24.)).background(rgb(HAIRLINE)))
            .child(list)
    }
}

fn section(title: &str) -> Label {
    heading(title.to_string(), 20.)
}

/// Label and description on the left, the control on the right.
fn setting_row(label: &str, desc: &str, control: impl IntoElement) -> Rect {
    rect()
        .horizontal()
        .width(Size::fill())
        .padding((10., 0.))
        .spacing(24.)
        .cross_align(Alignment::Center)
        .content(Content::Flex)
        .child(
            rect()
                .width(Size::flex(1.))
                .spacing(2.)
                .child(rect().font_size(14.).color(rgb(TEXT)).child(label.to_string()))
                .maybe_child((!desc.is_empty()).then(|| paragraph().span(desc.to_string()).font_size(12.).color(rgb(SUB)).width(Size::fill()))),
        )
        .child(control)
}

/// One EQ slider: vertical, -MAX_DB..=MAX_DB.
fn eq_slider(label: &'static str, db: f32, on_moved: impl Fn(f32) + 'static) -> Rect {
    let max = crate::eq::MAX_DB;
    rect()
        .width(Size::px(48.))
        .spacing(6.)
        .cross_align(Alignment::Center)
        .child(rect().font_size(11.).color(rgb(SUB)).child(format!("{db:+.0}")))
        .child(
            Slider::new(move |v: f64| on_moved(((v as f32 / 100.) * 2. * max - max).clamp(-max, max)))
                .direction(Direction::Vertical)
                // The wheel scrolls the settings page; it must not move bands on the way.
                .scroll_enabled(false)
                .size(Size::px(120.))
                .value(((db + max) / (2. * max) * 100.) as f64),
        )
        .child(rect().font_size(11.).color(rgb(SUB)).child(label))
}

#[derive(PartialEq)]
struct SettingsPage;

impl Component for SettingsPage {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let mut client_id = use_state(|| model.peek().web_client_id.clone());
        let mut eq_dirty = use_state(|| false);
        // The registry Run value is the source of truth.
        #[cfg(windows)]
        let mut autostart = use_state(crate::autostart::is_enabled);
        #[cfg(not(windows))]
        let mut autostart = use_state(|| false);
        let m = model.read();
        let choices = |labels: &[&'static str], active: i32, pick: fn(i32)| {
            rect().horizontal().spacing(GAP).children(labels.iter().enumerate().map(move |(i, label)| {
                Chip { label, active: i as i32 == active, on_press: EventHandler::new(move |_| pick(i as i32)) }.into_element()
            }))
        };
        // EQ changes apply at once and are saved on release.
        let set_eq = move |f: &dyn Fn(&mut EqState)| {
            // Copies of the handles, so this stays a plain `Fn` the sliders can share.
            let (mut model, mut eq_dirty) = (model, eq_dirty);
            let mut m = model.write();
            f(&mut m.eq);
            crate::eq::set(m.eq);
            eq_dirty.set(true);
        };
        let eq = m.eq;
        let bands = (0..EQ_BANDS.len()).map(|i| eq_slider(EQ_BANDS[i], eq.bands[i], move |db| set_eq(&|e: &mut EqState| e.bands[i] = db)).into_element());
        let presets = EQ_PRESETS.iter().map(|(name, bands)| {
            let bands = *bands;
            Chip {
                label: name,
                active: eq.bands.iter().zip(bands).all(|(a, b)| (a - b * crate::eq::MAX_DB).abs() < 0.05),
                on_press: EventHandler::new(move |_| {
                    let mut m = model.write();
                    m.eq.bands = bands.map(|b| b * crate::eq::MAX_DB);
                    crate::eq::set(m.eq);
                    send(Command::Eq(m.eq));
                }),
            }
            .into_element()
        });
        let sleep_minutes = m.sleep_minutes;
        ScrollView::new().child(
            rect()
                .width(Size::fill())
                .padding((8., 32., 32., 32.))
                .spacing(12.)
                .on_global_pointer_up(move |_| {
                    if eq_dirty() {
                        eq_dirty.set(false);
                        send(Command::Eq(model.peek().eq));
                    }
                })
                .child(heading("Settings", 32.))
                .child(section("Audio quality"))
                .child(setting_row("Streaming quality", "", choices(&QUALITIES, m.quality, |i| {
                    send(Command::Quality(Quality::from_ui(i as i64)));
                })))
                .child(setting_row(
                    "Normalize volume",
                    "Set the same volume level for all songs and podcasts",
                    Switch::new().toggled(m.normalize).on_toggle(move |_| {
                        let on = !model.peek().normalize;
                        model.write().normalize = on;
                        send(Command::Normalize(on));
                    }),
                ))
                .child(section("Playback"))
                .child(setting_row(
                    "Equalizer",
                    "Winamp's 10 bands, ±12 dB",
                    Switch::new().toggled(eq.on).on_toggle(move |_| {
                        let mut m = model.write();
                        m.eq.on = !m.eq.on;
                        crate::eq::set(m.eq);
                        send(Command::Eq(m.eq));
                    }),
                ))
                .child(rect().horizontal().spacing(GAP).children(presets))
                .child(
                    rect()
                        .horizontal()
                        .padding((8., 0.))
                        .spacing(4.)
                        .child(eq_slider("Preamp", eq.preamp, move |db| set_eq(&|e: &mut EqState| e.preamp = db)))
                        .child(rect().width(Size::px(16.)))
                        .children(bands),
                )
                .child(setting_row("Sleep timer", "Pauses playback after the chosen time", {
                    rect().horizontal().spacing(GAP).children(SLEEP_CHOICES.iter().map(move |(label, minutes)| {
                        let minutes = *minutes;
                        Chip {
                            label,
                            active: sleep_minutes == minutes,
                            on_press: EventHandler::new(move |_| {
                                model.write().sleep_minutes = minutes;
                                send(Command::SleepTimer(minutes));
                            }),
                        }
                        .into_element()
                    }))
                }))
                .child(section("Display"))
                .child(setting_row(
                    "Compact track list",
                    "One line per song, no covers (also saves memory)",
                    Switch::new().toggled(m.compact).on_toggle(move |_| {
                        let on = !model.peek().compact;
                        model.write().compact = on;
                        send(Command::Compact(on));
                    }),
                ))
                .child(section("Spotify API"))
                .child(setting_row(
                    "Low API usage",
                    "Fewer Spotify requests, for apps near their quota. When on: the library loads from a cache up to 6 hours old, liked marks are asked once per run, Home reuses Recently played for 60 s and checks new releases once a day. Changes made in other Spotify apps show up later; the refresh button reloads everything.",
                    Switch::new().toggled(m.low_api).on_toggle(move |_| {
                        let on = !model.peek().low_api;
                        model.write().low_api = on;
                        send(Command::LowApi(on));
                    }),
                ))
                .child(setting_row(
                    "Your own Spotify app",
                    "Client ID from developer.spotify.com (redirect URI http://127.0.0.1:8989/login). Empty uses the shared app.",
                    rect()
                        .horizontal()
                        .spacing(GAP)
                        .cross_align(Alignment::Center)
                        .child(Input::new(client_id).placeholder("Client ID").width(Size::px(300.)))
                        .child(Chip { label: "Save", active: false, on_press: EventHandler::new(move |_| send(Command::WebClientId(client_id.read().trim().to_string()))) }),
                ))
                .child(section("Startup"))
                .child(setting_row(
                    "Start with Windows",
                    "Starts in the tray",
                    Switch::new().toggled(autostart()).on_toggle(move |_| {
                        #[cfg(windows)]
                        {
                            if let Err(e) = crate::autostart::set(!autostart()) {
                                model.write().status = e;
                            }
                            // Show what Windows will actually do, even if the change failed.
                            autostart.set(crate::autostart::is_enabled());
                        }
                    }),
                ))
                .maybe_child((!m.update_version.is_empty()).then(|| {
                    rect().spacing(12.).child(section("Updates")).child(setting_row(&format!("Version {} is available", m.update_version), "Downloads, verifies the signature and restarts SlimSpot.", Chip {
                        label: "Update now",
                        active: true,
                        on_press: EventHandler::new(|_| {
                            if let Some(tx) = UPDATE_CLICKS.get() {
                                let _ = tx.send(());
                            }
                        }),
                    }))
                })),
        )
    }
}

/// Lyrics in place of the track list: the sung line is highlighted and kept in the middle;
/// clicking a synced line seeks there.
#[derive(PartialEq)]
struct LyricsView;

impl Component for LyricsView {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let mut tick = use_state(|| 0u64);
        use_future(move || async move {
            loop {
                tokio::time::sleep(POSITION_TICK).await;
                *tick.write() += 1;
            }
        });
        let _ = tick();
        let mut scroll = use_scroll_controller(ScrollConfig::default);
        let mut followed = use_state(|| None::<usize>);
        // Follows the sung line until the user scrolls; "Follow" turns it back on (Spotifast's).
        let mut following = use_state(|| true);
        let mut auto_y = use_state(|| 0i32);
        let m = model.read();
        let sung = crate::lyrics::current_line(&m.lyric_times, m.position());
        let (_, y): (i32, i32) = scroll.into();
        if following() && y != auto_y() && followed().is_some() {
            following.set(false);
        }
        if following() && (sung != followed() || y != auto_y()) {
            followed.set(sung);
            let target = sung.map_or(0, |i| -((i as f32 * LYRIC_LINE_H) as i32));
            scroll.scroll_to_y(target);
            let (_, now): (i32, i32) = scroll.into();
            auto_y.set(now);
        }
        let follow_chip = (!following()).then(|| {
            Chip {
                label: "Follow",
                active: true,
                on_press: EventHandler::new(move |_| {
                    following.set(true);
                    followed.set(None);
                }),
            }
        });
        let full = m.lyrics_full;
        let controls = rect()
            .position(Position::new_absolute().top(16.).right(24.))
            .horizontal()
            .spacing(GAP)
            .cross_align(Alignment::Center)
            .maybe_child(follow_chip)
            .child(Chip { label: if full { "Exit full screen" } else { "Full screen" }, active: false, on_press: EventHandler::new(move |_| set_lyrics_full(!full)) });
        let synced = !m.lyric_times.is_empty();
        let lines = m.lyric_lines.iter().enumerate().map(|(i, line)| {
            let start = m.lyric_times.get(i).copied();
            let color = match sung {
                Some(s) if s == i => TEXT,
                Some(s) if i < s => (110, 110, 118),
                _ => SUB,
            };
            rect()
                .width(Size::fill())
                .height(Size::px(LYRIC_LINE_H))
                .main_align(Alignment::Center)
                .on_press(move |_| {
                    if let Some(ms) = start {
                        send(Command::Seek(ms));
                    }
                })
                .child(one_line(if line.is_empty() { "♪".to_string() } else { line.clone() }, 26., color).font_weight(FontWeight::BOLD))
                .into_element()
        });
        let note_text = if m.lyrics_note.is_empty() { "Lyrics load when a song starts playing".to_string() } else { m.lyrics_note.clone() };
        let note = m.lyric_lines.is_empty().then(|| rect().padding(32.).child(one_line(note_text, 18., SUB)));
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(LinearGradient::new().angle(180.).stop(((52, 40, 80), 0.)).stop((SURFACE, 100.)))
            .maybe_child(note)
            .child(
                ScrollView::new_controlled(scroll).child(
                    rect()
                        .width(Size::fill())
                        .padding((LYRICS_TOP, 40., 400., 40.))
                        .children(lines)
                        .maybe_child((!synced && !m.lyric_lines.is_empty()).then(|| one_line("These lyrics aren't synced", 12., SUB))),
                ),
            )
            // After the lyrics, so the chips sit on top and take their own clicks.
            .child(controls)
    }
}

/// The main panel; remounted per page so it starts at the top and fades in.
#[derive(PartialEq)]
struct MainPanel {
    page_gen: u32,
    home: bool,
}

impl Component for MainPanel {
    fn render(&self) -> impl IntoElement {
        let anim = use_animation(|conf| {
            conf.on_creation(OnCreation::Run);
            (AnimNum::new(0., 1.).time(PAGE_ANIM_MS).ease(Ease::Out), AnimNum::new(12., 0.).time(PAGE_ANIM_MS).ease(Ease::Out))
        });
        let (opacity, offset) = anim.get().value();
        let page = if self.home { HomePage.into_element() } else { ListPage.into_element() };
        rect().width(Size::fill()).height(Size::fill()).opacity(opacity).offset_y(offset).child(page)
    }

    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.page_gen)
    }
}

#[derive(PartialEq)]
struct PlayerBar;

impl Component for PlayerBar {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let mut tick = use_state(|| 0u64);
        // Redraws the progress between backend position updates.
        use_future(move || async move {
            loop {
                tokio::time::sleep(POSITION_TICK).await;
                *tick.write() += 1;
                #[cfg(windows)]
                {
                    let m = model.peek();
                    crate::taskbar::update(m.playing, m.position() as u64, m.duration_ms as u64);
                }
            }
        });
        let _ = tick();
        let mut seeking = use_state(|| None::<f64>);
        let mut volume_drag = use_state(|| None::<f64>);
        let m = model.read();
        let now = m.now.clone().unwrap_or_default();
        let (position, duration) = (m.position(), m.duration_ms);
        let percent = seeking().unwrap_or(if duration > 0 { position as f64 * 100. / duration as f64 } else { 0. });
        let volume = volume_drag().unwrap_or(m.volume as f64);

        let left = rect()
            .horizontal()
            .width(Size::px(320.))
            .spacing(14.)
            .cross_align(Alignment::Center)
            .child(cover(&now.cover_url, m.cover(&now.cover_url), 56., RADIUS_COVER))
            .child(
                rect()
                    .width(Size::px(190.))
                    .spacing(2.)
                    .on_press(|_| send(Command::GoToPlaying))
                    .child(rect().font_weight(FontWeight::SEMI_BOLD).child(one_line(now.title.clone(), 14., TEXT)))
                    .child(one_line(now.artist.clone(), 12., SUB)),
            )
            .maybe_child((!now.uri.is_empty()).then(|| IconButton { path: I_HEART, size: 18., active: m.liked, filled: m.liked, on_press: Some(|| send(Command::ToggleLike)) }));

        let progress = rect()
            .horizontal()
            .width(Size::fill())
            .spacing(10.)
            .cross_align(Alignment::Center)
            .font_size(11.)
            .color(rgb(SUB))
            .child(rect().width(Size::px(36.)).text_align(TextAlign::End).child(fmt_ms((percent / 100. * duration as f64) as u32)))
            .child(
                rect()
                    .width(Size::func(|p| Some(p.parent - 92.)))
                    // Spotify gets one seek on release, not one per drag step.
                    .on_global_pointer_up(move |_| {
                        if let Some(p) = seeking() {
                            send(Command::Seek((p / 100. * duration as f64) as u32));
                            seeking.set(None);
                        }
                    })
                    .child(
                        Slider::new(move |p: f64| seeking.set(Some(p)))
                            .value(percent)
                            .size(Size::fill())
                            .background(rgb(HAIRLINE))
                            .thumb_background(rgb(TEXT))
                            .thumb_inner_background(rgb(TEXT))
                            .border_fill(rgb(TEXT)),
                    ),
            )
            .child(rect().width(Size::px(36.)).child(fmt_ms(duration)));

        let center = rect()
            .width(Size::func(|p| Some(p.parent - 640.)))
            .spacing(6.)
            .cross_align(Alignment::Center)
            .child(
                rect()
                    .horizontal()
                    .spacing(18.)
                    .cross_align(Alignment::Center)
                    .child(IconButton { path: I_SHUFFLE, size: 18., active: m.shuffle, filled: false, on_press: Some(|| send(Command::ToggleShuffle)) })
                    .child(IconButton { path: I_PREV, size: 18., active: false, filled: true, on_press: Some(|| send(Command::Prev)) })
                    .child(
                        rect()
                            .width(Size::px(36.))
                            .height(Size::px(36.))
                            .corner_radius(18.)
                            .background(rgb(TEXT))
                            .center()
                            .on_press(|_| send(Command::Toggle))
                            .child(icon(if m.playing { I_PAUSE } else { I_PLAY }, 16., ON_ACCENT, true)),
                    )
                    .child(IconButton { path: I_NEXT, size: 18., active: false, filled: true, on_press: Some(|| send(Command::Next)) })
                    .child(IconButton { path: I_REPEAT, size: 18., active: m.repeat != 0, filled: false, on_press: Some(|| send(Command::CycleRepeat)) }),
            )
            .child(progress);

        let right = rect()
            .horizontal()
            .width(Size::px(320.))
            .spacing(4.)
            .main_align(Alignment::End)
            .cross_align(Alignment::Center)
            .child(IconButton { path: I_PANEL, size: 18., active: m.side_tab == Some(TAB_NOW), filled: false, on_press: Some(|| toggle_side(TAB_NOW)) })
            .child(IconButton { path: I_QUEUE, size: 18., active: m.side_tab == Some(TAB_QUEUE), filled: false, on_press: Some(|| toggle_side(TAB_QUEUE)) })
            .child(IconButton { path: I_LYRICS, size: 18., active: m.show_lyrics, filled: false, on_press: Some(toggle_lyrics) })
            .child(IconButton { path: I_DEVICES, size: 18., active: m.devices_open, filled: false, on_press: Some(open_devices) })
            .child(IconButton { path: I_MINI, size: 18., active: m.mini, filled: false, on_press: Some(toggle_mini) })
            .child(icon(I_VOLUME, 18., SUB, false))
            .child(
                rect()
                    .width(Size::px(120.))
                    // Dragging sets the local mixer only; Spotify gets the final value once (see CLAUDE.md).
                    .on_global_pointer_up(move |_| {
                        if let Some(v) = volume_drag() {
                            send(Command::Volume { percent: v as f32, save: true });
                            volume_drag.set(None);
                        }
                    })
                    .child(
                        Slider::new(move |v: f64| {
                            volume_drag.set(Some(v));
                            send(Command::Volume { percent: v as f32, save: false });
                        })
                        .value(volume)
                        .size(Size::fill())
                        .background(rgb(HAIRLINE))
                        .thumb_background(rgb(TEXT))
                        .thumb_inner_background(rgb(TEXT))
                        .border_fill(rgb(TEXT)),
                    ),
            );

        rect()
            .horizontal()
            .width(Size::fill())
            .height(Size::px(PLAYER_H))
            .padding((0., 16.))
            .cross_align(Alignment::Center)
            .background(rgb(BASE))
            .child(left)
            .child(center)
            .child(right)
    }
}

/// Freya's dark component theme (inputs, menus, sliders, popups) in this app's colors.
fn theme() -> freya::prelude::Theme {
    let mut theme = dark_theme();
    let c = &mut theme.colors;
    c.primary = rgb(ACCENT);
    c.background = rgb(BASE);
    c.surface_primary = rgb(SURFACE);
    c.surface_secondary = rgb(ELEVATED);
    c.surface_tertiary = rgb(ELEVATED);
    c.border = rgb(HAIRLINE);
    c.border_focus = rgb(SUB);
    c.text_primary = rgb(TEXT);
    c.text_secondary = rgb(SUB);
    c.text_placeholder = rgb(SUB);
    theme
}

/// The search box's id, so shortcuts and the sidebar can focus it and know when the user is typing.
#[derive(Clone, Copy)]
struct SearchBox(AccessibilityId);

static SEARCH_BOX: OnceLock<AccessibilityId> = OnceLock::new();

fn focus_search() {
    if let Some(id) = SEARCH_BOX.get() {
        id.request_focus();
    }
}

/// Global shortcuts (see CLAUDE.md, Keyboard). Unmodified keys only act
/// while no text field has focus.
fn on_key(e: &KeyboardEventData, model: State<Model>, search: AccessibilityId) {
    let ctrl = e.modifiers.contains(Modifiers::CONTROL);
    let shift = e.modifiers.contains(Modifiers::SHIFT);
    let alt = e.modifiers.contains(Modifiers::ALT);
    let typing = search.is_focused();
    let (position, duration, current, muted_from, has_now) = {
        let m = model.peek();
        (m.position() as i64, m.duration_ms as i64, m.volume, m.muted_from, m.now.is_some())
    };
    let seek = |delta: i64| send(Command::Seek((position + delta).clamp(0, duration) as u32));
    let volume = |v: f32| send(Command::Volume { percent: v.clamp(0., 100.), save: true });
    match &e.key {
        Key::Named(NamedKey::Escape) => {
            search.request_unfocus();
            set_lyrics_full(false);
            let mut model = model;
            let mut m = model.write();
            m.show_settings = false;
            m.show_lyrics = false;
        }
        Key::Named(NamedKey::ArrowRight) if ctrl => send(Command::Next),
        Key::Named(NamedKey::ArrowLeft) if ctrl => send(Command::Prev),
        Key::Named(NamedKey::ArrowUp) if ctrl => volume(current + VOLUME_STEP),
        Key::Named(NamedKey::ArrowDown) if ctrl => volume(current - VOLUME_STEP),
        Key::Named(NamedKey::ArrowRight) if shift && !typing => seek(SEEK_STEP_MS as i64),
        Key::Named(NamedKey::ArrowLeft) if shift && !typing => seek(-(SEEK_STEP_MS as i64)),
        Key::Named(NamedKey::ArrowLeft) if alt => send(Command::Back),
        Key::Named(NamedKey::ArrowDown | NamedKey::ArrowUp) if !typing && !ctrl && !alt => {
            let mut model = model;
            let mut m = model.write();
            if m.tracks.is_empty() {
                return;
            }
            let down = e.key == Key::Named(NamedKey::ArrowDown);
            let at = match (m.cursor, down) {
                (None, _) => 0,
                (Some(i), true) => (i + 1).min(m.tracks.len() - 1),
                (Some(i), false) => i.saturating_sub(1),
            };
            m.cursor = Some(at);
            m.select(at, false, shift);
            m.reveal = Some(at.saturating_sub(3));
        }
        Key::Named(NamedKey::Enter) if !typing => {
            let uri = { let m = model.peek(); m.cursor.and_then(|i| m.tracks.get(i)).map(|t| t.uri.clone()) };
            if let Some(uri) = uri {
                send(Command::PlayUri(uri));
            }
        }
        Key::Named(NamedKey::Delete) if !typing => {
            let uris = model.peek().selected_uris();
            if !uris.is_empty() {
                send(Command::RemoveUris(uris));
            }
        }
        Key::Named(NamedKey::ArrowRight) if alt => send(Command::Forward),
        Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
            "q" => quit(),
            "s" => send(Command::ToggleShuffle),
            "r" => send(Command::CycleRepeat),
            "h" => send(Command::Home),
            // Ctrl+F finds in pages with tracks; Ctrl+L stays the global search.
            "f" if model.peek().playable => {
                let mut model = model;
                let mut m = model.write();
                m.find_open = !m.find_open;
            }
            "f" | "l" => search.request_focus(),
            "a" if !typing => {
                let mut model = model;
                let mut m = model.write();
                m.selected = (0..m.tracks.len()).filter(|&i| m.tracks[i].uri.starts_with("spotify:track:")).collect();
            }
            "c" if !typing => {
                let mut model = model;
                copy_selection(&mut model.write());
            }
            "x" if !typing => {
                let mut model = model;
                let uris = copy_selection(&mut model.write());
                if !uris.is_empty() {
                    send(Command::RemoveUris(uris));
                }
            }
            "v" if !typing => paste_links(),
            "," => toggle_settings(),
            "y" => toggle_lyrics(),
            "b" => {
                let mut model = model;
                let mut m = model.write();
                m.hide_sidebar = !m.hide_sidebar;
            }
            "u" => toggle_side(TAB_QUEUE),
            _ => {}
        },
        Key::Character(c) if !typing && !alt => match c.as_str() {
            " " => send(Command::Toggle),
            "/" => search.request_focus(),
            "?" => {
                let mut model = model;
                model.write().shortcuts_open = true;
            }
            "b" if has_now => send(Command::ToggleLike),
            "q" => toggle_side(TAB_QUEUE),
            "m" => {
                let mut model = model;
                model.write().muted_from = if muted_from.is_some() { None } else { Some(current) };
                volume(muted_from.unwrap_or(0.));
            }
            _ => {}
        },
        _ => {}
    }
}

fn app() -> impl IntoElement {
    use_init_theme(theme);
    let search = use_provide_context(|| {
        let id = AccessibilityId::new_unique();
        let _ = SEARCH_BOX.set(id);
        SearchBox(id)
    });
    let mut model = use_provide_context(|| State::create(Model { volume: 100., ..Default::default() }));
    MODEL.set(Some(model));
    use_future(move || async move {
        let Some(mut rx) = UPDATES.lock().unwrap().take() else { return };
        let mut overlay = (String::new(), false);
        while let Some(update) = rx.recv().await {
            let mut m = model.write();
            m.apply(update);
            // Apply whatever else is queued in the same frame.
            while let Ok(update) = rx.try_recv() {
                m.apply(update);
            }
            // The Windows media overlay follows track and play state.
            #[cfg(windows)]
            if let Some(now) = &m.now {
                if (now.uri.as_str(), m.playing) != (overlay.0.as_str(), overlay.1) {
                    overlay = (now.uri.clone(), m.playing);
                    crate::media_keys::now_playing(&now.title, &now.artist, &crate::covers::larger_cover(&now.cover_url), m.playing);
                }
            }
        }
    });
    let (page_gen, home, show_settings, show_lyrics, side_tab, playlist_popup, add_popup, devices_open, shortcuts_open, mini, lyrics_full, hide_sidebar) = {
        let m = model.read();
        (m.page_gen, m.page_kind == "Home", m.show_settings, m.show_lyrics, m.side_tab, m.playlist_popup, m.add_popup, m.devices_open, m.shortcuts_open, m.mini, m.lyrics_full, m.hide_sidebar)
    };
    let main: Element = if show_settings {
        SettingsPage.into_element()
    } else if show_lyrics {
        LyricsView.into_element()
    } else {
        MainPanel { page_gen, home }.into_element()
    };
    rect()
        .width(Size::fill())
        .height(Size::fill())
        .background(rgb(BASE))
        .color(rgb(TEXT))
        .on_global_key_down(move |e: Event<KeyboardEventData>| on_key(&e, model, search.0))
        .child(ContextMenuViewer::new())
        .maybe_child((!mini).then(|| {
            rect()
                .horizontal()
                .width(Size::fill())
                .height(Size::func(|p| Some(p.parent - PLAYER_H)))
                .padding((GAP, GAP, 0., GAP))
                .spacing(GAP)
                .content(Content::Flex)
                .maybe_child((!lyrics_full && !hide_sidebar).then_some(Sidebar))
                .child(
                    rect()
                        .width(Size::flex(1.))
                        .height(Size::fill())
                        .corner_radius(RADIUS_PANEL)
                        .overflow(Overflow::Clip)
                        .background(rgb(SURFACE))
                        .child(TopBar)
                        .child(main),
                )
                .maybe_child(side_tab.filter(|_| !lyrics_full).map(|tab| SidePanel { tab }))
        }))
        .child(PlayerBar)
        .child(CreditsPopup)
        .maybe_child(playlist_popup.map(|creating| PlaylistPopup { creating }))
        .maybe_child(add_popup.then_some(AddSongsPopup))
        .maybe_child(devices_open.then_some(DevicesPopup))
        .maybe_child(shortcuts_open.then_some(ShortcutsPopup))
}

/// The right panel: Now playing (big cover, lyrics preview, artist bio), Queue and Recent.
#[derive(PartialEq)]
struct SidePanel {
    tab: i32,
}

impl Component for SidePanel {
    fn render(&self) -> impl IntoElement {
        let model = use_model();
        let mut tick = use_state(|| 0u64);
        // The lyrics preview follows the playing position.
        use_future(move || async move {
            loop {
                tokio::time::sleep(POSITION_TICK).await;
                *tick.write() += 1;
            }
        });
        let _ = tick();
        let m = model.read();
        let tabs = rect().horizontal().spacing(GAP).padding((0., 0., 12., 0.)).children(SIDE_TABS.iter().enumerate().map(|(i, label)| {
            Chip { label, active: self.tab == i as i32, on_press: EventHandler::new(move |_| toggle_side(i as i32)) }.into_element()
        }));
        let body: Element = if self.tab == TAB_NOW {
            let now = m.now.clone().unwrap_or_default();
            let big = crate::covers::larger_cover(&now.cover_url);
            let sung = crate::lyrics::current_line(&m.lyric_times, m.position());
            let preview: Vec<Element> = match sung {
                Some(i) => m.lyric_lines.iter().skip(i).take(3).enumerate().map(|(k, line)| {
                    paragraph().span(line.clone()).font_size(15.).font_weight(FontWeight::BOLD).color(rgb(if k == 0 { TEXT } else { SUB })).width(Size::fill()).into_element()
                }).collect(),
                None => Vec::new(),
            };
            ScrollView::new()
                .child(
                    rect()
                        .width(Size::fill())
                        .spacing(10.)
                        .child(cover(&big, m.cover(&big), SIDE_PANEL_W - 32., 8.))
                        .child(paragraph().span(now.title.clone()).font_size(22.).font_weight(FontWeight::BOLD).color(rgb(TEXT)).width(Size::fill()))
                        .child(paragraph().span(now.artist.clone()).font_size(14.).color(rgb(SUB)).width(Size::fill()))
                        .maybe_child((!preview.is_empty()).then(|| {
                            rect().width(Size::fill()).padding(14.).spacing(4.).corner_radius(10.).background(rgb(ELEVATED)).child(one_line("Lyrics", 12., SUB)).children(preview)
                        }))
                        .maybe_child((!m.now_about.is_empty()).then(|| {
                            rect()
                                .width(Size::fill())
                                .padding(14.)
                                .spacing(4.)
                                .corner_radius(10.)
                                .background(rgb(ELEVATED))
                                .child(one_line("About the artist", 12., SUB))
                                .child(paragraph().span(format!("{}...", m.now_about)).font_size(12.).color(rgb(SUB)).width(Size::fill()))
                        })),
                )
                .into_element()
        } else {
            let queue = self.tab == TAB_QUEUE;
            let heading_text = if !queue { "Recently played" } else if m.side_rows.is_empty() { "Nothing queued" } else { "Next up" };
            let rows: Vec<Element> = m
                .side_rows
                .iter()
                .map(|item| SideRow { item: item.clone(), queue, cover: m.cover(&item.cover_url) }.into_element())
                .collect();
            rect().width(Size::fill()).height(Size::fill()).spacing(8.).child(one_line(heading_text, 13., SUB)).child(ScrollView::new().children(rows)).into_element()
        };
        rect()
            .width(Size::px(SIDE_PANEL_W))
            .height(Size::fill())
            .padding(16.)
            .corner_radius(RADIUS_PANEL)
            .background(rgb(SURFACE))
            .child(tabs)
            .child(body)
    }
}

/// A Queue or Recent row: a queue row skips ahead to it, a recent row plays that track alone.
#[derive(PartialEq)]
struct SideRow {
    item: Item,
    queue: bool,
    cover: Option<Bytes>,
}

impl Component for SideRow {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let uri = self.item.uri.clone();
        let queue = self.queue;
        rect()
            .horizontal()
            .width(Size::fill())
            .padding(6.)
            .spacing(10.)
            .corner_radius(6.)
            .background(rgb(mix(SURFACE, ELEVATED, t)))
            .cross_align(Alignment::Center)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(if queue { Command::SkipTo(uri.clone()) } else { Command::PlayUri(uri.clone()) }))
            .child(cover(&self.item.cover_url, self.cover.clone(), 40., 4.))
            .child(
                rect()
                    .width(Size::func(|p| Some(p.parent - 56.)))
                    .spacing(2.)
                    .child(one_line(self.item.title.clone(), 13., TEXT))
                    .child(one_line(self.item.artist.clone(), 12., SUB)),
            )
    }
}

/// "+" (create, nothing is created on cancel) and the pencil on own playlists (edit, cover).
#[derive(PartialEq)]
struct PlaylistPopup {
    creating: bool,
}

impl Component for PlaylistPopup {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let creating = self.creating;
        let name = use_state(|| {
            let m = model.peek();
            if creating { format!("My Playlist #{}", m.targets.len() + 1) } else { m.page_title.clone() }
        });
        let description = use_state(|| if creating { String::new() } else { model.peek().page_info.clone() });
        let close = move || {
            let mut model = model;
            model.write().playlist_popup = None;
        };
        let save = move || {
            let (n, d) = (name.read().trim().to_string(), description.read().trim().to_string());
            if n.is_empty() {
                return;
            }
            send(if creating { Command::CreatePlaylist { name: n, description: d } } else { Command::EditPlaylist { name: n, description: d } });
            let mut model = model;
            model.write().playlist_popup = None;
        };
        Popup::new()
            .width(Size::px(440.))
            .on_close_request(move |_| close())
            .child(PopupTitle::new(if creating { "Create playlist".to_string() } else { "Edit details".to_string() }))
            .child(
                PopupContent::new().child(
                    rect()
                        .width(Size::fill())
                        .spacing(10.)
                        .child(Input::new(name).placeholder("Name").width(Size::fill()).auto_focus(true).on_submit(move |_| save()))
                        .child(Input::new(description).placeholder("Add an optional description").width(Size::fill()).on_submit(move |_| save()))
                        .child(
                            rect()
                                .horizontal()
                                .spacing(GAP)
                                .maybe_child((!creating).then(|| {
                                    Chip {
                                        label: "Change cover...",
                                        active: false,
                                        on_press: EventHandler::new(move |_| {
                                            change_cover();
                                            let mut model = model;
                                            model.write().playlist_popup = None;
                                        }),
                                    }
                                }))
                                .child(Chip { label: "Cancel", active: false, on_press: EventHandler::new(move |_| close()) })
                                .child(Chip { label: if creating { "Create" } else { "Save" }, active: true, on_press: EventHandler::new(move |_| save()) }),
                        ),
                ),
            )
    }
}

/// Searches songs and adds a result to the open own playlist (appended in place by the backend).
#[derive(PartialEq)]
struct AddSongsPopup;

impl Component for AddSongsPopup {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let query = use_state(String::new);
        let m = model.read();
        let rows: Vec<Element> = m
            .add_results
            .iter()
            .map(|item| {
                let uri = item.uri.clone();
                rect()
                    .horizontal()
                    .width(Size::fill())
                    .padding(6.)
                    .spacing(10.)
                    .cross_align(Alignment::Center)
                    .content(Content::Flex)
                    .child(cover(&item.cover_url, m.cover(&item.cover_url), 40., 4.))
                    .child(rect().width(Size::flex(1.)).spacing(2.).child(one_line(item.title.clone(), 13., TEXT)).child(one_line(item.artist.clone(), 12., SUB)))
                    .child(Chip { label: "Add", active: false, on_press: EventHandler::new(move |_| send(Command::AddToPage(uri.clone()))) })
                    .into_element()
            })
            .collect();
        Popup::new()
            .width(Size::px(520.))
            .on_close_request(move |_| model.write().add_popup = false)
            .child(PopupTitle::new(format!("Add songs to {}", m.page_title)))
            .child(
                PopupContent::new().child(
                    rect()
                        .width(Size::fill())
                        .spacing(10.)
                        .child(Input::new(query).placeholder("Search for songs").width(Size::fill()).auto_focus(true).on_submit(|q: String| {
                            if !q.trim().is_empty() {
                                send(Command::AddSearch(q));
                            }
                        }))
                        .child(ScrollView::new().height(Size::px(360.)).children(rows)),
                ),
            )
    }
}

/// Spotify Connect devices; picking one moves playback there.
#[derive(PartialEq)]
struct DevicesPopup;

impl Component for DevicesPopup {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let m = model.read();
        let rows: Vec<Element> = m
            .devices
            .iter()
            .map(|d| {
                let (id, active) = (d.uri.clone(), d.artist.ends_with("playing here"));
                rect()
                    .horizontal()
                    .width(Size::fill())
                    .padding(10.)
                    .spacing(12.)
                    .corner_radius(8.)
                    .background(rgb(if active { ELEVATED } else { SURFACE }))
                    .cross_align(Alignment::Center)
                    .on_press(move |_| {
                        send(Command::Transfer(id.clone()));
                        if let Some(mut model) = MODEL.get() {
                            model.write().devices_open = false;
                        }
                    })
                    .child(icon(I_DEVICES, 22., if active { ACCENT } else { SUB }, false))
                    .child(rect().spacing(2.).child(one_line(d.title.clone(), 14., if active { ACCENT } else { TEXT })).child(one_line(d.artist.clone(), 12., SUB)))
                    .into_element()
            })
            .collect();
        let empty = m.devices.is_empty().then(|| one_line("Looking for devices...", 13., SUB));
        Popup::new()
            .width(Size::px(380.))
            .on_close_request(move |_| model.write().devices_open = false)
            .child(PopupTitle::new("Connect to a device".to_string()))
            .child(PopupContent::new().child(rect().width(Size::fill()).spacing(4.).maybe_child(empty).children(rows)))
    }
}

#[derive(PartialEq)]
struct ShortcutsPopup;

impl Component for ShortcutsPopup {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let rows = SHORTCUTS.iter().map(|(keys, what)| {
            rect()
                .horizontal()
                .width(Size::fill())
                .padding((3., 0.))
                .content(Content::Flex)
                .child(rect().width(Size::px(170.)).font_size(13.).font_weight(FontWeight::SEMI_BOLD).color(rgb(TEXT)).child(*keys))
                .child(rect().width(Size::flex(1.)).font_size(13.).color(rgb(SUB)).child(*what))
                .into_element()
        });
        Popup::new()
            .width(Size::px(460.))
            .on_close_request(move |_| model.write().shortcuts_open = false)
            .child(PopupTitle::new("Keyboard shortcuts".to_string()))
            .child(PopupContent::new().child(rect().width(Size::fill()).children(rows)))
    }
}

#[derive(PartialEq)]
struct CreditsPopup;

impl Component for CreditsPopup {
    fn render(&self) -> impl IntoElement {
        let mut model = use_model();
        let credits = model.read().credits.clone();
        Popup::new().width(Size::px(420.)).on_close_request(move |_| model.write().credits = None).maybe(credits.is_some(), |popup| {
            let (title, text) = credits.unwrap_or_default();
            popup
                .child(PopupTitle::new(format!("Credits · {title}")))
                .child(PopupContent::new().child(paragraph().span(text).font_size(13.).color(rgb(SUB)).width(Size::fill())))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(uri: &str) -> Item {
        Item { uri: uri.into(), ..Default::default() }
    }

    #[test]
    fn tiles_read_left_then_right() {
        let tiles = interleave(&[item("a"), item("c"), item("e")], &[item("b"), item("d")]);
        assert_eq!(tiles.iter().map(|i| i.uri.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn position_stops_at_the_track_end() {
        let ago = Some(Instant::now() - Duration::from_secs(10));
        let playing = Model { position_ms: 1000, duration_ms: 3000, playing: true, position_at: ago, ..Default::default() };
        assert_eq!(playing.position(), 3000);
        let paused = Model { position_ms: 1000, duration_ms: 3000, playing: false, position_at: ago, ..Default::default() };
        assert_eq!(paused.position(), 1000);
    }
}
