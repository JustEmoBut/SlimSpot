//! Media keys and the desktop's media controls on Linux: an MPRIS server on the session bus
//! (`org.mpris.MediaPlayer2.slimspot`), through the zbus already in the tree (Freya's `mundy`).

use std::collections::HashMap;
use std::sync::OnceLock;

use tokio::sync::mpsc;
use zbus::interface;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::player::Command;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.slimspot";
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";

/// What the desktop shows; sent from the UI thread to the bus task.
struct NowPlaying {
    uri: String,
    title: String,
    artist: String,
    cover_url: String,
    playing: bool,
}

static UPDATES: OnceLock<mpsc::UnboundedSender<NowPlaying>> = OnceLock::new();

struct Root;

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {}
    fn quit(&self) {}
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "SlimSpot"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "slimspot"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Player {
    tx: mpsc::UnboundedSender<Command>,
    now: NowPlaying,
}

impl Player {
    /// Play and Pause only toggle when they change something; the backend knows one toggle.
    fn toggle_if(&self, playing: bool) {
        if self.now.playing == playing {
            let _ = self.tx.send(Command::Toggle);
        }
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn next(&self) {
        let _ = self.tx.send(Command::Next);
    }
    fn previous(&self) {
        let _ = self.tx.send(Command::Prev);
    }
    fn play_pause(&self) {
        let _ = self.tx.send(Command::Toggle);
    }
    fn play(&self) {
        self.toggle_if(false);
    }
    fn pause(&self) {
        self.toggle_if(true);
    }
    fn stop(&self) {
        self.toggle_if(true);
    }
    #[zbus(property)]
    fn playback_status(&self) -> &str {
        if self.now.playing { "Playing" } else { "Paused" }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut map = HashMap::new();
        if self.now.uri.is_empty() {
            return map;
        }
        let mut put = |key: &str, value: Value| {
            if let Ok(v) = OwnedValue::try_from(value) {
                map.insert(key.to_string(), v);
            }
        };
        // A D-Bus object path allows only [A-Za-z0-9_]; Spotify ids are base62.
        let id: String = self.now.uri.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        if let Ok(path) = ObjectPath::try_from(format!("/org/slimspot/track/{id}")) {
            put("mpris:trackid", Value::from(path));
        }
        put("xesam:title", Value::from(self.now.title.clone()));
        put("xesam:artist", Value::from(vec![self.now.artist.clone()]));
        if !self.now.cover_url.is_empty() {
            put("mpris:artUrl", Value::from(self.now.cover_url.clone()));
        }
        map
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
}

async fn serve(tx: mpsc::UnboundedSender<Command>, mut updates: mpsc::UnboundedReceiver<NowPlaying>) -> zbus::Result<()> {
    let empty = NowPlaying { uri: String::new(), title: String::new(), artist: String::new(), cover_url: String::new(), playing: false };
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, Root)?
        .serve_at(OBJECT_PATH, Player { tx, now: empty })?
        .build()
        .await?;
    let player = connection.object_server().interface::<_, Player>(OBJECT_PATH).await?;
    while let Some(now) = updates.recv().await {
        let mut iface = player.get_mut().await;
        iface.now = now;
        iface.playback_status_changed(player.signal_emitter()).await?;
        iface.metadata_changed(player.signal_emitter()).await?;
    }
    Ok(())
}

/// Starts the MPRIS server on the tokio runtime the UI thread has entered. Without a session bus
/// (some minimal setups) media keys are just unavailable.
pub fn setup(tx: mpsc::UnboundedSender<Command>) -> Result<(), String> {
    let (updates_tx, updates_rx) = mpsc::unbounded_channel();
    UPDATES.set(updates_tx).map_err(|_| "Media keys already set up".to_string())?;
    tokio::spawn(async move {
        if let Err(e) = serve(tx, updates_rx).await {
            log::warn!("Media keys unavailable (MPRIS): {e}");
        }
    });
    Ok(())
}

/// Shows the playing track in the desktop's media controls. Best-effort; playback doesn't depend on it.
pub fn now_playing(uri: &str, title: &str, artist: &str, cover_url: &str, playing: bool) {
    if let Some(tx) = UPDATES.get() {
        let _ = tx.send(NowPlaying { uri: uri.into(), title: title.into(), artist: artist.into(), cover_url: cover_url.into(), playing });
    }
}
