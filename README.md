# SlimSpot

A minimal-RAM native Spotify client for Windows, written in Rust with [Slint](https://slint.dev) (software renderer) and [librespot](https://github.com/librespot-org/librespot). No browser engine, no GPU renderer: around 15 MB of private memory at startup and about 23 MB while playing.

> **Unofficial.** SlimSpot is not affiliated with or endorsed by Spotify. Playback goes through librespot, which signs in like Spotify's own desktop client; this is against Spotify's terms of use and your account could be restricted. Use it at your own risk. Playback needs **Spotify Premium**.

## Features

- Spotify Connect device ("SlimSpot"): control it from your phone, or move playback to other devices
- Library: playlists, Liked Songs, Your Episodes, saved albums, followed artists and podcasts, with filters, Recents/A-Z sort and name search
- Search with tabs (Songs, Albums, Artists, Playlists) and paging
- Artist pages (Popular, Albums, Singles, Compilations, Appears On, About), album and podcast pages
- Playlist editing: create, rename, description, cover upload, add songs, drag to reorder, remove
- Radio from a song, playlist, album or artist, saved as a playlist if you like it
- Queue, synced lyrics, track credits, find in page, sleep timer, mini player
- Themes (Indigo, Nord, Tokyo Night, Catppuccin Mocha, Rosé Pine, Dark)
- Windows integration: media keys and overlay, taskbar buttons and progress, tray, start with Windows

## Install

Requirements: Windows 10/11, a Spotify Premium account.

**Download:** get the zip from [Releases](https://github.com/JustEmoBut/SlimSpot/releases), unzip it and run `SlimSpot.exe`. The exe isn't code-signed, so SmartScreen may warn ("More info" → "Run anyway").

**From source:** needs the Rust toolchain; the exe icon needs the Windows SDK (`rc.exe`), without it the build only skips the icon.

```powershell
git clone https://github.com/JustEmoBut/SlimSpot.git
cd SlimSpot
.\install.ps1
```

`install.ps1` builds SlimSpot, copies it to `%LOCALAPPDATA%\Programs\SlimSpot`, adds a Start menu shortcut and turns on start with Windows (switch that off in Settings). `.\install.ps1 -Uninstall` removes it.

On the first start the browser opens twice: once to sign in for playback, once for the Web API (library and search).

## Your own Spotify app (optional)

Library, search and editing use the Spotify Web API. By default SlimSpot uses the public app shared with ncspot, spotify-player and Spotifast. If that app is busy or blocked, use your own:

1. Create an app at <https://developer.spotify.com/dashboard> with the redirect URI `http://127.0.0.1:8989/login` and the Web API enabled.
2. Copy its Client ID into **Settings → Spotify app** and press Save (or set the `SLIMSPOT_WEB_CLIENT_ID` environment variable).
3. Restart SlimSpot; the browser asks for permission once.

Apps in Spotify's Development Mode only accept the users added to them in the dashboard.

## Files

Everything lives in `%APPDATA%\SlimSpot`: sign-in tokens, `settings.json`, the audio cache and `slimspot.log` (warnings and errors only).

## License

MIT, see [LICENSE](LICENSE). The bundled Poppins font is under the SIL Open Font License (`assets/fonts/OFL.txt`).

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

Slint is used under its Royalty-free License 2.0, which asks for this attribution.
