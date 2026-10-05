//! Slint markup and the small helpers that push backend state onto the UI thread.

use slint::{Model, ModelRc, VecModel};

use crate::web::Item;

slint::slint! {
    import { ListView, Palette, ScrollView } from "std-widgets.slint";
    // Poppins (SIL OFL, assets/fonts/OFL.txt), embedded in the exe: ~160 KB per weight.
    import "../assets/fonts/Poppins-Regular.ttf";
    import "../assets/fonts/Poppins-SemiBold.ttf";
    export struct Row { title: string, artist: string, uri: string, cover-url: string, cover: image, duration: string, liked: bool }

    // Spotify-like dark palette. Colors whose hex starts with a digit and `e` are written as rgb():
    // inside slint! the Rust tokenizer reads them as float exponents.
    // "Midnight Indigo": deep navy surfaces, violet accent fading into cyan (ui-ux-pro-max music
    // palette, accent swapped away from Spotify green). Every color is rgb(): inside slint! a hex
    // like #5eead4 tokenizes as a Rust float exponent.
    // Palettes (settings → Theme): 0 Midnight Indigo (default), 1 Nord, 2 Tokyo Night,
    // 3 Catppuccin Mocha, 4 Rosé Pine, 5 Dark (neutral black/grey surfaces, Indigo's accents).
    // Values from each palette's published colors.
    export global Theme {
        in-out property <int> palette;
        out property <color> base: [rgb(8, 8, 26), rgb(46, 52, 64), rgb(26, 27, 38), rgb(17, 17, 27), rgb(25, 23, 36), rgb(0, 0, 0)][palette];
        out property <color> panel: [rgb(16, 16, 42), rgb(59, 66, 82), rgb(31, 35, 53), rgb(30, 30, 46), rgb(31, 29, 46), rgb(18, 18, 18)][palette];
        out property <color> raised: [rgb(27, 27, 56), rgb(67, 76, 94), rgb(41, 46, 66), rgb(49, 50, 68), rgb(38, 35, 58), rgb(31, 31, 31)][palette];
        out property <color> hover: [rgb(35, 35, 74), rgb(76, 86, 106), rgb(52, 59, 88), rgb(69, 71, 90), rgb(57, 53, 82), rgb(42, 42, 42)][palette];
        out property <color> selected: [rgb(44, 44, 94), rgb(86, 95, 118), rgb(61, 70, 110), rgb(88, 91, 112), rgb(82, 79, 103), rgb(51, 51, 51)][palette];
        out property <color> field: [rgb(27, 27, 56), rgb(67, 76, 94), rgb(41, 46, 66), rgb(49, 50, 68), rgb(38, 35, 58), rgb(36, 36, 36)][palette];
        out property <color> border: rgba(255, 255, 255, 0.08);
        out property <color> text: [rgb(248, 250, 252), rgb(236, 239, 244), rgb(192, 202, 245), rgb(205, 214, 244), rgb(224, 222, 244), rgb(255, 255, 255)][palette];
        out property <color> subdued: [rgb(163, 168, 195), rgb(180, 188, 204), rgb(169, 177, 214), rgb(166, 173, 200), rgb(144, 140, 170), rgb(179, 179, 179)][palette];
        out property <color> muted: [rgb(107, 111, 142), rgb(129, 138, 158), rgb(86, 95, 137), rgb(127, 132, 156), rgb(110, 106, 134), rgb(114, 114, 114)][palette];
        out property <color> track: [rgb(46, 46, 90), rgb(76, 86, 106), rgb(59, 66, 97), rgb(69, 71, 90), rgb(57, 53, 82), rgb(77, 77, 77)][palette];
        out property <color> accent: [rgb(139, 92, 246), rgb(136, 192, 208), rgb(122, 162, 247), rgb(203, 166, 247), rgb(196, 167, 231), rgb(139, 92, 246)][palette];
        out property <color> accent2: [rgb(34, 211, 238), rgb(163, 190, 140), rgb(187, 154, 247), rgb(137, 220, 235), rgb(235, 188, 186), rgb(34, 211, 238)][palette];
        // Progress fill only: the software renderer ignores border-radius on gradient backgrounds,
        // so rounded things (buttons, chips, badges) use the solid accent.
        out property <brush> glow: @linear-gradient(135deg, accent 0%, accent2 100%);
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
        // Show / hide a secret field (eye, and eye with a slash).
        out property <string> eye: "M2 12 C5 6 19 6 22 12 C19 18 5 18 2 12 Z M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9 Z";
        out property <string> eye-off: "M2 12 C5 6 19 6 22 12 C19 18 5 18 2 12 Z M12 9 A3 3 0 1 0 12 15 A3 3 0 1 0 12 9 Z M4 4 L20 20";
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
            drop-shadow-color: Theme.accent.transparentize(45%);
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
        // The current row is actually sounding (not paused): hover shows pause instead of play.
        in property <bool> audible;
        in property <length> cover-size: 40px;
        // Row menu (right-click and "..."); off for rows that aren't Spotify items (devices).
        in property <bool> menu: true;
        // Position shown left of the cover in track lists; 0 hides it.
        in property <int> number;
        // Sidebar rows can be removed from the library.
        in property <bool> sidebar;
        // Rows of an own playlist can be dragged; `dropped(n)` reports the drop n rows down (negative: up).
        in property <bool> draggable;
        callback dropped(int);
        // Set while a drag is under way, so the release isn't taken as a click.
        out property <bool> dragged;
        property <length> drag-start;
        // Position in the list and its length, so the drop stays inside the list.
        in property <int> index;
        in property <int> count;
        property <int> drop-offset: max(-root.index, min(root.count - 1 - root.index, round((self.mouse-y - root.drag-start) / root.height)));
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
            if (event.button == PointerEventButton.left && event.kind == PointerEventKind.down) {
                root.dragged = false;
                root.drag-start = self.mouse-y;
            }
            if (root.dragged && event.button == PointerEventButton.left && event.kind == PointerEventKind.up) {
                root.dropped(root.drop-offset);
            }
        }
        moved => {
            // A few pixels of jitter still count as a click.
            if (root.draggable && root.pressed && abs(self.mouse-y - root.drag-start) > 8px) { root.dragged = true; }
        }
        Rectangle {
            border-radius: 8px;
            background: root.dragged && root.pressed ? Theme.selected : root.selected ? Theme.selected : root.has-hover ? Theme.hover : transparent;
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
                    // "Your Episodes" gets Spotify's green tile.
                    background: data.uri == "liked" ? @linear-gradient(135deg, #450af5 0%, #c4efd9 100%) : data.uri == "episodes" ? rgb(0, 100, 80) : data.uri.starts-with("slimspot:") ? transparent : Theme.raised;
                    Image { source: data.cover; width: parent.width; height: parent.height; image-fit: cover; }
                    if data.uri.starts-with("slimspot:") : Icon { width: parent.width * 0.6; height: self.width; shape: Icons.search; tint: Theme.subdued; }
                    if data.uri == "episodes" : Icon { width: parent.width * 0.5; height: self.width; shape: Icons.radio; tint: Theme.text; }
                    if data.uri == "liked" : Icon { width: parent.width * 0.5; height: self.width; shape: Icons.heart; filled: true; tint: Theme.text; }
                    // Tracks: a play arrow on hover, equalizer bars while playing (like Spotify's row number).
                    if (data.uri.starts-with("spotify:track:") || data.uri.starts-with("spotify:episode:")) && (root.has-hover || root.playing) : Rectangle {
                        background: #00000099;
                        Icon {
                            width: parent.width * 0.5;
                            height: self.width;
                            shape: !root.playing ? Icons.play : !root.has-hover ? Icons.bars : root.audible ? Icons.pause : Icons.play;
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
        // Drop marker: an accent line where the dragged row will land (below the target row
        // when moving down, above it when moving up).
        if root.dragged && root.pressed && root.drop-offset != 0 : Rectangle {
            x: 8px;
            width: parent.width - 16px;
            height: 2px;
            y: (root.drop-offset > 0 ? root.drop-offset + 1 : root.drop-offset) * root.height - 1px;
            background: Theme.accent;
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
        // Total horizontal padding; the sidebar's filter row uses less so four chips fit.
        in property <length> pad: 20px;
        height: 30px;
        mouse-cursor: pointer;
        min-width: t.preferred-width + root.pad;
        Rectangle {
            border-radius: 15px;
            background: root.chosen ? Theme.accent : root.has-hover ? Theme.hover : Theme.raised;
            border-width: root.chosen ? 0px : 1px;
            border-color: Theme.border;
            t := Text { text: root.label; font-size: 12px; font-weight: 600; color: Theme.text; }
        }
    }

    // Home quick-access tile: cover and title, like Spotify's grid at the top of Home.
    component QuickTile inherits TouchArea {
        in property <Row> data;
        callback need-cover();
        init => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        changed data => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        height: 48px;
        horizontal-stretch: 1;
        mouse-cursor: pointer;
        Rectangle {
            border-radius: 6px;
            background: root.has-hover ? Theme.hover : Theme.raised;
            HorizontalLayout {
                spacing: 10px;
                padding-right: 8px;
                Rectangle {
                    width: 48px;
                    background: root.data.uri == "liked" ? Theme.accent : Theme.selected;
                    Image { source: root.data.cover; width: parent.width; height: parent.height; image-fit: cover; }
                    if root.data.uri == "liked" : Icon { width: 22px; height: 22px; shape: Icons.heart; filled: true; tint: Theme.text; }
                }
                VerticalLayout {
                    alignment: center;
                    Text { text: root.data.title; font-size: 12px; font-weight: 600; color: Theme.text; overflow: elide; }
                    if root.data.artist.starts-with("New release") : Text { text: root.data.artist; font-size: 11px; color: Theme.accent; overflow: elide; }
                }
            }
        }
    }

    // ---- Winamp classic skin (src/skin.rs loads the .wsz; sprite coordinates are Winamp's / webamp's) ----
    export struct Glyph { x: int, y: int }
    export struct WaSkin {
        main: image, cbuttons: image, titlebar: image, posbar: image, volume: image, balance: image,
        shufrep: image, numbers: image, nums-ex: bool, text: image, playpaus: image, monoster: image,
        eqmain: image, pledit: image,
        pl-normal: color, pl-current: color, pl-normal-bg: color, pl-selected-bg: color, pl-font: string,
        loaded: bool,
    }
    export struct EqPreset { name: string, bands: [float] }

    // A (sx, sy, w, h) cut of a sprite sheet drawn at skin pixel (px, py), scaled by `s`.
    component Sprite inherits Rectangle {
        in property <image> src;
        in property <length> s: 1px;
        in property <int> px;
        in property <int> py;
        in property <int> sx;
        in property <int> sy;
        in property <int> w;
        in property <int> h;
        x: px * s;
        y: py * s;
        width: w * s;
        height: h * s;
        Image {
            width: 100%;
            height: 100%;
            source: root.src;
            source-clip-x: root.sx;
            source-clip-y: root.sy;
            source-clip-width: root.w;
            source-clip-height: root.h;
            image-fit: fill;
            image-rendering: pixelated;
        }
    }

    // Shows (nx, ny) normally and (ax, ay) while pressed.
    component SkinButton inherits Rectangle {
        in property <image> src;
        in property <length> s: 1px;
        in property <int> px;
        in property <int> py;
        in property <int> w;
        in property <int> h;
        in property <int> nx;
        in property <int> ny;
        in property <int> ax;
        in property <int> ay;
        callback clicked();
        x: px * s;
        y: py * s;
        width: w * s;
        height: h * s;
        Sprite { src: root.src; s: root.s; w: root.w; h: root.h; sx: ta.pressed ? root.ax : root.nx; sy: ta.pressed ? root.ay : root.ny; }
        ta := TouchArea { clicked => { root.clicked(); } }
    }

    // Horizontal skin slider: draws only the thumb; the caller draws the track. value 0..1.
    component SkinSlider inherits Rectangle {
        in property <image> src;
        in property <length> s: 1px;
        in property <int> px;
        in property <int> py;
        in property <int> w;
        in property <int> h;
        in property <int> tw;
        in property <int> th;
        in property <int> ty;
        in property <int> nx;
        in property <int> ny;
        in property <int> ax;
        in property <int> ay;
        in property <bool> enabled: true;
        in-out property <float> value;
        out property <bool> dragging: ta.pressed;
        callback moved(float);
        callback released(float);
        x: px * s;
        y: py * s;
        width: w * s;
        height: h * s;
        function at(mx: length) -> float { return clamp((mx / root.s - root.tw / 2) / (root.w - root.tw), 0, 1); }
        if root.enabled : Sprite {
            src: root.src;
            s: root.s;
            px: round(root.value * (root.w - root.tw));
            py: root.ty;
            w: root.tw;
            h: root.th;
            sx: ta.pressed ? root.ax : root.nx;
            sy: ta.pressed ? root.ay : root.ny;
        }
        ta := TouchArea {
            enabled: root.enabled;
            pointer-event(e) => {
                if (e.button != PointerEventButton.left) { return; }
                if (e.kind == PointerEventKind.down) { root.value = at(self.mouse-x); root.moved(root.value); }
                if (e.kind == PointerEventKind.up) { root.released(root.value); }
            }
            moved => { if (self.pressed) { root.value = at(self.mouse-x); root.moved(root.value); } }
        }
    }

    // EQ band slider (14x63, +12 dB at the top); value -1..1.
    component EqSlider inherits Rectangle {
        in property <image> src;
        in property <length> s: 1px;
        in property <int> px;
        in property <int> py;
        in-out property <float> value;
        callback moved(float);
        callback released(float);
        x: px * s;
        y: py * s;
        width: 14 * s;
        height: 63 * s;
        property <int> frame: round((root.value + 1) / 2 * 27);
        function at(my: length) -> float { return clamp(1 - (my / root.s - 5.5) / (63 - 11) * 2, -1, 1); }
        Sprite { src: root.src; s: root.s; w: 14; h: 63; sx: 13 + Math.mod(root.frame, 14) * 15; sy: 164 + floor(root.frame / 14) * 65; }
        Sprite { src: root.src; s: root.s; px: 1; py: round((1 - root.value) / 2 * (63 - 11)); w: 11; h: 11; sx: 0; sy: ta.pressed ? 176 : 164; }
        ta := TouchArea {
            pointer-event(e) => {
                if (e.button != PointerEventButton.left) { return; }
                if (e.kind == PointerEventKind.down) { root.value = at(self.mouse-y); root.moved(root.value); }
                if (e.kind == PointerEventKind.up) { root.released(root.value); }
            }
            moved => { if (self.pressed) { root.value = at(self.mouse-y); root.moved(root.value); } }
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
        min-width: root.skin-mode ? root.wa-w : root.mini ? 480px : 760px;
        min-height: root.skin-mode ? root.wa-h : root.mini ? 72px : 480px;
        max-width: root.skin-mode ? root.wa-w : 100000px;
        max-height: root.skin-mode ? root.wa-h : 100000px;
        always-on-top: root.mini || (root.skin-mode && root.wa-on-top);
        // The skin draws its own title bars (moved by `instance::start_move`).
        no-frame: root.skin-mode;
        // Settings → GPU renderer (applies on restart).
        in-out property <bool> gpu;
        callback set-gpu(bool);
        // Winamp skin mode: the window becomes the skin's main (+ EQ + playlist) windows, stacked.
        in-out property <bool> skin-mode;
        in property <WaSkin> skin;
        in-out property <bool> wa-double;
        in-out property <bool> wa-on-top;
        in-out property <bool> wa-eq: true;
        in-out property <bool> wa-pl: true;
        in-out property <bool> wa-remaining;
        in-out property <bool> wa-auto;
        in-out property <int> wa-sel: -1;
        in-out property <length> wa-pl-y;
        // Scrolling song title and the playlist's time readouts, in text.bmp cells (from Rust).
        in property <[Glyph]> wa-title;
        in property <[Glyph]> wa-pl-time;
        in property <[Glyph]> wa-mini-time;
        property <bool> wa-blink;
        property <length> wa-s: root.wa-double ? 2px : 1px;
        property <length> wa-w: 275 * root.wa-s;
        property <int> wa-eq-y: 116;
        property <int> wa-pl-top: root.wa-eq ? 232 : 116;
        property <length> wa-h: (116 + (root.wa-eq ? 116 : 0) + (root.wa-pl ? 232 : 0)) * root.wa-s;
        changed wa-h => { if (root.skin-mode) { root.skin-resize(root.wa-w, root.wa-h); } }
        changed wa-w => { if (root.skin-mode) { root.skin-resize(root.wa-w, root.wa-h); } }
        callback skin-toggle();
        callback skin-choose();
        callback skin-builtin();
        callback skin-resize(length, length);
        callback wa-drag();
        callback wa-minimize();
        callback wa-close();
        // EQ window state; bands and preamp -1..1 (±12 dB), balance -1..1. eq-changed(save).
        in-out property <bool> eq-on;
        in-out property <float> eq-preamp;
        in-out property <[float]> eq-bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        in-out property <float> wa-balance;
        callback eq-changed(bool);
        // Winamp's built-in presets, in dB / 12.
        property <[EqPreset]> eq-presets: [
            { name: "Flat", bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
            { name: "Classical", bands: [0, 0, 0, 0, 0, 0, -0.36, -0.36, -0.36, -0.48] },
            { name: "Dance", bands: [0.48, 0.36, 0.12, 0, 0, -0.28, -0.36, -0.36, 0, 0] },
            { name: "Full Bass", bands: [0.4, 0.4, 0.4, 0.24, 0.08, -0.2, -0.4, -0.52, -0.56, -0.56] },
            { name: "Full Treble", bands: [-0.48, -0.48, -0.48, -0.2, 0.12, 0.56, 0.8, 0.8, 0.8, 0.84] },
            { name: "Pop", bands: [-0.08, 0.24, 0.36, 0.4, 0.28, 0, -0.12, -0.12, -0.08, -0.08] },
            { name: "Rock", bands: [0.4, 0.24, -0.28, -0.4, -0.16, 0.2, 0.44, 0.56, 0.56, 0.56] },
            { name: "Techno", bands: [0.4, 0.28, 0, -0.28, -0.24, 0, 0.4, 0.48, 0.48, 0.44] },
        ];
        in property <string> status: "Starting...";
        // Big heading of the main panel and the back/forward arrows next to it.
        in property <string> page-title: "Home";
        in property <string> page-kind;
        in property <string> page-info;
        // Artist/album pages: the page's URI and whether it's followed/saved (header button).
        in property <string> page-uri;
        in property <bool> page-saved;
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
            row-menu.close();
            // Removing from the library asks first (a playlist of your own is deleted).
            if (action == "unfollow") {
                root.confirm-row = root.menu-row;
                confirm-popup.show();
                return;
            }
            root.row-action(action, root.menu-row.uri);
        }
        in-out property <Row> confirm-row;
        callback play-page();
        callback filter-library(int, string, int);
        // Find in page: 0 = page order, 1 = title, 2 = artist, 3 = duration. Rust resets both on a new page.
        in-out property <string> page-filter-text;
        in-out property <int> page-sort;
        callback filter-page(string, int);
        callback move-row(int, int);
        // "Add songs" popup on own playlists: search results and adding one to the page.
        in property <[Row]> add-results;
        callback add-search(string);
        callback add-to-page(string);
        callback save-radio();
        // Settings → Theme: Rust saves it and recolors the title bar.
        callback set-theme(int);
        // Settings → Spotify app Client ID (settings.json `web_client_id`).
        in-out property <string> web-client-id;
        in-out property <bool> show-client-id;
        callback set-web-client-id(string);
        // Search page tab (0 = All, Songs, Albums, Artists, Playlists); -1 on other pages.
        in property <int> search-tab: -1;
        callback choose-search-tab(int);
        // Tab labels of the open page (search, artist); empty elsewhere.
        in property <[string]> page-tabs;
        // Artist "About" tab text, shown in place of the list.
        in property <string> page-about;
        // Home tiles, two rows (Spotify's quick-access grid).
        in property <[Row]> quick-top;
        in property <[Row]> quick-bottom;
        // Now-playing panel: start of the artist's biography.
        in property <string> now-about;
        // Track credits popup.
        in property <string> credits-title;
        in property <string> credits-text;
        public function show-credits() { credits-popup.show(); }
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
        callback create-playlist(string, string);
        // The details popup creates a new playlist instead of editing the open one.
        in-out property <bool> creating;
        // Own playlist's edit popup: name, description (prefilled from page-info) and cover.
        callback edit-playlist(string, string);
        callback change-cover();
        function save-details() {
            if (root.creating) { root.create-playlist(root.rename-text, root.desc-text); } else { root.edit-playlist(root.rename-text, root.desc-text); }
        }
        in-out property <string> rename-text;
        in-out property <string> desc-text;
        // Sidebar sort: 0 = library order, 1 = recently opened, 2 = A-Z.
        in-out property <int> library-sort;
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

        // Winamp's Stop: pause and go back to the start.
        function wa-stop() {
            if (root.playing) { root.toggle(); }
            root.position = 0;
            root.seek(0);
        }
        // Time display: elapsed, or remaining after a click on it.
        property <float> wa-ms: root.wa-remaining && root.duration > 0 ? max(0, root.duration - root.position) : root.position;
        property <int> wa-min: min(99, floor(root.wa-ms / 60000));
        property <int> wa-sec: Math.mod(floor(root.wa-ms / 1000), 60);
        property <int> wa-kbps: root.quality == 0 ? 96 : root.quality == 1 ? 160 : 320;

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
                    if (event.text == "f" && root.page-playable) { page-filter.focus(); return accept; }
                    if (event.text == "f" || event.text == "l") { query.focus(); return accept; }
                    if (event.text == "s") { root.toggle-shuffle(); return accept; }
                    if (event.text == "r") { root.cycle-repeat(); return accept; }
                    if (event.text == "q") { root.quit(); return accept; }
                    if (event.text == "y") { root.show-lyrics = !root.show-lyrics; return accept; }
                    if (event.text == "e") { root.start-radio(); return accept; }
                    if (event.text == "u") { root.show-lyrics = false; root.show-queue(); return accept; }
                    if (event.text == "w") { root.skin-toggle(); return accept; }
                }
                if (root.skin-mode && !event.modifiers.control && !event.modifiers.alt) {
                    if (event.text == "z") { root.prev(); return accept; }
                    if (event.text == "x") { if (!root.playing) { root.toggle(); } return accept; }
                    if (event.text == "c") { root.toggle(); return accept; }
                    if (event.text == "v") { root.wa-stop(); return accept; }
                    if (event.text == "b") { root.next(); return accept; }
                }
                if (event.modifiers.alt && event.text == Key.LeftArrow) { root.go-back(); return accept; }
                if (event.modifiers.alt && event.text == Key.RightArrow) { root.go-forward(); return accept; }
                if (event.text == " ") { root.toggle(); return accept; }
                if (event.text == Key.Escape) { self.focus(); return accept; }
                reject
            }
        if root.skin-mode : wa := Rectangle {
            property <length> s: root.wa-s;
            property <length> menu-x;
            property <length> menu-y;
            // Menus open where they were asked for, kept inside the (small) window.
            function open-menu(x: length, y: length) {
                self.menu-x = max(0px, min(x, self.width - 150px));
                self.menu-y = max(0px, min(y, self.height - 130px));
                options-menu.show();
            }
            // ADD / REM / SEL / MISC under the playlist: one popup, items picked by `pl-kind`.
            property <string> pl-kind;
            // The selected row, else the playing track.
            property <string> pl-uri: root.wa-sel >= 0 && root.wa-sel < root.tracks.length ? root.tracks[root.wa-sel].uri : root.current-track;
            property <bool> pl-track: self.pl-uri.starts-with("spotify:track:");
            property <bool> pl-chosen: root.wa-sel >= 0 && root.wa-sel < root.tracks.length;
            function open-pl(kind: string, x: length, y: length) {
                self.pl-kind = kind;
                self.menu-x = max(0px, min(x, self.width - 190px));
                self.menu-y = max(0px, min(y - 150px, self.height - 150px));
                pl-menu.show();
            }
            function pl-act(act: string) {
                if (act == "search") { root.skin-toggle(); query.focus(); }
                if (act == "liked") { root.wa-sel = -1; root.open-list("liked"); }
                if (act == "home") { root.wa-sel = -1; root.go-home(); }
                if (act == "queue-view") { root.wa-sel = -1; root.show-queue(); }
                if (act == "sel-none") { root.wa-sel = -1; }
                if (act == "sel-first" && root.tracks.length > 0) { root.wa-sel = 0; root.wa-pl-y = 0; }
                if (act == "sel-last" && root.tracks.length > 0) { root.wa-sel = root.tracks.length - 1; root.wa-pl-y = -max(0px, root.tracks.length * 13 * self.s - 174 * self.s); }
                if (act == "remove" || act == "queue" || act == "like" || act == "radio" || act == "album" || act == "artist" || act == "copy") {
                    // These change the page (or its rows): the old selection index means nothing there.
                    if (act == "remove" || act == "radio" || act == "album" || act == "artist") { root.wa-sel = -1; }
                    root.row-action(act, self.pl-uri);
                }
            }
            function open-lists(x: length, y: length) {
                self.menu-x = max(0px, min(x - 200px, self.width - 220px));
                self.menu-y = max(0px, min(y - 240px, self.height - 240px));
                lists-menu.show();
            }
            background: black;
            // Winamp blinks the time while paused.
            Timer {
                interval: 1s;
                running: !root.playing && root.now.uri != "";
                triggered => { root.wa-blink = !root.wa-blink; }
            }
            // ---------- Main window ----------
            Sprite { src: root.skin.main; s: parent.s; w: 275; h: 116; }
            Sprite { src: root.skin.titlebar; s: parent.s; w: 275; h: 14; sx: 27; sy: 0; }
            TouchArea { x: 0; y: 0; width: 240 * parent.s; height: 14 * parent.s; pointer-event(e) => {
                if (e.kind == PointerEventKind.down && e.button == PointerEventButton.left) { root.wa-drag(); }
                if (e.kind == PointerEventKind.up && e.button == PointerEventButton.right) { wa.open-menu(self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y); }
            } }
            // Options button: the options menu (also right click on any title bar).
            SkinButton { src: root.skin.titlebar; s: parent.s; px: 6; py: 3; w: 9; h: 9; nx: 0; ny: 0; ax: 0; ay: 9; clicked => { wa.open-menu(6 * wa.s, 12 * wa.s); } }
            SkinButton { src: root.skin.titlebar; s: parent.s; px: 244; py: 3; w: 9; h: 9; nx: 9; ny: 0; ax: 9; ay: 9; clicked => { root.wa-minimize(); } }
            SkinButton { src: root.skin.titlebar; s: parent.s; px: 254; py: 3; w: 9; h: 9; nx: 0; ny: 18; ax: 9; ay: 18; clicked => { root.skin-toggle(); } }
            SkinButton { src: root.skin.titlebar; s: parent.s; px: 264; py: 3; w: 9; h: 9; nx: 18; ny: 0; ax: 18; ay: 9; clicked => { root.wa-close(); } }
            // Clutter bar: O = options menu, A = always on top, D = double size.
            Sprite { src: root.skin.titlebar; s: parent.s; px: 10; py: 22; w: 8; h: 43; sx: 304; sy: 0; }
            if root.wa-on-top : Sprite { src: root.skin.titlebar; s: root.wa-s; px: 10; py: 33; w: 8; h: 7; sx: 312; sy: 55; }
            if root.wa-double : Sprite { src: root.skin.titlebar; s: root.wa-s; px: 10; py: 47; w: 8; h: 8; sx: 328; sy: 69; }
            TouchArea { x: 10 * parent.s; y: 25 * parent.s; width: 8 * parent.s; height: 8 * parent.s; clicked => { wa.open-menu(18 * wa.s, 25 * wa.s); } }
            TouchArea { x: 10 * parent.s; y: 33 * parent.s; width: 8 * parent.s; height: 7 * parent.s; clicked => { root.wa-on-top = !root.wa-on-top; } }
            TouchArea { x: 10 * parent.s; y: 47 * parent.s; width: 8 * parent.s; height: 8 * parent.s; clicked => { root.wa-double = !root.wa-double; } }
            // Play state, time, title, bitrate, stereo.
            Sprite { src: root.skin.playpaus; s: parent.s; px: 26; py: 28; w: 9; h: 9; sx: root.playing ? 0 : root.position > 0 ? 9 : 18; sy: 0; }
            Rectangle {
                visible: root.now.uri != "" && (root.playing || !root.wa-blink);
                if root.wa-remaining && !root.skin.nums-ex : Sprite { src: root.skin.numbers; s: root.wa-s; px: 39; py: 32; w: 5; h: 1; sx: 20; sy: 6; }
                if root.wa-remaining && root.skin.nums-ex : Sprite { src: root.skin.numbers; s: root.wa-s; px: 36; py: 26; w: 9; h: 13; sx: 99; sy: 0; }
                Sprite { src: root.skin.numbers; s: root.wa-s; px: 48; py: 26; w: 9; h: 13; sx: floor(root.wa-min / 10) * 9; }
                Sprite { src: root.skin.numbers; s: root.wa-s; px: 60; py: 26; w: 9; h: 13; sx: Math.mod(root.wa-min, 10) * 9; }
                Sprite { src: root.skin.numbers; s: root.wa-s; px: 78; py: 26; w: 9; h: 13; sx: floor(root.wa-sec / 10) * 9; }
                Sprite { src: root.skin.numbers; s: root.wa-s; px: 90; py: 26; w: 9; h: 13; sx: Math.mod(root.wa-sec, 10) * 9; }
            }
            TouchArea { x: 36 * parent.s; y: 26 * parent.s; width: 63 * parent.s; height: 13 * parent.s; clicked => { root.wa-remaining = !root.wa-remaining; } }
            for g[i] in root.wa-title : Sprite { src: root.skin.text; s: root.wa-s; px: 111 + i * 5; py: 27; w: 5; h: 6; sx: g.x; sy: g.y; }
            if root.now.uri != "" : Rectangle {
                for i in 3 : Sprite {
                    property <int> place: i == 0 ? 100 : i == 1 ? 10 : 1;
                    src: root.skin.text;
                    s: root.wa-s;
                    px: 111 + i * 5;
                    py: 43;
                    w: 5;
                    h: 6;
                    sx: root.wa-kbps < self.place && i < 2 ? 150 : Math.mod(floor(root.wa-kbps / self.place), 10) * 5;
                    sy: root.wa-kbps < self.place && i < 2 ? 0 : 6;
                }
                // librespot always decodes to 44.1 kHz stereo.
                Sprite { src: root.skin.text; s: root.wa-s; px: 156; py: 43; w: 5; h: 6; sx: 20; sy: 6; }
                Sprite { src: root.skin.text; s: root.wa-s; px: 161; py: 43; w: 5; h: 6; sx: 20; sy: 6; }
            }
            Sprite { src: root.skin.monoster; s: parent.s; px: 212; py: 41; w: 27; h: 12; sx: 29; sy: 12; }
            Sprite { src: root.skin.monoster; s: parent.s; px: 239; py: 41; w: 29; h: 12; sx: 0; sy: root.now.uri != "" ? 0 : 12; }
            // Volume and balance: the track is one of 28 frames picked by the value.
            Sprite { src: root.skin.volume; s: parent.s; px: 107; py: 57; w: 68; h: 13; sy: round(root.volume / 100 * 27) * 15; }
            SkinSlider {
                src: root.skin.volume; s: parent.s; px: 107; py: 57; w: 68; h: 13;
                tw: 14; th: 11; ty: 1; nx: 15; ny: 422; ax: 0; ay: 422;
                value: root.volume / 100;
                moved(v) => { root.volume = v * 100; root.set-volume(root.volume, false); }
                released(v) => { root.set-volume(v * 100, true); }
            }
            Sprite { src: root.skin.balance; s: parent.s; px: 177; py: 57; w: 38; h: 13; sx: 9; sy: round(abs(root.wa-balance) * 27) * 15; }
            SkinSlider {
                src: root.skin.balance; s: parent.s; px: 177; py: 57; w: 38; h: 13;
                tw: 14; th: 11; ty: 1; nx: 15; ny: 422; ax: 0; ay: 422;
                value: (root.wa-balance + 1) / 2;
                // Snaps to the centre like Winamp's.
                moved(v) => { root.wa-balance = abs(v * 2 - 1) < 0.08 ? 0 : v * 2 - 1; root.eq-changed(false); }
                released(v) => { root.eq-changed(true); }
            }
            SkinButton { src: root.skin.shufrep; s: parent.s; px: 219; py: 58; w: 23; h: 12; nx: 0; ny: root.wa-eq ? 73 : 61; ax: 46; ay: root.wa-eq ? 73 : 61; clicked => { root.wa-eq = !root.wa-eq; } }
            SkinButton { src: root.skin.shufrep; s: parent.s; px: 242; py: 58; w: 23; h: 12; nx: 23; ny: root.wa-pl ? 73 : 61; ax: 69; ay: root.wa-pl ? 73 : 61; clicked => { root.wa-pl = !root.wa-pl; } }
            // Seek bar.
            Sprite { src: root.skin.posbar; s: parent.s; px: 16; py: 72; w: 248; h: 10; }
            SkinSlider {
                src: root.skin.posbar; s: parent.s; px: 16; py: 72; w: 248; h: 10;
                tw: 29; th: 10; ty: 0; nx: 248; ny: 0; ax: 278; ay: 0;
                enabled: root.duration > 0;
                value: root.position / max(root.duration, 1);
                moved(v) => { root.seeking = true; root.position = v * root.duration; }
                released(v) => { root.seeking = false; root.seek(v * root.duration); }
            }
            // Transport.
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 16; py: 88; w: 23; h: 18; nx: 0; ny: 0; ax: 0; ay: 18; clicked => { root.prev(); } }
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 39; py: 88; w: 23; h: 18; nx: 23; ny: 0; ax: 23; ay: 18; clicked => { if (!root.playing) { root.toggle(); } } }
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 62; py: 88; w: 23; h: 18; nx: 46; ny: 0; ax: 46; ay: 18; clicked => { root.toggle(); } }
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 85; py: 88; w: 23; h: 18; nx: 69; ny: 0; ax: 69; ay: 18; clicked => { root.wa-stop(); } }
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 108; py: 88; w: 22; h: 18; nx: 92; ny: 0; ax: 92; ay: 18; clicked => { root.next(); } }
            // Eject ("open"): back to the normal window to pick something to play.
            SkinButton { src: root.skin.cbuttons; s: parent.s; px: 136; py: 89; w: 22; h: 16; nx: 114; ny: 0; ax: 114; ay: 16; clicked => { root.skin-toggle(); } }
            SkinButton { src: root.skin.shufrep; s: parent.s; px: 164; py: 89; w: 47; h: 15; nx: 28; ny: root.shuffle ? 30 : 0; ax: 28; ay: root.shuffle ? 45 : 15; clicked => { root.toggle-shuffle(); } }
            SkinButton { src: root.skin.shufrep; s: parent.s; px: 210; py: 89; w: 28; h: 15; nx: 0; ny: root.repeat != 0 ? 30 : 0; ax: 0; ay: root.repeat != 0 ? 45 : 15; clicked => { root.cycle-repeat(); } }

            // ---------- Equalizer ----------
            if root.wa-eq : Rectangle {
                property <length> s: root.wa-s;
                x: 0;
                y: root.wa-eq-y * self.s;
                width: 275 * self.s;
                height: 116 * self.s;
                Sprite { src: root.skin.eqmain; s: parent.s; w: 275; h: 116; }
                Sprite { src: root.skin.eqmain; s: parent.s; w: 275; h: 14; sx: 0; sy: 134; }
                TouchArea { x: 0; y: 0; width: 260 * parent.s; height: 14 * parent.s; pointer-event(e) => {
                if (e.kind == PointerEventKind.down && e.button == PointerEventButton.left) { root.wa-drag(); }
                if (e.kind == PointerEventKind.up && e.button == PointerEventButton.right) { wa.open-menu(self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y); }
            } }
                SkinButton { src: root.skin.eqmain; s: parent.s; px: 264; py: 3; w: 9; h: 9; nx: 0; ny: 116; ax: 0; ay: 125; clicked => { root.wa-eq = false; } }
                SkinButton {
                    src: root.skin.eqmain; s: parent.s; px: 14; py: 18; w: 26; h: 12;
                    nx: root.eq-on ? 69 : 10; ny: 119; ax: root.eq-on ? 187 : 128; ay: 119;
                    clicked => { root.eq-on = !root.eq-on; root.eq-changed(true); }
                }
                // AUTO (per-file presets) has nothing to load here; it only toggles like Winamp's.
                SkinButton {
                    src: root.skin.eqmain; s: parent.s; px: 40; py: 18; w: 32; h: 12;
                    nx: root.wa-auto ? 95 : 36; ny: 119; ax: root.wa-auto ? 213 : 154; ay: 119;
                    clicked => { root.wa-auto = !root.wa-auto; }
                }
                Sprite { src: root.skin.eqmain; s: parent.s; px: 86; py: 17; w: 113; h: 19; sx: 0; sy: 294; }
                SkinButton { src: root.skin.eqmain; s: parent.s; px: 217; py: 18; w: 44; h: 12; nx: 224; ny: 164; ax: 224; ay: 176; clicked => { presets-popup.show(); } }
                EqSlider {
                    src: root.skin.eqmain; s: parent.s; px: 21; py: 38;
                    value: root.eq-preamp;
                    moved(v) => { root.eq-preamp = v; root.eq-changed(false); }
                    released(v) => { root.eq-changed(true); }
                }
                for band[i] in root.eq-bands : EqSlider {
                    src: root.skin.eqmain; s: root.wa-s; px: 78 + i * 18; py: 38;
                    value: band;
                    moved(v) => { root.eq-bands[i] = v; root.eq-changed(false); }
                    released(v) => { root.eq-changed(true); }
                }
                presets-popup := PopupWindow {
                    x: 217 * parent.s;
                    y: 31 * parent.s;
                    width: 110px;
                    Rectangle {
                        background: #d4d0c8;
                        border-width: 1px;
                        border-color: #404040;
                        VerticalLayout {
                            padding: 2px;
                            for p in root.eq-presets : Rectangle {
                                height: 18px;
                                background: pa.has-hover ? #0a246a : transparent;
                                Text { x: 8px; text: p.name; font-size: 11px; vertical-alignment: center; color: pa.has-hover ? white : black; }
                                pa := TouchArea {
                                    clicked => {
                                        root.eq-bands = p.bands;
                                        root.eq-on = true;
                                        root.eq-changed(true);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ---------- Playlist ----------
            if root.wa-pl : Rectangle {
                property <length> s: root.wa-s;
                // Scroll handle position over the list's scroll range.
                property <length> pl-range: max(1px, root.tracks.length * 13 * self.s - 174 * self.s);
                property <float> pl-pos: clamp(-root.wa-pl-y / self.pl-range, 0, 1);
                x: 0;
                y: root.wa-pl-top * self.s;
                width: 275 * self.s;
                height: 232 * self.s;
                Rectangle { x: 12 * parent.s; y: 20 * parent.s; width: 243 * parent.s; height: 174 * parent.s; background: root.skin.pl-normal-bg; }
                for i in 11 : Sprite { src: root.skin.pledit; s: root.wa-s; px: i * 25; w: 25; h: 20; sx: 127; sy: 0; }
                Sprite { src: root.skin.pledit; s: parent.s; w: 25; h: 20; sx: 0; sy: 0; }
                Sprite { src: root.skin.pledit; s: parent.s; px: 87; w: 100; h: 20; sx: 26; sy: 0; }
                Sprite { src: root.skin.pledit; s: parent.s; px: 250; w: 25; h: 20; sx: 153; sy: 0; }
                TouchArea { x: 0; y: 0; width: 260 * parent.s; height: 20 * parent.s; pointer-event(e) => {
                if (e.kind == PointerEventKind.down && e.button == PointerEventButton.left) { root.wa-drag(); }
                if (e.kind == PointerEventKind.up && e.button == PointerEventButton.right) { wa.open-menu(self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y); }
            } }
                TouchArea { x: 264 * parent.s; y: 3 * parent.s; width: 9 * parent.s; height: 9 * parent.s; clicked => { root.wa-pl = false; } }
                for i in 6 : Sprite { src: root.skin.pledit; s: root.wa-s; py: 20 + i * 29; w: 12; h: 29; sx: 0; sy: 42; }
                for i in 6 : Sprite { src: root.skin.pledit; s: root.wa-s; px: 255; py: 20 + i * 29; w: 20; h: 29; sx: 31; sy: 42; }
                Sprite { src: root.skin.pledit; s: parent.s; py: 194; w: 125; h: 38; sx: 0; sy: 72; }
                Sprite { src: root.skin.pledit; s: parent.s; px: 125; py: 194; w: 150; h: 38; sx: 126; sy: 72; }
                // The page's tracks; double-click plays (the normal list plays on a single click).
                ListView {
                    x: 12 * parent.s;
                    y: 20 * parent.s;
                    width: 243 * parent.s;
                    height: 174 * parent.s;
                    vertical-scrollbar-policy: always-off;
                    horizontal-scrollbar-policy: always-off;
                    content-y <=> root.wa-pl-y;
                    for row[i] in root.tracks : Rectangle {
                        property <length> s: root.wa-s;
                        property <color> ink: row.uri == root.current-track ? root.skin.pl-current : root.skin.pl-normal;
                        height: 13 * self.s;
                        background: i == root.wa-sel ? root.skin.pl-selected-bg : transparent;
                        Text {
                            x: 2 * parent.s;
                            width: parent.width - 40 * parent.s;
                            height: parent.height;
                            text: (row.uri.starts-with("slimspot:") ? "" : (i + 1) + ". ") + (row.artist == "" ? "" : row.artist + " - ") + row.title;
                            font-family: root.skin.pl-font;
                            font-size: 9 * parent.s;
                            color: parent.ink;
                            vertical-alignment: center;
                            overflow: elide;
                        }
                        Text {
                            x: parent.width - 36 * parent.s;
                            width: 34 * parent.s;
                            height: parent.height;
                            text: row.duration;
                            font-family: root.skin.pl-font;
                            font-size: 9 * parent.s;
                            color: parent.ink;
                            horizontal-alignment: right;
                            vertical-alignment: center;
                        }
                        TouchArea {
                            clicked => { root.wa-sel = i; }
                            double-clicked => { root.play-uri(row.uri); }
                        }
                    }
                }
                Sprite { src: root.skin.pledit; s: parent.s; px: 260; py: 20 + round(parent.pl-pos * (174 - 18)); w: 8; h: 18; sx: handle.pressed ? 61 : 52; sy: 53; }
                handle := TouchArea {
                    x: 260 * parent.s;
                    y: 20 * parent.s;
                    width: 8 * parent.s;
                    height: 174 * parent.s;
                    moved => {
                        if (self.pressed) {
                            root.wa-pl-y = -clamp((self.mouse-y / parent.s - 9) / (174 - 18), 0, 1) * parent.pl-range;
                        }
                    }
                }
                // Running time and mini transport in the bottom-right corner.
                for g[i] in root.wa-pl-time : Sprite { src: root.skin.text; s: root.wa-s; px: 132 + i * 5; py: 204; w: 5; h: 6; sx: g.x; sy: g.y; }
                for g[i] in root.wa-mini-time : Sprite { src: root.skin.text; s: root.wa-s; px: 191 + i * 5; py: 217; w: 5; h: 6; sx: g.x; sy: g.y; }
                for kind[i] in ["add", "rem", "sel", "misc"] : TouchArea {
                    x: (11 + i * 29) * root.wa-s;
                    y: 204 * root.wa-s;
                    width: 25 * root.wa-s;
                    height: 18 * root.wa-s;
                    clicked => { wa.open-pl(kind, self.absolute-position.x, self.absolute-position.y); }
                }
                TouchArea {
                    x: 230 * parent.s;
                    y: 204 * parent.s;
                    width: 22 * parent.s;
                    height: 18 * parent.s;
                    clicked => { wa.open-lists(self.absolute-position.x, self.absolute-position.y); }
                }
                TouchArea { x: 130 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { root.prev(); } }
                TouchArea { x: 139 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { if (!root.playing) { root.toggle(); } } }
                TouchArea { x: 148 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { root.toggle(); } }
                TouchArea { x: 157 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { root.wa-stop(); } }
                TouchArea { x: 166 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { root.next(); } }
                TouchArea { x: 175 * parent.s; y: 216 * parent.s; width: 9 * parent.s; height: 8 * parent.s; clicked => { root.skin-toggle(); } }
            }

            // ---------- Menus, in the playlist's colors like Spotifast's ----------
            options-menu := PopupWindow {
                x: wa.menu-x;
                y: wa.menu-y;
                width: 150px;
                Rectangle {
                    background: root.skin.pl-normal-bg;
                    border-width: 1px;
                    border-color: root.skin.pl-normal;
                    VerticalLayout {
                        padding: 3px;
                        for item[i] in [
                            { label: "Big window (Ctrl+W)", on: false },
                            { label: "Choose skin...", on: false },
                            { label: "Built-in skin", on: false },
                            { label: "Double size", on: root.wa-double },
                            { label: "Always on top", on: root.wa-on-top },
                            { label: "Quit", on: false },
                        ] : Rectangle {
                            height: 20px;
                            background: oa.has-hover ? root.skin.pl-selected-bg : transparent;
                            Text { x: 8px; text: (item.on ? "✓ " : "") + item.label; font-size: 11px; vertical-alignment: center; color: root.skin.pl-normal; }
                            oa := TouchArea {
                                clicked => {
                                    if (i == 0) { root.skin-toggle(); }
                                    if (i == 1) { root.skin-choose(); }
                                    if (i == 2) { root.skin-builtin(); }
                                    if (i == 3) { root.wa-double = !root.wa-double; }
                                    if (i == 4) { root.wa-on-top = !root.wa-on-top; }
                                    if (i == 5) { root.quit(); }
                                }
                            }
                        }
                    }
                }
            }
            pl-menu := PopupWindow {
                x: wa.menu-x;
                y: wa.menu-y;
                width: 190px;
                Rectangle {
                    background: root.skin.pl-normal-bg;
                    border-width: 1px;
                    border-color: root.skin.pl-normal;
                    VerticalLayout {
                        padding: 3px;
                        for item in (wa.pl-kind == "add" ? [
                            { label: "Search Spotify...", act: "search", on: true },
                            { label: "Liked Songs", act: "liked", on: true },
                            { label: "Recently played", act: "home", on: true },
                            { label: "Queue", act: "queue-view", on: true },
                        ] : wa.pl-kind == "rem" ? [
                            { label: "Remove from this playlist", act: "remove", on: root.editable && wa.pl-chosen && wa.pl-track },
                            { label: "Clear selection", act: "sel-none", on: wa.pl-chosen },
                        ] : wa.pl-kind == "sel" ? [
                            { label: "Select first", act: "sel-first", on: root.tracks.length > 0 },
                            { label: "Select last", act: "sel-last", on: root.tracks.length > 0 },
                            { label: "Select none", act: "sel-none", on: wa.pl-chosen },
                        ] : [
                            { label: "Add to queue", act: "queue", on: wa.pl-track },
                            { label: "Save to Liked Songs", act: "like", on: wa.pl-track },
                            { label: "Start radio", act: "radio", on: wa.pl-track },
                            { label: "Go to album", act: "album", on: wa.pl-track },
                            { label: "Go to artist", act: "artist", on: wa.pl-track },
                            { label: "Copy link", act: "copy", on: wa.pl-uri != "" },
                        ]) : Rectangle {
                            height: 20px;
                            background: item.on && pm.has-hover ? root.skin.pl-selected-bg : transparent;
                            Text {
                                x: 8px;
                                text: item.label;
                                font-size: 11px;
                                vertical-alignment: center;
                                color: root.skin.pl-normal;
                                opacity: item.on ? 1 : 0.4;
                            }
                            pm := TouchArea {
                                enabled: item.on;
                                clicked => { wa.pl-act(item.act); }
                            }
                        }
                    }
                }
            }
            lists-menu := PopupWindow {
                x: wa.menu-x;
                y: wa.menu-y;
                width: 220px;
                height: 240px;
                Rectangle {
                    background: root.skin.pl-normal-bg;
                    border-width: 1px;
                    border-color: root.skin.pl-normal;
                    VerticalLayout {
                        padding: 3px;
                        Text { text: "Your Library"; font-size: 11px; font-weight: 600; color: root.skin.pl-current; height: 18px; vertical-alignment: center; }
                        ListView {
                            vertical-stretch: 1;
                            for list in root.lists : Rectangle {
                                height: 20px;
                                background: la.has-hover ? root.skin.pl-selected-bg : transparent;
                                Text { x: 6px; width: parent.width - 12px; text: list.title; font-size: 11px; vertical-alignment: center; overflow: elide; color: list.uri == root.current-list ? root.skin.pl-current : root.skin.pl-normal; }
                                la := TouchArea {
                                    clicked => {
                                        root.wa-pl = true;
                                        root.wa-sel = -1;
                                        root.open-list(list.uri);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        VerticalLayout {
            padding: 8px;
            spacing: 8px;
            visible: !root.skin-mode;
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
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.plus; dot: false; size: 18px; clicked => {
                                // Like editing, but nothing exists until Save (Spotify creates at once).
                                root.creating = true;
                                root.rename-text = "My Playlist #" + (root.targets.length + 1);
                                root.desc-text = "";
                                rename-popup.show();
                            } }
                        }
                        HorizontalLayout {
                            spacing: 4px;
                            alignment: start;
                            // Like Spotify: no "All" chip; clicking the chosen filter again clears it.
                            for label[i] in ["Playlists", "Albums", "Artists", "Podcasts"] : Chip {
                                label: label;
                                pad: 14px;
                                chosen: root.library-kind == i + 1;
                                clicked => {
                                    root.library-kind = root.library-kind == i + 1 ? 0 : i + 1;
                                    root.filter-library(root.library-kind, lib-filter.text, root.library-sort);
                                }
                            }
                        }
                        // The sort chip sits next to the search field, like Spotify's sort control.
                        HorizontalLayout {
                            spacing: 4px;
                        Rectangle {
                            horizontal-stretch: 1;
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
                                edited => { root.filter-library(root.library-kind, self.text, root.library-sort); }
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
                            Chip {
                                y: 1px;
                                label: ["Sort", "Recents", "A-Z"][root.library-sort];
                                pad: 14px;
                                chosen: root.library-sort != 0;
                                clicked => { root.library-sort = mod(root.library-sort + 1, 3); root.filter-library(root.library-kind, lib-filter.text, root.library-sort); }
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
                                    drop-shadow-color: Theme.accent.transparentize(40%);
                                    animate drop-shadow-blur { duration: 150ms; }
                                    Icon { width: 26px; height: 26px; shape: Icons.play; filled: true; tint: Theme.text; }
                                }
                            }
                            // Like Spotify's page shuffle: the same shuffle state as the player bar.
                            if root.page-playable : IconButton {
                                y: (parent.height - self.height) / 2;
                                shape: Icons.shuffle;
                                active: root.shuffle;
                                size: 22px;
                                clicked => { root.toggle-shuffle(); }
                            }
                            // Like Spotify: an outlined "Follow" pill on artists, a round +/check on albums.
                            if root.page-kind == "Artist" : TouchArea {
                                width: follow-text.preferred-width + 32px;
                                height: 32px;
                                y: (parent.height - self.height) / 2;
                                mouse-cursor: pointer;
                                clicked => { root.row-action(root.page-saved ? "unfollow" : "save", root.page-uri); }
                                Rectangle {
                                    border-radius: 16px;
                                    border-width: 1px;
                                    border-color: parent.has-hover ? Theme.text : Theme.subdued;
                                    follow-text := Text { text: root.page-saved ? "Following" : "Follow"; font-size: 13px; font-weight: 600; color: Theme.text; }
                                }
                            }
                            if root.page-kind == "Album" || root.page-kind == "Podcast" : TouchArea {
                                width: 32px;
                                height: 32px;
                                y: (parent.height - self.height) / 2;
                                mouse-cursor: pointer;
                                clicked => { root.row-action(root.page-saved ? "unfollow" : "save", root.page-uri); }
                                Rectangle {
                                    border-radius: 16px;
                                    border-width: root.page-saved ? 0px : 1.5px;
                                    border-color: parent.has-hover ? Theme.text : Theme.subdued;
                                    background: root.page-saved ? Theme.accent : transparent;
                                    Icon { width: 20px; height: 20px; shape: root.page-saved ? Icons.check : Icons.plus; tint: Theme.text; }
                                }
                            }
                            VerticalLayout {
                                spacing: 2px;
                                alignment: center;
                                if root.page-kind != "" : Text { text: root.page-info == "" ? root.page-kind : root.page-kind + " · " + root.page-info; font-size: 12px; font-weight: 600; color: Theme.text; overflow: elide; min-width: 0px; }
                                HorizontalLayout {
                                    spacing: 8px;
                                    alignment: start;
                                    // min-width 0: a long title elides instead of pushing the header buttons out of the panel.
                                    Text { text: root.page-title; font-size: 32px; font-weight: 800; color: Theme.text; overflow: elide; min-width: 0px; }
                                    if root.editable : IconButton {
                                        y: (parent.height - self.height) / 2;
                                        shape: Icons.pencil;
                                        dot: false;
                                        size: 18px;
                                        clicked => { root.creating = false; root.rename-text = root.page-title; root.desc-text = root.page-info; rename-popup.show(); }
                                    }
                                    if root.page-uri.starts-with("spotify:playlist:") || root.page-uri.starts-with("spotify:album:") || root.page-uri.starts-with("spotify:artist:") : IconButton {
                                        y: (parent.height - self.height) / 2;
                                        shape: Icons.radio;
                                        dot: false;
                                        size: 18px;
                                        clicked => { root.row-action("radio", root.page-uri); }
                                    }
                                    if root.page-kind == "Album" : Chip {
                                        y: (parent.height - self.height) / 2;
                                        label: "More by this artist";
                                        clicked => { root.row-action("more-by", root.page-uri); }
                                    }
                                    if root.page-kind == "Radio" : Chip {
                                        y: (parent.height - self.height) / 2;
                                        label: "Save as playlist";
                                        clicked => { root.save-radio(); }
                                    }
                                    if root.editable : IconButton {
                                        y: (parent.height - self.height) / 2;
                                        shape: Icons.plus;
                                        dot: false;
                                        size: 18px;
                                        clicked => { add-popup.show(); }
                                    }
                                }
                            }
                        }
                        if root.search-tab >= 0 && !root.show-lyrics : HorizontalLayout {
                            spacing: 4px;
                            alignment: start;
                            for label[i] in root.page-tabs : Chip {
                                label: label;
                                chosen: root.search-tab == i;
                                clicked => { if (root.search-tab != i) { root.choose-search-tab(i); } }
                            }
                        }
                        HorizontalLayout {
                            spacing: 6px;
                            Text { text: root.status; font-size: 12px; color: Theme.subdued; overflow: elide; vertical-alignment: center; horizontal-stretch: 1; }
                            // Find and sort within the page (Ctrl+F); backend filters, no request.
                            if root.page-playable && !root.show-lyrics : Chip {
                                label: ["Custom order", "Title", "Artist", "Duration"][root.page-sort];
                                clicked => { root.page-sort = mod(root.page-sort + 1, 4); root.filter-page(page-filter.text, root.page-sort); }
                            }
                            Rectangle {
                                visible: root.page-playable && !root.show-lyrics;
                                width: self.visible ? 180px : 0px;
                                height: 30px;
                                border-radius: 6px;
                                background: page-filter.has-focus ? Theme.hover : Theme.field;
                                Icon { x: 8px; width: 14px; height: 14px; y: (parent.height - 14px) / 2; shape: Icons.search; tint: Theme.subdued; }
                                page-filter := TextInput {
                                    x: 28px;
                                    width: parent.width - 36px;
                                    height: parent.height;
                                    vertical-alignment: center;
                                    single-line: true;
                                    font-size: 12px;
                                    color: Theme.text;
                                    text <=> root.page-filter-text;
                                    edited => { root.filter-page(self.text, root.page-sort); }
                                }
                                if page-filter.text == "" : Text {
                                    x: 28px;
                                    height: parent.height;
                                    vertical-alignment: center;
                                    text: "Find in page";
                                    font-size: 12px;
                                    color: Theme.subdued;
                                }
                            }
                        }
                        Rectangle { height: 1px; background: #ffffff1a; }
                        if root.show-lyrics : LyricsView {
                            vertical-stretch: 1;
                            lines: root.lyric-lines;
                            current: root.lyric-index;
                            note: root.lyrics-note;
                        }
                        if !root.show-lyrics && root.editable && root.tracks.length == 0 : HorizontalLayout {
                            alignment: center;
                            padding-top: 24px;
                            Chip { label: "Let's find something for your playlist"; chosen: true; clicked => { add-popup.show(); } }
                        }
                        if !root.show-lyrics && root.page-about != "" : ScrollView {
                            vertical-stretch: 1;
                            content-height: about-text.preferred-height + 16px;
                            about-text := Text {
                                y: 8px;
                                width: parent.width - 16px;
                                text: root.page-about;
                                wrap: word-wrap;
                                font-size: 14px;
                                color: Theme.subdued;
                            }
                        }
                        if !root.show-lyrics && root.page-kind == "Home" && root.quick-top.length > 0 : VerticalLayout {
                            spacing: 6px;
                            HorizontalLayout {
                                spacing: 6px;
                                for q[i] in root.quick-top : QuickTile { data: q; clicked => { root.play-uri(q.uri); } need-cover => { root.need-cover(6, i, q.uri, q.cover-url); } }
                            }
                            HorizontalLayout {
                                spacing: 6px;
                                for q[i] in root.quick-bottom : QuickTile { data: q; clicked => { root.play-uri(q.uri); } need-cover => { root.need-cover(7, i, q.uri, q.cover-url); } }
                            }
                        }
                        if !root.show-lyrics : ListView {
                            visible: root.page-about == "";
                            vertical-stretch: 1;
                            content-y <=> root.list-y;
                            init => { root.list-height = self.visible-height; }
                            changed visible-height => { root.list-height = self.visible-height; }
                            for row[i] in tracks: RowItem {
                                data: row;
                                number: root.numbered && (row.uri.starts-with("spotify:track:") || row.uri.starts-with("spotify:episode:")) ? i + 1 : 0;
                                playing: row.uri == root.current-track;
                                audible: self.playing && root.playing;
                                // "Show more" and similar app rows have no menu.
                                menu: !row.uri.starts-with("slimspot:");
                                draggable: root.editable && root.page-sort == 0 && root.page-filter-text == "";
                                index: i;
                                count: root.tracks.length;
                                // Like Spotify: the playing row pauses/resumes instead of restarting.
                                clicked => { if (!self.dragged) { if (self.playing) { root.toggle(); } else { root.play-uri(row.uri); } } }
                                dropped(n) => { root.move-row(i, i + n); }
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
                        // Spotify's "Lyrics preview": the sung line and the next two (Ctrl+Y opens all).
                        if root.lyric-lines.length > 0 : Rectangle {
                            border-radius: 8px;
                            background: Theme.raised;
                            VerticalLayout {
                                padding: 12px;
                                spacing: 4px;
                                Text { text: "Lyrics preview"; font-size: 12px; font-weight: 600; color: Theme.subdued; }
                                for k in [0, 1, 2] : Text {
                                    text: root.lyric-lines[max(0, root.lyric-index) + k];
                                    font-size: 14px;
                                    font-weight: 600;
                                    color: k == 0 && root.lyric-index >= 0 ? Theme.text : Theme.subdued;
                                    overflow: elide;
                                }
                            }
                        }
                        if root.now-about != "" : Rectangle {
                            border-radius: 8px;
                            background: Theme.raised;
                            VerticalLayout {
                                padding: 12px;
                                spacing: 4px;
                                Text { text: "About the artist"; font-size: 12px; font-weight: 600; color: Theme.subdued; }
                                Text { text: root.now-about + "..."; font-size: 12px; color: Theme.subdued; wrap: word-wrap; }
                            }
                        }
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
                        if !root.menu-playlists && (is-track || root.menu-row.uri.starts-with("spotify:playlist:") || root.menu-row.uri.starts-with("spotify:album:") || root.menu-row.uri.starts-with("spotify:artist:")) : MenuEntry { label: "Start radio"; shape: Icons.radio; clicked => { root.menu-act("radio"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Go to artist"; shape: Icons.person; clicked => { root.menu-act("artist"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "Go to album"; shape: Icons.disc; clicked => { root.menu-act("album"); } }
                        if !root.menu-playlists && is-track : MenuEntry { label: "View credits"; shape: Icons.person; clicked => { root.menu-act("credits"); } }
                        if !root.menu-playlists && !is-track : MenuEntry { label: "Open"; shape: Icons.open; clicked => { root.menu-act("open"); } }
                        if !root.menu-playlists : Rectangle { height: 1px; background: Theme.border; }
                        if !root.menu-playlists : MenuEntry { label: "Copy link"; shape: Icons.link; clicked => { root.menu-act("copy"); } }
                        if !root.menu-playlists && is-track && root.editable && !root.menu-sidebar : MenuEntry { label: "Remove from this playlist"; shape: Icons.trash; danger: true; clicked => { root.menu-act("remove"); } }
                        if !root.menu-playlists && !root.menu-sidebar && (root.menu-row.uri.starts-with("spotify:album:") || root.menu-row.uri.starts-with("spotify:artist:") || root.menu-row.uri.starts-with("spotify:show:")) : MenuEntry { label: root.menu-row.uri.starts-with("spotify:artist:") ? "Follow" : "Save to Your Library"; shape: Icons.plus; clicked => { root.menu-act("save"); } }
                        if !root.menu-playlists && root.menu-sidebar && (root.menu-row.uri.starts-with("spotify:playlist:") || root.menu-row.uri.starts-with("spotify:album:") || root.menu-row.uri.starts-with("spotify:artist:") || root.menu-row.uri.starts-with("spotify:show:")) : MenuEntry { label: "Remove from Your Library"; shape: Icons.trash; danger: true; clicked => { root.menu-act("unfollow"); } }
                        if root.menu-playlists : MenuEntry { label: "Back"; shape: Icons.back; clicked => { root.menu-playlists = false; } }
                        if root.menu-playlists : Rectangle { height: 1px; background: Theme.border; }
                        if root.menu-playlists : VerticalLayout {
                            spacing: 2px;
                            for t in root.targets : MenuEntry { label: t.title; shape: Icons.plus; clicked => { root.menu-act("add:" + t.uri); } }
                        }
                    }
                }
            }
            credits-popup := PopupWindow {
                x: (root.width - 420px) / 2;
                y: 120px;
                width: 420px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 8px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 16px;
                    drop-shadow-color: #00000099;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 10px;
                        Text { text: "Credits · " + root.credits-title; font-size: 15px; font-weight: 700; color: Theme.text; overflow: elide; }
                        Text { text: root.credits-text; font-size: 13px; color: Theme.subdued; wrap: word-wrap; }
                    }
                }
            }
            add-popup := PopupWindow {
                x: (root.width - 480px) / 2;
                y: 96px;
                width: 480px;
                height: min(520px, root.height - 120px);
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 8px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 16px;
                    drop-shadow-color: #00000099;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 10px;
                        Text { text: "Add songs to " + root.page-title; font-size: 15px; font-weight: 700; color: Theme.text; overflow: elide; }
                        Rectangle {
                            height: 36px;
                            border-radius: 6px;
                            background: Theme.field;
                            Icon { x: 8px; width: 16px; height: 16px; y: (parent.height - 16px) / 2; shape: Icons.search; tint: Theme.subdued; }
                            add-query := TextInput {
                                init => { self.focus(); }
                                x: 32px;
                                width: parent.width - 40px;
                                height: parent.height;
                                vertical-alignment: center;
                                single-line: true;
                                font-size: 13px;
                                color: Theme.text;
                                accepted => { root.add-search(self.text); }
                            }
                            if add-query.text == "" : Text {
                                x: 32px;
                                height: parent.height;
                                vertical-alignment: center;
                                text: "Search for songs (Enter)";
                                font-size: 13px;
                                color: Theme.subdued;
                            }
                        }
                        ListView {
                            vertical-stretch: 1;
                            for row[i] in root.add-results : HorizontalLayout {
                                spacing: 8px;
                                RowItem {
                                    horizontal-stretch: 1;
                                    data: row;
                                    menu: false;
                                    need-cover => { root.need-cover(5, i, row.uri, row.cover-url); }
                                }
                                Chip { y: (parent.height - self.height) / 2; label: "Add"; clicked => { root.add-to-page(row.uri); } }
                            }
                        }
                    }
                }
            }
            confirm-popup := PopupWindow {
                x: (root.width - 360px) / 2;
                y: 160px;
                width: 360px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 8px;
                    border-width: 1px;
                    border-color: Theme.border;
                    drop-shadow-blur: 16px;
                    drop-shadow-color: #00000099;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        Text { text: "Remove from Your Library?"; font-size: 15px; font-weight: 700; color: Theme.text; }
                        Text {
                            text: root.confirm-row.uri.starts-with("spotify:playlist:")
                                ? "\"" + root.confirm-row.title + "\" will be removed. If it's your own playlist, it is deleted."
                                : "\"" + root.confirm-row.title + "\" will be removed from Your Library.";
                            wrap: word-wrap;
                            font-size: 13px;
                            color: Theme.subdued;
                        }
                        HorizontalLayout {
                            spacing: 8px;
                            Rectangle { horizontal-stretch: 1; }
                            Chip { label: "Cancel"; clicked => { confirm-popup.close(); } }
                            Chip { label: "Remove"; chosen: true; clicked => { root.row-action("unfollow", root.confirm-row.uri); confirm-popup.close(); } }
                        }
                    }
                }
            }
            rename-popup := PopupWindow {
                x: 320px;
                y: 120px;
                width: 380px;
                close-policy: close-on-click-outside;
                Rectangle {
                    background: Theme.raised;
                    border-radius: 8px;
                    drop-shadow-blur: 16px;
                    drop-shadow-color: #00000099;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        Text { text: root.creating ? "Create playlist" : "Edit details"; font-size: 15px; font-weight: 700; color: Theme.text; }
                        Rectangle {
                            height: 36px;
                            border-radius: 4px;
                            background: Theme.field;
                            TextInput {
                                // Popup content is created on every show, so this focuses each time.
                                init => { self.focus(); self.select-all(); }
                                text <=> root.rename-text;
                                x: 10px;
                                width: parent.width - 20px;
                                height: parent.height;
                                vertical-alignment: center;
                                single-line: true;
                                font-size: 14px;
                                color: Theme.text;
                                accepted => { root.save-details(); rename-popup.close(); }
                            }
                        }
                        Rectangle {
                            height: 72px;
                            border-radius: 4px;
                            background: Theme.field;
                            desc-input := TextInput {
                                text <=> root.desc-text;
                                x: 10px;
                                y: 8px;
                                width: parent.width - 20px;
                                height: parent.height - 16px;
                                wrap: word-wrap;
                                single-line: false;
                                font-size: 13px;
                                color: Theme.text;
                            }
                            if desc-input.text == "" : Text {
                                x: 10px;
                                y: 8px;
                                text: "Add an optional description";
                                font-size: 13px;
                                color: Theme.subdued;
                            }
                        }
                        HorizontalLayout {
                            spacing: 8px;
                            // The cover needs an existing playlist; it can be set after creating.
                            if !root.creating : Chip { label: "Change cover..."; clicked => { root.change-cover(); rename-popup.close(); } }
                            Rectangle { horizontal-stretch: 1; }
                            Chip { label: root.creating ? "Create" : "Save"; chosen: true; clicked => { root.save-details(); rename-popup.close(); } }
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
                        SectionTitle { text: "THEME"; shape: Icons.settings; }
                        HorizontalLayout {
                            spacing: 4px;
                            alignment: start;
                            for name[i] in ["Indigo", "Nord", "Tokyo", "Mocha", "Rosé", "Dark"] : Chip {
                                label: name;
                                pad: 14px;
                                chosen: Theme.palette == i;
                                clicked => { Theme.palette = i; root.set-theme(i); }
                            }
                        }
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
                        // Optional own Spotify app for the Web API (like Spotifast's personal Client ID).
                        SectionTitle { text: "SPOTIFY APP"; shape: Icons.link; }
                        Text {
                            text: "Optional: your own app's Client ID from developer.spotify.com (redirect URI http://127.0.0.1:8989/login). Empty uses the shared app.";
                            wrap: word-wrap;
                            font-size: 11px;
                            color: Theme.subdued;
                        }
                        HorizontalLayout {
                            spacing: 6px;
                            Rectangle {
                                horizontal-stretch: 1;
                                height: 30px;
                                border-radius: 6px;
                                background: Theme.field;
                                client-id-input := TextInput {
                                    x: 8px;
                                    width: parent.width - 16px;
                                    height: parent.height;
                                    vertical-alignment: center;
                                    single-line: true;
                                    font-size: 12px;
                                    color: Theme.text;
                                    // Hidden like a password field; the eye button shows it.
                                    input-type: root.show-client-id ? InputType.text : InputType.password;
                                    text <=> root.web-client-id;
                                    accepted => { root.set-web-client-id(self.text); }
                                }
                                if client-id-input.text == "" : Text { x: 8px; height: parent.height; vertical-alignment: center; text: "Client ID"; font-size: 12px; color: Theme.muted; }
                            }
                            IconButton { y: (parent.height - self.height) / 2; shape: root.show-client-id ? Icons.eye-off : Icons.eye; dot: false; size: 18px; clicked => { root.show-client-id = !root.show-client-id; } }
                            Chip { label: "Save"; pad: 14px; clicked => { root.set-web-client-id(root.web-client-id); } }
                        }
                        SectionTitle { text: "APPEARANCE"; shape: Icons.mini; }
                        Switch { label: "GPU acceleration (applies on restart)"; on <=> root.gpu; toggled(v) => { root.set-gpu(v); } }
                        HorizontalLayout {
                            spacing: 6px;
                            alignment: start;
                            Chip { label: "Winamp skin (Ctrl+W)"; pad: 14px; clicked => { root.skin-toggle(); settings-popup.close(); } }
                            Chip { label: "Choose skin..."; pad: 14px; clicked => { root.skin-choose(); settings-popup.close(); } }
                            Chip { label: "Built-in"; pad: 14px; clicked => { root.skin-builtin(); settings-popup.close(); } }
                        }
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
        // Rust shows the playing track here.
        in-out property <string> tip: "SlimSpot";
        tooltip: root.tip;
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

/// EQ window sliders run -1..1 for ±`eq::MAX_DB`.
pub fn set_eq(app: &App, eq: crate::eq::EqState) {
    let max = crate::eq::MAX_DB;
    app.set_eq_on(eq.on);
    app.set_eq_preamp(eq.preamp / max);
    app.set_eq_bands(ModelRc::new(VecModel::from(eq.bands.iter().map(|b| b / max).collect::<Vec<_>>())));
    app.set_wa_balance(eq.balance);
}

pub fn get_eq(app: &App) -> crate::eq::EqState {
    let max = crate::eq::MAX_DB;
    let mut bands = [0.0; 10];
    for (b, v) in bands.iter_mut().zip(app.get_eq_bands().iter()) {
        *b = (v * max).clamp(-max, max);
    }
    crate::eq::EqState { on: app.get_eq_on(), preamp: app.get_eq_preamp() * max, bands, balance: app.get_wa_balance() }
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
