//! Player preferences that survive restarts: volume, shuffle, repeat, quality, normalisation,
//! and the last track (so a restart can pick up where it stopped).

use std::path::{Path, PathBuf};

use librespot::playback::config::Bitrate;

use crate::web::Item;

const SETTINGS_FILE: &str = "settings.json";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    /// Off -> All -> One -> Off, like Spotify's repeat button.
    pub fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }

    /// Matches the `repeat` int property in the Slint markup.
    pub fn as_ui(self) -> i32 {
        self as i32
    }

    fn from_ui(v: i64) -> Self {
        match v {
            1 => Repeat::All,
            2 => Repeat::One,
            _ => Repeat::Off,
        }
    }
}

/// Streaming bitrate. librespot defaults to 160 kbps; Premium allows 320, so that's ours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Quality {
    Low,
    Normal,
    #[default]
    High,
}

impl Quality {
    pub fn bitrate(self) -> Bitrate {
        match self {
            Quality::Low => Bitrate::Bitrate96,
            Quality::Normal => Bitrate::Bitrate160,
            Quality::High => Bitrate::Bitrate320,
        }
    }

    /// Index into the quality ComboBox in the Slint markup.
    pub fn as_ui(self) -> i32 {
        self as i32
    }

    pub fn from_ui(v: i64) -> Self {
        match v {
            0 => Quality::Low,
            1 => Quality::Normal,
            _ => Quality::High,
        }
    }
}

/// What was playing when the app last saved, shown paused at startup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastSession {
    pub item: Item,
    pub position_ms: u32,
    pub duration_ms: u32,
    /// Playlist/album/artist/collection URI it was played from; `None` for search results or links.
    pub context: Option<String>,
}

impl LastSession {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "uri": self.item.uri,
            "title": self.item.title,
            "artist": self.item.artist,
            "cover_url": self.item.cover_url,
            "position_ms": self.position_ms,
            "duration_ms": self.duration_ms,
            "context": self.context,
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let uri = v["uri"].as_str().filter(|u| u.starts_with("spotify:track:"))?.to_string();
        Some(LastSession {
            item: Item { title: s("title"), artist: s("artist"), uri, cover_url: s("cover_url"), ..Default::default() },
            position_ms: v["position_ms"].as_u64().unwrap_or(0) as u32,
            duration_ms: v["duration_ms"].as_u64().unwrap_or(0) as u32,
            context: v["context"].as_str().map(String::from),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0..=100, as shown on the slider.
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub quality: Quality,
    /// Spotify's loudness normalisation (librespot "auto": album gain in albums, track gain otherwise).
    pub normalize: bool,
    pub last: Option<LastSession>,
    /// Library URI -> when it was last opened (Unix seconds), for the "Recents" sort.
    pub opened: std::collections::HashMap<String, u64>,
    /// Color palette index (`Theme.palette` in the markup).
    pub theme: i32,
    path: PathBuf,
}

impl Settings {
    /// Missing or unreadable file means defaults; a corrupt field falls back individually.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(SETTINGS_FILE);
        let json: serde_json::Value = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Settings {
            volume: json["volume"].as_f64().map_or(100.0, |v| v.clamp(0.0, 100.0) as f32),
            shuffle: json["shuffle"].as_bool().unwrap_or(false),
            repeat: Repeat::from_ui(json["repeat"].as_i64().unwrap_or(0)),
            quality: json["quality"].as_i64().map_or(Quality::default(), Quality::from_ui),
            normalize: json["normalize"].as_bool().unwrap_or(false),
            last: LastSession::from_json(&json["last"]),
            opened: json["opened"]
                .as_object()
                .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_u64()?))).collect())
                .unwrap_or_default(),
            theme: json["theme"].as_i64().map_or(0, |t| t as i32),
            path,
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::json!({
            "volume": self.volume,
            "shuffle": self.shuffle,
            "repeat": self.repeat.as_ui(),
            "quality": self.quality.as_ui(),
            "normalize": self.normalize,
            "last": self.last.as_ref().map(LastSession::to_json),
            "opened": self.opened,
            "theme": self.theme,
        });
        std::fs::write(&self.path, json.to_string()).map_err(|e| format!("Saving settings failed: {e}"))
    }

    /// librespot's mixer takes 0..=u16::MAX.
    pub fn mixer_volume(&self) -> u16 {
        volume_to_mixer(self.volume)
    }
}

pub fn volume_to_mixer(percent: f32) -> u16 {
    (percent.clamp(0.0, 100.0) / 100.0 * u16::MAX as f32).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_and_defaults() {
        let dir = std::env::temp_dir().join(format!("slimspot-settings-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(SETTINGS_FILE));

        let defaults = Settings::load(&dir);
        assert_eq!((defaults.volume, defaults.shuffle, defaults.repeat), (100.0, false, Repeat::Off));
        assert_eq!((defaults.quality, defaults.normalize), (Quality::High, false));

        assert_eq!(defaults.last, None);
        let last = LastSession {
            item: Item { title: "T".into(), artist: "A".into(), uri: "spotify:track:x".into(), cover_url: "c".into(), ..Default::default() },
            position_ms: 61_000,
            duration_ms: 200_000,
            context: Some("spotify:album:y".into()),
        };
        let s = Settings { volume: 42.0, shuffle: true, repeat: Repeat::One, quality: Quality::Low, normalize: true, last: Some(last), ..defaults };
        s.save().unwrap();
        assert_eq!(Settings::load(&dir), s);

        std::fs::write(dir.join(SETTINGS_FILE), r#"{"volume": 900, "repeat": "x"}"#).unwrap();
        let clamped = Settings::load(&dir);
        assert_eq!((clamped.volume, clamped.repeat), (100.0, Repeat::Off));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn repeat_cycles_and_volume_maps() {
        assert_eq!(Repeat::Off.next().next().next(), Repeat::Off);
        assert_eq!(volume_to_mixer(100.0), u16::MAX);
        assert_eq!(volume_to_mixer(0.0), 0);
        assert_eq!(volume_to_mixer(150.0), u16::MAX);
        assert_eq!(Quality::from_ui(Quality::Normal.as_ui() as i64), Quality::Normal);
        assert_eq!(Quality::from_ui(99), Quality::High);
    }
}
