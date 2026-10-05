//! Winamp classic skins (.wsz): a zip of BMP sprite sheets plus pledit.txt colors.

use std::collections::HashMap;

use crate::ui::{Glyph, WaSkin};

// Zip record signatures and the largest end-of-central-directory search window (22 + 65535 comment).
const EOCD_SIG: u32 = 0x0605_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
const EOCD_SEARCH: usize = 22 + u16::MAX as usize;
/// Skins are a few hundred KB; anything bigger inside one isn't a sprite sheet.
const MAX_SKIN_ENTRY: usize = 8 << 20;
const DEFLATE: u16 = 8;

/// SlimSpot's own skin (examples/default_skin.rs), under the classic file names. It is shown when
/// no .wsz is chosen and fills in any sheet a chosen skin lacks.
macro_rules! builtin {
    ($($name:literal),*) => { &[$((concat!($name, ".bmp"), include_bytes!(concat!("../assets/skin/", $name, ".png")))),*] };
}
const BUILTIN: &[(&str, &[u8])] = builtin!(
    "main", "cbuttons", "titlebar", "posbar", "volume", "balance", "shufrep", "numbers", "text", "playpaus", "monoster",
    "eqmain", "pledit"
);
const BUILTIN_PLEDIT: &[u8] = include_bytes!("../assets/skin/pledit.txt");

/// text.bmp cells (Winamp's 5x6 font).
pub const CHAR_W: i32 = 5;
pub const CHAR_H: i32 = 6;

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Entries by lowercased file name (folders dropped: some skins nest everything one level down).
/// Also reads the update zip (update.rs); entries over `max_entry` bytes are skipped.
pub fn unzip(zip: &[u8], max_entry: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    let bad = || "Not a zip file (a .wsz skin is one)".to_string();
    let start = zip.len().saturating_sub(EOCD_SEARCH);
    let eocd = (start..zip.len().saturating_sub(21)).rev().find(|&i| u32_at(zip, i) == Some(EOCD_SIG)).ok_or_else(bad)?;
    let count = u16_at(zip, eocd + 10).ok_or_else(bad)? as usize;
    let mut at = u32_at(zip, eocd + 16).ok_or_else(bad)? as usize;
    let mut files = HashMap::new();
    for _ in 0..count {
        if u32_at(zip, at) != Some(CENTRAL_SIG) {
            return Err(bad());
        }
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let size = u32_at(zip, at + 20).ok_or_else(bad)? as usize;
        let (name_len, extra, comment) = (u16_at(zip, at + 28), u16_at(zip, at + 30), u16_at(zip, at + 32));
        let (name_len, extra, comment) = (name_len.ok_or_else(bad)? as usize, extra.ok_or_else(bad)? as usize, comment.ok_or_else(bad)? as usize);
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?).to_lowercase();
        at += 46 + name_len + extra + comment;
        let base = name.rsplit(['/', '\\']).next().unwrap_or_default().to_string();
        if base.is_empty() || u32_at(zip, local) != Some(LOCAL_SIG) {
            continue;
        }
        let data_at = local + 30 + u16_at(zip, local + 26).ok_or_else(bad)? as usize + u16_at(zip, local + 28).ok_or_else(bad)? as usize;
        let raw = zip.get(data_at..data_at + size).ok_or_else(bad)?;
        let data = match method {
            0 => raw.to_vec(),
            DEFLATE => match miniz_oxide::inflate::decompress_to_vec_with_limit(raw, max_entry) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("skin entry {name}: inflate failed: {e:?}");
                    continue;
                }
            },
            other => {
                log::warn!("skin entry {name}: unsupported compression {other}");
                continue;
            }
        };
        files.insert(base, data);
    }
    Ok(files)
}

fn image(files: &HashMap<String, Vec<u8>>, name: &str) -> Option<slint::Image> {
    // Built-in sheets are PNGs under .bmp names; the format is sniffed from the bytes.
    let img = image::load_from_memory(files.get(name)?)
        .inspect_err(|e| log::warn!("skin {name}: {e}"))
        .ok()?
        .to_rgba8();
    Some(slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(img.as_raw(), img.width(), img.height())))
}

/// `#RRGGBB` values of pledit.txt's [Text] section, keys lowercased.
fn pledit_colors(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect()
}

fn color(hex: Option<&String>, fallback: u32) -> slint::Color {
    let rgb = hex.and_then(|h| u32::from_str_radix(h.trim_start_matches('#').get(..6)?, 16).ok()).unwrap_or(fallback);
    slint::Color::from_argb_encoded(0xff00_0000 | rgb)
}

/// Loads a .wsz (or, for "", the built-in skin) on the UI thread (`slint::Image` is !Send).
pub fn load(path: &str) -> Result<WaSkin, String> {
    let mut files = match path {
        "" => HashMap::new(),
        _ => unzip(&std::fs::read(path).map_err(|e| format!("Can't read the skin: {e}"))?, MAX_SKIN_ENTRY)?,
    };
    // nums_ex.bmp replaces numbers.bmp in newer skins and carries its own minus sign.
    let nums_ex = files.contains_key("nums_ex.bmp");
    if !path.is_empty() && !files.contains_key("main.bmp") {
        return Err("The skin has no main.bmp".into());
    }
    for (name, data) in BUILTIN {
        files.entry(name.to_string()).or_insert_with(|| data.to_vec());
    }
    files.entry("pledit.txt".into()).or_insert_with(|| BUILTIN_PLEDIT.to_vec());
    let img = |n: &str| image(&files, n).unwrap_or_default();
    let main = image(&files, "main.bmp").ok_or("The skin's main.bmp can't be read")?;
    let colors = files.get("pledit.txt").map(|t| pledit_colors(&String::from_utf8_lossy(t))).unwrap_or_default();
    Ok(WaSkin {
        main,
        cbuttons: img("cbuttons.bmp"),
        titlebar: img("titlebar.bmp"),
        posbar: img("posbar.bmp"),
        volume: img("volume.bmp"),
        balance: img("balance.bmp"),
        shufrep: img("shufrep.bmp"),
        numbers: img(if nums_ex { "nums_ex.bmp" } else { "numbers.bmp" }),
        nums_ex,
        text: img("text.bmp"),
        playpaus: img("playpaus.bmp"),
        monoster: img("monoster.bmp"),
        eqmain: img("eqmain.bmp"),
        pledit: img("pledit.bmp"),
        pl_normal: color(colors.get("normal"), 0x00ff00),
        pl_current: color(colors.get("current"), 0xffffff),
        pl_normal_bg: color(colors.get("normalbg"), 0x000000),
        pl_selected_bg: color(colors.get("selectedbg"), 0x0000c6),
        pl_font: colors.get("font").cloned().unwrap_or_else(|| "Arial".into()).into(),
        loaded: true,
    })
}

/// text.bmp cell of a character: row 0 letters, row 1 digits and punctuation, row 2 a few extras.
fn cell(c: char) -> (i32, i32) {
    let c = match c.to_ascii_uppercase() {
        'Ç' | 'ç' => 'C',
        'Ğ' | 'ğ' => 'G',
        'İ' | 'ı' => 'I',
        'Ş' | 'ş' => 'S',
        'Ü' | 'ü' => 'U',
        'ö' => 'Ö',
        'ä' => 'Ä',
        'å' => 'Å',
        c => c,
    };
    match c {
        'A'..='Z' => (c as i32 - 'A' as i32, 0),
        '0'..='9' => (c as i32 - '0' as i32, 1),
        '"' => (26, 0),
        '@' => (27, 0),
        '…' => (10, 1),
        '.' => (11, 1),
        ':' => (12, 1),
        '(' | '{' | '<' => (13, 1),
        ')' | '}' | '>' => (14, 1),
        '-' | '–' | '—' | '~' => (15, 1),
        '\'' | '`' | '’' | '‘' => (16, 1),
        '!' => (17, 1),
        '_' => (18, 1),
        '+' => (19, 1),
        '\\' => (20, 1),
        '/' => (21, 1),
        '[' => (22, 1),
        ']' => (23, 1),
        '^' => (24, 1),
        '&' => (25, 1),
        '%' => (26, 1),
        ',' => (27, 1),
        '=' => (28, 1),
        '$' => (29, 1),
        '#' => (30, 1),
        'Å' => (0, 2),
        'Ö' => (1, 2),
        'Ä' => (2, 2),
        '?' => (3, 2),
        '*' => (4, 2),
        _ => (30, 0), // space
    }
}

/// Pixel offsets into text.bmp, one per character.
pub fn glyphs(text: &str) -> Vec<Glyph> {
    text.chars().map(cell).map(|(col, row)| Glyph { x: col * CHAR_W, y: row * CHAR_H }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored (method 0) zip with one entry, as written by any zip tool.
    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let mut z = Vec::new();
        let local = [&LOCAL_SIG.to_le_bytes()[..], &[0; 22], &(name.len() as u16).to_le_bytes(), &[0, 0], name.as_bytes(), data].concat();
        z.extend_from_slice(&local);
        let central_at = z.len() as u32;
        let size = (data.len() as u32).to_le_bytes();
        let central = [&CENTRAL_SIG.to_le_bytes()[..], &[0; 16], &size, &size, &(name.len() as u16).to_le_bytes(), &[0; 12], &0u32.to_le_bytes(), name.as_bytes()].concat();
        z.extend_from_slice(&central);
        let eocd = [&EOCD_SIG.to_le_bytes()[..], &[0; 6], &1u16.to_le_bytes(), &(central.len() as u32).to_le_bytes(), &central_at.to_le_bytes(), &[0, 0]].concat();
        z.extend_from_slice(&eocd);
        z
    }

    #[test]
    fn unzips_nested_names_case_insensitively() {
        let files = unzip(&stored_zip("Skin/PLEDIT.TXT", b"[Text]\r\nNormal=#00FF00\r\n"), MAX_SKIN_ENTRY).unwrap();
        let colors = pledit_colors(&String::from_utf8_lossy(&files["pledit.txt"]));
        assert_eq!(colors["normal"], "#00FF00");
        assert_eq!(color(colors.get("normal"), 0), slint::Color::from_rgb_u8(0, 255, 0));
        assert!(unzip(b"not a zip", MAX_SKIN_ENTRY).is_err());
    }

    #[test]
    fn maps_text_font_cells() {
        let g = glyphs("Az9:ş?");
        let at: Vec<(i32, i32)> = g.iter().map(|g| (g.x, g.y)).collect();
        assert_eq!(at, vec![(0, 0), (125, 0), (45, 6), (60, 6), (90, 0), (15, 12)]);
    }
}
