//! Updates from GitHub Releases: one check at startup, install on request.
//!
//! Releases are tagged `vX.Y.Z` with a `SlimSpot-X.Y.Z-windows-x64.zip` asset holding `SlimSpot.exe`
//! (checked on v0.1.0, 2026-10-05). The running exe is renamed to `.old` (Windows allows renaming a
//! running exe, not overwriting it), the new one takes its name, and it is started with
//! `--wait-pid` so it waits for this process to quit before taking the single-instance guard.

use std::path::{Path, PathBuf};

use librespot::core::session::Session;

const LATEST_URL: &str = "https://api.github.com/repos/JustEmoBut/SlimSpot/releases/latest";
const ASSET_SUFFIX: &str = "-windows-x64.zip";
const EXE_NAME: &str = "slimspot.exe";
/// GitHub answers API calls without a User-Agent with 403.
const USER_AGENT: &str = concat!("SlimSpot/", env!("CARGO_PKG_VERSION"));
/// Release downloads redirect once (to objects.githubusercontent.com); a few hops to be safe.
const MAX_REDIRECTS: usize = 5;
/// The exe is ~20 MB; anything far bigger inside the zip isn't it.
const MAX_EXE: usize = 64 << 20;
pub const WAIT_PID_ARG: &str = "--wait-pid";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub zip_url: String,
}

/// `X.Y.Z` (an optional leading `v`) as numbers; anything else is no version.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').splitn(3, '.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// The release, if `latest` (the GitHub API's JSON) is newer than `current` and has the zip.
fn newer(latest: &serde_json::Value, current: &str) -> Option<Release> {
    let tag = latest["tag_name"].as_str()?;
    if parse_version(tag)? <= parse_version(current)? {
        return None;
    }
    let zip_url = latest["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(ASSET_SUFFIX)))?["browser_download_url"]
        .as_str()?
        .to_string();
    Some(Release { version: tag.trim_start_matches('v').to_string(), zip_url })
}

async fn get(session: &Session, url: &str) -> Result<bytes::Bytes, String> {
    let mut url = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let req = http::Request::builder()
            .uri(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/vnd.github+json, application/octet-stream")
            .body(bytes::Bytes::new())
            .map_err(|e| e.to_string())?;
        let resp = session.http_client().request_fut(req).map_err(|e| format!("Update request failed: {e}"))?.await;
        let resp = resp.map_err(|e| format!("Update request failed: {e}"))?;
        let status = resp.status();
        if status.is_redirection() {
            url = resp.headers().get("location").and_then(|l| l.to_str().ok()).ok_or("Update redirect without a location")?.to_string();
            continue;
        }
        let body = http_body_util::BodyExt::collect(resp.into_body()).await.map_err(|e| format!("Update download failed: {e}"))?.to_bytes();
        return match status.as_u16() {
            200..=299 => Ok(body),
            // A private repository (or none) answers 404 to anonymous calls.
            code => Err(format!("Update check: HTTP {code}")),
        };
    }
    Err("Update download: too many redirects".into())
}

/// The newer release, if any. Errors (offline, private repo) are for the log only.
pub async fn check(session: &Session) -> Result<Option<Release>, String> {
    let body = get(session, LATEST_URL).await?;
    let latest: serde_json::Value = serde_json::from_slice(&body).map_err(|e| format!("Update check: {e}"))?;
    Ok(newer(&latest, env!("CARGO_PKG_VERSION")))
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Leftover of the previous update; removed at startup once that process has quit.
pub fn remove_old() {
    if let Ok(exe) = std::env::current_exe() {
        let old = old_path(&exe);
        if old.exists() {
            if let Err(e) = std::fs::remove_file(&old) {
                log::warn!("removing {}: {e}", old.display());
            }
        }
    }
}

/// Downloads the release and swaps the exe in place; the caller then starts it and quits.
pub async fn install(session: &Session, release: &Release) -> Result<PathBuf, String> {
    let zip = get(session, &release.zip_url).await?;
    let files = crate::skin::unzip(&zip, MAX_EXE)?;
    let new_exe = files.get(EXE_NAME).ok_or("The update has no SlimSpot.exe")?;
    let exe = std::env::current_exe().map_err(|e| format!("Can't find the running exe: {e}"))?;
    let staged = exe.with_extension("exe.new");
    std::fs::write(&staged, new_exe).map_err(|e| format!("Can't write the update: {e}"))?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Can't move the running exe aside: {e}"))?;
    if let Err(e) = std::fs::rename(&staged, &exe) {
        // Put the old one back so the app still starts.
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Can't put the update in place: {e}"));
    }
    Ok(exe)
}

/// Starts the installed exe, which waits for this process to quit first.
pub fn relaunch(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .args([WAIT_PID_ARG, &std::process::id().to_string()])
        .spawn()
        .map(drop)
        .map_err(|e| format!("Can't start the update: {e}"))
}

/// `--wait-pid N`: blocks until process N ends (at most `timeout`), so the updated exe doesn't
/// meet the old instance's single-instance guard.
#[cfg(windows)]
pub fn wait_for_previous(timeout: std::time::Duration) {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

    let mut args = std::env::args().skip_while(|a| a != WAIT_PID_ARG).skip(1);
    let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) else { return };
    if let Ok(handle) = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        unsafe {
            WaitForSingleObject(handle, timeout.as_millis() as u32);
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "assets": [
                { "name": "notes.txt", "browser_download_url": "x" },
                { "name": format!("SlimSpot-{}-windows-x64.zip", tag.trim_start_matches('v')), "browser_download_url": "https://example/z.zip" },
            ]
        })
    }

    #[test]
    fn offers_only_newer_releases_with_the_zip() {
        assert_eq!(newer(&release("v0.2.0"), "0.1.0"), Some(Release { version: "0.2.0".into(), zip_url: "https://example/z.zip".into() }));
        assert_eq!(newer(&release("v0.10.0"), "0.9.3").map(|r| r.version), Some("0.10.0".into()));
        assert_eq!(newer(&release("v0.1.0"), "0.1.0"), None);
        assert_eq!(newer(&release("v0.0.9"), "0.1.0"), None);
        assert_eq!(newer(&release("nightly"), "0.1.0"), None);
        assert_eq!(newer(&serde_json::json!({ "tag_name": "v9.0.0", "assets": [] }), "0.1.0"), None);
    }
}
