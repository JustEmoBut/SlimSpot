//! "Start with the system" on Linux: an XDG autostart entry that launches the current binary into
//! the tray. The file is the source of truth, so the setting reflects what the desktop will do.

use std::path::PathBuf;

/// Command-line flag: start hidden in the tray.
pub const TRAY_ARG: &str = "--tray";

/// `$XDG_CONFIG_HOME/autostart/slimspot.desktop` (`~/.config` by default).
fn entry_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(config.join("autostart").join("slimspot.desktop"))
}

pub fn is_enabled() -> bool {
    entry_path().is_some_and(|p| p.exists())
}

pub fn set(enabled: bool) -> Result<(), String> {
    let path = entry_path().ok_or("No home directory for the autostart entry")?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Changing autostart failed: {e}")),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| format!("Can't find own binary: {e}"))?;
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=SlimSpot\nExec=\"{}\" {TRAY_ARG}\nIcon=slimspot\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Changing autostart failed: {e}"))?;
    }
    std::fs::write(&path, entry).map_err(|e| format!("Changing autostart failed: {e}"))
}
