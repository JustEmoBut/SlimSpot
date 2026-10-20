---
paths:
  - "src/**/*.rs"
  - "Cargo.toml"
---

# Rust / Freya / librespot rules for SlimSpot

## Verifying library behaviour
- Read the pinned crate's source in `~/.cargo/registry/src/*/<crate>-<version>/` before coding against it; librespot and Freya internals have repeatedly differed from what names suggest. Freya is a release candidate (0.5.0-rc): its own docs (`freya-*/src/_docs`) and component examples are the reference, not memory.
- Before coding against a Web API endpoint, fetch a real response through the app's own `WebApi` (temporary debug code that is removed afterwards), so rotated refresh tokens are saved. Never probe with a separate client that discards tokens.
- Runtime diagnosis: the fast/release binaries have no console (`windows_subsystem = "windows"`). Launch with stderr redirected (`Start-Process -RedirectStandardError`), plus temporary `eprintln!("TEMP ...")`; remove every `TEMP` line and confirm with a case-sensitive search before finishing.

## Freya
- The UI thread enters the tokio runtime (`rt.enter()` in `main`): components use `tokio::time::sleep` in `use_future`, which Freya's executor polls. Without it the first timer panics ("no reactor running"). UI state is single-threaded: backend threads send `UiUpdate`s, never touch `State`.
- `State::read()`/`peek()` guards must end before `write()` on the same state in the same scope (RefCell panic), e.g. inside a `VirtualScrollView` builder: compute, `drop(m)`, then write.
- `try_consume_root_context` doesn't find what `app` provided with `use_provide_context`; plain `fn()` handlers reach the model through the `MODEL` thread-local instead.
- In a horizontal `rect`, `Size::fill()` is the whole parent width, not what's left: use `.content(Content::Flex)` on the parent and `Size::flex(1.)` on the growing child.
- A later sibling draws on top and takes the clicks: overlay controls (absolute chips) go after the content they float over.
- Reordering keyed children (`render_key`) can panic in the accessibility tree (`accessibility/tree.rs:426`, "Option::unwrap() on a None value"): the library sort crashed after a few clicks. Lists whose order changes in place stay unkeyed (`LibraryRow`).
- `Input` stops presses inside it (focus handling), so a `.trailing()`/`.leading()` button never gets its press; put clickable extras next to the `Input`.
- Pointer events carry no keyboard modifiers; read Ctrl/Shift with `GetKeyState` at click time.
- `Slider` reacts to the mouse wheel: on scrolling pages set `.scroll_enabled(false)`, or scrolling past it changes values.
- Hiding the window inside `with_on_close` doesn't stick; queue it with `Platform::get().with_window(id, ...)`. Freya shows the window right after the `with_window_handle` hook, so a start-hidden (`--tray`) hide is queued the same way.
- Covers: `ImageViewer` takes encoded bytes with an id and decodes at layout size; clipping with `overflow: Clip` + `corner_radius` rounds images (no baked corners needed).
- Animations repaint every frame while running; on the software renderer prefer stepped updates (a `use_future` tick) for anything that runs continuously, like the playing-row bars.
- Slint and Freya can't share a Cargo graph (both `links = "skia"`); don't add Slint back as an optional dependency.

## Testing the UI
- Capture the window with a screen grab of its rect (`System.Drawing` `CopyFromScreen`) after bringing it to the front; the app window's process is `slimspot`.
- Synthetic clicks: move the cursor, wait ~250 ms, then press. A click right after `SetCursorPos` can land on the element under the previous position.
- Don't change the owner's account or settings while testing (follow, save, add, create, reorder, settings toggles); if a test changes a setting, put it back and check `settings.json`.

## librespot
- `Spirc::new` needs a fresh, unconnected `Session`; it connects it itself.
- `HttpClient::request` turns non-2xx into opaque errors and gives up on 429 when Retry-After > 10 s; use `request_fut` and handle status yourself.
- Spirc commands are ignored while the device isn't active; local-only effects (e.g. volume) must also be applied directly.
- "Unable to read audio file: end of stream" with "Audio key response timeout" right before it is a dropped session, not a corrupt cache: librespot continues without decryption and the decoder chokes. A truly bad cached file is removed and re-downloaded by librespot itself ("Unable to read cached audio file ... Trying to download it"). Read the log lines before the error before blaming the cache.

## Style
- Named constants with a comment saying where the value comes from (measured, Spotify limit, librespot default).
- Deliberate shortcuts get a `ponytail:` comment naming the ceiling and the upgrade path.
