<p align="center">
  <img src="assets/icon-256.png" width="128" height="128" alt="SlimSpot logo">
</p>

<h1 align="center">SlimSpot</h1>

<p align="center">
  <b>A tiny, native Spotify client for Windows.</b><br>
  Rust · Slint (software renderer) · librespot — no browser engine, no webview.
</p>

<p align="center">
  <a href="https://github.com/JustEmoBut/SlimSpot/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/JustEmoBut/SlimSpot"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-green"></a>
  <img alt="Platform: Windows" src="https://img.shields.io/badge/platform-Windows%2010%2F11-blue">
</p>

## Why SlimSpot?

Memory is the whole point. SlimSpot uses about **19 MB of private memory at startup** and stays well under **50 MB while playing**, even with a large playlist open. For comparison, Electron/Tauri/egui clients typically use 100–250 MB.

| Client | Private memory |
|---|---|
| **SlimSpot** (idle / playing) | ~19 MB / ~25 MB |
| Spotifast (egui, claimed) | 100–250 MB |
| A Tauri-based client (measured) | ~194 MB |

> **Unofficial.** SlimSpot is not affiliated with or endorsed by Spotify. Playback goes through librespot, which signs in like Spotify's own desktop client; this is against Spotify's terms of use and your account could be restricted. Use it at your own risk. Playback needs **Spotify Premium**.

## Features

- Spotify Connect device ("SlimSpot"): control it from your phone, or move playback to other devices
- Library: playlists, Liked Songs, Your Episodes, saved albums, followed artists and podcasts, with filters, Recents/A-Z sort and name search
- Search with tabs (Songs, Albums, Artists, Playlists) and paging
- Artist pages (Popular, Albums, Singles, Compilations, Appears On, About), album and podcast pages
- Playlist editing: create, rename, description, cover upload, add songs, drag to reorder, remove
- Radio from a song, playlist, album or artist, saved as a playlist if you like it
- Queue and recently played in the right panel, synced lyrics (click a line to jump, full screen), track credits, find in page, sleep timer, mini player
- Select several songs (Ctrl/Shift-click, Ctrl+A), copy and paste song links, drag songs onto a playlist or Liked Songs in the sidebar
- Keyboard shortcuts for nearly everything; press `?` for the list
- Themes (Spotifast, Indigo, Nord, Tokyo Night, Catppuccin Mocha, Rosé Pine, Dark), compact track list, searchable settings
- 10-band equalizer and preamp that really shape the sound (Settings → Playback)
- Optional GPU rendering (Settings → Display; uses far more memory, ~100 MB)
- Updates itself from GitHub Releases: an "Update to vX" button appears when a new version is out
- Windows integration: media keys and overlay, taskbar buttons and progress, tray, start with Windows

## Install

Requirements: Windows 10/11, a Spotify Premium account.

**Download:** get the zip from [Releases](https://github.com/JustEmoBut/SlimSpot/releases), unzip it and run `SlimSpot.exe`. The exe isn't code-signed, so SmartScreen may warn ("More info" → "Run anyway").

**From source:** see [Building from source](#building-from-source) below, or just run:

```powershell
git clone https://github.com/JustEmoBut/SlimSpot.git
cd SlimSpot
.\install.ps1
```

`install.ps1` builds SlimSpot, copies it to `%LOCALAPPDATA%\Programs\SlimSpot`, adds a Start menu shortcut and turns on start with Windows (switch that off in Settings). Options: `-Release` (fully optimised LTO build), `-NoAutostart`, `-Uninstall`. A running SlimSpot is stopped first, so it also works for updating.

On the first start the browser opens twice: once to sign in for playback, once for the Web API (library and search).

## Keyboard shortcuts

Press `?` in the app for the full list. The most used ones:

| Keys | Action |
|---|---|
| `Space` | Play / pause |
| `Shift` + `←` / `→` | Seek 10 s |
| `M` | Mute / unmute |
| `B` | Like the current song |
| `Q` / `Ctrl+U` | Queue |
| `/` or `Ctrl+L` | Search |
| `Ctrl+F` | Find in page |
| `Ctrl+Y` | Lyrics |
| `Ctrl+H` | Home |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+,` | Settings |
| `↑` / `↓`, `Enter` | Move through the list, play the row |
| `Ctrl+C` / `Ctrl+V` | Copy song links / paste them into your playlist |
| `Ctrl+Q` | Quit (closing the window only hides it to the tray) |

## Building from source

### Requirements

- **Windows 10/11** (x64)
- **Rust** stable, 1.85 or newer (edition 2024) — install with [rustup](https://rustup.rs) and the MSVC toolchain (`x86_64-pc-windows-msvc`)
- **Visual Studio Build Tools** with "Desktop development with C++" (MSVC linker and Windows SDK)
- Optional: the Windows SDK's `rc.exe` on `PATH` (or found by the build script) to embed the exe icon; without it the build prints a warning and skips the icon

### Build

```powershell
git clone https://github.com/JustEmoBut/SlimSpot.git
cd SlimSpot

# Day-to-day build (fast to rebuild, same memory use as release)
cargo build --profile fast        # -> target\fast\slimspot.exe

# Fully optimised build (LTO, ~4 min, slightly smaller exe)
cargo build --release             # -> target\release\slimspot.exe

# Run the tests
cargo test
```

The first build downloads and compiles all dependencies and takes a few minutes; later `fast` builds take ~25–40 s.

### Notes for forks

- `fast` and `release` builds embed the update-signing public key from `assets/update-public-key.pem` and refuse to compile without one. If you publish your own releases, generate your own key pair:
  ```sh
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out slimspot-update.pem   # keep private, never commit
  openssl pkey -in slimspot-update.pem -pubout -out assets/update-public-key.pem
  ```
  and point the update check in `src/update.rs` at your repository.
- `Cargo.lock` pins `vergen` 9.0.6 because librespot 0.8.0's build script fails with newer versions; avoid a blanket `cargo update`.

### Making a release

1. Bump `version` in `Cargo.toml` (the in-app update check compares against it).
2. `.\install.ps1 -Release` (or `cargo build --release`).
3. Zip `SlimSpot.exe`, `LICENSE`, `OFL.txt` and `README.md` as `SlimSpot-X.Y.Z-windows-x64.zip`.
4. Sign it: `openssl dgst -sha256 -sign slimspot-update.pem -out SlimSpot-X.Y.Z-windows-x64.zip.sig SlimSpot-X.Y.Z-windows-x64.zip`
5. Publish both files on a `vX.Y.Z` GitHub release. Running copies then show "Update to vX.Y.Z"; a release without a valid `.sig` is never offered.

## Project layout

| Path | What it does |
|---|---|
| `src/main.rs` | Wires the UI to the player, tray, timers |
| `src/ui.rs` | Slint markup and UI helpers |
| `src/player.rs` | Playback backend: login, Spotify Connect device, command loop |
| `src/web.rs` | Spotify Web API: library, search, playlists, queue |
| `src/covers.rs` | Cover thumbnails (only for visible rows) |
| `src/eq.rs` | 10-band equalizer |
| `src/update.rs` | Signed self-update from GitHub Releases |
| `src/lyrics.rs`, `src/nav.rs`, `src/settings.rs`, … | Lyrics, back/forward history, settings and Windows integration |
| `assets/` | Icon (`icon.svg` is the source), fonts, update public key |

## Your own Spotify app (optional)

Library, search and editing use the Spotify Web API. By default SlimSpot uses the public app shared with ncspot, spotify-player and Spotifast. If that app is busy or blocked, use your own:

1. Create an app at <https://developer.spotify.com/dashboard> with the redirect URI `http://127.0.0.1:8989/login` and the Web API enabled.
2. Copy its Client ID into **Settings → Spotify app** and press Save (or set the `SLIMSPOT_WEB_CLIENT_ID` environment variable).
3. Restart SlimSpot; the browser asks for permission once.

Apps in Spotify's Development Mode only accept the users added to them in the dashboard. If you delete your app later, SlimSpot notices at the next start, removes its Client ID from Settings and goes back to the shared app (the environment variable, if you used it, has to be removed by hand).

## Files

Everything lives in `%APPDATA%\SlimSpot`: sign-in tokens, `settings.json`, a library cache (`library.json`), the audio cache and `slimspot.log` (warnings and errors only).

## License

MIT, see [LICENSE](LICENSE). The bundled Poppins font is under the SIL Open Font License (`assets/fonts/OFL.txt`).

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

Slint is used under its Royalty-free License 2.0, which asks for this attribution.
