//! The window: a Freya UI driven by `UiUpdate`s from the backend. Commands go back over the
//! backend's `Command` channel.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use freya::animation::*;
use freya::prelude::*;
use freya::winit::window::Theme as WindowTheme;
use tokio::sync::mpsc;

use crate::player::Command;
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
pub fn run(commands: mpsc::UnboundedSender<Command>, cover_requests: mpsc::UnboundedSender<String>) {
    let _ = COMMANDS.set(commands);
    let _ = COVER_REQUESTS.set(cover_requests);
    launch(
        LaunchConfig::new()
            .with_font("Poppins", &include_bytes!("../assets/fonts/Poppins-Regular.ttf")[..])
            .with_font("Poppins", &include_bytes!("../assets/fonts/Poppins-SemiBold.ttf")[..])
            .with_default_font("Poppins")
            .with_window(
                WindowConfig::new(app)
                    .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
                    .with_title("SlimSpot")
                    .with_renderer(RendererPreference::Software)
                    .with_window_attributes(|attributes, _| attributes.with_theme(Some(WindowTheme::Dark)))
                    .with_window_handle(|window| {
                        #[cfg(windows)]
                        crate::instance::dark_title_bar(window, BASE, SUB, HAIRLINE);
                    })
                    // ponytail: closing quits until the tray is back (phase 2); then it hides to the tray again.
                    .with_on_close(|_, _| {
                        quit();
                        CloseDecision::KeepOpen
                    }),
            ),
    );
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
const ROW_H: f32 = 56.;
const HOVER_MS: u64 = 150;
const PAGE_ANIM_MS: u64 = 250;
// Covers kept as encoded bytes (a few KB each); Freya decodes and caches them at drawn size.
const MAX_COVERS: usize = 300;
const POSITION_TICK: Duration = Duration::from_millis(500);

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
                Rows::Tracks => self.tracks = items,
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
                // ponytail: playlist targets, side panel, devices and "Add songs" results come with their screens (phase 2).
                Rows::Targets | Rows::SideRows | Rows::Devices | Rows::AddResults => {}
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
            }
            UiUpdate::PageInfo(info) => self.page_info = info,
            UiUpdate::PageCover(url) => self.page_cover = url,
            UiUpdate::Settings { volume, shuffle, repeat, .. } => {
                self.volume = volume;
                self.shuffle = shuffle;
                self.repeat = repeat;
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
            UiUpdate::Cover(url, bytes) => {
                self.cover_order.push_back(url.clone());
                self.covers.insert(url, bytes);
                while self.cover_order.len() > MAX_COVERS {
                    if let Some(old) = self.cover_order.pop_front() {
                        self.covers.remove(&old);
                        REQUESTED.with_borrow_mut(|r| r.remove(&old));
                    }
                }
            }
            UiUpdate::Show => Platform::get().with_window(Platform::window_id(), |w| {
                #[cfg(windows)]
                crate::instance::bring_to_front(w);
            }),
            UiUpdate::InvokeQuit => quit(),
            UiUpdate::QuitEventLoop => std::process::exit(0),
            // ponytail: lyrics, credits, settings, queue, page tabs and the update chip come with their screens (phase 2).
            UiUpdate::Lyrics(..)
            | UiUpdate::ClearLyrics(_)
            | UiUpdate::PageSaved(_)
            | UiUpdate::ScrollTop
            | UiUpdate::Reveal(_)
            | UiUpdate::LikedChecked(_)
            | UiUpdate::ResetLibraryChips
            | UiUpdate::StartupSettings { .. }
            | UiUpdate::NowTintDefault
            | UiUpdate::NowAbout(_)
            | UiUpdate::Credits { .. }
            | UiUpdate::UpdateVersion(_)
            | UiUpdate::WebClientId(_) => {}
        }
    }
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
            .child(icon(self.path, 22., color, self.active))
            .child(rect().font_size(15.).font_weight(FontWeight::SEMI_BOLD).color(rgb(color)).child(self.label))
    }
}

#[derive(PartialEq)]
struct LibraryRow {
    item: Item,
    selected: bool,
    cover: Option<Bytes>,
}

impl Component for LibraryRow {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let uri = self.item.uri.clone();
        let bg = if self.selected { HOVER } else { mix(SURFACE, ELEVATED, t) };
        let round = self.item.uri.starts_with("spotify:artist:");
        rect()
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
            .child(list_cover(&self.item, self.cover.clone(), 48., if round { 24. } else { RADIUS_COVER }))
            .child(
                rect()
                    .width(Size::func(|p| Some(p.parent - 64.)))
                    .spacing(2.)
                    .child(one_line(self.item.title.clone(), 14., if self.selected { ACCENT } else { TEXT }))
                    .child(one_line(self.item.artist.clone(), 12., SUB)),
            )
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
            .spacing(14.)
            .on_pointer_enter(move |_| anim.start())
            .on_pointer_leave(move |_| anim.reverse())
            .on_press(move |_| send(Command::OpenList(open.clone())))
            .child(list_cover(&self.item, self.cover.clone(), 64., 0.))
            .child(
                rect()
                    .width(Size::func(|p| Some(p.parent - 140.)))
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
}

impl Component for TrackRow {
    fn render(&self) -> impl IntoElement {
        let (mut anim, t) = use_hover();
        let mut hovered = use_state(|| false);
        let (uri, current) = (self.item.uri.clone(), self.current);
        let marker = if hovered() {
            icon(if current && self.playing { I_PAUSE } else { I_PLAY }, 14., TEXT, true).into_element()
        } else if current {
            icon(I_VOLUME, 16., ACCENT, false).into_element()
        } else if self.numbered {
            rect().color(rgb(SUB)).font_size(14.).child(format!("{}", self.index + 1)).into_element()
        } else {
            rect().into_element()
        };
        let duration = if self.item.duration_ms > 0 { fmt_ms(self.item.duration_ms) } else { String::new() };
        rect()
            .horizontal()
            .width(Size::fill())
            .height(Size::px(ROW_H))
            .padding((0., 16.))
            .spacing(16.)
            .corner_radius(6.)
            .background(rgb(mix(SURFACE, ELEVATED, t)))
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
            // The playing row pauses/resumes instead of restarting.
            .on_press(move |_| send(if current { Command::Toggle } else { Command::PlayUri(uri.clone()) }))
            .child(rect().width(Size::px(24.)).cross_align(Alignment::Center).child(marker))
            .child(cover(&self.item.cover_url, self.cover.clone(), 40., 4.))
            .child(
                rect()
                    .width(Size::flex(1.))
                    .spacing(2.)
                    .child(one_line(self.item.title.clone(), 15., if current { ACCENT } else { TEXT }))
                    .child(one_line(self.item.artist.clone(), 13., SUB)),
            )
            .child(
                rect()
                    .width(Size::px(32.))
                    .opacity(if self.item.liked { 1. } else { 0. })
                    .child(icon(I_HEART, 16., ACCENT, true)),
            )
            .child(rect().width(Size::px(44.)).color(rgb(SUB)).font_size(13.).text_align(TextAlign::End).child(duration))
    }

    fn render_key(&self) -> DiffKey {
        DiffKey::from(&self.index)
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
        let m = model.read();
        let home = m.page_kind == "Home";
        let nav = rect()
            .width(Size::fill())
            .padding(8.)
            .corner_radius(RADIUS_PANEL)
            .background(rgb(SURFACE))
            .child(NavItem { path: I_HOME, label: "Home", active: home, on_press: || send(Command::Home) })
            .child(NavItem { path: I_SEARCH, label: "Search", active: m.page_kind == "Search", on_press: || {} });
        let rows: Vec<Element> = m
            .lists
            .iter()
            .map(|item| LibraryRow { item: item.clone(), selected: item.uri == m.current_list, cover: m.cover(&item.cover_url) }.into_element())
            .collect();
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
                    .child(icon(I_LIBRARY, 22., SUB, false))
                    .child(rect().font_size(15.).font_weight(FontWeight::SEMI_BOLD).color(rgb(TEXT)).child("Your Library")),
            )
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
                    .placeholder("What do you want to play?")
                    .width(Size::px(360.))
                    .on_submit(|q: String| {
                        if !q.trim().is_empty() {
                            send(Command::Submit(q));
                        }
                    }),
            )
            .child(rect().padding((0., 8.)).child(one_line(m.status.clone(), 12., SUB)))
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
            let row: Vec<Element> = tiles.by_ref().take(4).collect();
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
                TrackRow { index, item: item.clone(), current, playing: m.playing, numbered: false, cover: m.cover(&item.cover_url) }.into_element()
            })
            .collect();
        ScrollView::new()
            .child(
                rect()
                    .width(Size::fill())
                    .padding((8., 24., 24., 24.))
                    .spacing(16.)
                    .background(LinearGradient::new().angle(180.).stop(((40, 60, 52), 0.)).stop((SURFACE, 100.)))
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
        let model = use_model();
        let m = model.read();
        let header_cover = crate::covers::larger_cover(&m.page_cover);
        let has_cover = !header_cover.is_empty();
        let header = rect()
            .horizontal()
            .width(Size::fill())
            .padding((24., 24., 24., 24.))
            .spacing(24.)
            .cross_align(Alignment::End)
            .background(LinearGradient::new().angle(180.).stop(((88, 64, 128), 0.)).stop(((40, 30, 58), 100.)))
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
            .background(LinearGradient::new().angle(180.).stop(((40, 30, 58), 0.)).stop((SURFACE, 100.)))
            .maybe_child(m.playable.then(|| PlayFab { size: 56., playing: m.playing && !m.current_list.is_empty() && m.current_list == m.page_uri }))
            .child(IconButton { path: I_SHUFFLE, size: 26., active: m.shuffle, filled: false, on_press: Some(|| send(Command::ToggleShuffle)) });
        let columns = rect()
            .horizontal()
            .width(Size::fill())
            .padding((8., 40.))
            .spacing(16.)
            .content(Content::Flex)
            .color(rgb(SUB))
            .font_size(12.)
            .child(rect().width(Size::px(24.)).child(if m.numbered { "#" } else { "" }))
            .child(rect().width(Size::px(40.)))
            .child(rect().width(Size::flex(1.)).child("Title"))
            .child(rect().width(Size::px(32.)))
            .child(rect().width(Size::px(44.)).cross_align(Alignment::End).child(icon(I_CLOCK, 16., SUB, false)));
        let len = m.tracks.len();
        let list = rect().width(Size::fill()).height(Size::fill()).padding((0., 24.)).child(
            VirtualScrollView::new(move |item, _| {
                let m = model.read();
                let Some(track) = m.tracks.get(item.index) else { return rect().into_element() };
                let current = track.uri == m.current_track;
                TrackRow {
                    index: item.index,
                    item: track.clone(),
                    current,
                    playing: m.playing,
                    numbered: m.numbered,
                    cover: m.cover(&track.cover_url),
                }
                .into_element()
            })
            .length(len)
            .item_size(ROW_H),
        );
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .child(header)
            .child(actions)
            .child(columns)
            .child(rect().width(Size::fill()).height(Size::px(1.)).margin((0., 24.)).background(rgb(HAIRLINE)))
            .child(list)
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

fn app() -> impl IntoElement {
    let mut model = use_provide_context(|| State::create(Model { volume: 100., ..Default::default() }));
    use_future(move || async move {
        let Some(mut rx) = UPDATES.lock().unwrap().take() else { return };
        while let Some(update) = rx.recv().await {
            let mut m = model.write();
            m.apply(update);
            // Apply whatever else is queued in the same frame.
            while let Ok(update) = rx.try_recv() {
                m.apply(update);
            }
        }
    });
    let (page_gen, home) = {
        let m = model.read();
        (m.page_gen, m.page_kind == "Home")
    };
    rect()
        .width(Size::fill())
        .height(Size::fill())
        .background(rgb(BASE))
        .color(rgb(TEXT))
        .on_global_key_down(|e: Event<KeyboardEventData>| {
            if e.modifiers.contains(Modifiers::CONTROL) && e.key == Key::Character("q".into()) {
                quit();
            }
        })
        .child(
            rect()
                .horizontal()
                .width(Size::fill())
                .height(Size::func(|p| Some(p.parent - PLAYER_H)))
                .padding((GAP, GAP, 0., GAP))
                .spacing(GAP)
                .child(Sidebar)
                .child(
                    rect()
                        .width(Size::fill())
                        .height(Size::fill())
                        .corner_radius(RADIUS_PANEL)
                        .overflow(Overflow::Clip)
                        .background(rgb(SURFACE))
                        .child(TopBar)
                        .child(MainPanel { page_gen, home }),
                ),
        )
        .child(PlayerBar)
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
