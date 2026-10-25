<p align="center">
  <img src="assets/icon.png" alt="PinShot icon" width="128" height="128">
</p>

<h1 align="center">PinShot</h1>

<p align="center">
  Capture your screen, mark it up, pin it, read its text, or record it as MP4 or GIF.<br>
  Windows, macOS and Linux. One small native program, no runtime to install.
</p>

---

## Features

- **Capture and mark up**: rectangle, ellipse, line, arrow, pen, highlighter, text, numbers, mosaic and blur. Shapes stay editable.
- **Several monitors**: drag a selection across screens.
- **Pin to screen**: keep an image in an always-on-top window. Resize, fade, flip.
- **OCR**: copy text out of an image. Runs offline; supports Turkish, English and other Latin-script languages.
- **Screen recording**: MP4 or GIF, up to 60 fps, with system sound and microphone (MP4).

## Screenshots

<p align="center">
  <img src="docs/images/editor.png" alt="Selecting an area and marking it up with the toolbar" width="860">
</p>

<p align="center">
  <img src="docs/images/settings.png" alt="Settings window" height="360">
  &nbsp;
  <img src="docs/images/about.png" alt="About window" height="360">
</p>

## Install

Download the file for your system from [Releases](../../releases) and check it with `SHA256SUMS.txt`.

| System              | File                                                         |
| ------------------- | ------------------------------------------------------------ |
| Windows (x64)       | `windows-amd64-setup.exe`, or the portable `.zip`            |
| macOS               | `macos-arm64.dmg` (Apple Silicon), `macos-amd64.dmg` (Intel) |
| Linux (x64 / ARM64) | `linux-amd64.deb`, `linux-arm64.deb`, or `.tar.gz`           |

**macOS:** the app is not notarised. The first time, right-click it and choose **Open**. Then allow **Screen Recording**
(screenshots, recording, system sound) and **Microphone** (voice) in System Settings → Privacy & Security, and restart
PinShot. System sound needs macOS 13 or newer.

**Linux:** `sudo apt install ./PinShot-*.deb`. On Wayland, screenshots work through the desktop portal; global shortcuts
and screen recording do not, so bind a desktop shortcut to `pinshot --capture`.

PinShot lives in the system tray. Run `pinshot --selftest` if something does not work: it checks screen capture, OCR and
ffmpeg and tells you what is missing.

## Shortcuts

Change them in **Settings** (tray menu).

| Action                      | Windows / Linux | macOS             |
| --------------------------- | --------------- | ----------------- |
| Capture an area             | `Ctrl+Alt+A`    | `Control+Shift+A` |
| Capture the whole monitor   | `Ctrl+Alt+F`    | `Control+Shift+F` |
| Capture the last area again | `Ctrl+Alt+R`    | `Control+Shift+R` |
| Start / stop recording      | `Ctrl+Alt+V`    | `Control+Shift+V` |
| Pin the clipboard           | `F3`            | `Control+Shift+P` |

## Using it

Press the capture shortcut, drag over an area, draw if you like, then press `Enter` to copy.

| Key                     | Action                                                                                |
| ----------------------- | ------------------------------------------------------------------------------------- |
| `Enter` / double-click  | Copy                                                                                  |
| `Ctrl+S`                | Save to a file                                                                        |
| `F3` / `Ctrl+P`         | Pin to the screen                                                                     |
| `O`                     | Read the text (OCR)                                                                   |
| `Ctrl+A`                | Select the whole monitor                                                              |
| `V R E L A P H T N M B` | Select, rectangle, ellipse, line, arrow, pen, highlighter, text, number, mosaic, blur |
| `Shift` while drawing   | Squares, circles, straight lines                                                      |
| `Ctrl+Z` / `Ctrl+Y`     | Undo / redo                                                                           |
| `Esc` / right-click     | Back / cancel                                                                         |

On macOS, `Ctrl` here means `⌘ Command`.

**Pinned images:** drag to move, scroll to resize, `Alt`+scroll for transparency, double-click or `Esc` to close,
right-click for more.

**Recording:** press the recording shortcut (or the red button in the toolbar), pick MP4 or GIF, drag an area, and stop
with the same shortcut. The file is saved to your capture folder and copied to the clipboard. MP4 uses full colour
detail by default; turn that off in the panel if a player cannot play it.

## Command line

The commands talk to the running PinShot, which makes them handy for desktop shortcuts.

```
pinshot --capture | --fullscreen | --repeat | --window | --record | --stop-recording | --pin-clipboard | --settings | --about | --quit
pinshot --selftest
```

## Known limits

- No scrolling screenshots and no automatic window selection.
- No Windows on ARM build yet.
- With monitors at different scale factors, a selection across the gap can include a black strip.

## Development

```
cargo build -p pinshot
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
tools/fmt.sh                  # rustfmt + prettier (tools/fmt.sh --install-hook formats on commit)
```

Crates: `core` (annotations, rendering, config, shortcuts, i18n), `ocr`, `record` (ffmpeg and audio), `app` (the
program). Status and notes: [docs/RUST_PORT.md](docs/RUST_PORT.md). Versions are calendar based, `YYYY.M.N` (`N` counts releases within the month). Release: push a tag such as `v2026.10.0`.

Settings are in `~/.config/PinShot/config.json` on Linux (a `PinShot` folder in your user directory elsewhere); logs are in
`~/.local/share/PinShot/logs`.

## Third-party software

Recording runs [FFmpeg](https://ffmpeg.org/) as a separate program (GPL build from
[imageio-ffmpeg](https://github.com/imageio/imageio-ffmpeg); PinShot does not link to it). OCR uses the PaddleOCR
PP-OCRv4 detection and PP-OCRv5 Latin recognition models (Apache-2.0) with the pure-Rust `tract` engine.

## Author and license

Created by **Nuri Can OZTURK** · [github.com/nuricanozturk01](https://github.com/nuricanozturk01). MIT, see
[LICENSE](LICENSE). The bundled FFmpeg keeps its own license.
