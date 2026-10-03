//! Slint markup and the small helpers that push backend state onto the UI thread.

use slint::{ModelRc, VecModel};

use crate::web::Item;

slint::slint! {
    import { Button, CheckBox, ComboBox, LineEdit, ListView, Palette, Slider } from "std-widgets.slint";
    export struct Row { title: string, artist: string, uri: string, cover-url: string, cover: image }

    component RowItem inherits TouchArea {
        in property <Row> data;
        in property <bool> selected;
        // Fired whenever this (possibly recycled) row instance shows a row without a loaded cover.
        callback need-cover();
        init => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        changed data => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        height: 40px;
        Rectangle {
            background: root.selected ? #1db95440 : root.has-hover ? #8882 : transparent;
            HorizontalLayout {
                padding-left: 4px;
                spacing: 8px;
                alignment: start;
                Rectangle {
                    width: 32px;
                    height: 32px;
                    y: 4px;
                    // Spotify has no image for Liked Songs; mimic its gradient tile.
                    background: data.uri == "liked" ? @linear-gradient(135deg, #450af5 0%, #c4efd9 100%) : #8883;
                    Image { source: data.cover; width: 32px; height: 32px; }
                }
                VerticalLayout {
                    alignment: center;
                    Text { text: data.title; overflow: elide; }
                    Text { text: data.artist; overflow: elide; font-size: 11px; opacity: 0.7; }
                }
            }
        }
    }

    // One line per row at a fixed height, so the current line can be centered by arithmetic.
    component LyricsView inherits Rectangle {
        in property <[string]> lines;
        in property <int> current: -1;
        in property <string> note;
        property <length> line-height: 36px;
        // Keep the sung line in the middle; the user can still scroll in between line changes.
        changed current => {
            if (current >= 0) {
                lv.content-y = min(0px, -(current * line-height) + lv.visible-height / 2 - line-height / 2);
            }
        }
        if lines.length == 0 : Text {
            text: note;
            horizontal-alignment: center;
            vertical-alignment: center;
            opacity: 0.7;
        }
        lv := ListView {
            for line[i] in lines : Text {
                text: line;
                height: line-height;
                vertical-alignment: center;
                overflow: elide;
                font-size: i == current ? 18px : 15px;
                font-weight: i == current ? 700 : 400;
                color: i == current ? rgb(94, 234, 212) : Palette.foreground;
                opacity: current < 0 || i <= current ? 1 : 0.55;
            }
        }
    }

    export component App inherits Window {
        title: "SlimSpot";
        icon: root.app-icon;
        // Decoded from assets/icon-64.png at startup (no runtime SVG renderer).
        in property <image> app-icon;
        callback quit();
        preferred-width: 820px;
        preferred-height: 560px;
        in property <string> status: "Starting...";
        in property <bool> playing: false;
        in property <[Row]> lists;
        in property <[Row]> tracks;
        in property <string> current-list;
        in property <string> current-track;
        callback submit(string);
        callback open-list(string);
        callback play-uri(string);
        callback toggle();
        callback prev();
        callback next();
        callback seek(float);
        // save: false while dragging, true on release (so the settings file isn't rewritten per pixel).
        callback set-volume(float, bool);
        callback toggle-shuffle();
        // Index into the quality ComboBox: 0 = 96, 1 = 160, 2 = 320 kbps (matches settings::Quality).
        in-out property <int> quality: 2;
        in-out property <bool> normalize;
        callback set-quality(int);
        callback set-normalize(bool);
        // Whether the playing track is in Liked Songs.
        in property <bool> liked;
        callback toggle-like();
        // Spotify Connect devices (Row.uri = device id), fetched when the popup opens.
        in property <[Row]> devices;
        callback load-devices();
        callback transfer(string);
        // Mirrors the Windows Run registry value (see autostart.rs).
        in-out property <bool> start-with-windows;
        callback set-autostart(bool);
        callback cycle-repeat();
        // Lyrics pane replaces the track list while `show-lyrics` is on.
        in-out property <bool> show-lyrics;
        in property <[string]> lyric-lines;
        // Start time per line in ms; empty when the lyrics aren't synced.
        in property <[int]> lyric-times;
        in property <int> lyric-index: -1;
        in property <string> lyrics-note: "No track playing";
        // kind: 0 = track list, 1 = sidebar, 2 = now playing.
        callback need-cover(int, int, string, string);

        in property <Row> now;
        // Advanced by a Rust timer while playing; corrected by player events.
        in-out property <float> position;
        in property <float> duration;
        in-out property <float> volume: 100;
        in property <bool> shuffle;
        // 0 = off, 1 = all, 2 = one (matches settings::Repeat).
        in property <int> repeat;
        // True while the seek slider is dragged, so the timer doesn't fight the user.
        in-out property <bool> seeking;
        changed now => {
            if (now.cover-url != "" && now.cover.width == 0) { root.need-cover(2, 0, now.uri, now.cover-url) }
            root.now-playing-changed();
        }
        changed playing => { root.now-playing-changed(); }
        // Workaround: after minimize/restore on Windows the software renderer only repaints damaged
        // regions into a cleared buffer, leaving holes (window size stays the same, so nothing else
        // notices). Rust flips this on restore; the full-window rectangle below then marks the
        // whole window dirty for one full repaint.
        in-out property <bool> repaint-flip;
        // Mirrors title/artist/playing state to the OS media overlay.
        callback now-playing-changed();

        pure function fmt(ms: float) -> string {
            let s = floor(ms / 1000);
            let sec = Math.mod(s, 60);
            return floor(s / 60) + ":" + (sec < 10 ? "0" : "") + sec;
        }

        Rectangle { width: 100%; height: 100%; background: repaint-flip ? #00000001 : #00000002; }
        // Keyboard shortcuts. Keys a focused text field uses (Space, arrows) reach this scope only
        // when the field doesn't handle them; Esc hands focus back here from the search box.
        keys := FocusScope {
            init => { self.focus(); }
            key-pressed(event) => {
                if (event.modifiers.control) {
                    if (event.text == Key.RightArrow) { root.next(); return accept; }
                    if (event.text == Key.LeftArrow) { root.prev(); return accept; }
                    if (event.text == Key.UpArrow) { root.volume = min(100, root.volume + 5); root.set-volume(root.volume, true); return accept; }
                    if (event.text == Key.DownArrow) { root.volume = max(0, root.volume - 5); root.set-volume(root.volume, true); return accept; }
                    if (event.text == "f" || event.text == "l") { query.focus(); return accept; }
                    if (event.text == "s") { root.toggle-shuffle(); return accept; }
                    if (event.text == "r") { root.cycle-repeat(); return accept; }
                    if (event.text == "q") { root.quit(); return accept; }
                    if (event.text == "y") { root.show-lyrics = !root.show-lyrics; return accept; }
                }
                if (event.text == " ") { root.toggle(); return accept; }
                if (event.text == Key.Escape) { self.focus(); return accept; }
                reject
            }
        VerticalLayout {
            HorizontalLayout {
                padding: 12px;
                spacing: 12px;
                vertical-stretch: 1;
                ListView {
                    width: 240px;
                    for row[i] in lists: RowItem {
                        data: row;
                        selected: row.uri == root.current-list;
                        clicked => { root.open-list(row.uri); }
                        need-cover => { root.need-cover(1, i, row.uri, row.cover-url); }
                    }
                }
                VerticalLayout {
                    spacing: 8px;
                    Text { text: status; wrap: word-wrap; }
                    HorizontalLayout {
                        spacing: 8px;
                        query := LineEdit {
                            placeholder-text: "Search, or paste a track link";
                            accepted => { root.submit(self.text); }
                        }
                        Button { text: "Go"; clicked => { root.submit(query.text); } }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        alignment: start;
                        Text { text: "Quality"; vertical-alignment: center; font-size: 11px; }
                        ComboBox {
                            model: ["96 kbps", "160 kbps", "320 kbps"];
                            current-index <=> root.quality;
                            selected => { root.set-quality(self.current-index); }
                        }
                        CheckBox {
                            text: "Normalize volume";
                            checked <=> root.normalize;
                            toggled => { root.set-normalize(self.checked); }
                        }
                        CheckBox {
                            text: "Start with Windows";
                            checked <=> root.start-with-windows;
                            toggled => { root.set-autostart(self.checked); }
                        }
                    }
                    if root.show-lyrics : LyricsView {
                        vertical-stretch: 1;
                        lines: root.lyric-lines;
                        current: root.lyric-index;
                        note: root.lyrics-note;
                    }
                    if !root.show-lyrics : ListView {
                        vertical-stretch: 1;
                        for row[i] in tracks: RowItem {
                            data: row;
                            selected: row.uri == root.current-track;
                            clicked => { root.play-uri(row.uri); }
                            need-cover => { root.need-cover(0, i, row.uri, row.cover-url); }
                        }
                    }
                }
            }
            devices-popup := PopupWindow {
                x: root.width - 312px;
                y: root.height - 72px - 236px;
                width: 300px;
                height: 230px;
                Rectangle {
                    background: Palette.background;
                    border-color: Palette.border;
                    border-width: 1px;
                    border-radius: 6px;
                    VerticalLayout {
                        padding: 8px;
                        spacing: 4px;
                        Text { text: "Play on"; font-weight: 700; }
                        if root.devices.length == 0 : Text { text: "Loading devices... (open Spotify on the other device if it's missing)"; wrap: word-wrap; opacity: 0.7; }
                        ListView {
                            for d in root.devices : RowItem {
                                data: d;
                                clicked => { root.transfer(d.uri); devices-popup.close(); }
                            }
                        }
                    }
                }
            }
            // Player bar
            HorizontalLayout {
                padding: 12px;
                padding-top: 0px;
                spacing: 12px;
                height: 72px;
                Rectangle {
                    width: 56px;
                    height: 56px;
                    y: 0px;
                    background: #8883;
                    Image { source: now.cover; width: 56px; height: 56px; }
                }
                VerticalLayout {
                    width: 200px;
                    alignment: center;
                    Text { text: now.title; overflow: elide; }
                    Text { text: now.artist; overflow: elide; font-size: 11px; opacity: 0.7; }
                }
                VerticalLayout {
                    horizontal-stretch: 1;
                    alignment: center;
                    spacing: 4px;
                    HorizontalLayout {
                        alignment: center;
                        spacing: 6px;
                        Button {
                            text: "Shuffle";
                            checkable: true;
                            checked: root.shuffle;
                            clicked => { root.toggle-shuffle(); }
                        }
                        Button { text: "Prev"; clicked => { root.prev(); } }
                        Button { text: playing ? "Pause" : "Play"; clicked => { root.toggle(); } }
                        Button { text: "Next"; clicked => { root.next(); } }
                        Button {
                            text: "Lyrics";
                            checkable: true;
                            checked <=> root.show-lyrics;
                        }
                        // Not checkable: a checkable button flips itself on click, which would show
                        // "liked" even when the request fails. The state comes only from the backend.
                        Button {
                            text: root.liked ? "Liked" : "Like";
                            primary: root.liked;
                            enabled: root.now.uri != "";
                            clicked => { root.toggle-like(); }
                        }
                        Button {
                            text: "Devices";
                            clicked => { root.load-devices(); devices-popup.show(); }
                        }
                        Button {
                            text: root.repeat == 2 ? "Repeat: One" : root.repeat == 1 ? "Repeat: All" : "Repeat: Off";
                            checkable: true;
                            checked: root.repeat != 0;
                            clicked => { root.cycle-repeat(); }
                        }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        Text { text: fmt(position); vertical-alignment: center; font-size: 11px; }
                        Slider {
                            minimum: 0;
                            maximum: max(duration, 1);
                            value <=> root.position;
                            enabled: duration > 0;
                            changed => { root.seeking = true; }
                            released(v) => { root.seeking = false; root.seek(v); }
                        }
                        Text { text: fmt(duration); vertical-alignment: center; font-size: 11px; }
                    }
                }
                HorizontalLayout {
                    width: 140px;
                    spacing: 6px;
                    Text { text: "Vol"; vertical-alignment: center; font-size: 11px; }
                    Slider {
                        horizontal-stretch: 1;
                        minimum: 0;
                        maximum: 100;
                        value <=> root.volume;
                        changed(v) => { root.set-volume(v, false); }
                        released(v) => { root.set-volume(v, true); }
                    }
                }
            }
        }
        }
    }

    // Tray icon: closing the window hides it here and playback continues.
    export component Tray inherits SystemTrayIcon {
        in property <bool> playing;
        in property <image> tray-icon;
        icon: root.tray-icon;
        callback show-window();
        callback toggle();
        callback next();
        callback prev();
        callback quit();
        tooltip: "SlimSpot";
        clicked => { root.show-window(); }
        Menu {
            MenuItem { title: "Show SlimSpot"; activated => { root.show-window(); } }
            MenuItem { title: root.playing ? "Pause" : "Play"; activated => { root.toggle(); } }
            MenuItem { title: "Next"; activated => { root.next(); } }
            MenuItem { title: "Previous"; activated => { root.prev(); } }
            MenuSeparator {}
            MenuItem { title: "Quit"; activated => { root.quit(); } }
        }
    }
}

impl From<Item> for Row {
    fn from(i: Item) -> Self {
        Row {
            title: i.title.into(),
            artist: i.artist.into(),
            uri: i.uri.into(),
            cover_url: i.cover_url.into(),
            cover: Default::default(),
        }
    }
}

pub fn set_status(ui: &slint::Weak<App>, text: impl Into<String>) {
    let text = text.into();
    let _ = ui.upgrade_in_event_loop(move |app| app.set_status(text.into()));
}

pub fn set_playing(ui: &slint::Weak<App>, playing: bool) {
    let _ = ui.upgrade_in_event_loop(move |app| app.set_playing(playing));
}

pub fn set_position(ui: &slint::Weak<App>, ms: u32) {
    let _ = ui.upgrade_in_event_loop(move |app| {
        if !app.get_seeking() {
            app.set_position(ms as f32);
        }
    });
}

pub fn set_rows(ui: &slint::Weak<App>, items: Vec<Item>, setter: fn(&App, ModelRc<Row>)) {
    let _ = ui.upgrade_in_event_loop(move |app| {
        let rows: Vec<Row> = items.into_iter().map(Row::from).collect();
        setter(&app, ModelRc::new(VecModel::from(rows)));
    });
}

/// Shows lyrics for `uri` unless another track has started meanwhile.
pub fn set_lyrics(ui: &slint::Weak<App>, uri: String, result: Result<Option<crate::lyrics::Lyrics>, String>) {
    let _ = ui.upgrade_in_event_loop(move |app| {
        if app.get_now().uri != uri.as_str() {
            return;
        }
        let (lines, times, note) = match result {
            Ok(Some(l)) => {
                let times = l.times_ms.unwrap_or_default().into_iter().map(|t| t as i32).collect::<Vec<_>>();
                (l.lines, times, String::new())
            }
            Ok(None) => (Vec::new(), Vec::new(), "No lyrics for this track".into()),
            Err(e) => (Vec::new(), Vec::new(), e),
        };
        let lines: Vec<slint::SharedString> = lines.into_iter().map(Into::into).collect();
        app.set_lyric_lines(ModelRc::new(VecModel::from(lines)));
        app.set_lyric_times(ModelRc::new(VecModel::from(times)));
        app.set_lyric_index(-1);
        app.set_lyrics_note(note.into());
    });
}

pub fn clear_lyrics(ui: &slint::Weak<App>, note: &str) {
    let note = note.to_string();
    let _ = ui.upgrade_in_event_loop(move |app| {
        app.set_lyric_lines(ModelRc::default());
        app.set_lyric_times(ModelRc::default());
        app.set_lyric_index(-1);
        app.set_lyrics_note(note.into());
    });
}

pub fn set_current(ui: &slint::Weak<App>, uri: String, setter: fn(&App, slint::SharedString)) {
    let _ = ui.upgrade_in_event_loop(move |app| setter(&app, uri.into()));
}
