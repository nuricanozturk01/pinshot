# Project status

Branch `main`. The old Python edition lives on the `master` branch and is no longer developed.

## What works

Verified on Linux/X11 (3 monitors, Linux Mint): capture overlays with annotations, toolbar, pins and their context menu,
OCR (Turkish and English checked by a test), MP4/GIF recording with system sound, tray, global hotkeys, single instance and
CLI, settings window (shortcut capture, folder picker, autostart), About, colour picker, English/Turkish UI.

Written but **not run on real hardware** (CI only compiles it): Windows capture and audio, macOS capture, audio and
permissions, tray on Windows/macOS. Wayland capture goes through the desktop portal; it was exercised on X11 by forcing
`XDG_SESSION_TYPE=wayland`, and a headless-sway CI job (`tools/wayland_ci.sh`) is set up but has not run yet.

## Performance (release build, 3 monitors)

|                                             | Before  | Now                                            |
| ------------------------------------------- | ------- | ---------------------------------------------- |
| CPU while moving the mouse over the overlay | ~80 %   | ~1 %                                           |
| Memory idle                                 | ~126 MB | ~28 MB                                         |
| Memory with the overlay open                | ~140 MB | ~125-140 MB (screen copies and window buffers) |
| Memory after the overlay closes             | ~97 MB  | ~55 MB                                         |

Changes behind this: the hint strip no longer forces a full-width redraw, the dim and loupe loops work on rows, grabs are
converted in place, `mimalloc` replaces the glibc allocator and is collected when the overlay closes.

## Next steps

1. Real-device checks on Windows and macOS (capture, system sound, permissions, installers) and on a real Wayland
   session. Screen recording on Wayland still uses X11 grabs (needs PipeWire).
2. Look at the first run of the Wayland CI job (`continue-on-error` until it is green, then make it required).
3. Cut the first release: push a tag `vX.Y.Z`; `.github/workflows/release.yml` builds every platform.
4. Known flake: the first mouse press after the overlay opens was lost once with synthetic XTest input; 20 scripted runs
   did not reproduce it.
5. Peak memory with the overlay open could drop further (one screen copy per monitor is the floor, plus the window
   buffers).

## Architecture

| Crate           | Role                                                                                                                                                      |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/core`   | No window system: geometry, annotations, editor state machine, undo/redo, rendering (tiny-skia), text, composition, config, shortcuts, i18n, pin geometry |
| `crates/ocr`    | PP-OCRv4 detection + PP-OCRv5 Latin recognition, run with tract (pure Rust). Models live in `assets/ocr`                                                  |
| `crates/record` | ffmpeg pipeline and audio capture (PulseAudio/PipeWire, WASAPI via cpal, macOS helper)                                                                    |
| `crates/app`    | The `pinshot` program: winit + softbuffer windows, overlay, toolbar, pins, tray, hotkeys, settings, About, colour picker                                  |

Windows are drawn in software (tiny-skia into a softbuffer surface) so the same code runs on every system; there is no Qt
or GTK. The file dialog uses the desktop portal on Linux.

## Tooling

- `tools/fmt.sh` formats Rust (rustfmt) and YAML/Markdown (prettier); `--check` is what CI runs, `--install-hook` formats
  on commit.
- CI (`.github/workflows/rust.yml`): fmt + prettier + clippy with `-D warnings`, tests on Linux (x64, ARM), Windows and
  macOS (ARM, Intel), and the headless Wayland job. The toolchain is `stable`, so run `rustup update stable` locally to match
  the lints.
- Release (`.github/workflows/release.yml`, `packaging/package.py`): `.deb` and `.tar.gz` (Linux), zip and Inno Setup
  installer (Windows), ad-hoc signed `.app` in a `.dmg` (macOS). ffmpeg is bundled next to the program.

## Known differences from the Python edition

- WebP is saved lossless (the pure-Rust encoder has no lossy mode), so the quality setting affects JPEG only.
- OCR uses a Latin-script model: Turkish and English work, Chinese, Japanese and Korean text does not.
