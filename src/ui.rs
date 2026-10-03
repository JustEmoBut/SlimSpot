//! Slint markup and the small helpers that push backend state onto the UI thread.

use slint::{Model, ModelRc, VecModel};

use crate::web::Item;

slint::slint! {
    import { ListView, Palette } from "std-widgets.slint";
    // Poppins (SIL OFL, assets/fonts/OFL.txt), embedded in the exe: ~160 KB per weight.
    import "../assets/fonts/Poppins-Regular.ttf";
    import "../assets/fonts/Poppins-SemiBold.ttf";
    export struct Row { title: string, artist: string, uri: string, cover-url: string, cover: image, duration: string, liked: bool }

    // Spotify-like dark palette. Colors whose hex starts with a digit and `e` are written as rgb():
    // inside slint! the Rust tokenizer reads them as float exponents.
    // "Midnight Indigo": deep navy surfaces, violet accent fading into cyan (ui-ux-pro-max music
    // palette, accent swapped away from Spotify green). Every color is rgb(): inside slint! a hex
    // like #5eead4 tokenizes as a Rust float exponent.
    global Theme {
        out property <color> base: rgb(8, 8, 26);
        out property <color> panel: rgb(16, 16, 42);
        out property <color> raised: rgb(27, 27, 56);
        out property <color> hover: rgb(35, 35, 74);
        out property <color> selected: rgb(44, 44, 94);
        out property <color> field: rgb(27, 27, 56);
        out property <color> border: rgba(255, 255, 255, 0.08);
        out property <color> text: rgb(248, 250, 252);
        out property <color> subdued: rgb(163, 168, 195);
        out property <color> muted: rgb(107, 111, 142);
        out property <color> track: rgb(46, 46, 90);
        out property <color> accent: rgb(139, 92, 246);
        out property <color> accent2: rgb(34, 211, 238);
        // Progress fill only: the software renderer ignores border-radius on gradient backgrounds,
        // so rounded things (buttons, chips, badges) use the solid accent.
        out property <brush> glow: @linear-gradient(135deg, rgb(139, 92, 246) 0%, rgb(34, 211, 238) 100%);
    }

    // 24x24 line/fill icons, drawn here (no icon font or image assets).
    global Icons {
        out property <string> play: "M8 5 L19 12 L8 19 Z";
        out property <string> pause: "M6.5 5 H10 V19 H6.5 Z M14 5 H17.5 V19 H14 Z";
        out property <string> next: "M5.5 5.5 L15 12 L5.5 18.5 Z M16 5 H18.5 V19 H16 Z";
        out property <string> prev: "M18.5 5.5 L9 12 L18.5 18.5 Z M5.5 5 H8 V19 H5.5 Z";
        out property <string> shuffle: "M3 7 H7 L15 17 H20 M3 17 H7 L9.6 13.8 M14.4 10.2 L15 7 H20 M17 4 L20 7 L17 10 M17 14 L20 17 L17 20";
        out property <string> repeat: "M4 12 V10 C4 8 5.5 6.5 7.5 6.5 H18 M15 3.5 L18 6.5 L15 9.5 M20 12 V14 C20 16 18.5 17.5 16.5 17.5 H6 M9 20.5 L6 17.5 L9 14.5";
        out property <string> heart: "M12 20 C12 20 3 14.5 3 8.8 C3 6 5.1 4 7.6 4 C9.4 4 11 5 12 6.6 C13 5 14.6 4 16.4 4 C18.9 4 21 6 21 8.8 C21 14.5 12 20 12 20 Z";
        out property <string> lyrics: "M9.5 3.5 H14.5 V11.5 C14.5 14 9.5 14 9.5 11.5 Z M6 11 C6 18 18 18 18 11 M12 16.5 V21 M8.5 21 H15.5";
        out property <string> radio: "M8.8 8.8 C7 10.6 7 13.4 8.8 15.2 M15.2 8.8 C17 10.6 17 13.4 15.2 15.2 M5.8 5.8 C2.4 9.2 2.4 14.8 5.8 18.2 M18.2 5.8 C21.6 9.2 21.6 14.8 18.2 18.2 M12 10.6 C12.8 10.6 13.4 11.2 13.4 12 C13.4 12.8 12.8 13.4 12 13.4 C11.2 13.4 10.6 12.8 10.6 12 C10.6 11.2 11.2 10.6 12 10.6 Z";
        out property <string> devices: "M3 5 H14 V15 H3 Z M6 19 H11 M8.5 15 V19 M17 7 H21 V19 H17 Z";
        out property <string> volume: "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9 C16.8 10.5 16.8 13.5 15.5 15 M18 6.5 C20.8 9.5 20.8 14.5 18 17.5";
        out property <string> muted: "M4 9.5 H7.5 L12 5.5 V18.5 L7.5 14.5 H4 Z M15.5 9.5 L20.5 14.5 M20.5 9.5 L15.5 14.5";
        out property <string> search: "M17 10.5 C17 14.1 14.1 17 10.5 17 C6.9 17 4 14.1 4 10.5 C4 6.9 6.9 4 10.5 4 C14.1 4 17 6.9 17 10.5 Z M15.5 15.5 L20 20";
        out property <string> settings: "M4 7 H20 M4 12 H20 M4 17 H20 M9 5 V9 M15 10 V14 M7 15 V19";
        out property <string> back: "M15 5 L8 12 L15 19";
        out property <string> forward: "M9 5 L16 12 L9 19";
        out property <string> home: "M4 11 L12 4 L20 11 V20 H14.5 V14 H9.5 V20 H4 Z";
        out property <string> more: "M5 10.5 H8 V13.5 H5 Z M10.5 10.5 H13.5 V13.5 H10.5 Z M16 10.5 H19 V13.5 H16 Z";
        out property <string> check: "M6.5 12.5 L10.5 16.5 L17.5 8.5";
        out property <string> person: "M12 12 C14.2 12 16 10.2 16 8 C16 5.8 14.2 4 12 4 C9.8 4 8 5.8 8 8 C8 10.2 9.8 12 12 12 Z M4.5 20 C4.5 16.5 7.8 14 12 14 C16.2 14 19.5 16.5 19.5 20";
        out property <string> disc: "M12 3 A9 9 0 1 0 12 21 A9 9 0 1 0 12 3 Z M12 10 A2 2 0 1 0 12 14 A2 2 0 1 0 12 10 Z";
        out property <string> link: "M10 14 L14 10 M9 7 L11 5 C12.7 3.3 15.3 3.3 17 5 L19 7 C20.7 8.7 20.7 11.3 19 13 L17 15 M15 17 L13 19 C11.3 20.7 8.7 20.7 7 19 L5 17 C3.3 15.3 3.3 12.7 5 11 L7 9";
        out property <string> trash: "M5 7 H19 M10 7 V5 H14 V7 M7 7 L8 20 H16 L17 7";
        out property <string> open: "M14 5 H19 V10 M19 5 L11 13 M17 14 V19 H5 V7 H10";
        out property <string> plus: "M12 5 V19 M5 12 H19";
        out property <string> pencil: "M5 19 L6 14.5 L15.5 5 L19 8.5 L9.5 18 Z M13.5 7 L17 10.5";
        out property <string> panel: "M3.5 5 H20.5 V19 H3.5 Z M14.5 5 V19";
        out property <string> mini: "M3.5 5 H20.5 V19 H3.5 Z M11 12 H18 V17 H11 Z";
        out property <string> bars: "M5 10 H8 V19 H5 Z M10.5 5 H13.5 V19 H10.5 Z M16 13 H19 V19 H16 Z";
        out property <string> queue: "M4 6 H20 M4 11 H20 M4 16 H11 M15 14 V20 L20 17 Z";
    }

    component Icon inherits Path {
        in property <string> shape;
        in property <bool> filled;
        in property <color> tint: Theme.subdued;
        commands: shape;
        viewbox-width: 24;
        viewbox-height: 24;
        fill: filled ? tint : transparent;
        stroke: filled ? transparent : tint;
        stroke-width: 1.8px;
        stroke-line-cap: round;
        stroke-line-join: round;
    }

    // Flat icon button: grey, white on hover, green with a dot underneath while `active`.
    component IconButton inherits TouchArea {
        in property <string> shape;
        in property <bool> filled;
        in property <bool> active;
        in property <bool> dot: active;
        in property <length> size: 20px;
        width: 34px;
        height: 34px;
        mouse-cursor: pointer;
        Icon {
            x: (parent.width - root.size) / 2;
            y: (parent.height - root.size) / 2;
            width: root.size;
            height: root.size;
            shape: root.shape;
            filled: root.filled;
            tint: !root.enabled ? Theme.muted : root.active ? Theme.accent : root.has-hover ? Theme.text : Theme.subdued;
        }
        if root.dot : Rectangle {
            x: (parent.width - 4px) / 2;
            y: parent.height - 4px;
            width: 4px;
            height: 4px;
            border-radius: 2px;
            background: Theme.accent;
        }
    }

    // The round white play/pause button in the middle of the player bar.
    component PlayButton inherits TouchArea {
        in property <bool> playing;
        width: 36px;
        height: 36px;
        mouse-cursor: pointer;
        Rectangle {
            width: root.has-hover ? 40px : 36px;
            height: self.width;
            border-radius: self.width / 2;
            background: Theme.accent;
            drop-shadow-blur: root.has-hover ? 14px : 8px;
            drop-shadow-color: rgba(139, 92, 246, 0.55);
            animate width, drop-shadow-blur { duration: 120ms; }
            Icon {
                width: 18px;
                height: 18px;
                shape: root.playing ? Icons.pause : Icons.play;
                filled: true;
                tint: Theme.text;
            }
        }
    }

    // Slim progress/volume bar: white fill, green with a knob while hovered or dragged.
    component Bar inherits Rectangle {
        in-out property <float> value;
        in property <float> maximum: 1;
        in property <bool> enabled: true;
        callback dragged(float);
        callback released(float);
        property <float> fraction: root.maximum > 0 ? max(0, min(1, root.value / root.maximum)) : 0;
        property <bool> hot: ta.has-hover || ta.pressed;
        height: 14px;
        Rectangle {
            y: (parent.height - self.height) / 2;
            height: 4px;
            border-radius: 2px;
            background: Theme.track;
            Rectangle {
                x: 0;
                width: parent.width * root.fraction;
                border-radius: 2px;
                background: root.enabled ? Theme.glow : Theme.muted;
            }
        }
        if root.hot && root.enabled : Rectangle {
            x: parent.width * root.fraction - 6px;
            y: (parent.height - 12px) / 2;
            width: 12px;
            height: 12px;
            border-radius: 6px;
            background: Theme.text;
        }
        ta := TouchArea {
            enabled: root.enabled;
            mouse-cursor: pointer;
            pointer-event(event) => {
                if (event.kind == PointerEventKind.down) {
                    root.value = max(0, min(1, self.mouse-x / root.width)) * root.maximum;
                    root.dragged(root.value);
                } else if (event.kind == PointerEventKind.up) {
                    root.released(root.value);
                }
            }
            moved => {
                if (self.pressed) {
                    root.value = max(0, min(1, self.mouse-x / root.width)) * root.maximum;
                    root.dragged(root.value);
                }
            }
        }
    }

    component RowItem inherits TouchArea {
        in property <Row> data;
        in property <bool> selected;
        // Playing rows show their title in green, like Spotify.
        in property <bool> playing;
        in property <length> cover-size: 40px;
        // Row menu (right-click and "..."); off for rows that aren't Spotify items (devices).
        in property <bool> menu: true;
        // Position shown left of the cover in track lists; 0 hides it.
        in property <int> number;
        // Sidebar rows can be removed from the library.
        in property <bool> sidebar;
        // Right-click or "...": open the app's menu at this window position.
        callback open-menu(length, length);
        // Fired whenever this (possibly recycled) row instance shows a row without a loaded cover.
        callback need-cover();
        init => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        changed data => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        height: root.cover-size + 16px;
        mouse-cursor: pointer;
        pointer-event(event) => {
            if (root.menu && event.button == PointerEventButton.right && event.kind == PointerEventKind.up) {
                root.open-menu(self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y);
            }
        }
        Rectangle {
            border-radius: 8px;
            background: root.selected ? Theme.selected : root.has-hover ? Theme.hover : transparent;
            animate background { duration: 120ms; }
            // Selected sidebar entry: a short accent bar on the left edge.
            if root.selected : Rectangle {
                x: 0;
                width: 3px;
                height: parent.height * 0.5;
                y: parent.height * 0.25;
                border-radius: 2px;
                background: Theme.accent;
            }
            HorizontalLayout {
                padding-left: 8px;
                // Room for the hover "more" button.
                padding-right: root.menu ? 44px : 8px;
                spacing: 12px;
                if root.number > 0 : Text {
                    width: 24px;
                    text: root.number;
                    horizontal-alignment: right;
                    vertical-alignment: center;
                    font-size: 14px;
                    color: root.playing ? Theme.accent : Theme.subdued;
                }
                Rectangle {
                    width: root.cover-size;
                    height: root.cover-size;
                    y: (parent.height - self.height) / 2;
                    // Artists are round, everything else gets slightly rounded corners.
                    border-radius: data.uri.starts-with("spotify:artist:") ? root.cover-size / 2 : 4px;
                    clip: true;
                    // Spotify has no image for Liked Songs; mimic its gradient tile.
                    background: data.uri == "liked" ? @linear-gradient(135deg, #450af5 0%, #c4efd9 100%) : data.uri.starts-with("slimspot:") ? transparent : Theme.raised;
                    Image { source: data.cover; width: parent.width; height: parent.height; image-fit: cover; }
                    if data.uri.starts-with("slimspot:") : Icon { width: parent.width * 0.6; height: self.width; shape: Icons.search; tint: Theme.subdued; }
                    if data.uri == "liked" : Icon { width: parent.width * 0.5; height: self.width; shape: Icons.heart; filled: true; tint: Theme.text; }
                    // Tracks: a play arrow on hover, equalizer bars while playing (like Spotify's row number).
                    if data.uri.starts-with("spotify:track:") && (root.has-hover || root.playing) : Rectangle {
                        background: #00000099;
                        Icon {
                            width: parent.width * 0.5;
                            height: self.width;
                            shape: root.playing && !root.has-hover ? Icons.bars : Icons.play;
                            filled: true;
                            tint: root.playing ? Theme.accent : Theme.text;
                        }
                    }
                }
                VerticalLayout {
                    alignment: center;
                    spacing: 2px;
                    horizontal-stretch: 1;
                    Text { text: data.title; overflow: elide; font-size: 14px; color: root.playing ? Theme.accent : Theme.text; }
                    Text { text: data.artist; overflow: elide; font-size: 12px; color: Theme.subdued; }
                }
                // Spotify's green circle with a dark tick; a filled path would hide the tick, so two layers.
                if data.liked : Rectangle {
                    width: 16px;
                    height: 16px;
                    y: (parent.height - self.height) / 2;
                    border-radius: 8px;
                    background: Theme.accent;
                    Icon { width: 14px; height: 14px; shape: Icons.check; tint: Theme.text; }
                }
                if data.duration != "" : Text {
                    width: 40px;
                    text: data.duration;
                    horizontal-alignment: right;
                    vertical-alignment: center;
                    font-size: 13px;
                    color: Theme.subdued;
                }
            }
        }
        // The "..." on hover opens the same menu as a right-click.
        if root.menu && root.has-hover : IconButton {
            x: parent.width - self.width - 8px;
            y: (parent.height - self.height) / 2;
            shape: Icons.more;
            filled: true;
            dot: false;
            size: 18px;
            clicked => { root.open-menu(self.absolute-position.x, self.absolute-position.y + self.height); }
        }
    }

    // On/off switch for settings (replaces std-widgets' CheckBox, which follows the Fluent look).
    component Switch inherits TouchArea {
        in property <string> label;
        in-out property <bool> on;
        callback toggled(bool);
        height: 34px;
        mouse-cursor: pointer;
        clicked => {
            root.on = !root.on;
            root.toggled(root.on);
        }
        HorizontalLayout {
            spacing: 12px;
            Text { text: root.label; vertical-alignment: center; horizontal-stretch: 1; font-size: 13px; color: Theme.text; }
            Rectangle {
                width: 38px;
                height: 22px;
                y: (parent.height - self.height) / 2;
                border-radius: 11px;
                background: root.on ? Theme.accent : Theme.track;
                animate background { duration: 150ms; }
                Rectangle {
                    x: root.on ? parent.width - self.width - 3px : 3px;
                    y: 3px;
                    width: 16px;
                    height: 16px;
                    border-radius: 8px;
                    background: Theme.text;
                    animate x { duration: 150ms; easing: ease-out; }
                }
            }
        }
    }

    // Small heading with an icon, used inside popups.
    component SectionTitle inherits HorizontalLayout {
        in property <string> text;
        in property <string> shape;
        spacing: 8px;
        height: 24px;
        Icon { width: 16px; height: 16px; y: (parent.height - self.height) / 2; shape: root.shape; tint: Theme.accent2; }
        Text { text: root.text; vertical-alignment: center; font-size: 12px; font-weight: 600; color: Theme.subdued; }
    }

    // One Spotify Connect device in the devices popup.
    component DeviceEntry inherits TouchArea {
        in property <Row> data;
        in property <bool> active;
        height: 48px;
        mouse-cursor: pointer;
        Rectangle {
            border-radius: 8px;
            background: root.has-hover ? Theme.hover : transparent;
            HorizontalLayout {
                padding-left: 10px;
                padding-right: 10px;
                spacing: 12px;
                Icon { width: 22px; height: 22px; y: (parent.height - self.height) / 2; shape: Icons.devices; tint: root.active ? Theme.accent2 : Theme.subdued; }
                VerticalLayout {
                    alignment: center;
                    spacing: 1px;
                    Text { text: data.title; overflow: elide; font-size: 13px; font-weight: 600; color: root.active ? Theme.accent2 : Theme.text; }
                    Text { text: data.artist; overflow: elide; font-size: 11px; color: Theme.subdued; }
                }
            }
        }
    }

    // One entry of the row menu.
    component MenuEntry inherits TouchArea {
        in property <string> label;
        in property <string> shape;
        in property <bool> danger;
        in property <bool> submenu;
        height: 34px;
        mouse-cursor: pointer;
        Rectangle {
            border-radius: 6px;
            background: root.has-hover ? Theme.hover : transparent;
            HorizontalLayout {
                padding-left: 10px;
                padding-right: 10px;
                spacing: 10px;
                Icon {
                    width: 16px;
                    height: 16px;
                    y: (parent.height - self.height) / 2;
                    shape: root.shape;
                    tint: root.danger ? rgb(248, 113, 113) : root.has-hover ? Theme.accent2 : Theme.subdued;
                }
                Text {
                    text: root.label;
                    vertical-alignment: center;
                    horizontal-stretch: 1;
                    overflow: elide;
                    font-size: 13px;
                    color: root.danger ? rgb(248, 113, 113) : Theme.text;
                }
                if root.submenu : Icon { width: 14px; height: 14px; y: (parent.height - self.height) / 2; shape: Icons.forward; tint: Theme.subdued; }
            }
        }
    }

    // One line per row at a fixed height, so the current line can be centered by arithmetic.
    component LyricsView inherits Rectangle {
        in property <[string]> lines;
        in property <int> current: -1;
        in property <string> note;
        // Two text lines per slot: long lines wrap instead of being cut, and every slot keeps the
        // same height so the sung line can still be centered arithmetically.
        property <length> line-height: 64px;
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
            font-size: 18px;
            color: Theme.subdued;
        }
        lv := ListView {
            for line[i] in lines : Text {
                text: line;
                height: line-height;
                vertical-alignment: center;
                wrap: word-wrap;
                overflow: elide;
                font-size: 22px;
                font-weight: 700;
                // Sung and current lines are white, upcoming ones dimmed.
                color: current < 0 || i <= current ? Theme.text : #ffffff66;
            }
        }
    }

    // A pill-shaped choice used in the settings popup.
    component Chip inherits TouchArea {
        in property <string> label;
        in property <bool> chosen;
        height: 30px;
        mouse-cursor: pointer;
        min-width: t.preferred-width + 20px;
        Rectangle {
            border-radius: 15px;
            background: root.chosen ? Theme.accent : root.has-hover ? Theme.hover : Theme.raised;
            border-width: root.chosen ? 0px : 1px;
            border-color: Theme.border;
            t := Text { text: root.label; font-size: 12px; font-weight: 600; color: Theme.text; }
        }
    }

    export component App inherits Window {
        title: "SlimSpot";
        default-font-family: "Poppins";
        // Embedded here rather than set from Rust: winit gets the icon only when its image cache key
        // changes, and a pixel-buffer image has none, so an icon from Rust never reached the title bar.
        icon: @image-url("../assets/icon-64.png");
        background: Theme.base;
        callback quit();
        preferred-width: 1040px;
        preferred-height: 680px;
        min-width: root.mini ? 480px : 760px;
        min-height: root.mini ? 72px : 480px;
        always-on-top: root.mini;
        in property <string> status: "Starting...";
        // Big heading of the main panel and the back/forward arrows next to it.
        in property <string> page-title: "Home";
        in property <string> page-kind;
        in property <bool> can-back;
        in property <bool> can-forward;
        callback go-back();
        callback go-forward();
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
        // Index of the chosen quality: 0 = 96, 1 = 160, 2 = 320 kbps (matches settings::Quality).
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
        callback start-radio();
        callback show-queue();
        callback go-to-playing();
        callback go-home();
        // Row menu: what it was opened on and where.
        in-out property <Row> menu-row;
        in-out property <bool> menu-sidebar;
        in-out property <bool> menu-playlists;
        in-out property <length> menu-x;
        in-out property <length> menu-y;
        public function open-row-menu(row: Row, x: length, y: length, sidebar: bool) {
            root.menu-row = row;
            root.menu-sidebar = sidebar;
            root.menu-playlists = false;
            // Kept inside the window; the menu is at most ~330 px tall.
            root.menu-x = min(x, root.width - 248px);
            root.menu-y = min(y, root.height - 340px);
            row-menu.show();
        }
        function menu-act(action: string) {
            root.row-action(action, root.menu-row.uri);
            row-menu.close();
        }
        callback play-page();
        callback filter-library(int, string, bool);
        in property <bool> numbered;
        // Compact window with only the player bar.
        in-out property <bool> mini;
        callback toggle-mini();
        // Right-hand "Now playing" panel with a large cover.
        in-out property <bool> show-now-panel;
        in-out property <image> now-big;
        // Header cover of playlist/album pages; Rust sets the URL, the image is fetched on change.
        in property <string> page-cover-url;
        in property <image> page-cover;
        changed page-cover-url => { if (page-cover-url != "") { root.need-cover(4, 0, page-cover-url, page-cover-url); } }
        changed show-now-panel => { if (show-now-panel && now.cover-url != "") { root.need-cover(3, 0, now.uri, now.cover-url); } }
        // Sleep timer choice: 0 off, -1 end of track, otherwise minutes.
        in-out property <int> sleep-minutes;
        callback set-sleep(int);
        callback create-playlist();
        callback rename-playlist(string);
        in-out property <string> rename-text;
        in-out property <bool> library-az;
        in property <bool> page-playable;
        in-out property <int> library-kind;
        in property <[Row]> targets;
        in property <bool> editable;
        // Track list scroll position, so Rust can reveal the playing row.
        in-out property <length> list-y;
        in-out property <length> list-height;
        // Rows checked for liked marks; scrolling near that point asks for the next batch.
        in-out property <int> liked-checked;
        callback check-liked-more();
        changed list-y => {
            if (root.liked-checked < root.tracks.length && (-root.list-y + root.list-height) / 56px > root.liked-checked - 20) {
                // Moved ahead at once (by the backend's batch size) so further scrolling doesn't queue
                // the same batch again; the backend sets the real value when the batch is done.
                root.liked-checked += 200;
                root.check-liked-more();
            }
        }
        public function reveal(index: int) {
            // RowItem height in the track list: 40px cover + 16px padding. Puts the row about a third
            // down, but never scrolls past the end of the list.
            let end = max(0px, root.tracks.length * 56px - root.list-height);
            root.list-y = -min(end, max(0px, index * 56px - root.list-height / 3));
        }
        // Dark shade of the playing cover's average color; tints the top of the main panel.
        in property <color> now-tint: Theme.panel;
        // Row menu: action is radio/artist/album/like/open/copy, uri is the row's.
        callback row-action(string, string);
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
        // True while the seek bar is dragged, so the timer doesn't fight the user.
        in-out property <bool> seeking;
        changed now => {
            if (now.cover-url != "" && now.cover.width == 0) { root.need-cover(2, 0, now.uri, now.cover-url) }
            if (show-now-panel && now.cover-url != "") { root.now-big = now.cover; root.need-cover(3, 0, now.uri, now.cover-url) }
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
        // The std widgets left (ListView scrollbars) follow the dark palette.
        init => { Palette.color-scheme = ColorScheme.dark; }

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
                    if (event.text == "e") { root.start-radio(); return accept; }
                    if (event.text == "u") { root.show-lyrics = false; root.show-queue(); return accept; }
                }
                if (event.modifiers.alt && event.text == Key.LeftArrow) { root.go-back(); return accept; }
                if (event.modifiers.alt && event.text == Key.RightArrow) { root.go-forward(); return accept; }
                if (event.text == " ") { root.toggle(); return accept; }
                if (event.text == Key.Escape) { self.focus(); return accept; }
                reject
            }
        VerticalLayout {
            padding: 8px;
            spacing: 8px;
            // Collapsed rather than removed in mini mode: ids inside (the search box) stay reachable.
            HorizontalLayout {
                spacing: 8px;
                vertical-stretch: 1;
                visible: !root.mini;
                // Explicit bounds override the children's minimum, which would push the bar out of a mini window.
                min-height: 0px;
                max-height: root.mini ? 0px : 100000px;
                // Your Library
                Rectangle {
                    width: 280px;
                    border-radius: 12px;
                    background: Theme.panel;
                    border-width: 1px;
                    border-color: Theme.border;
                    VerticalLayout {
                        padding: 8px;
                        spacing: 8px;
                        HorizontalLayout {
                            padding-left: 8px;
                            height: 32px;
                            Text {
                                text: "Your Library";
                                font-size: 16px;
                                font-weight: 700;
                                color: Theme.text;
                                vertical-alignment: center;
                                horizontal-stretch: 1;
                            }
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.plus; dot: false; size: 18px; clicked => { root.create-playlist(); } }
                        }
                        HorizontalLayout {
                            spacing: 4px;
                            alignment: start;
                            // Like Spotify: no "All" chip; clicking the chosen filter again clears it.
                            for label[i] in ["Playlists", "Albums", "Artists"] : Chip {
                                label: label;
                                chosen: root.library-kind == i + 1;
                                clicked => {
                                    root.library-kind = root.library-kind == i + 1 ? 0 : i + 1;
                                    root.filter-library(root.library-kind, lib-filter.text, root.library-az);
                                }
                            }
                            Chip {
                                label: "A-Z";
                                chosen: root.library-az;
                                clicked => { root.library-az = !root.library-az; root.filter-library(root.library-kind, lib-filter.text, root.library-az); }
                            }
                        }
                        Rectangle {
                            height: 32px;
                            border-radius: 6px;
                            background: lib-filter.has-focus ? Theme.hover : Theme.field;
                            Icon { x: 8px; width: 16px; height: 16px; y: (parent.height - 16px) / 2; shape: Icons.search; tint: Theme.subdued; }
                            lib-filter := TextInput {
                                x: 32px;
                                width: parent.width - 40px;
                                height: parent.height;
                                vertical-alignment: center;
                                single-line: true;
                                font-size: 13px;
                                color: Theme.text;
                                edited => { root.filter-library(root.library-kind, self.text, root.library-az); }
                            }
                            if lib-filter.text == "" : Text {
                                x: 32px;
                                height: parent.height;
                                vertical-alignment: center;
                                text: "Search in Your Library";
                                font-size: 13px;
                                color: Theme.muted;
                            }
                        }
                        ListView {
                            for row[i] in lists: RowItem {
                                data: row;
                                cover-size: 48px;
                                selected: row.uri == root.current-list;
                                sidebar: true;
                                open-menu(x, y) => { root.open-row-menu(row, x, y, true); }
                                clicked => { root.open-list(row.uri); }
                                need-cover => { root.need-cover(1, i, row.uri, row.cover-url); }
                            }
                        }
                    }
                }
                // Main panel
                Rectangle {
                    border-radius: 12px;
                    background: Theme.panel;
                    border-width: 1px;
                    border-color: Theme.border;
                    Rectangle {
                        y: 0;
                        height: 280px;
                        border-radius: 8px;
                        background: @linear-gradient(180deg, root.now-tint 0%, Theme.panel 100%);
                        animate background { duration: 600ms; }
                    }
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        HorizontalLayout {
                            spacing: 8px;
                            height: 44px;
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.home; filled: root.page-kind == "Home"; dot: false; clicked => { root.go-home(); } }
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.back; dot: false; enabled: root.can-back; clicked => { root.go-back(); } }
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.forward; dot: false; enabled: root.can-forward; clicked => { root.go-forward(); } }
                            // Search pill
                            Rectangle {
                                max-width: 420px;
                                border-radius: 22px;
                                background: query.has-focus ? Theme.hover : Theme.field;
                                border-width: query.has-focus ? 2px : 0px;
                                border-color: Theme.accent;
                                HorizontalLayout {
                                    padding-left: 12px;
                                    padding-right: 16px;
                                    spacing: 10px;
                                    Icon { width: 20px; height: 20px; y: (parent.height - 20px) / 2; shape: Icons.search; tint: Theme.subdued; }
                                    Rectangle {
                                        // Full height + centered text: otherwise the caret is drawn at the top.
                                        query := TextInput {
                                            width: parent.width;
                                            height: parent.height;
                                            vertical-alignment: center;
                                            single-line: true;
                                            font-size: 14px;
                                            color: Theme.text;
                                            accepted => { root.submit(self.text); }
                                        }
                                        if query.text == "" : Text {
                                            x: 0;
                                            width: parent.width;
                                            height: parent.height;
                                            text: "What do you want to play? Search or paste a link";
                                            font-size: 14px;
                                            color: Theme.muted;
                                            vertical-alignment: center;
                                            overflow: elide;
                                        }
                                    }
                                }
                            }
                            Rectangle { horizontal-stretch: 1; }
                            settings-button := IconButton {
                                y: (parent.height - self.height) / 2;
                                shape: Icons.settings;
                                clicked => { settings-popup.show(); }
                            }
                        }
                        if !root.show-lyrics : HorizontalLayout {
                            spacing: 16px;
                            if root.page-cover-url != "" : Rectangle {
                                width: 96px;
                                height: 96px;
                                border-radius: 6px;
                                clip: true;
                                background: Theme.raised;
                                drop-shadow-blur: 12px;
                                drop-shadow-color: #00000080;
                                Image { source: root.page-cover; width: parent.width; height: parent.height; image-fit: cover; }
                            }
                            if root.page-playable : TouchArea {
                                width: 56px;
                                height: 56px;
                                y: (parent.height - self.height) / 2;
                                mouse-cursor: pointer;
                                clicked => { root.play-page(); }
                                Rectangle {
                                    border-radius: self.width / 2;
                                    background: Theme.accent;
                                    drop-shadow-blur: parent.has-hover ? 22px : 12px;
                                    drop-shadow-color: rgba(139, 92, 246, 0.6);
                                    animate drop-shadow-blur { duration: 150ms; }
                                    Icon { width: 26px; height: 26px; shape: Icons.play; filled: true; tint: Theme.text; }
                                }
                            }
                            VerticalLayout {
                                spacing: 2px;
                                alignment: center;
                                if root.page-kind != "" : Text { text: root.page-kind; font-size: 12px; font-weight: 600; color: Theme.text; }
                                HorizontalLayout {
                                    spacing: 8px;
                                    alignment: start;
                                    Text { text: root.page-title; font-size: 32px; font-weight: 800; color: Theme.text; overflow: elide; }
                                    if root.editable : IconButton {
                                        y: (parent.height - self.height) / 2;
                                        shape: Icons.pencil;
                                        dot: false;
                                        size: 18px;
                                        clicked => { root.rename-text = root.page-title; rename-popup.show(); }
                                    }
                                }
                            }
                        }
                        Text { text: root.status; font-size: 12px; color: Theme.subdued; overflow: elide; }
                        Rectangle { height: 1px; background: #ffffff1a; }
                        if root.show-lyrics : LyricsView {
                            vertical-stretch: 1;
                            lines: root.lyric-lines;
                            current: root.lyric-index;
                            note: root.lyrics-note;
                        }
                        if !root.show-lyrics : ListView {
                            vertical-stretch: 1;
                            content-y <=> root.list-y;
                            init => { root.list-height = self.visible-height; }
                            changed visible-height => { root.list-height = self.visible-height; }
                            for row[i] in tracks: RowItem {
                                data: row;
                                number: root.numbered && row.uri.starts-with("spotify:track:") ? i + 1 : 0;
                                playing: row.uri == root.current-track;
                                // "Show more tracks" and similar app rows have no menu.
                                menu: !row.uri.starts-with("slimspot:");
                                clicked => { root.play-uri(row.uri); }
                                open-menu(x, y) => { root.open-row-menu(row, x, y, false); }
                                need-cover => { root.need-cover(0, i, row.uri, row.cover-url); }
                            }
                        }
                    }
                }
                if root.show-now-panel : Rectangle {
                    width: 300px;
                    border-radius: 12px;
                    background: Theme.panel;
                    border-width: 1px;
                    border-color: Theme.border;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        alignment: start;
                        Text { text: "Now playing"; font-size: 16px; font-weight: 700; color: Theme.text; }
                        Rectangle {
                            height: self.width;
                            border-radius: 8px;
                            clip: true;
                            background: Theme.raised;
                            Image { source: root.now-big; width: parent.width; height: parent.height; image-fit: cover; }
                        }
                        Text { text: now.title; font-size: 22px; font-weight: 700; color: Theme.text; wrap: word-wrap; }
                        Text { text: now.artist; font-size: 14px; color: Theme.subdued; wrap: word-wrap; }
                    }
                }
            }
            row-menu := PopupWindow {
                x: root.menu-x;
                y: root.menu-y;
                width: 240px;
                close-policy: close-on-click-outside;
                property <bool> is-track: root.menu-row.uri.starts-with("spotify:track:");
                Rectangle {
                    background: Theme.raised;
                    border-radius: 10px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 24px;
                    drop-shadow-color: rgba(0, 0, 0, 0.6);
                    VerticalLayout {
                        padding: 6px;
                        spacing: 2px;
                        Text {
                            text: root.menu-playlists ? "Add to playlist" : root.menu-row.title;
                            height: 28px;
                            vertical-alignment: center;
                            x: 10px;
                            overflow: elide;
                            font-size: 12px;
                            font-weight: 600;
                            color: Theme.subdued;
                        }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Add to queue"; shape: Icons.queue; clicked => { root.menu-act("queue"); } }
                        if !root.menu-playlists && is-track && root.targets.length > 0 : MenuEntry { label: "Add to playlist"; shape: Icons.plus; submenu: true; clicked => { root.menu-playlists = true; } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Save to Liked Songs"; shape: Icons.heart; clicked => { root.menu-act("like"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Start radio"; shape: Icons.radio; clicked => { root.menu-act("radio"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Go to artist"; shape: Icons.person; clicked => { root.menu-act("artist"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Go to album"; shape: Icons.disc; clicked => { root.menu-act("album"); } }
                        if !root.menu-playlists && !is-track : MenuEntry { label: "Open"; shape: Icons.open; clicked => { root.menu-act("open"); } }
                        if !root.menu-playlists : Rectangle { height: 1px; background: Theme.border; }
                        if !root.menu-playlists : MenuEntry { label: "Copy link"; shape: Icons.link; clicked => { root.menu-act("copy"); } }
                        if !root.menu-playlists && is-track && root.editable && !root.menu-sidebar : MenuEntry { label: "Remove from this playlist"; shape: Icons.trash; danger: true; clicked => { root.menu-act("remove"); } }
                        if !root.menu-playlists && root.menu-sidebar && root.menu-row.uri.starts-with("spotify:playlist:") : MenuEntry { label: "Remove from Your Library"; shape: Icons.trash; danger: true; clicked => { root.menu-act("unfollow"); } }
                        if root.menu-playlists : MenuEntry { label: "Back"; shape: Icons.back; clicked => { root.menu-playlists = false; } }
                        if root.menu-playlists : Rectangle { height: 1px; background: Theme.border; }
                        if root.menu-playlists : VerticalLayout {
                            spacing: 2px;
                            for t in root.targets : MenuEntry { label: t.title; shape: Icons.plus; clicked => { root.menu-act("add:" + t.uri); } }
                        }
                    }
                }
            }
            rename-popup := PopupWindow {
                x: 320px;
                y: 120px;
                width: 360px;
                height: 120px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 8px;
                    drop-shadow-blur: 16px;
                    drop-shadow-color: #00000099;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        Text { text: "Rename playlist"; font-size: 15px; font-weight: 700; color: Theme.text; }
                        Rectangle {
                            height: 36px;
                            border-radius: 4px;
                            background: Theme.field;
                            TextInput {
                                text <=> root.rename-text;
                                x: 10px;
                                width: parent.width - 20px;
                                height: parent.height;
                                vertical-alignment: center;
                                single-line: true;
                                font-size: 14px;
                                color: Theme.text;
                                accepted => { root.rename-playlist(self.text); rename-popup.close(); }
                            }
                        }
                    }
                }
            }
            settings-popup := PopupWindow {
                x: root.width - 420px;
                y: 64px;
                width: 400px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 12px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 24px;
                    drop-shadow-color: rgba(0, 0, 0, 0.6);
                    VerticalLayout {
                        padding: 16px;
                        spacing: 10px;
                        alignment: start;
                        Text { text: "Settings"; font-size: 16px; font-weight: 600; color: Theme.text; }
                        SectionTitle { text: "AUDIO QUALITY"; shape: Icons.volume; }
                        HorizontalLayout {
                            spacing: 6px;
                            alignment: start;
                            for label[i] in ["96 kbps", "160 kbps", "320 kbps"] : Chip {
                                label: label;
                                chosen: root.quality == i;
                                clicked => { root.quality = i; root.set-quality(i); }
                            }
                        }
                        Switch { label: "Normalize volume"; on <=> root.normalize; toggled(v) => { root.set-normalize(v); } }
                        Rectangle { height: 1px; background: Theme.border; }
                        SectionTitle { text: "SLEEP TIMER"; shape: Icons.pause; }
                        HorizontalLayout {
                            spacing: 6px;
                            alignment: start;
                            for choice[i] in [{ label: "Off", m: 0 }, { label: "15 min", m: 15 }, { label: "30 min", m: 30 }, { label: "1 hr", m: 60 }, { label: "End of track", m: -1 }] : Chip {
                                label: choice.label;
                                chosen: root.sleep-minutes == choice.m;
                                clicked => { root.sleep-minutes = choice.m; root.set-sleep(choice.m); }
                            }
                        }
                        Rectangle { height: 1px; background: Theme.border; }
                        SectionTitle { text: "STARTUP"; shape: Icons.home; }
                        Switch { label: "Start with Windows"; on <=> root.start-with-windows; toggled(v) => { root.set-autostart(v); } }
                    }
                }
            }
            devices-popup := PopupWindow {
                x: root.width - 340px;
                y: root.height - 96px - 300px;
                width: 320px;
                height: 290px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 12px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 24px;
                    drop-shadow-color: rgba(0, 0, 0, 0.6);
                    VerticalLayout {
                        padding: 14px;
                        spacing: 8px;
                        Text { text: "Connect to a device"; font-size: 16px; font-weight: 600; color: Theme.text; }
                        SectionTitle { text: "SPOTIFY CONNECT"; shape: Icons.devices; }
                        if root.devices.length == 0 : Text {
                            text: "Looking for devices... (open Spotify on the other device if it's missing)";
                            wrap: word-wrap;
                            font-size: 12px;
                            color: Theme.subdued;
                        }
                        ListView {
                            for d in root.devices : DeviceEntry {
                                data: d;
                                active: d.artist.ends-with("playing here");
                                clicked => { root.transfer(d.uri); devices-popup.close(); }
                            }
                        }
                    }
                }
            }
            // Player bar: a floating capsule under the panels.
            Rectangle {
            height: 80px;
            border-radius: 16px;
            background: Theme.raised;
            border-width: 1px;
            border-color: Theme.border;
            drop-shadow-blur: 18px;
            drop-shadow-color: rgba(0, 0, 0, 0.5);
            HorizontalLayout {
                height: 80px;
                spacing: 16px;
                padding-left: 8px;
                padding-right: 8px;
                // Now playing
                HorizontalLayout {
                    width: 30%;
                    spacing: 12px;
                    alignment: start;
                    Rectangle {
                        width: 56px;
                        height: 56px;
                        y: (parent.height - self.height) / 2;
                        border-radius: 4px;
                        clip: true;
                        background: Theme.raised;
                        Image { source: now.cover; width: parent.width; height: parent.height; image-fit: cover; }
                    }
                    VerticalLayout {
                        alignment: center;
                        spacing: 2px;
                        Text {
                            text: now.title; overflow: elide; font-size: 14px; color: Theme.text;
                            TouchArea { mouse-cursor: pointer; clicked => { root.go-to-playing(); } }
                        }
                        Text { text: now.artist; overflow: elide; font-size: 12px; color: Theme.subdued; }
                    }
                    // Not a toggle: the heart shows only what Spotify reported, even if a click fails.
                    IconButton {
                        y: (parent.height - self.height) / 2;
                        shape: Icons.heart;
                        filled: root.liked;
                        active: root.liked;
                        dot: false;
                        size: 18px;
                        enabled: root.now.uri != "";
                        clicked => { root.toggle-like(); }
                    }
                }
                // Controls + progress
                VerticalLayout {
                    horizontal-stretch: 1;
                    alignment: center;
                    spacing: 4px;
                    HorizontalLayout {
                        alignment: center;
                        spacing: 12px;
                        IconButton { shape: Icons.shuffle; active: root.shuffle; size: 18px; clicked => { root.toggle-shuffle(); } }
                        IconButton { shape: Icons.prev; filled: true; size: 18px; clicked => { root.prev(); } }
                        PlayButton { y: (parent.height - self.height) / 2; playing: root.playing; clicked => { root.toggle(); } }
                        IconButton { shape: Icons.next; filled: true; size: 18px; clicked => { root.next(); } }
                        Rectangle {
                            width: 34px;
                            IconButton { shape: Icons.repeat; active: root.repeat != 0; size: 18px; clicked => { root.cycle-repeat(); } }
                            // Repeat one: a small "1" badge like Spotify's.
                            if root.repeat == 2 : Text { x: 21px; y: 3px; text: "1"; font-size: 9px; font-weight: 700; color: Theme.accent; }
                        }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        Text { text: fmt(position); width: 40px; horizontal-alignment: right; vertical-alignment: center; font-size: 11px; color: Theme.subdued; }
                        Bar {
                            horizontal-stretch: 1;
                            maximum: max(root.duration, 1);
                            value <=> root.position;
                            enabled: root.duration > 0;
                            dragged => { root.seeking = true; }
                            released(v) => { root.seeking = false; root.seek(v); }
                        }
                        Text { text: fmt(duration); width: 40px; vertical-alignment: center; font-size: 11px; color: Theme.subdued; }
                    }
                }
                // Extras + volume
                HorizontalLayout {
                    width: 30%;
                    spacing: 4px;
                    alignment: end;
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.panel; active: root.show-now-panel; size: 18px; clicked => { root.show-now-panel = !root.show-now-panel; } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.mini; active: root.mini; size: 18px; clicked => { root.toggle-mini(); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.lyrics; active: root.show-lyrics; size: 18px; clicked => { root.show-lyrics = !root.show-lyrics; } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.queue; size: 18px; clicked => { root.show-lyrics = false; root.show-queue(); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.radio; size: 18px; enabled: root.now.uri != ""; clicked => { root.start-radio(); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.devices; size: 18px; clicked => { root.load-devices(); devices-popup.show(); } }
                    IconButton {
                        y: (parent.height - self.height) / 2;
                        shape: root.volume <= 0 ? Icons.muted : Icons.volume;
                        size: 18px;
                        dot: false;
                        clicked => { root.volume = root.volume > 0 ? 0 : 50; root.set-volume(root.volume, true); }
                    }
                    Bar {
                        y: (parent.height - self.height) / 2;
                        width: 100px;
                        maximum: 100;
                        value <=> root.volume;
                        dragged(v) => { root.set-volume(v, false); }
                        released(v) => { root.set-volume(v, true); }
                    }
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
            duration: if i.duration_ms > 0 { fmt_duration(i.duration_ms).into() } else { Default::default() },
            liked: i.liked,
        }
    }
}

/// "3:07", like the player bar.
fn fmt_duration(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Sets the liked mark on track-list rows with these URIs, in place so loaded covers stay.
pub fn set_liked_rows(ui: &slint::Weak<App>, uris: Vec<String>, liked: bool) {
    let _ = ui.upgrade_in_event_loop(move |app| {
        let model = app.get_tracks();
        for i in 0..model.row_count() {
            let Some(mut row) = model.row_data(i) else { continue };
            if row.liked != liked && uris.iter().any(|u| row.uri == u.as_str()) {
                row.liked = liked;
                model.set_row_data(i, row);
            }
        }
    });
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

pub fn set_page_header(ui: &slint::Weak<App>, kind: &'static str, title: String, can_back: bool, can_forward: bool) {
    let _ = ui.upgrade_in_event_loop(move |app| {
        app.set_page_kind(kind.into());
        app.set_page_title(title.into());
        app.set_can_back(can_back);
        app.set_can_forward(can_forward);
    });
}

pub fn set_current(ui: &slint::Weak<App>, uri: String, setter: fn(&App, slint::SharedString)) {
    let _ = ui.upgrade_in_event_loop(move |app| setter(&app, uri.into()));
}
