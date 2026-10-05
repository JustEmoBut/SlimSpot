//! Draws SlimSpot's own Winamp-classic skin into `assets/skin/*.png` (embedded by src/skin.rs).
//! Sprite positions follow the classic skin format (webamp's skinSprites); the art is original,
//! in the Midnight Indigo palette, so no third-party skin ships with the app.
//!
//! Run after changing it: `cargo run --example default_skin`

use image::{Rgba, RgbaImage};

type C = [u8; 3];
const BASE: C = [8, 8, 26];
const PANEL: C = [16, 16, 42];
const RAISED: C = [27, 27, 56];
const SELECTED: C = [44, 44, 94];
const LIGHT: C = [70, 70, 130];
const SHADOW: C = [4, 4, 12];
const LCD: C = [4, 4, 16];
const TEXT: C = [248, 250, 252];
const SUBDUED: C = [163, 168, 195];
const MUTED: C = [107, 111, 142];
const ACCENT: C = [139, 92, 246];
const ACCENT2: C = [34, 211, 238];

/// 5x6 font: one string of five rows per glyph, '#' = ink. Order matches text.bmp's cells.
const FONT: &[(char, [&str; 5])] = &[
    ('A', [".###.", "#...#", "#####", "#...#", "#...#"]),
    ('B', ["####.", "#...#", "####.", "#...#", "####."]),
    ('C', [".####", "#....", "#....", "#....", ".####"]),
    ('D', ["####.", "#...#", "#...#", "#...#", "####."]),
    ('E', ["#####", "#....", "####.", "#....", "#####"]),
    ('F', ["#####", "#....", "####.", "#....", "#...."]),
    ('G', [".####", "#....", "#..##", "#...#", ".###."]),
    ('H', ["#...#", "#...#", "#####", "#...#", "#...#"]),
    ('I', ["#####", "..#..", "..#..", "..#..", "#####"]),
    ('J', ["..###", "...#.", "...#.", "#..#.", ".##.."]),
    ('K', ["#..#.", "#.#..", "##...", "#.#..", "#..#."]),
    ('L', ["#....", "#....", "#....", "#....", "#####"]),
    ('M', ["#...#", "##.##", "#.#.#", "#...#", "#...#"]),
    ('N', ["#...#", "##..#", "#.#.#", "#..##", "#...#"]),
    ('O', [".###.", "#...#", "#...#", "#...#", ".###."]),
    ('P', ["####.", "#...#", "####.", "#....", "#...."]),
    ('Q', [".###.", "#...#", "#.#.#", "#..#.", ".##.#"]),
    ('R', ["####.", "#...#", "####.", "#..#.", "#...#"]),
    ('S', [".####", "#....", ".###.", "....#", "####."]),
    ('T', ["#####", "..#..", "..#..", "..#..", "..#.."]),
    ('U', ["#...#", "#...#", "#...#", "#...#", ".###."]),
    ('V', ["#...#", "#...#", "#...#", ".#.#.", "..#.."]),
    ('W', ["#...#", "#...#", "#.#.#", "##.##", "#...#"]),
    ('X', ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"]),
    ('Y', ["#...#", ".#.#.", "..#..", "..#..", "..#.."]),
    ('Z', ["#####", "...#.", "..#..", ".#...", "#####"]),
    ('"', [".#.#.", ".#.#.", ".....", ".....", "....."]),
    ('@', [".###.", "#.###", "#.##.", "#....", ".###."]),
    ('0', [".###.", "#..##", "#.#.#", "##..#", ".###."]),
    ('1', ["..#..", ".##..", "..#..", "..#..", ".###."]),
    ('2', ["####.", "....#", ".###.", "#....", "#####"]),
    ('3', ["####.", "....#", ".###.", "....#", "####."]),
    ('4', ["#..#.", "#..#.", "#####", "...#.", "...#."]),
    ('5', ["#####", "#....", "####.", "....#", "####."]),
    ('6', [".###.", "#....", "####.", "#...#", ".###."]),
    ('7', ["#####", "....#", "...#.", "..#..", "..#.."]),
    ('8', [".###.", "#...#", ".###.", "#...#", ".###."]),
    ('9', [".###.", "#...#", ".####", "....#", ".###."]),
    ('…', [".....", ".....", ".....", ".....", "#.#.#"]),
    ('.', [".....", ".....", ".....", ".....", "..#.."]),
    (':', [".....", "..#..", ".....", "..#..", "....."]),
    ('(', ["...#.", "..#..", "..#..", "..#..", "...#."]),
    (')', [".#...", "..#..", "..#..", "..#..", ".#..."]),
    ('-', [".....", ".....", ".###.", ".....", "....."]),
    ('\'', ["..#..", "..#..", ".....", ".....", "....."]),
    ('!', ["..#..", "..#..", "..#..", ".....", "..#.."]),
    ('_', [".....", ".....", ".....", ".....", "#####"]),
    ('+', [".....", "..#..", ".###.", "..#..", "....."]),
    ('\\', ["#....", ".#...", "..#..", "...#.", "....#"]),
    ('/', ["....#", "...#.", "..#..", ".#...", "#...."]),
    ('[', ["..##.", "..#..", "..#..", "..#..", "..##."]),
    (']', [".##..", "..#..", "..#..", "..#..", ".##.."]),
    ('^', ["..#..", ".#.#.", ".....", ".....", "....."]),
    ('&', [".##..", "#..#.", ".##.#", "#..#.", ".##.#"]),
    ('%', ["#...#", "...#.", "..#..", ".#...", "#...#"]),
    (',', [".....", ".....", ".....", "..#..", ".#..."]),
    ('=', [".....", ".###.", ".....", ".###.", "....."]),
    ('$', [".####", "#.#..", ".###.", "..#.#", "####."]),
    ('#', [".#.#.", "#####", ".#.#.", "#####", ".#.#."]),
    ('Å', ["..#..", ".###.", "#...#", "#####", "#...#"]),
    ('Ö', ["#...#", ".###.", "#...#", "#...#", ".###."]),
    ('Ä', ["#...#", ".###.", "#...#", "#####", "#...#"]),
    ('?', [".###.", "#...#", "..##.", ".....", "..#.."]),
    ('*', [".....", "#.#.#", ".###.", "#.#.#", "....."]),
];

/// text.bmp cell of each FONT entry (same table as `skin::cell`).
fn cell(c: char) -> (u32, u32) {
    match c {
        'A'..='Z' => (c as u32 - 'A' as u32, 0),
        '"' => (26, 0),
        '@' => (27, 0),
        '0'..='9' => (c as u32 - '0' as u32, 1),
        'Å' => (0, 2),
        'Ö' => (1, 2),
        'Ä' => (2, 2),
        '?' => (3, 2),
        '*' => (4, 2),
        _ => (10 + "….:()-'!_+\\/[]^&%,=$#".chars().position(|p| p == c).expect("glyph has a cell") as u32, 1),
    }
}

struct Sheet(RgbaImage);

impl Sheet {
    fn new(w: u32, h: u32, bg: C) -> Self {
        Sheet(RgbaImage::from_pixel(w, h, Rgba([bg[0], bg[1], bg[2], 255])))
    }

    fn px(&mut self, x: u32, y: u32, c: C) {
        if x < self.0.width() && y < self.0.height() {
            self.0.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
        }
    }

    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, c: C) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.px(xx, yy, c);
            }
        }
    }

    /// One-pixel bevel: `top` on the top/left edges, `bottom` on the bottom/right.
    fn bevel(&mut self, x: u32, y: u32, w: u32, h: u32, top: C, bottom: C) {
        self.fill(x, y, w, 1, top);
        self.fill(x, y, 1, h, top);
        self.fill(x, y + h - 1, w, 1, bottom);
        self.fill(x + w - 1, y, 1, h, bottom);
    }

    /// A raised (or, pressed, sunken) button face.
    fn button(&mut self, x: u32, y: u32, w: u32, h: u32, pressed: bool) {
        self.fill(x, y, w, h, if pressed { SELECTED } else { RAISED });
        if pressed {
            self.bevel(x, y, w, h, SHADOW, LIGHT);
        } else {
            self.bevel(x, y, w, h, LIGHT, SHADOW);
        }
    }

    /// Recessed display area.
    fn well(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.fill(x, y, w, h, LCD);
        self.bevel(x, y, w, h, SHADOW, LIGHT);
    }

    fn glyph(&mut self, x: u32, y: u32, c: char, ink: C) {
        let rows = FONT.iter().find(|(g, _)| *g == c).map(|(_, r)| r);
        for (dy, row) in rows.into_iter().flatten().enumerate() {
            for (dx, b) in row.chars().enumerate() {
                if b == '#' {
                    self.px(x + dx as u32, y + dy as u32, ink);
                }
            }
        }
    }

    fn text(&mut self, x: u32, y: u32, s: &str, ink: C) {
        for (i, c) in s.chars().enumerate() {
            self.glyph(x + i as u32 * 6, y, c, ink);
        }
    }

    fn text_width(s: &str) -> u32 {
        (s.chars().count() as u32 * 6).saturating_sub(1)
    }

    fn centered(&mut self, x: u32, y: u32, w: u32, h: u32, s: &str, ink: C) {
        self.text(x + w.saturating_sub(Self::text_width(s)) / 2, y + (h - 5) / 2, s, ink);
    }

    /// Pixel art from rows of '#', drawn at (x, y).
    fn art(&mut self, x: u32, y: u32, rows: &[&str], ink: C) {
        for (dy, row) in rows.iter().enumerate() {
            for (dx, b) in row.chars().enumerate() {
                if b == '#' {
                    self.px(x + dx as u32, y + dy as u32, ink);
                }
            }
        }
    }

    fn save(&self, name: &str) {
        self.0.save(format!("assets/skin/{name}.png")).expect("write sprite sheet");
    }
}

fn lerp(a: C, b: C, t: f32) -> C {
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8)
}

/// Title bar strip: grip lines either side of a centered caption.
fn title(s: &mut Sheet, x: u32, y: u32, w: u32, caption: &str, active: bool) {
    s.fill(x, y, w, 14, if active { RAISED } else { PANEL });
    let tw = Sheet::text_width(caption) + 8;
    let cx = x + (w - tw) / 2;
    let grip = if active { ACCENT } else { MUTED };
    for gy in [4, 6, 8] {
        s.fill(x + 20, y + gy, cx - x - 24, 1, grip);
        s.fill(cx + tw + 4, y + gy, x + w - cx - tw - 24, 1, grip);
    }
    s.text(cx + 4, y + 4, caption, if active { TEXT } else { SUBDUED });
}

fn title_button(s: &mut Sheet, x: u32, y: u32, pressed: bool, icon: &[&str]) {
    s.button(x, y, 9, 9, pressed);
    s.art(x + 2, y + 2, icon, TEXT);
}

const ICON_CLOSE: &[&str] = &["#...#", ".#.#.", "..#..", ".#.#.", "#...#"];
const ICON_MIN: &[&str] = &[".....", ".....", ".....", ".....", "#####"];
const ICON_SHADE: &[&str] = &["#####", "#...#", "#####", ".....", "....."];
const ICON_MENU: &[&str] = &["#####", ".....", "#####", ".....", "#####"];

fn main_bmp() {
    let mut s = Sheet::new(275, 116, PANEL);
    s.bevel(0, 0, 275, 116, LIGHT, SHADOW);
    // Left display: clutter bar, play state, time and the (empty) visualizer.
    s.well(8, 20, 95, 43);
    s.fill(9, 21, 9, 41, BASE);
    s.text(80, 33, ":", ACCENT2);
    // Title, bitrate and frequency displays.
    s.well(108, 22, 160, 11);
    s.well(108, 40, 21, 11);
    s.text(130, 43, "KBPS", SUBDUED);
    s.well(154, 40, 14, 11);
    s.text(170, 43, "KHZ", SUBDUED);
    s.fill(16, 85, 243, 1, SHADOW);
    s.fill(16, 86, 243, 1, LIGHT);
    s.text(250, 96, "SS", ACCENT);
    s.save("main");
}

fn cbuttons() {
    let mut s = Sheet::new(136, 36, PANEL);
    let icons: [(u32, u32, u32, &[&str]); 6] = [
        (0, 23, 18, &["#....#", "#...##", "#..###", "#...##", "#....#"]),
        (23, 23, 18, &["#...", "##..", "###.", "##..", "#..."]),
        (46, 23, 18, &["##.##", "##.##", "##.##", "##.##", "##.##"]),
        (69, 23, 18, &["#####", "#####", "#####", "#####", "#####"]),
        (92, 22, 18, &["#....#", "##...#", "###..#", "##...#", "#....#"]),
        (114, 22, 16, &["..#..", ".###.", "#####", ".....", "#####"]),
    ];
    for (x, w, h, icon) in icons {
        for pressed in [false, true] {
            let y = if pressed { h } else { 0 };
            s.button(x, y, w, h, pressed);
            let iw = icon[0].len() as u32;
            let shift = pressed as u32;
            s.art(x + (w - iw) / 2 + shift, y + (h - 5) / 2 + shift, icon, if pressed { ACCENT2 } else { TEXT });
        }
    }
    s.save("cbuttons");
}

fn titlebar() {
    let mut s = Sheet::new(344, 87, PANEL);
    title(&mut s, 27, 0, 275, "SLIMSPOT", true);
    title(&mut s, 27, 15, 275, "SLIMSPOT", false);
    for pressed in [false, true] {
        let y = if pressed { 9 } else { 0 };
        title_button(&mut s, 0, y, pressed, ICON_MENU);
        title_button(&mut s, 9, y, pressed, ICON_MIN);
        title_button(&mut s, 18, y, pressed, ICON_CLOSE);
    }
    title_button(&mut s, 0, 18, false, ICON_SHADE);
    title_button(&mut s, 9, 18, true, ICON_SHADE);
    // Clutter bar O A I D V, and each letter lit.
    let letters = [('O', 3, 8), ('A', 11, 7), ('I', 18, 7), ('D', 25, 8), ('V', 33, 7)];
    s.fill(304, 0, 8, 43, BASE);
    s.fill(312, 0, 8, 43, BASE);
    for (i, (c, y, h)) in letters.into_iter().enumerate() {
        s.glyph(305, y + 1, c, MUTED);
        s.glyph(313, y + 1, c, SHADOW);
        let sx = 304 + i as u32 * 8;
        let sy = 44 + y;
        s.fill(sx, sy, 8, h, BASE);
        s.glyph(sx + 1, sy + 1, c, ACCENT2);
    }
    s.save("titlebar");
}

fn posbar() {
    let mut s = Sheet::new(307, 10, PANEL);
    s.well(0, 3, 248, 4);
    for (x, pressed) in [(248, false), (278, true)] {
        s.button(x, 0, 29, 10, pressed);
        s.fill(x + 4, 4, 21, 2, if pressed { ACCENT2 } else { ACCENT });
    }
    s.save("posbar");
}

/// 28 slider frames, 15 px apart, then the thumb (normal at x 15, pressed at x 0) at y 422.
fn level_slider(name: &str, width: u32, track_x: u32, track_w: u32, from_center: bool) {
    let mut s = Sheet::new(width, 433, PANEL);
    for i in 0..28u32 {
        let y = i * 15;
        let t = i as f32 / 27.0;
        s.well(track_x, y + 4, track_w, 5);
        let fill = ((track_w - 2) as f32 * if from_center { t / 2.0 } else { t }).round() as u32;
        let color = lerp(ACCENT, ACCENT2, t);
        if from_center {
            let mid = track_x + track_w / 2;
            s.fill(mid - fill, y + 5, fill * 2, 3, color);
        } else {
            s.fill(track_x + 1, y + 5, fill, 3, color);
        }
    }
    for (x, pressed) in [(15, false), (0, true)] {
        s.button(x, 422, 14, 11, pressed);
        s.fill(x + 4, 426, 6, 3, if pressed { ACCENT2 } else { SUBDUED });
    }
    s.save(name);
}

fn shufrep() {
    let mut s = Sheet::new(92, 85, PANEL);
    // Rows: off, off pressed, on, on pressed.
    for (row, (on, pressed)) in [(false, false), (false, true), (true, false), (true, true)].into_iter().enumerate() {
        let y = row as u32 * 15;
        for (x, w, label) in [(0, 28, "REP"), (28, 47, "SHUFFLE")] {
            s.button(x, y, w, 15, pressed);
            // SHUFFLE is 41 px in this font: a narrower lamp so it fits the 47 px button.
            s.fill(x + 2, y + 6, 2, 3, if on { ACCENT2 } else { SHADOW });
            s.text(x + 5, y + 5, label, if on { TEXT } else { SUBDUED });
        }
    }
    // EQ / PL toggles: off and on, then pressed.
    for (x, label, pressed) in [(0, "EQ", false), (23, "PL", false), (46, "EQ", true), (69, "PL", true)] {
        for (y, on) in [(61, false), (73, true)] {
            s.button(x, y, 23, 12, pressed);
            s.fill(x + 3, y + 5, 3, 3, if on { ACCENT2 } else { SHADOW });
            s.text(x + 9, y + 4, label, if on { TEXT } else { SUBDUED });
        }
    }
    s.save("shufrep");
}

/// Seven-segment digits, 9x13; the middle bar of the 2 doubles as the minus sign (x 20, y 6).
fn numbers() {
    let mut s = Sheet::new(99, 13, LCD);
    // Segments a b c d e f g.
    let digits = ["abcdef", "bc", "abged", "abgcd", "fgbc", "afgcd", "afgedc", "abc", "abcdefg", "abcdfg"];
    for (n, segs) in digits.iter().enumerate() {
        let x = n as u32 * 9;
        for seg in segs.chars() {
            match seg {
                'a' => s.fill(x + 2, 0, 5, 1, ACCENT2),
                'g' => s.fill(x + 2, 6, 5, 1, ACCENT2),
                'd' => s.fill(x + 2, 12, 5, 1, ACCENT2),
                'f' => s.fill(x + 1, 1, 1, 5, ACCENT2),
                'b' => s.fill(x + 7, 1, 1, 5, ACCENT2),
                'e' => s.fill(x + 1, 7, 1, 5, ACCENT2),
                'c' => s.fill(x + 7, 7, 1, 5, ACCENT2),
                _ => unreachable!(),
            }
        }
    }
    s.save("numbers");
}

fn text_bmp() {
    let mut s = Sheet::new(155, 18, LCD);
    for (c, _) in FONT {
        let (col, row) = cell(*c);
        s.glyph(col * 5, row * 6, *c, ACCENT2);
    }
    s.save("text");
}

fn playpaus() {
    let mut s = Sheet::new(42, 9, LCD);
    s.art(2, 1, &["#....", "###..", "#####", "###..", "#...."], ACCENT2);
    s.art(11, 2, &["##.##", "##.##", "##.##", "##.##", "##.##"], SUBDUED);
    s.art(20, 2, &["#####", "#####", "#####", "#####", "#####"], MUTED);
    s.save("playpaus");
}

fn monoster() {
    let mut s = Sheet::new(56, 24, PANEL);
    for (y, ink) in [(0, ACCENT2), (12, MUTED)] {
        // 29 and 27 px cells: "STEREO" doesn't fit the font, so the short forms.
        s.text(3, y + 4, "STER", ink);
        s.text(31, y + 4, "MONO", ink);
    }
    s.save("monoster");
}

fn eqmain() {
    let mut s = Sheet::new(275, 315, PANEL);
    // Window background.
    s.bevel(0, 0, 275, 116, LIGHT, SHADOW);
    s.text(8, 104, "PREAMP", SUBDUED);
    s.text(40, 40, "+12", MUTED);
    s.text(46, 67, "0", MUTED);
    s.text(40, 95, "-12", MUTED);
    for (i, f) in ["60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K"].iter().enumerate() {
        // Labels are wider than the 18 px pitch allows side by side: alternate two rows.
        let cx = 78 + i as u32 * 18 + 7;
        s.text(cx - Sheet::text_width(f) / 2, 103 + (i as u32 % 2) * 6, f, SUBDUED);
    }
    // Title bars and close button.
    title(&mut s, 0, 134, 275, "EQUALIZER", true);
    title(&mut s, 0, 149, 275, "EQUALIZER", false);
    title_button(&mut s, 0, 116, false, ICON_CLOSE);
    title_button(&mut s, 0, 125, true, ICON_CLOSE);
    // ON / AUTO: off, on, off pressed, on pressed.
    for (x, w, label, on, pressed) in [
        (10, 26, "ON", false, false),
        (69, 26, "ON", true, false),
        (128, 26, "ON", false, true),
        (187, 26, "ON", true, true),
        (36, 32, "AUTO", false, false),
        (95, 32, "AUTO", true, false),
        (154, 32, "AUTO", false, true),
        (213, 32, "AUTO", true, true),
    ] {
        s.button(x, 119, w, 12, pressed);
        s.fill(x + 3, 123, 3, 3, if on { ACCENT2 } else { SHADOW });
        s.text(x + 9, 122, label, if on { TEXT } else { SUBDUED });
    }
    // 28 band slider frames (14x63), low to high, and the thumb.
    for i in 0..28u32 {
        let (x, y) = (13 + (i % 14) * 15, 164 + (i / 14) * 65);
        s.fill(x, y, 14, 63, PANEL);
        s.well(x + 5, y, 4, 63);
        let color = lerp(ACCENT2, ACCENT, i as f32 / 27.0);
        s.fill(x + 6, y + 1, 2, 61, lerp(LCD, color, 0.6));
        s.fill(x + 3, y + 31, 8, 1, MUTED);
    }
    for (y, pressed) in [(164, false), (176, true)] {
        s.button(0, y, 11, 11, pressed);
        s.fill(3, y + 5, 5, 1, if pressed { ACCENT2 } else { SUBDUED });
    }
    for (y, pressed) in [(164, false), (176, true)] {
        s.button(224, y, 44, 12, pressed);
        s.centered(224, y, 44, 12, "PRESETS", if pressed { ACCENT2 } else { TEXT });
    }
    // Response graph background.
    s.well(0, 294, 113, 19);
    s.fill(1, 303, 111, 1, RAISED);
    s.save("eqmain");
}

fn pledit() {
    let mut s = Sheet::new(280, 186, PANEL);
    for (y, active) in [(0, true), (21, false)] {
        let bg = if active { RAISED } else { PANEL };
        let grip = if active { ACCENT } else { MUTED };
        // Left corner, caption, tile and right corner (with the close button).
        for x in [0, 127, 153] {
            s.fill(x, y, 25, 20, bg);
            for gy in [5, 7, 9] {
                s.fill(x, y + gy, 25, 1, grip);
            }
            s.fill(x, y + 19, 25, 1, SHADOW);
        }
        s.fill(26, y, 100, 20, bg);
        s.centered(26, y, 100, 14, "PLAYLIST", if active { TEXT } else { SUBDUED });
        s.fill(26, y + 19, 100, 1, SHADOW);
        s.fill(153 + 9, y + 2, 16, 11, bg);
        title_button(&mut s, 153 + 11, y + 3, false, ICON_CLOSE);
    }
    // Side tiles; the right one holds the scroll groove.
    s.fill(0, 42, 12, 29, PANEL);
    s.fill(11, 42, 1, 29, SHADOW);
    s.fill(31, 42, 20, 29, PANEL);
    s.fill(31, 42, 1, 29, LIGHT);
    s.well(31 + 4, 42, 10, 29);
    for (x, pressed) in [(52, false), (61, true)] {
        s.button(x, 53, 8, 18, pressed);
        s.fill(x + 2, 61, 4, 1, if pressed { ACCENT2 } else { SUBDUED });
    }
    // Bottom left: ADD REM SEL MISC.
    s.fill(0, 72, 125, 38, PANEL);
    s.fill(0, 72, 125, 1, LIGHT);
    for (i, label) in ["ADD", "REM", "SEL", "MISC"].iter().enumerate() {
        let x = 11 + i as u32 * 29;
        s.button(x, 72 + 10, 25, 18, false);
        s.centered(x, 72 + 10, 25, 18, label, SUBDUED);
    }
    // Bottom right: time readouts, mini transport, LIST, resize grip.
    s.fill(126, 72, 150, 38, PANEL);
    s.fill(126, 72, 150, 1, LIGHT);
    s.well(126 + 4, 72 + 8, 94, 10);
    s.well(126 + 63, 72 + 20, 31, 10);
    let icons: [&[&str]; 6] = [
        &["#..#", "#.##", "####", "#.##", "#..#"],
        &["#...", "##..", "###.", "##..", "#..."],
        &["##.#", "##.#", "##.#", "##.#", "##.#"],
        &["####", "####", "####", "####", "####"],
        &["#..#", "##.#", "####", "##.#", "#..#"],
        &[".##.", "####", "....", "####", "...."],
    ];
    for (i, icon) in icons.iter().enumerate() {
        s.art(126 + 6 + i as u32 * 9, 72 + 23, icon, SUBDUED);
    }
    // LIST (wider than the button in this font): a list icon instead.
    s.button(126 + 104, 72 + 10, 22, 18, false);
    s.art(126 + 104 + 6, 72 + 10 + 5, &["#.#####", ".......", "#.#####", ".......", "#.#####"], SUBDUED);
    for i in 0..4u32 {
        s.fill(126 + 146 - i * 3, 72 + 34 - i, 1 + i * 3, 1, MUTED);
    }
    // Bottom tile.
    s.fill(179, 0, 25, 38, PANEL);
    s.fill(179, 0, 25, 1, LIGHT);
    s.save("pledit");
    std::fs::write(
        "assets/skin/pledit.txt",
        "[Text]\r\nNormal=#A3A8C3\r\nCurrent=#22D3EE\r\nNormalBG=#08081A\r\nSelectedBG=#2C2C5E\r\nFont=Poppins\r\n",
    )
    .expect("write pledit.txt");
}

fn main() {
    std::fs::create_dir_all("assets/skin").expect("create assets/skin");
    main_bmp();
    cbuttons();
    titlebar();
    posbar();
    level_slider("volume", 68, 0, 68, false);
    level_slider("balance", 47, 9, 38, true);
    shufrep();
    numbers();
    text_bmp();
    playpaus();
    monoster();
    eqmain();
    pledit();
}
