# Rust port

Branch `rust`. The Python edition on `master` stays the released product until this port reaches parity.

## Goals

- Lower idle and peak resource use (no Qt, no Python runtime, no bundled interpreter).
- One small native binary per platform.
- Same features, same settings file, same shortcuts.

## Status (where we stopped)

Working and tested on Linux/X11 (3 monitors): capture overlays with annotations, toolbar, pins + context menu,
OCR (pure-Rust tract, PP-OCRv4), screen recording (MP4/GIF, system sound verified with PulseAudio/PipeWire),
tray (ksni), global hotkeys, single instance + CLI, toasts, English/Turkish UI (system language by default).

Written but **not run on real hardware** (CI compiles them): Windows capture/audio (GDI, cpal), macOS capture/audio
(CoreGraphics, avfoundation, audiotap helper), tray-icon on Windows/macOS.

### Next steps
1. Real-device checks: Windows and macOS (capture, system sound, permissions, installers), Wayland portal capture on a
   real Wayland session (the portal path was only exercised on X11 by forcing `XDG_SESSION_TYPE=wayland`), folder picker
   and autostart in the settings window. Screen recording on Wayland still uses X11 grabs (needs PipeWire).
2. Known flake: the very first mouse press/release after the overlay opens was lost once with XTest; 20 scripted
   runs did not reproduce it.
3. Cut the first Rust release: tag `rust-vX.Y.Z` runs `.github/workflows/rust-release.yml` (pre-release). Replacing the
   Python build means merging `rust` into `master` and pointing `release.yml` at `packaging/rust/package.py`.
4. Idle memory: ~126 MB RSS idle in release; investigate (fonts, ksni/zbus, embedded models are mapped lazily).

Done: settings window, About (`--about`), colour picker, `--selftest`, clippy clean (`-D warnings`), Wayland portal
capture, macOS privacy-pane shortcut, packaging script (`packaging/rust/package.py`, deb/tar.gz verified locally) and
release workflow.

## Architecture

| Layer | Crate / library | Status |
|---|---|---|
| Logic without a window system: geometry, annotations, editor state machine, undo/redo, rendering (tiny-skia), text, composition, config, shortcuts, i18n, pin geometry | `crates/core` | **done** |
| OCR (PP-OCRv4 via tract) | `crates/ocr` | **done** |
| Recording (ffmpeg pipeline, audio) | `crates/record` | **done** |
| Widget kit drawn in software (buttons, sliders, toolbar) | `crates/ui` (winit + softbuffer) | planned |
| Screens, hotkeys, tray, clipboard, dialogs, autostart, single instance | `crates/platform` | planned |
| OCR (PP-OCR ONNX models through `ort`) | `crates/ocr` | planned |
| Recording (ffmpeg sidecar), audio (cpal / platform helpers), island | `crates/record` | planned |
| The application | `crates/app` (`pinshot` binary) | planned |

## Phases

0. Scaffold, CI. **done**
1. Core crate with ports of the Python tests. **done**
2. UI kit.
3. Platform layer (Linux X11 first, then Windows, macOS).
4. Capture overlay, editor, pins.
5. OCR.
6. Recording and audio.
7. Settings dialog, About, CLI.
8. Packaging (deb, dmg, exe) and release workflow.

## Parity checklist (Python → Rust)

- [x] Annotation shapes, hit testing, handles, shift constraints
- [x] Undo/redo (geometry/text only)
- [x] Rendering: rect, ellipse, line, arrow, pen, marker, text, number, mosaic, blur
- [x] Multi-monitor composition with mixed scale factors
- [x] Config file compatible with the Python edition
- [x] Shortcut strings (including the macOS Control/Command swap)
- [x] Saving PNG / JPG / WebP
- [ ] Overlay editor, toolbar, pins
- [ ] Global hotkeys, tray, single instance, CLI
- [x] OCR, colour picker
- [ ] MP4 / GIF recording, audio, island
- [x] Settings dialog
- [x] About
- [x] Installers (script + workflow; deb/tar.gz built locally, Windows/macOS only by CI)

## Known differences

- WebP is saved lossless: the pure-Rust encoder has no lossy mode, so the quality setting affects JPEG only.

## Verification and risks

Only Linux/X11 can be run on the development machine. Windows and macOS code is verified by CI builds and
unit tests of the OS-independent parts; real-device checks (permissions, system sound, Retina, shortcuts) must be
done by hand before a Rust release replaces the Python one.
