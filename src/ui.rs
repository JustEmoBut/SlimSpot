//! Slint markup and the small helpers that push backend state onto the UI thread.

use slint::{Model, ModelRc, VecModel};

use crate::web::Item;

slint::slint! {
    import { ListView, Palette, ScrollView } from "std-widgets.slint";
    // Poppins (SIL OFL, assets/fonts/OFL.txt), embedded in the exe: ~160 KB per weight.
    import "../assets/fonts/Poppins-Regular.ttf";
    import "../assets/fonts/Poppins-SemiBold.ttf";
    export struct Row { title: string, artist: string, uri: string, cover-url: string, cover: image, duration: string, liked: bool, marked: bool }

    // Palettes (settings → Theme), values from each palette's published colors. Every color is
    // rgb(): inside slint! a hex like #5eead4 tokenizes as a Rust float exponent.
    export global Theme {
        // 0-5: Midnight Indigo, Nord, Tokyo Night, Catppuccin Mocha, Rosé Pine, Dark; 6: Spotifast
        // (its dark palette, src/theme.rs of crmne/spotifast), the default.
        in-out property <int> palette: 6;
        out property <color> base: [rgb(8, 8, 26), rgb(46, 52, 64), rgb(26, 27, 38), rgb(17, 17, 27), rgb(25, 23, 36), rgb(0, 0, 0), rgb(15, 17, 20)][palette];
        out property <color> panel: [rgb(16, 16, 42), rgb(59, 66, 82), rgb(31, 35, 53), rgb(30, 30, 46), rgb(31, 29, 46), rgb(18, 18, 18), rgb(21, 24, 28)][palette];
        out property <color> raised: [rgb(27, 27, 56), rgb(67, 76, 94), rgb(41, 46, 66), rgb(49, 50, 68), rgb(38, 35, 58), rgb(31, 31, 31), rgb(29, 33, 39)][palette];
        out property <color> hover: [rgb(35, 35, 74), rgb(76, 86, 106), rgb(52, 59, 88), rgb(69, 71, 90), rgb(57, 53, 82), rgb(42, 42, 42), rgb(38, 43, 51)][palette];
        out property <color> selected: [rgb(44, 44, 94), rgb(86, 95, 118), rgb(61, 70, 110), rgb(88, 91, 112), rgb(82, 79, 103), rgb(51, 51, 51), rgb(47, 53, 63)][palette];
        out property <color> field: [rgb(27, 27, 56), rgb(67, 76, 94), rgb(41, 46, 66), rgb(49, 50, 68), rgb(38, 35, 58), rgb(36, 36, 36), rgb(29, 33, 39)][palette];
        out property <color> border: rgba(255, 255, 255, 0.08);
        out property <color> text: [rgb(248, 250, 252), rgb(236, 239, 244), rgb(192, 202, 245), rgb(205, 214, 244), rgb(224, 222, 244), rgb(255, 255, 255), rgb(242, 244, 246)][palette];
        out property <color> subdued: [rgb(163, 168, 195), rgb(180, 188, 204), rgb(169, 177, 214), rgb(166, 173, 200), rgb(144, 140, 170), rgb(179, 179, 179), rgb(169, 177, 188)][palette];
        out property <color> muted: [rgb(107, 111, 142), rgb(129, 138, 158), rgb(86, 95, 137), rgb(127, 132, 156), rgb(110, 106, 134), rgb(114, 114, 114), rgb(110, 119, 132)][palette];
        out property <color> track: [rgb(46, 46, 90), rgb(76, 86, 106), rgb(59, 66, 97), rgb(69, 71, 90), rgb(57, 53, 82), rgb(77, 77, 77), rgb(47, 53, 63)][palette];
        out property <color> accent: [rgb(139, 92, 246), rgb(136, 192, 208), rgb(122, 162, 247), rgb(203, 166, 247), rgb(196, 167, 231), rgb(139, 92, 246), rgb(30, 215, 96)][palette];
        // Icons and labels drawn on an accent background (dark on Spotifast's bright green).
        out property <color> on-accent: [text, text, text, text, text, text, rgb(10, 20, 14)][palette];
        out property <color> accent2: [rgb(34, 211, 238), rgb(163, 190, 140), rgb(187, 154, 247), rgb(137, 220, 235), rgb(235, 188, 186), rgb(34, 211, 238), rgb(60, 232, 122)][palette];
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
        out property <string> refresh: "M19.5 12 A7.5 7.5 0 1 1 17 6.4 M17.5 2.5 V6.8 H13.2";
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
        out property <string> library: "M4 4 V20 M8.5 4 V20 M13 5 L17.5 19.5";
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
                tint: Theme.on-accent;
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
        // Part of the track list's multi-selection (Ctrl/Shift-click, arrows, Ctrl+A).
        in property <bool> marked;
        // One line, no cover (Settings → Compact track list).
        in property <bool> compact;
        // Sidebar: the play button over the cover on hover.
        callback play-cover();
        // Ctrl/Shift-click: changes the selection instead of playing (`select(ctrl, shift)`).
        callback select(bool, bool);
        out property <bool> modifier-click;
        // Reordering within the list (own playlist, unsorted): shows the drop marker line.
        in property <bool> reorder;
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
        // Draggable rows report the drop n rows down (negative: up) and the window position it ended at.
        in property <bool> draggable;
        callback dropped(int, length, length);
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
        init => { if (!compact && data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        changed data => { if (!compact && data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        height: root.compact ? 36px : root.cover-size + 16px;
        mouse-cursor: pointer;
        pointer-event(event) => {
            if (root.menu && event.button == PointerEventButton.right && event.kind == PointerEventKind.up) {
                root.open-menu(self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y);
            }
            if (event.button == PointerEventButton.left && event.kind == PointerEventKind.down) {
                root.dragged = false;
                root.drag-start = self.mouse-y;
                root.modifier-click = event.modifiers.control || event.modifiers.shift;
                if (root.modifier-click) { root.select(event.modifiers.control, event.modifiers.shift); }
            }
            if (root.dragged && event.button == PointerEventButton.left && event.kind == PointerEventKind.up) {
                root.dropped(root.drop-offset, self.absolute-position.x + self.mouse-x, self.absolute-position.y + self.mouse-y);
            }
        }
        moved => {
            // A few pixels of jitter still count as a click.
            if (root.draggable && root.pressed && abs(self.mouse-y - root.drag-start) > 8px) { root.dragged = true; }
        }
        Rectangle {
            border-radius: 8px;
            background: root.dragged && root.pressed ? Theme.selected : root.selected || root.marked ? Theme.selected : root.has-hover ? Theme.hover : transparent;
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
                if !root.compact : Rectangle {
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
                    // Sidebar: play the entry without opening it (Spotifast's cover play button).
                    if root.sidebar && root.has-hover && !data.uri.starts-with("slimspot:") : cover-play := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.play-cover(); }
                        Rectangle {
                            background: #00000099;
                            Icon { width: parent.width * 0.5; height: self.width; shape: Icons.play; filled: true; tint: cover-play.has-hover ? Theme.accent : Theme.text; }
                        }
                    }
                }
                if !root.compact : VerticalLayout {
                    alignment: center;
                    spacing: 2px;
                    horizontal-stretch: 1;
                    Text { text: data.title; overflow: elide; font-size: 14px; color: root.playing ? Theme.accent : Theme.text; }
                    Text { text: data.artist; overflow: elide; font-size: 12px; color: Theme.subdued; }
                }
                if root.compact : HorizontalLayout {
                    spacing: 12px;
                    horizontal-stretch: 1;
                    Text { text: data.title; overflow: elide; vertical-alignment: center; horizontal-alignment: left; font-size: 13px; horizontal-stretch: 1; color: root.playing ? Theme.accent : Theme.text; }
                    Text { text: data.artist; overflow: elide; vertical-alignment: center; horizontal-alignment: left; font-size: 13px; horizontal-stretch: 1; color: Theme.subdued; }
                }
                // Spotify's green circle with a dark tick; a filled path would hide the tick, so two layers.
                if data.liked : Rectangle {
                    width: 16px;
                    height: 16px;
                    y: (parent.height - self.height) / 2;
                    border-radius: 8px;
                    background: Theme.accent;
                    Icon { width: 14px; height: 14px; shape: Icons.check; tint: Theme.on-accent; }
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
        if root.reorder && root.dragged && root.pressed && root.drop-offset != 0 : Rectangle {
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
        in property <string> desc;
        in-out property <bool> on;
        callback toggled(bool);
        // A TouchArea isn't a layout: take the height of the (wrapped) label and description.
        min-height: max(34px, content.preferred-height);
        mouse-cursor: pointer;
        clicked => {
            root.on = !root.on;
            root.toggled(root.on);
        }
        content := HorizontalLayout {
            spacing: 12px;
            VerticalLayout {
                horizontal-stretch: 1;
                alignment: center;
                Text { text: root.label; font-size: 13px; color: Theme.text; wrap: word-wrap; }
                if root.desc != "" : Text { text: root.desc; font-size: 11px; color: Theme.subdued; wrap: word-wrap; }
            }
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

    // Settings page: bold section heading (Spotify's settings layout).
    component SettingsHeading inherits Text {
        font-size: 16px;
        font-weight: 600;
        color: Theme.text;
        height: 40px;
        vertical-alignment: bottom;
    }

    // Settings page row: label (and description) on the left, the control on the right.
    component SettingRow inherits HorizontalLayout {
        in property <string> label;
        in property <string> desc;
        spacing: 16px;
        min-height: 34px;
        VerticalLayout {
            horizontal-stretch: 1;
            alignment: center;
            Text { text: root.label; font-size: 13px; color: Theme.text; wrap: word-wrap; }
            if root.desc != "" : Text { text: root.desc; font-size: 11px; color: Theme.subdued; wrap: word-wrap; }
        }
        @children
    }

    // One vertical EQ slider, value -1..1 (±12 dB); moved while dragging, released at the end.
    component EqBand inherits VerticalLayout {
        in property <string> label;
        in-out property <float> value;
        callback moved(float);
        callback released();
        spacing: 6px;
        width: 34px;
        area := TouchArea {
            height: 120px;
            mouse-cursor: pointer;
            function set-from(y: length) {
                root.value = max(-1, min(1, 1 - 2 * y / self.height));
                root.moved(root.value);
            }
            pointer-event(e) => {
                if (e.kind == PointerEventKind.down) { self.set-from(self.mouse-y); }
                if (e.kind == PointerEventKind.up) { root.released(); }
            }
            moved => { if (self.pressed) { self.set-from(self.mouse-y); } }
            double-clicked => { root.value = 0; root.moved(0); root.released(); }
            Rectangle { x: (parent.width - 4px) / 2; width: 4px; border-radius: 2px; background: Theme.track; }
            Rectangle { x: (parent.width - 12px) / 2; y: (parent.height - 1px) / 2; width: 12px; height: 1px; background: Theme.border; }
            Rectangle {
                x: (parent.width - 4px) / 2;
                y: root.value >= 0 ? (1 - root.value) * parent.height / 2 : parent.height / 2;
                width: 4px;
                height: abs(root.value) * parent.height / 2;
                background: Theme.accent;
            }
            Rectangle {
                x: (parent.width - 14px) / 2;
                y: (1 - root.value) * (parent.height - 14px) / 2;
                width: 14px;
                height: 14px;
                border-radius: 7px;
                background: area.has-hover || area.pressed ? Theme.accent2 : Theme.text;
            }
        }
        Text { text: root.label; font-size: 10px; color: Theme.subdued; horizontal-alignment: center; }
    }

    // Home / Search at the top of the sidebar (Spotifast's sidebar).
    component NavEntry inherits TouchArea {
        in property <string> label;
        in property <string> shape;
        in property <bool> chosen;
        height: 40px;
        mouse-cursor: pointer;
        HorizontalLayout {
            padding-left: 12px;
            spacing: 16px;
            Icon { width: 24px; height: 24px; y: (parent.height - self.height) / 2; shape: root.shape; filled: root.chosen; tint: root.chosen || root.has-hover ? Theme.text : Theme.subdued; }
            Text { text: root.label; vertical-alignment: center; font-size: 15px; font-weight: 700; color: root.chosen || root.has-hover ? Theme.text : Theme.subdued; }
        }
    }

    // Home's "Recently played" shelf: a cover card (Spotifast's Home shelves).
    component ShelfCard inherits TouchArea {
        in property <Row> data;
        callback need-cover();
        init => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        changed data => { if (data.cover-url != "" && data.cover.width == 0) { need-cover() } }
        width: 136px;
        height: 188px;
        mouse-cursor: pointer;
        Rectangle {
            border-radius: 8px;
            background: root.has-hover ? Theme.hover : transparent;
            VerticalLayout {
                padding: 8px;
                spacing: 6px;
                Rectangle {
                    height: 120px;
                    border-radius: 6px;
                    background: Theme.selected;
                    Image { source: root.data.cover; width: parent.width; height: parent.height; image-fit: cover; }
                }
                Text { text: root.data.title; font-size: 12px; font-weight: 600; color: Theme.text; overflow: elide; }
                Text { text: root.data.artist; font-size: 11px; color: Theme.subdued; overflow: elide; }
            }
        }
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
            t := Text { text: root.label; font-size: 12px; font-weight: 600; color: root.chosen ? Theme.on-accent : Theme.text; }
        }
    }

    component LyricsView inherits Rectangle {
        in property <[string]> lines;
        // Line start times (ms) when Spotify synced every line; empty otherwise.
        in property <[int]> times;
        in property <int> current: -1;
        in property <string> note;
        // Full screen: bigger text.
        in property <bool> full;
        // A synced line was clicked: jump there (ms).
        callback seek(int);
        callback toggle-full();
        // Two text lines per slot: long lines wrap instead of being cut, and every slot keeps the
        // same height so the sung line can still be centered arithmetically.
        property <length> line-height: full ? 96px : 64px;
        // Follows the sung line until the user scrolls; "Follow" turns it back on (Spotifast's).
        property <bool> following: true;
        property <length> auto-y;
        function recenter() {
            if (current >= 0) {
                self.auto-y = min(0px, -(current * line-height) + lv.visible-height / 2 - line-height / 2);
                lv.content-y = self.auto-y;
            }
        }
        changed current => { if (following) { self.recenter(); } }
        changed lines => { self.following = true; }
        if lines.length == 0 : Text {
            text: note;
            horizontal-alignment: center;
            vertical-alignment: center;
            font-size: 18px;
            color: Theme.subdued;
        }
        lv := ListView {
            // A scroll the follow logic didn't make stops following.
            changed content-y => { if (abs(self.content-y - root.auto-y) > 2px) { root.following = false; } }
            for line[i] in lines : TouchArea {
                height: line-height;
                mouse-cursor: root.times.length > i ? pointer : default;
                clicked => { if (root.times.length > i) { root.following = true; root.seek(root.times[i]); } }
                Text {
                    text: line;
                    width: 100%;
                    height: 100%;
                    vertical-alignment: center;
                    horizontal-alignment: root.full ? center : left;
                    wrap: word-wrap;
                    overflow: elide;
                    font-size: root.full ? 34px : 22px;
                    font-weight: 700;
                    // Sung and current lines are white, upcoming ones dimmed; hover shows a clickable line.
                    color: current < 0 || i <= current || parent.has-hover ? Theme.text : #ffffff66;
                }
            }
        }
        HorizontalLayout {
            x: parent.width - self.width - 8px;
            y: 8px;
            spacing: 6px;
            if !root.following && root.times.length > 0 : Chip {
                label: "Follow";
                chosen: true;
                pad: 14px;
                clicked => { root.following = true; root.recenter(); }
            }
            if root.lines.length > 0 : Chip {
                label: root.full ? "Exit full screen" : "Full screen";
                pad: 14px;
                clicked => { root.toggle-full(); }
            }
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

    export struct EqPreset { name: string, bands: [float] }

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
        // Settings → GPU renderer (applies on restart).
        in-out property <bool> gpu;
        callback set-gpu(bool);
        in-out property <bool> low-api;
        callback set-low-api(bool);
        callback refresh();
        // Set when GitHub has a newer release.
        in property <string> update-version;
        callback install-update();
        // Settings EQ; bands and preamp -1..1 (±12 dB), balance -1..1. eq-changed(save).
        in-out property <bool> eq-on;
        in-out property <float> eq-preamp;
        in-out property <[float]> eq-bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        in-out property <float> eq-balance;
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
        // Right panel tab: 0 = now playing, 1 = queue, 2 = recent (Spotifast's Queue/Recent panel).
        in-out property <int> side-tab;
        // Track list keyboard cursor (arrow keys), -1 = none; reset when a new page's rows arrive.
        in-out property <int> kb-row: -1;
        changed tracks => { root.kb-row = -1; }
        // Multi-selection lives in the rows (`Row.marked`), changed from Rust on the UI thread.
        callback select-row(int, bool, bool);
        callback select-all();
        callback copy-selection();
        // Ctrl+X cuts (copy + remove), Delete only removes.
        callback remove-selection(bool);
        callback paste-links();
        // A dragged row (and the selection it belongs to) dropped on a sidebar entry: (list, row).
        callback drop-on-list(string, string);
        in-out property <bool> show-sidebar: true;
        property <float> unmuted-volume: 50;
        in property <[Row]> side-rows;
        callback side-recent();
        // A queue row: skip ahead to it.
        callback skip-to(string);
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
        // Home: albums of the recently played tracks, newest first.
        in property <[Row]> shelf;
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
            if (root.liked-checked < root.tracks.length && (-root.list-y + root.list-height) / root.row-h > root.liked-checked - 20) {
                // Moved ahead at once (by the backend's batch size) so further scrolling doesn't queue
                // the same batch again; the backend sets the real value when the batch is done.
                root.liked-checked += 200;
                root.check-liked-more();
            }
        }
        public function reveal(index: int) {
            // RowItem height in the track list: 40px cover + 16px padding. Puts the row about a third
            // down, but never scrolls past the end of the list.
            let end = max(0px, root.tracks.length * root.row-h - root.list-height);
            root.list-y = -min(end, max(0px, index * root.row-h - root.list-height / 3));
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
        // The Settings page covers the main panel's content (header row stays); navigating closes it.
        in-out property <bool> show-settings;
        in-out property <string> settings-query;
        // Case-insensitive substring match ("" matches everything); Slint strings have no `contains`.
        pure callback text-matches(string, string) -> bool;
        // Settings → Compact track list: one-line rows without covers (Spotifast's).
        in-out property <bool> compact;
        callback set-compact(bool);
        out property <length> row-h: root.compact ? 36px : 56px;
        changed page-title => { root.show-settings = false; }
        changed page-uri => { root.show-settings = false; }
        in property <[string]> lyric-lines;
        // Start time per line in ms; empty when the lyrics aren't synced.
        in property <[int]> lyric-times;
        in property <int> lyric-index: -1;
        // Full-screen lyrics: the window goes full screen with only the lyrics and the player bar.
        in-out property <bool> lyrics-full;
        callback set-fullscreen(bool);
        function toggle-lyrics-full() {
            root.lyrics-full = !root.lyrics-full;
            if (root.lyrics-full) { root.show-lyrics = true; root.show-settings = false; }
            root.set-fullscreen(root.lyrics-full);
        }
        changed show-lyrics => { if (!root.show-lyrics && root.lyrics-full) { root.toggle-lyrics-full(); } }
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

        function toggle-mute() {
            if (root.volume > 0) { root.unmuted-volume = root.volume; root.volume = 0; } else { root.volume = root.unmuted-volume; }
            root.set-volume(root.volume, true);
        }
        // Moves the keyboard cursor and keeps it on screen.
        function move-cursor(to: int, extend: bool) {
            if (root.tracks.length == 0) { return; }
            root.kb-row = max(0, min(root.tracks.length - 1, to));
            root.select-row(root.kb-row, false, extend);
            if (root.kb-row * root.row-h < -root.list-y) { root.list-y = -root.kb-row * root.row-h; }
            if ((root.kb-row + 1) * root.row-h > -root.list-y + root.list-height) { root.list-y = -((root.kb-row + 1) * root.row-h - root.list-height); }
        }
        // A track row dropped over the sidebar: add it to the entry under the pointer. True when
        // the drop was on the sidebar (handled), false to let the list reorder.
        function drop-on-sidebar(uri: string, x: length, y: length) -> bool {
            if (!root.show-sidebar || x < lib-list.absolute-position.x || x > lib-list.absolute-position.x + lib-list.width
                || y < lib-list.absolute-position.y || y > lib-list.absolute-position.y + lib-list.height) { return false; }
            // Sidebar rows are 64 px (48 px cover + 16).
            let i = floor((y - lib-list.absolute-position.y - lib-list.content-y) / 64px);
            if (i >= 0 && i < root.lists.length) { root.drop-on-list(root.lists[i].uri, uri); }
            return true;
        }
        // Opens the right panel on a tab (-1 closes it); Queue and Recent load their rows.
        function open-side(tab: int) {
            root.show-now-panel = tab >= 0;
            if (tab < 0) { return; }
            root.side-tab = tab;
            if (tab == 1) { root.show-queue(); }
            if (tab == 2) { root.side-recent(); }
        }

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
                    if (event.text == "u") { root.open-side(1); return accept; }
                    if (event.text == "a") { root.select-all(); return accept; }
                    if (event.text == "c") { root.copy-selection(); return accept; }
                    if (event.text == "x") { root.remove-selection(true); return accept; }
                    if (event.text == "v") { root.paste-links(); return accept; }
                    if (event.text == "b") { root.show-sidebar = !root.show-sidebar; return accept; }
                    if (event.text == "h") { root.show-settings = false; root.go-home(); return accept; }
                    if (event.text == ",") { root.show-settings = !root.show-settings; return accept; }
                }
                if (event.modifiers.shift && event.text == Key.LeftArrow) { root.position = max(0, root.position - 10000); root.seek(root.position); return accept; }
                if (event.modifiers.shift && event.text == Key.RightArrow) { root.position = min(root.duration, root.position + 10000); root.seek(root.position); return accept; }
                if (event.text == Key.UpArrow) { root.move-cursor(root.kb-row < 0 ? 0 : root.kb-row - 1, event.modifiers.shift); return accept; }
                if (event.text == Key.DownArrow) { root.move-cursor(root.kb-row + 1, event.modifiers.shift); return accept; }
                if (event.text == Key.Return && root.kb-row >= 0 && root.kb-row < root.tracks.length) { root.play-uri(root.tracks[root.kb-row].uri); return accept; }
                if (event.text == Key.Delete) { root.remove-selection(false); return accept; }
                if (!event.modifiers.control && !event.modifiers.alt) {
                    if (event.text == "m") { root.toggle-mute(); return accept; }
                    if (event.text == "b" && root.now.uri != "") { root.toggle-like(); return accept; }
                    if (event.text == "q") { root.open-side(root.show-now-panel && root.side-tab == 1 ? -1 : 1); return accept; }
                    if (event.text == "/") { query.focus(); return accept; }
                    if (event.text == "?") { shortcuts-popup.show(); return accept; }
                }
                if (event.modifiers.alt && event.text == Key.LeftArrow) { root.go-back(); return accept; }
                if (event.modifiers.alt && event.text == Key.RightArrow) { root.go-forward(); return accept; }
                if (event.text == " ") { root.toggle(); return accept; }
                if (event.text == Key.Escape) { if (root.lyrics-full) { root.toggle-lyrics-full(); } root.show-settings = false; self.focus(); return accept; }
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
                // Home / Search, then Your Library
                VerticalLayout {
                    visible: root.show-sidebar && !root.lyrics-full;
                    width: self.visible ? 280px : 0px;
                    spacing: 8px;
                    Rectangle {
                        border-radius: 12px;
                        background: Theme.panel;
                        border-width: 1px;
                        border-color: Theme.border;
                        VerticalLayout {
                            padding: 8px;
                            NavEntry { label: "Home"; shape: Icons.home; chosen: root.page-kind == "Home" && !root.show-settings; clicked => { root.show-settings = false; root.go-home(); } }
                            NavEntry { label: "Search"; shape: Icons.search; chosen: query.has-focus || root.search-tab >= 0; clicked => { root.show-settings = false; query.focus(); } }
                        }
                    }
                    Rectangle {
                        vertical-stretch: 1;
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
                                Icon { width: 22px; height: 22px; y: (parent.height - self.height) / 2; shape: Icons.library; tint: Theme.subdued; }
                                Text {
                                    text: "Your Library";
                                    font-size: 16px;
                                    font-weight: 700;
                                    color: Theme.text;
                                    vertical-alignment: center;
                                    horizontal-stretch: 1;
                                }
                                IconButton { y: (parent.height - self.height) / 2; shape: Icons.refresh; dot: false; size: 18px; clicked => { root.refresh(); } }
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
                            lib-list := ListView {
                                for row[i] in lists: RowItem {
                                    data: row;
                                    cover-size: 48px;
                                    selected: row.uri == root.current-list;
                                    sidebar: true;
                                    open-menu(x, y) => { root.open-row-menu(row, x, y, true); }
                                    clicked => { root.show-settings = false; root.open-list(row.uri); }
                                    double-clicked => { root.play-uri(row.uri); }
                                    play-cover => { root.play-uri(row.uri); }
                                    need-cover => { root.need-cover(1, i, row.uri, row.cover-url); }
                                }
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
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.back; dot: false; enabled: root.can-back || root.show-settings; clicked => { if (root.show-settings) { root.show-settings = false; } else { root.go-back(); } } }
                            IconButton { y: (parent.height - self.height) / 2; shape: Icons.forward; dot: false; enabled: root.can-forward; clicked => { root.show-settings = false; root.go-forward(); } }
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
                            // A newer GitHub release (update.rs); one click downloads, swaps the exe and restarts.
                            if root.update-version != "" : Chip {
                                y: (parent.height - self.height) / 2;
                                label: "Update to v" + root.update-version;
                                chosen: true;
                                pad: 14px;
                                clicked => { root.install-update(); }
                            }
                            settings-button := IconButton {
                                y: (parent.height - self.height) / 2;
                                shape: Icons.settings;
                                active: root.show-settings;
                                clicked => { root.show-settings = !root.show-settings; }
                            }
                        }
                        if !root.show-lyrics && root.page-kind != "Home" : HorizontalLayout {
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
                                    Icon { width: 26px; height: 26px; shape: Icons.play; filled: true; tint: Theme.on-accent; }
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
                                    Icon { width: 20px; height: 20px; shape: root.page-saved ? Icons.check : Icons.plus; tint: root.page-saved ? Theme.on-accent : Theme.text; }
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
                            if root.page-playable && !root.show-lyrics && root.page-kind != "Home" : Chip {
                                label: ["Custom order", "Title", "Artist", "Duration"][root.page-sort];
                                clicked => { root.page-sort = mod(root.page-sort + 1, 4); root.filter-page(page-filter.text, root.page-sort); }
                            }
                            Rectangle {
                                visible: root.page-playable && !root.show-lyrics && root.page-kind != "Home";
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
                            times: root.lyric-times;
                            current: root.lyric-index;
                            note: root.lyrics-note;
                            full: root.lyrics-full;
                            seek(ms) => { root.position = ms; root.seek(ms); }
                            toggle-full => { root.toggle-lyrics-full(); }
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
                        if !root.show-lyrics && root.page-kind == "Home" : home-scroll := ScrollView {
                            vertical-stretch: 1;
                            VerticalLayout {
                                width: home-scroll.visible-width;
                                spacing: 12px;
                                alignment: start;
                                Text { text: root.page-title; font-size: 28px; font-weight: 700; color: Theme.text; }
                                HorizontalLayout {
                                    spacing: 8px;
                                    VerticalLayout {
                                        horizontal-stretch: 1;
                                        spacing: 8px;
                                        for q[i] in root.quick-top : QuickTile { data: q; clicked => { root.play-uri(q.uri); } need-cover => { root.need-cover(6, i, q.uri, q.cover-url); } }
                                    }
                                    VerticalLayout {
                                        horizontal-stretch: 1;
                                        spacing: 8px;
                                        for q[i] in root.quick-bottom : QuickTile { data: q; clicked => { root.play-uri(q.uri); } need-cover => { root.need-cover(7, i, q.uri, q.cover-url); } }
                                    }
                                }
                                if root.shelf.length > 0 : Text { text: "Recently played"; font-size: 18px; font-weight: 700; color: Theme.text; }
                                // Scrolls sideways (horizontal scrollbar or Shift+wheel) while the page scrolls down.
                                if root.shelf.length > 0 : ScrollView {
                                    height: 200px;
                                    content-width: root.shelf.length * 136px;
                                    content-height: 188px;
                                    HorizontalLayout {
                                        for card[i] in root.shelf : ShelfCard {
                                            data: card;
                                            clicked => { root.open-list(card.uri); }
                                            need-cover => { root.need-cover(8, i, card.uri, card.cover-url); }
                                        }
                                    }
                                }
                                if root.tracks.length > 0 : Text { text: "Recent tracks"; font-size: 18px; font-weight: 700; color: Theme.text; }
                                for row[i] in tracks: RowItem {
                                    data: row;
                                    playing: row.uri == root.current-track;
                                    audible: self.playing && root.playing;
                                    menu: true;
                                    marked: row.marked || i == root.kb-row;
                                    draggable: true;
                                    index: i;
                                    count: root.tracks.length;
                                    select(ctrl, shift) => { root.kb-row = i; root.select-row(i, ctrl, shift); }
                                    dropped(n, x, y) => { root.drop-on-sidebar(row.uri, x, y); }
                                    clicked => { if (!self.dragged && !self.modifier-click) { if (self.playing) { root.toggle(); } else { root.play-uri(row.uri); } } }
                                    open-menu(x, y) => { root.open-row-menu(row, x, y, false); }
                                    need-cover => { root.need-cover(0, i, row.uri, row.cover-url); }
                                }
                            }
                        }
                        if !root.show-lyrics && root.page-kind != "Home" : ListView {
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
                                marked: row.marked || i == root.kb-row;
                                compact: root.compact;
                                // Any track can be dragged onto a sidebar playlist; only an own, unsorted playlist reorders.
                                draggable: row.uri.starts-with("spotify:track:");
                                reorder: root.editable && root.page-sort == 0 && root.page-filter-text == "";
                                index: i;
                                count: root.tracks.length;
                                select(ctrl, shift) => { root.kb-row = i; root.select-row(i, ctrl, shift); }
                                // Like Spotify: the playing row pauses/resumes instead of restarting.
                                clicked => { if (!self.dragged && !self.modifier-click) { if (self.playing) { root.toggle(); } else { root.play-uri(row.uri); } } }
                                dropped(n, x, y) => { if (!root.drop-on-sidebar(row.uri, x, y) && self.reorder) { root.move-row(i, i + n); } }
                                open-menu(x, y) => { root.open-row-menu(row, x, y, false); }
                                need-cover => { root.need-cover(0, i, row.uri, row.cover-url); }
                            }
                        }
                    }
                    if root.show-settings : Rectangle {
                        // Below the header row (padding 16 + 44 + spacing 12).
                        x: 1px;
                        y: 72px;
                        width: parent.width - 2px;
                        height: parent.height - self.y - 1px;
                        border-radius: 12px;
                        background: Theme.panel;
                        ScrollView {
                            VerticalLayout {
                                padding-left: max(24px, (parent.width - 680px) / 2);
                                padding-right: max(24px, (parent.width - 680px) / 2);
                                padding-bottom: 24px;
                                spacing: 4px;
                                alignment: start;
                                Text { text: "Settings"; font-size: 28px; font-weight: 700; color: Theme.text; }
                                // Narrows the page to matching rows; sections without matches disappear.
                                Rectangle {
                                    height: 34px;
                                    border-radius: 17px;
                                    background: settings-filter.has-focus ? Theme.hover : Theme.field;
                                    Icon { x: 12px; width: 16px; height: 16px; y: (parent.height - 16px) / 2; shape: Icons.search; tint: Theme.subdued; }
                                    settings-filter := TextInput {
                                        x: 36px;
                                        width: parent.width - 48px;
                                        height: parent.height;
                                        vertical-alignment: center;
                                        single-line: true;
                                        font-size: 13px;
                                        color: Theme.text;
                                        text <=> root.settings-query;
                                    }
                                    if settings-filter.text == "" : Text { x: 36px; height: parent.height; vertical-alignment: center; text: "Search settings"; font-size: 13px; color: Theme.muted; }
                                }
                                if root.text-matches(root.settings-query, "Audio quality Streaming quality kbps bitrate Normalize volume loudness") : SettingsHeading { text: "Audio quality"; }
                                if root.text-matches(root.settings-query, "Streaming quality kbps bitrate") : VerticalLayout {
                                    SettingRow {
                                        label: "Streaming quality";
                                        for label[i] in ["96 kbps", "160 kbps", "320 kbps"] : Chip {
                                            y: (parent.height - self.height) / 2;
                                            label: label;
                                            chosen: root.quality == i;
                                            clicked => { root.quality = i; root.set-quality(i); }
                                        }
                                    }
                                }
                                if root.text-matches(root.settings-query, "Normalize volume loudness") : VerticalLayout {
                                    Switch { label: "Normalize volume"; desc: "Set the same volume level for all songs and podcasts"; on <=> root.normalize; toggled(v) => { root.set-normalize(v); } }
                                }
                                if root.text-matches(root.settings-query, "Playback Equalizer EQ presets bass treble preamp Sleep timer") : SettingsHeading { text: "Playback"; }
                                if root.text-matches(root.settings-query, "Equalizer EQ presets bass treble preamp") : VerticalLayout {
                                    Switch { label: "Equalizer"; desc: "Double-click a slider to reset it"; on <=> root.eq-on; toggled(v) => { root.eq-changed(true); } }
                                    HorizontalLayout {
                                        spacing: 4px;
                                        alignment: start;
                                        for p in root.eq-presets : Chip {
                                            label: p.name;
                                            pad: 10px;
                                            clicked => { root.eq-bands = p.bands; root.eq-on = true; root.eq-changed(true); }
                                        }
                                    }
                                    Rectangle {
                                        height: 170px;
                                        border-radius: 8px;
                                        background: Theme.raised;
                                        HorizontalLayout {
                                            padding: 12px;
                                            alignment: space-between;
                                            EqBand {
                                                label: "Pre";
                                                value: root.eq-preamp;
                                                moved(v) => { root.eq-preamp = v; root.eq-changed(false); }
                                                released => { root.eq-changed(true); }
                                            }
                                            for band[i] in root.eq-bands : EqBand {
                                                label: ["60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K"][i];
                                                value: band;
                                                moved(v) => { root.eq-bands[i] = v; root.eq-changed(false); }
                                                released => { root.eq-changed(true); }
                                            }
                                        }
                                    }
                                }
                                if root.text-matches(root.settings-query, "Sleep timer") : VerticalLayout {
                                    SettingRow {
                                        label: "Sleep timer";
                                        for choice[i] in [{ label: "Off", m: 0 }, { label: "15 min", m: 15 }, { label: "30 min", m: 30 }, { label: "1 hr", m: 60 }, { label: "End of track", m: -1 }] : Chip {
                                            y: (parent.height - self.height) / 2;
                                            label: choice.label;
                                            pad: 10px;
                                            chosen: root.sleep-minutes == choice.m;
                                            clicked => { root.sleep-minutes = choice.m; root.set-sleep(choice.m); }
                                        }
                                    }
                                }
                                if root.text-matches(root.settings-query, "Display Theme palette colors Compact track list GPU acceleration renderer memory") : SettingsHeading { text: "Display"; }
                                if root.text-matches(root.settings-query, "Theme palette colors") : VerticalLayout {
                                    SettingRow {
                                        label: "Theme";
                                        for t in [{ name: "Spotifast", i: 6 }, { name: "Indigo", i: 0 }, { name: "Nord", i: 1 }, { name: "Tokyo", i: 2 }, { name: "Mocha", i: 3 }, { name: "Rosé", i: 4 }, { name: "Dark", i: 5 }] : Chip {
                                            y: (parent.height - self.height) / 2;
                                            label: t.name;
                                            pad: 10px;
                                            chosen: Theme.palette == t.i;
                                            clicked => { Theme.palette = t.i; root.set-theme(t.i); }
                                        }
                                    }
                                }
                                if root.text-matches(root.settings-query, "Compact track list") : VerticalLayout {
                                    Switch { label: "Compact track list"; desc: "One line per song, no covers (also saves memory)"; on <=> root.compact; toggled(v) => { root.set-compact(v); } }
                                }
                                if root.text-matches(root.settings-query, "GPU acceleration renderer memory") : VerticalLayout {
                                    Switch { label: "GPU acceleration"; desc: "Smoother on some PCs, but uses about 90 MB more memory. Applies on restart."; on <=> root.gpu; toggled(v) => { root.set-gpu(v); } }
                                }
                                if root.text-matches(root.settings-query, "Spotify API Low API usage quota requests cache Your own Spotify app Client ID developer") : SettingsHeading { text: "Spotify API"; }
                                if root.text-matches(root.settings-query, "Low API usage quota requests cache") : VerticalLayout {
                                    Switch {
                                        label: "Low API usage";
                                        desc: "Fewer Spotify requests, for apps near their quota. When on: the library loads from a cache up to 6 hours old, liked marks are asked once per run, Home reuses Recently played for 60 s and checks new releases once a day. Changes made in other Spotify apps show up later; the refresh button next to Your Library reloads everything.";
                                        on <=> root.low-api;
                                        toggled(v) => { root.set-low-api(v); }
                                    }
                                }
                                if root.text-matches(root.settings-query, "Your own Spotify app Client ID developer") : VerticalLayout {
                                    // Optional own Spotify app for the Web API (like Spotifast's personal Client ID).
                                    SettingRow {
                                        label: "Your own Spotify app";
                                        desc: "Client ID from developer.spotify.com (redirect URI http://127.0.0.1:8989/login). Empty uses the shared app.";
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
                                }
                                if root.text-matches(root.settings-query, "Startup Start with Windows tray autostart") : SettingsHeading { text: "Startup"; }
                                if root.text-matches(root.settings-query, "Start with Windows tray autostart") : VerticalLayout {
                                    Switch { label: "Start with Windows"; desc: "Starts in the tray"; on <=> root.start-with-windows; toggled(v) => { root.set-autostart(v); } }
                                }
                            }
                        }
                    }
                }
                if root.show-now-panel && !root.lyrics-full : Rectangle {
                    width: 300px;
                    border-radius: 12px;
                    background: Theme.panel;
                    border-width: 1px;
                    border-color: Theme.border;
                    VerticalLayout {
                        padding: 16px;
                        spacing: 12px;
                        // The queue/recent list takes the rest of the height; the now-playing card sits on top.
                        alignment: root.side-tab == 0 ? LayoutAlignment.start : LayoutAlignment.stretch;
                        HorizontalLayout {
                            spacing: 6px;
                            alignment: start;
                            for t[i] in ["Now playing", "Queue", "Recent"] : Chip {
                                label: t;
                                pad: 12px;
                                chosen: root.side-tab == i;
                                clicked => { root.open-side(i); }
                            }
                        }
                        if root.side-tab != 0 : Text {
                            text: root.side-tab == 1 ? (root.side-rows.length > 0 ? "Next up" : "Nothing queued") : "Recently played";
                            font-size: 14px;
                            font-weight: 700;
                            color: Theme.text;
                        }
                        if root.side-tab != 0 : ListView {
                            vertical-stretch: 1;
                            for row[i] in root.side-rows : RowItem {
                                data: row;
                                index: i;
                                count: root.side-rows.length;
                                menu: true;
                                playing: row.uri == root.current-track;
                                audible: self.playing && root.playing;
                                clicked => { if (root.side-tab == 1) { root.skip-to(row.uri); } else { root.play-uri(row.uri); } }
                                open-menu(x, y) => { root.open-row-menu(row, x, y, false); }
                                need-cover => { root.need-cover(9, i, row.uri, row.cover-url); }
                            }
                        }
                        if root.side-tab == 0 : Rectangle {
                            height: self.width;
                            border-radius: 8px;
                            clip: true;
                            background: Theme.raised;
                            Image { source: root.now-big; width: parent.width; height: parent.height; image-fit: cover; }
                        }
                        if root.side-tab == 0 : Text { text: now.title; font-size: 22px; font-weight: 700; color: Theme.text; wrap: word-wrap; }
                        if root.side-tab == 0 : Text { text: now.artist; font-size: 14px; color: Theme.subdued; wrap: word-wrap; }
                        // Spotify's "Lyrics preview": the sung line and the next two (Ctrl+Y opens all).
                        if root.side-tab == 0 && root.lyric-lines.length > 0 : Rectangle {
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
                        if root.side-tab == 0 && root.now-about != "" : Rectangle {
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
            shortcuts-popup := PopupWindow {
                x: (root.width - 440px) / 2;
                y: 80px;
                width: 440px;
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
                        spacing: 4px;
                        Text { text: "Keyboard shortcuts"; font-size: 16px; font-weight: 700; color: Theme.text; }
                        for k in [
                            { keys: "Space", what: "Play or pause" },
                            { keys: "Ctrl+← / Ctrl+→", what: "Previous / next" },
                            { keys: "Shift+← / Shift+→", what: "Seek 10 seconds" },
                            { keys: "Ctrl+↑ / Ctrl+↓", what: "Volume" },
                            { keys: "M", what: "Mute" },
                            { keys: "B", what: "Like the playing song" },
                            { keys: "Ctrl+S / Ctrl+R", what: "Shuffle / repeat" },
                            { keys: "Q or Ctrl+U", what: "Queue" },
                            { keys: "/ or Ctrl+L", what: "Search" },
                            { keys: "Ctrl+F", what: "Find in page" },
                            { keys: "↑ / ↓, Shift to extend", what: "Move through songs" },
                            { keys: "Enter", what: "Play the selected song" },
                            { keys: "Ctrl+A / Ctrl+C", what: "Select all / copy links" },
                            { keys: "Ctrl+V / Ctrl+X", what: "Paste links / cut (own playlists)" },
                            { keys: "Delete", what: "Remove selected (own playlists)" },
                            { keys: "Ctrl+B", what: "Show or hide the sidebar" },
                            { keys: "Ctrl+H / Ctrl+,", what: "Home / Settings" },
                            { keys: "Alt+← / Alt+→", what: "Back / forward" },
                            { keys: "Ctrl+Y / Ctrl+E", what: "Lyrics / radio" },
                            { keys: "Ctrl+Q", what: "Quit" },
                        ] : HorizontalLayout {
                            height: 22px;
                            Text { text: k.keys; width: 190px; font-size: 12px; font-weight: 600; color: Theme.text; vertical-alignment: center; }
                            Text { text: k.what; font-size: 12px; color: Theme.subdued; vertical-alignment: center; }
                        }
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
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.panel; active: root.show-now-panel && root.side-tab == 0; size: 18px; clicked => { root.open-side(root.show-now-panel && root.side-tab == 0 ? -1 : 0); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.mini; active: root.mini; size: 18px; clicked => { root.toggle-mini(); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.lyrics; active: root.show-lyrics; size: 18px; clicked => { root.show-lyrics = !root.show-lyrics; } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.queue; active: root.show-now-panel && root.side-tab == 1; size: 18px; clicked => { root.open-side(root.show-now-panel && root.side-tab == 1 ? -1 : 1); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.radio; size: 18px; enabled: root.now.uri != ""; clicked => { root.start-radio(); } }
                    IconButton { y: (parent.height - self.height) / 2; shape: Icons.devices; size: 18px; clicked => { root.load-devices(); devices-popup.show(); } }
                    IconButton {
                        y: (parent.height - self.height) / 2;
                        shape: root.volume <= 0 ? Icons.muted : Icons.volume;
                        size: 18px;
                        dot: false;
                        clicked => { root.toggle-mute(); }
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
            marked: false,
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
    app.set_eq_balance(eq.balance);
}

pub fn get_eq(app: &App) -> crate::eq::EqState {
    let max = crate::eq::MAX_DB;
    let mut bands = [0.0; 10];
    for (b, v) in bands.iter_mut().zip(app.get_eq_bands().iter()) {
        *b = (v * max).clamp(-max, max);
    }
    crate::eq::EqState { on: app.get_eq_on(), preamp: app.get_eq_preamp() * max, bands, balance: app.get_eq_balance() }
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
