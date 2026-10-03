---
paths:
  - "src/**/*.rs"
  - "Cargo.toml"
---

# Rust / Slint / librespot rules for SlimSpot

## Verifying library behaviour
- Read the pinned crate's source in `~/.cargo/registry/src/*/<crate>-<version>/` before coding against it; librespot and Slint internals have repeatedly differed from what names suggest.
- Before coding against a Web API endpoint, fetch a real response through the app's own `WebApi` (temporary debug code that is removed afterwards), so rotated refresh tokens are saved. Never probe with a separate client that discards tokens.
- Runtime diagnosis: the release binary has no console (`windows_subsystem = "windows"`). Use a debug build launched with stderr redirected, plus temporary `eprintln!("TEMP ...")`; remove every `TEMP` line and confirm with a case-sensitive search before finishing.

## Slint
- `slint::Image` is `!Send`: backend code works with `web::Item` and converts to `ui::Row` on the UI thread (`set_rows`).
- The window handle exists only after the event loop starts; anything needing the HWND retries from a `slint::Timer`.
- Software renderer + Windows: after minimize/restore only damaged regions repaint into a cleared buffer. Keep the `repaint-flip` workaround.
- `row` is a reserved property name (GridLayout); don't name component properties `row`.
- `alignment: center` on a layout shrinks a `Slider` to its handle; give it `horizontal-stretch: 1` instead.
- Inside `slint::slint!`, hex colors that start with a digit followed by `e` (e.g. `#5eead4`) fail to tokenize as Rust ("expected at least one digit in exponent"); write them as `rgb(...)`.
- `ListView.viewport-y` is deprecated in 1.18; use `content-y`.
- The software renderer ignores `border-radius` in `clip: true` (its `combine_clip` drops the radius). Rounded or circular images must be masked in the pixels; a `Rectangle`'s own rounded background still renders fine.
- A `checkable` Button flips its own `checked` on click and breaks a one-way binding; for state owned by the backend use a plain Button (or `primary:`) and set it from Rust.
- Computer-use/SendKeys Escape presses never reach the app (no key event at all), while a real keyboard's Esc works: test Esc by hand, don't "fix" it from automated runs.
- To check the UI visually, capture the window with Win32 `PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT)` from Windows PowerShell 5.1 (`System.Drawing`); screen capture grabs whatever window is in front. The app window is class `Window Class`, title `SlimSpot` (the tray has its own hidden window).

## librespot
- `Spirc::new` needs a fresh, unconnected `Session`; it connects it itself.
- `HttpClient::request` turns non-2xx into opaque errors and gives up on 429 when Retry-After > 10 s; use `request_fut` and handle status yourself.
- Spirc commands are ignored while the device isn't active; local-only effects (e.g. volume) must also be applied directly.

## Style
- Named constants with a comment saying where the value comes from (measured, Spotify limit, librespot default).
- Deliberate shortcuts get a `ponytail:` comment naming the ceiling and the upgrade path.
