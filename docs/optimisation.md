# Memory optimisation

Why PinShot kept 40-120 MB after a capture, what was changed, and what was measured. Measured on Linux/X11, three
monitors (5560x1920 desktop, 42.7 MB of raw pixels), release build, `RssAnon` from `/proc/<pid>/status`.

## Findings

1. **Fonts were the permanent floor (about 30 MB).** `fontdue::Font::from_bytes` expands every glyph of a font into
   outlines at load time. Noto Sans has 3884 glyphs: 15.1 MB for Regular and 15.0 MB for Bold, held in a `static
   OnceLock` for the life of the process. The first text drawn (a toast, the toolbar) paid this. It is live data, so
   no allocator call can return it. This is why memory stayed near 40 MB after a capture closed, with no clipboard
   involved.
2. **The OCR engine was not freed on time.** Models cost +44 MB, +70 MB after a recognition (measured). Dropping it
   returns everything, but the 5-minute timeout lived in `about_to_wait` and nothing scheduled a wake-up for it. An idle
   tray app has no events, so the engine stayed until some unrelated event arrived.
3. **Several paths never returned memory.** `release_unused()` ran only when the overlay closed. The quick paths
   (`--fullscreen`, `--window`, `--repeat`), pin and OCR window closing, toast expiry, and copy/save (the picture was
   still alive when `close_overlay` ran its sweep) never swept. The quick fullscreen path stayed at 91 MB.
4. **Pins held two copies.** `PinWin` kept `image` and a `cache`; `scale_to` returned a full clone when the window had
   the image's own size.
5. **Not a leak.** Three capture/cancel cycles on a fresh process stayed flat (43.3 MB). The clipboard keeps the last
   copied image by design (the owning process serves it, about 30 MB for a full 3-monitor desktop).

## Changes

| Area                | Change                                                                                                       |
| ------------------- | ------------------------------------------------------------------------------------------------------------ |
| `core/src/text.rs`  | Always-loaded faces are Latin subsets (1202 glyphs). The full faces load lazily, only when a character outside the subset (Cyrillic, Greek) is first used. `load_substitutions: false` (glyphs are looked up by character, never by index). |
| `assets/fonts/`     | Added `NotoSans-Regular-Latin.ttf` and `NotoSans-Bold-Latin.ttf` (about 95 KB each). Full faces stay as the fallback. |
| `app/src/app.rs`    | The OCR idle timeout now sets `ControlFlow::WaitUntil`, retries after 5 s while a recognition runs, and sweeps after dropping the engine (`OCR_IDLE`). Toast expiry sweeps. |
| `app/src/app/ocr.rs`| The picture is dropped in the OCR thread before the main thread is notified; `ocr_done` and `close_ocr` sweep.  |
| `app/src/app/pinned.rs` | `close_pin` and `close_all_pins` sweep after the pin is dropped.                                         |
| `app/src/app/output.rs` | `overlay_copy` and `save_chosen` drop the rendered picture first, then sweep.                            |
| `app/src/app/capture.rs`| Fullscreen, active-window and last-region captures sweep when finished (captures dropped first).         |
| `record/src/lib.rs` | The frame source (and its full-size frame copy) is dropped as soon as capturing stops, before the file is finalised. |
| `app/src/app/rec.rs`| Sweep when a recording finishes, fails or is cancelled.                                                      |
| `app/src/pins.rs`   | A pin shown at the image's own size draws the image directly (no second copy). `scale_to` copies its source only when the size already matches. |

The subsets were made with `pyftsubset` (fonttools) from the full faces:

```
U+0020-007E, U+00A0-024F, U+02B0-02FF, U+0300-036F, U+1E00-1EFF, U+2000-206F,
U+20A0-20CF, U+2100-214F, U+2190-21FF, U+2200-22FF, U+25A0-25FF, U+FFFD
--layout-features=kern --no-hinting --desubroutinize --notdef-outline --name-IDs=*
```

That covers Turkish and the other Latin-script languages, Vietnamese, punctuation, currency and common math signs.

## Results

Same isolated instance, same steps, old = commit `9ce638c`, new = this change:

| Step                                       | Before     | After      |
| ------------------------------------------ | ---------- | ---------- |
| Idle, first toast shown                    | 47.1 MB    | 24.5 MB    |
| Capture closed (overlay cancelled)         | 43.3 MB    | 25.2 MB    |
| Overlay open (3 monitors)                  | about 103 MB | about 82 MB |
| `--fullscreen` copy                        | 91.5 MB    | 24.0 MB    |
| Pin open (2560x1440)                       | 119.7 MB   | 70.7 MB    |
| Pin closed                                 | 119.7 MB   | 24.8 MB    |

No growth over repeated cycles (25.20 to 25.23 MB over two).

Cost of the extra sweeps (`mi_collect(true)`, measured in isolation): about 1 microsecond with nothing to free, under
0.4 ms when freeing roughly 88 MB. Touching 43 MB again after a sweep costs about 1.6 ms. The first Cyrillic or Greek
character loads the full face once: about 6.5 ms.

### Screen recording

Reviewed and measured at library level (`pinshot-record` with synthetic `Bgr0` frames into real ffmpeg, 30 fps, 12 s,
mimalloc). The design is already bounded: a 3-frame queue (`QUEUE_FRAMES`) that drops frames instead of growing, audio
written to files by ffmpeg child processes, one fresh frame buffer per grab plus one clean copy for the island mask.

| Recording        | While recording | Peak  | After finish | After drop + collect |
| ---------------- | --------------- | ----- | ------------ | -------------------- |
| 1920x1080 MP4    | 12 MB           | 20 MB | 12 MB        | 3 MB                 |
| 2560x1440 MP4    | 21 MB           | 36 MB | 21 MB        | 5 MB                 |
| 1280x720 GIF     | 8 MB            | 12 MB | 5 MB         | 3 MB                 |

Flat over the whole run, no dropped frames, no leak. The only gap was that nothing swept when a recording ended, so
12-21 MB stayed until the next capture. The recording thread frees its frames when it exits, which can be just after
`Event::Finished` reaches the main thread, so the sweep in `rec_finished` is best effort; the sweep when the "recording
ready" notification expires (8 s) is the guaranteed one. Not measured: the app's real X11 grab, cursor drawing and island
masking, and the ffmpeg child process (separate process, not part of PinShot's RSS).

## Behaviour and compatibility

- Latin text (Turkish included) draws the same: glyph bitmaps, advances, line height and ascent are asserted equal to the
  full faces at 20 px for the Turkish letters, digits and common punctuation. Pixel-comparing frame tests pass.
- Cyrillic and Greek still render; they load the full faces on first use, and that session keeps the extra ~30 MB.
- A character in neither face (an arrow, emoji, CJK) draws the same box as before; the first one also loads the full
  face.
- The OCR models are reloaded on the next recognition after the idle drop, as before.

## Verified and not verified

Verified: `cargo test --workspace`, `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass;
live capture, fullscreen copy and pin open/close on Linux/X11; the ffmpeg end-to-end recording tests (MP4, GIF).

Not verified: Windows, macOS and ARM builds (no platform code was added; CI builds them); the 5-minute OCR drop end to
end; a real `--record` session in the app; closing a pin with Esc or the context menu; `just check` and `cargo deny`.

## Remaining

- The glyph cache in `Fonts::glyph` is keyed by `(char, size, bold)` and never evicted. It grows slowly with distinct
  sizes and characters.
- Each pin redraw allocates a window-sized temporary `Pixmap`.
- A copied image stays on the X11 clipboard while PinShot runs; that cannot be freed without losing the clipboard.
- Dropping the `static` fonts after idle would save the remaining ~9 MB but needs `Fonts::embedded()` to stop returning
  `&'static`; not done.
