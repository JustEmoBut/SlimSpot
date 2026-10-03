# SlimSpot

Personal-use, minimum-RAM native Spotify client for Windows. Rust + Slint (software renderer) + librespot.
Spotify ToS concerns are accepted by the owner; this is not for distribution.

## Goal & how it's judged
- RAM is the deciding metric. Measure **Private bytes** of the release build while a track plays, sampled 3× over 10 s
  (`Get-Process slimspot`), after opening a playlist and scrolling. Working Set includes shared DLLs and is secondary.
- Reference points: ~15–20 MB private while playing with covers. Spotifast (egui) claims 100–250 MB; a Tauri app measured 194 MB.
- Any new feature reports its RAM delta. No GPU renderer, no webview, no browser engine.

## Commands
- Test: `cargo test` (unit tests live next to the code they test)
- Release build: `cargo build --release` → `target/release/slimspot.exe`
- Install/update for daily use: `.\install.ps1` (build, copy to `%LOCALAPPDATA%\Programs\SlimSpot\SlimSpot.exe`, Start menu shortcut, autostart on first install). `-NoAutostart`, `-Uninstall`. It stops a running SlimSpot first.
- Runtime files: `%APPDATA%\SlimSpot\` — `credentials.json` (librespot), `web_refresh_token`, `settings.json`, `slimspot.log` (warn/error only), `audio/` cache
- Web API client id: env `SLIMSPOT_WEB_CLIENT_ID` (owner's own dev app; redirect `http://127.0.0.1:8989/login`). Without it, the client id the saved token belongs to is reused.

## Architecture (`src/`)
| Module | Responsibility |
|---|---|
| `main.rs` | Wires Slint callbacks to `player::Command`, tray, UI timers (seek interpolation, minimize-restore repaint, media-key setup retry) |
| `ui.rs` | `slint::slint!` markup + helpers that push state to the UI thread |
| `player.rs` | Backend tokio thread: login, Spotify Connect device (Spirc), command/event loop, reconnect |
| `web.rs` | Spotify Web API (separate OAuth grant): search, playlists, Liked Songs, albums, artist albums, saved albums, followed artists, like/unlike, devices, transfer |
| `covers.rs` | Thumbnails for instantiated rows only, 64 px, max 300 decoded |
| `settings.rs` | `settings.json`: volume, shuffle, repeat, quality, normalize, last session |
| `media_keys.rs` | Windows SystemMediaTransportControls (windows-only) |
| `logger.rs` | File logger for warn/error, including librespot's |
| `instance.rs` | Single instance via a named event; a second launch wakes the running window (windows-only) |
| `lyrics.rs` | Lyrics via librespot `spclient().get_lyrics` (Spotify's color-lyrics, no Web API), parsing and current-line lookup |
| `autostart.rs` | "Start with Windows": HKCU `Run` value `"<exe>" --tray` (windows-only) |
| `build.rs` | Embeds `assets/icon.ico` into the exe via the Windows SDK's `rc.exe` (skipped with a warning if missing) |

## Confirmed decisions
- **Spirc owns playback state** (queue, shuffle, repeat, track advance). Local actions become Spirc calls; there is no local queue. Playlists/albums/artists/Liked Songs play as Spotify contexts; search results as a track list.
- **Two OAuth grants**: librespot playback (Spotify desktop client id, port 8898) and Web API (own app, port 8989). The playback token is rejected/rate-limited on api.spotify.com.
- **Quality/normalisation are per-Player**: changing them restarts the Connect device and resumes at the same position.
- **Volume**: slider drags set the local mixer only; Spirc (and thus Spotify) gets one update on release. Per-step Spirc updates hit 429 and stalled the loop.
- Spotify Connect device name: `SlimSpot`, type Computer.
- **Close hides to the tray**, playback continues; real quit is tray "Quit" or `Ctrl+Q` (saves the session, shuts Spirc down so no stale device lingers). The event loop runs via `run_event_loop_until_quit`, not `app.run()`.
- **Last session is shown paused and loaded on the first Play**, never at startup: loading would make SlimSpot the active device and pause the phone. Saved on track change, pause and quit (`settings.json` → `last`).
- **Icons**: `assets/icon.svg` is the source; `icon-64.png`/`icon-256.png` are rendered from it (headless Edge, 64 downscaled from 256) and embedded with `include_bytes!`, so no runtime SVG renderer.
- **Keyboard**: shortcuts live in one root `FocusScope`; Esc returns focus to it from the search box.
- **Lyrics**: fetched per track change, not cached on disk; shown in place of the track list (`Ctrl+Y`). Fixed line height + elide so the sung line is centered arithmetically; highlighted on the 500 ms position tick.
- **Radio**: `spclient().get_context("spotify:station:track:<id>")` is resolved once and its 50 tracks are shown and played as a track list. Spotify reshuffles a station on every resolve, so loading the station context into Spirc would play a different order than the list shown.
- **Track metadata** (radio, artist popular tracks) comes from librespot `Track::get`, fetched concurrently in `player::track_items`. Radio raised private RAM to ~25 MB (vs ~17 MB) on 2026-10-03, cause unconfirmed; batching the requests is the noted fix if it matters.
- **Design**: Spotify-like dark UI (familiarity for the owner). Colors live in the Slint `Theme` global, icons in `Icons` as 24×24 SVG path strings rendered with `Path` (no icon font/assets). Settings sit in a popup behind the top-right icon. Spotify's logo and icon artwork are not copied.
- **Rounded covers are baked into pixels** (`covers::corner_coverage`): the software renderer ignores `border-radius` when clipping images. Artists are round, other covers get small rounded corners.
- **Exe icon without a build-dependency crate**: `build.rs` calls `rc.exe` directly. `assets/icon.ico` holds PNG-compressed 16–256 px entries generated from `icon-256.png`.
- **Autostart**: the registry `Run` value is the source of truth (the checkbox reads it back after every change). `install.ps1` turns it on only for a first install, so turning it off in the app survives updates.
- **`--tray` start**: the window is shown once so its HWND exists for the media keys, then hidden as soon as they attach (Slint creates the native window only on first show).

## Known external constraints (verified against the live API, 2026-10-03)
- Development Mode Web API: `search` and `/artists/{id}/albums` reject `limit > 10`; playlists/saved tracks accept 50.
- `/artists/{id}/top-tracks` returns 403 → top tracks come from librespot metadata (`Artist::get` + `Track::get`).
- `/me/tracks/contains` returns 403 → like state and like/unlike use the unified `/me/library` (`contains`, `PUT`, `DELETE`) with `uris=`. `/me/albums`, `/me/following?type=artist` (cursor paging under `artists`) and `/me/player/devices` accept limit 50.
- Changing `WEB_SCOPES` changes the saved grant key, so the next start asks for a browser consent once.
- Spotify-owned/editorial playlists can return 403/404 to third-party apps.
- Spotify doesn't always rotate refresh tokens; librespot-oauth then returns `""` — keep the previous token (`web::refresh`).

## Dependency pins
- `Cargo.lock` pins `vergen` 9.0.6: librespot-core 0.8.0's build script fails with 9.1.0. Don't blanket `cargo update`.
- `librespot`/`librespot-oauth` =0.8.0, `slint` =1.18.1, `windows` =0.62.2 (already pulled in by cpal/winit; only features added).
- Prefer crates already in `Cargo.lock`; check with `cargo tree -d` that an addition doesn't add a second version.
