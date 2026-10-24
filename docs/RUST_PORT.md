# Rust port

Branch `rust`. The Python edition on `master` stays the released product until this port reaches parity.

## Goals

- Lower idle and peak resource use (no Qt, no Python runtime, no bundled interpreter).
- One small native binary per platform.
- Same features, same settings file, same shortcuts.

## Architecture

| Layer | Crate / library | Status |
|---|---|---|
| Logic that needs no window system: geometry, colour, annotation model, hit testing, undo/redo, software rendering (tiny-skia), text (fontdue + embedded Noto Sans), multi-monitor composition, config, shortcut strings, image saving | `crates/core` | **done, 44 tests** |
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
- [ ] OCR, colour picker
- [ ] MP4 / GIF recording, audio, island
- [ ] Settings dialog, About
- [ ] Installers

## Known differences

- WebP is saved lossless: the pure-Rust encoder has no lossy mode, so the quality setting affects JPEG only.

## Verification and risks

Only Linux/X11 can be run on the development machine. Windows and macOS code is verified by CI builds and
unit tests of the OS-independent parts; real-device checks (permissions, system sound, Retina, shortcuts) must be
done by hand before a Rust release replaces the Python one.
