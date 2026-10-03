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
| `web.rs` | Spotify Web API (separate OAuth grant): search (+ paged tracks), queue (view, add), recently played, liked checks, playlists (create, rename, remove, add/remove tracks), Liked Songs, albums, artist albums, saved albums, followed artists, like/unlike, devices, transfer |
| `covers.rs` | Thumbnails for instantiated rows only, 64 px, max 300 decoded; 300 px for the now-playing panel and page header (one each) |
| `settings.rs` | `settings.json`: volume, shuffle, repeat, quality, normalize, last session |
| `media_keys.rs` | Windows SystemMediaTransportControls (windows-only) |
| `logger.rs` | File logger for warn/error, including librespot's |
| `instance.rs` | Single instance via a named event; a second launch wakes the running window (windows-only) |
| `lyrics.rs` | Lyrics via librespot `spclient().get_lyrics` (Spotify's color-lyrics, no Web API), parsing and current-line lookup |
| `nav.rs` | Back/forward history of whole pages (rows included, 20 deep) |
| `clipboard.rs` | Win32 plain-text clipboard and `open.spotify.com` links for "Copy link" (windows-only) |
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
- **Track metadata** (radio, artist popular tracks, Spotify-made playlists) comes from librespot `Track::get` in `player::track_items`, at most 8 lookups in flight. All 50 radio lookups at once left private RAM ~10 MB higher for good (15.9 -> 26 MB); capped, a radio costs ~2.5 MB (2026-10-04).
- **Design**: Spotify-like dark UI (familiarity for the owner). Colors live in the Slint `Theme` global, icons in `Icons` as 24×24 SVG path strings rendered with `Path` (no icon font/assets). Settings sit in a popup behind the top-right icon. Spotify's logo and icon artwork are not copied.
- **Queue view** (`Ctrl+U`, bar icon): Spirc keeps its queue private, so the page comes from Web API `/me/player/queue`. It opens as a track list; clicking a row plays that list, not the original context.
- **Search "Show more tracks"**: a full search page (10 tracks) ends with a `slimspot:more` row; clicking it appends the next `offset` page to the same history entry (`nav::History::current_mut`). Rows with a `slimspot:` URI have no context menu.
- **Playlist editing**: "Add to playlist" offers only the user's own playlists (`web::Item.artist_uri` holds a playlist's owner URI, compared with `spotify:user:<username>`). "Remove from this playlist" shows only on such a page (`Page.editable`) and removes every occurrence of the track.
- **Go to playing** (click the now-playing title): opens the context the track was started from in this window and scrolls the list to it (`reveal`, row height 56 px, clamped to the list end). Tracks started elsewhere have no known context.
- **Page header**: a kind label ("Playlist", "Album", ...; `Page.kind`) above the title. Track rows show a play arrow on hover and equalizer bars while playing, over the cover.
- **Scroll**: opening a page or going back/forward resets the track list to the top (`scroll_to_top`); pages that change in place ("Show more", removing a row) keep their scroll. "Show more" drops tracks already on the page, since later search pages repeat earlier hits.
- **Home** is the recently played tracks (`/me/player/recently-played`, opened at startup and by the Home button). Spotify's own "Made for you" shelf is not reachable: Web API search for "Daily Mix"/"Discover Weekly" only finds other users' copies (Spotifast's approach, verified 2026-10-04). Opening a Spotify-made playlist falls back to librespot `get_context` when the Web API refuses it.
- **Track rows** carry `duration_ms` and `liked`. Liked marks are fetched after a page is on screen (`mark_liked`, 40 URIs per request, first 400 tracks) and set in place (`set_liked_rows`) so covers stay. Rows of playlists/albums/radio/queue are numbered; the header shows "N songs, about X hr" and a green play button that starts the page from its first track. Hovering a row shows "..." which opens the same `ContextMenuArea` via `show()`.
- **Library filters** (All/Playlists/Albums/Artists + name filter) filter the loaded sidebar rows in the backend (`Command::FilterLibrary`); no request.
- **Library**: "+" creates "My Playlist #N" (private) and opens it; the pencil next to an own playlist's title renames it; "Remove from Your Library" in a sidebar row's menu unfollows (Spotify's delete for own playlists). Changes update the sidebar and "Add to playlist" targets without a reload. "A-Z" sorts the sidebar; there is no "All" chip, clicking the chosen filter clears it.
- **Now playing panel** (bar button) shows the playing track's 300 px cover; **page headers** of playlists/albums show the page's cover. Rows keep only the smallest cover URL, so `covers::larger_cover` rewrites it to the 300 px variant (album image ids, mosaic paths).
- **Mini player** (bar button): the panels are collapsed (`visible: false`, `min-height: 0`, `max-height: 0`), not removed, because ids inside them (the search box) are referenced from the root; the window shrinks to 860x96 and stays on top, and the previous size is restored.
- **Sleep timer** (settings): 15/30/60 min pauses via a `sleep_until` branch in the backend loop; "End of track" pauses on the next `TrackChanged`.
- **Queue rows**: clicking one skips ahead with `Spirc::next` (the playing context carries on) and drops the passed rows locally, since `/me/player/queue` lags right after a skip.
- **Status line** holds transient messages and errors; play/pause resets it to the page's item count instead of "Playing".
- **Cover tint**: the alpha-weighted average color of the now-playing cover, dimmed to 45%, colors a gradient at the top of the main panel (`now-tint`).
- **Rounded covers are baked into pixels** (`covers::corner_coverage`): the software renderer ignores `border-radius` when clipping images. Artists are round, other covers get small rounded corners.
- **Pages and navigation**: every main-panel screen (list, album, artist, search, radio) is a `player::Page` pushed onto `nav::History`; back/forward restores it without a request. Album/artist pages take their title from the loaded data, playlists from the clicked row.
- **Row menu** (`ContextMenuArea` in `RowItem`): actions travel as `Command::RowAction` and re-enter the backend queue as the command they stand for (`OpenList`, `Radio(Some(uri))`). `web::Item` carries the first artist and the album URI for this; the now-playing item from librespot has no album id, so "Go to album" is unavailable for tracks started elsewhere.
- **Exe icon without a build-dependency crate**: `build.rs` calls `rc.exe` directly. `assets/icon.ico` holds PNG-compressed 16–256 px entries generated from `icon-256.png`.
- **Autostart**: the registry `Run` value is the source of truth (the checkbox reads it back after every change). `install.ps1` turns it on only for a first install, so turning it off in the app survives updates.
- **`--tray` start**: the window is shown once so its HWND exists for the media keys, then hidden as soon as they attach (Slint creates the native window only on first show).

## Known external constraints (verified against the live API, 2026-10-03)
- Development Mode Web API: `search` and `/artists/{id}/albums` reject `limit > 10`; playlists/saved tracks accept 50. Search `offset` works up to 990.
- `/me/player/queue` works (empty: `{"currently_playing":null,"queue":[]}`). `POST /me/player/queue?uri=` answers 404 with no active device and 200 with a non-JSON body on success, so non-GET 2xx bodies that don't parse count as success.
- `POST /me/playlists` creates, `PUT /playlists/{id}` renames, `DELETE /playlists/{id}/followers` removes from the library (all verified live 2026-10-04).
- `/me/library/contains` rejects more than 40 URIs (400 "Too many uris requested"). Recently played needs `user-read-recently-played`.
- Playlist edits need `playlist-modify-private`/`-public`; `POST /playlists/{id}/items` `{"uris":[..]}` adds, `DELETE` with `{"items":[{"uri":..}]}` removes (both verified live).
- `/artists/{id}/top-tracks` returns 403 → top tracks come from librespot metadata (`Artist::get` + `Track::get`).
- `/me/tracks/contains` returns 403 → like state and like/unlike use the unified `/me/library` (`contains`, `PUT`, `DELETE`) with `uris=`. `/me/albums`, `/me/following?type=artist` (cursor paging under `artists`) and `/me/player/devices` accept limit 50.
- Changing `WEB_SCOPES` changes the saved grant key, so the next start asks for a browser consent once.
- Spotify-owned/editorial playlists can return 403/404 to third-party apps.
- Spotify doesn't always rotate refresh tokens; librespot-oauth then returns `""` — keep the previous token (`web::refresh`).

## Dependency pins
- `Cargo.lock` pins `vergen` 9.0.6: librespot-core 0.8.0's build script fails with 9.1.0. Don't blanket `cargo update`.
- `librespot`/`librespot-oauth` =0.8.0, `slint` =1.18.1, `windows` =0.62.2 (already pulled in by cpal/winit; only features added).
- Prefer crates already in `Cargo.lock`; check with `cargo tree -d` that an addition doesn't add a second version.
