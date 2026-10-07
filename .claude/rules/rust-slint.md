---
paths:
  - "src/**/*.rs"
  - "Cargo.toml"
---

# Rust / Slint / librespot rules for SlimSpot

## Verifying library behaviour
- Read the pinned crate's source in `~/.cargo/registry/src/*/<crate>-<version>/` before coding against it; librespot and Slint internals have repeatedly differed from what names suggest.
- Before coding against a Web API endpoint, fetch a real response through the app's own `WebApi` (temporary debug code that is removed afterwards), so rotated refresh tokens are saved. Never probe with a separate client that discards tokens.
- Runtime diagnosis: the fast/release binaries have no console (`windows_subsystem = "windows"`). Use a debug build launched with stderr redirected, plus temporary `eprintln!("TEMP ...")`; remove every `TEMP` line and confirm with a case-sensitive search before finishing.

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
- An element inside `if cond : ...` can't be referenced by id from outside it, and a PopupWindow's children can't be written from outside the popup: collapse with `visible` + `min/max-height: 0` instead of `if`, and bind popup inputs to root properties (`text <=> root.x`).
- A layout's minimum height comes from its children; `max-height: 0` alone doesn't shrink it, an explicit `min-height: 0px` is needed too.
- The software renderer also ignores `border-radius` on gradient backgrounds (they render square); rounded elements use solid colors.
- Calling a PopupWindow's `close()` inside its own click handler drops the rest of that handler (later callbacks never run). Invoke callbacks first, then close; a Rust callback that opens a modal (file dialog) should defer it with `slint::Timer::single_shot(Duration::ZERO, ..)`.
- `if cond : for x in list : ...` doesn't parse; put the `for` inside a layout under the `if`.
- The winit backend passes `Window.icon` to Windows only when the image's cache key changes; a `slint::Image::from_rgba8` image has no key, so a window icon set from Rust never shows. Embed it with `@image-url(...)` in the markup.
- Fonts embed with `import "../assets/fonts/X.ttf";` inside `slint!` (path relative to the source file) plus `default-font-family` on the Window.
- Computer-use/SendKeys Escape presses never reach the app (no key event at all), while a real keyboard's Esc works: test Esc by hand, don't "fix" it from automated runs.
- Compiling in `renderer-femtovg` makes FemtoVG the winit backend's default renderer; select `"software"` by name with `slint::BackendSelector` before the first window.
- `WindowMoveArea` doesn't move a `no-frame` window on Windows here (winit posts `WM_NCLBUTTONDOWN` asynchronously). Send it synchronously after `ReleaseCapture`, then dispatch a `PointerReleased` to Slint on the next event-loop turn (the call runs inside the press handler and Slint sets the grab only after it returns): the modal move loop eats the button-up and Slint would route every later click to the pressed TouchArea.
- When min size == max size Slint makes the window non-resizable and removes `WS_MAXIMIZEBOX`, but only restores the button at window creation; set the style bit back yourself after relaxing the constraints.
- Sizing a window in the same turn as setting `no-frame` leaves the old caption and borders as a margin; size it again on the next event-loop turn.
- To check the UI visually, capture the window with Win32 `PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT)` from Windows PowerShell 5.1 (`System.Drawing`); screen capture grabs whatever window is in front. The app window is class `Window Class`, title `SlimSpot` (the tray has its own hidden window).

## librespot
- `Spirc::new` needs a fresh, unconnected `Session`; it connects it itself.
- `HttpClient::request` turns non-2xx into opaque errors and gives up on 429 when Retry-After > 10 s; use `request_fut` and handle status yourself.
- Spirc commands are ignored while the device isn't active; local-only effects (e.g. volume) must also be applied directly.
- "Unable to read audio file: end of stream" with "Audio key response timeout" right before it is a dropped session, not a corrupt cache: librespot continues without decryption and the decoder chokes. A truly bad cached file is removed and re-downloaded by librespot itself ("Unable to read cached audio file ... Trying to download it"). Read the log lines before the error before blaming the cache.

## Style
- Named constants with a comment saying where the value comes from (measured, Spotify limit, librespot default).
- Deliberate shortcuts get a `ponytail:` comment naming the ceiling and the upgrade path.
