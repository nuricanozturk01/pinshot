<p align="center">
  <img src="assets/icon.png" alt="PinShot icon" width="128" height="128">
</p>

<h1 align="center">PinShot</h1>

<p align="center">
  Capture your screen, mark it up, pin it, or record it as MP4 or GIF.<br>
  Windows, macOS and Linux.
</p>

---

## What is it for?

Taking a screenshot is easy. What you do next is the slow part: point at something, blur a secret, copy text from an
image, keep a picture next to your work, or record a short clip.

PinShot does all of that from one shortcut. Press it, select an area, and the result is on your clipboard.

## Features

- **Capture and mark up** – rectangle, ellipse, line, arrow, pen, highlighter, text, numbers, mosaic and blur. Every shape
  can be moved, resized and changed later.
- **Several monitors** – start a selection on one monitor and drag it onto the next one.
- **Pin to screen** – keep an image, file or text in an always-on-top window. Resize it, make it transparent, flip it.
- **OCR** – turn text in an image into real text. It runs on your computer; nothing is uploaded.
- **Screen recording** – MP4 or GIF, up to 60 fps. For MP4 you can add system sound, the microphone, or both.
- **Color picker** – a magnifier shows the color under the cursor; one key copies it.

## Install

Download the file for your system from [Releases](../../releases).

| System | File |
|---|---|
| Windows (x64) | `…-windows-amd64-setup.exe` or the portable `.zip` |
| macOS, Apple Silicon | `…-macos-arm64.dmg` |
| macOS, Intel | `…-macos-amd64.dmg` |
| Linux (x64 / ARM64) | `…-linux-amd64.deb`, `…-linux-arm64.deb` or `.tar.gz` |

Check your download with `SHA256SUMS.txt`.

**macOS** – the app is not notarized by Apple. Right-click it and choose *Open* the first time. Then allow
*Screen Recording* (and *Accessibility* for shortcuts, *Microphone* for sound) in System Settings → Privacy & Security.

**Linux** – `sudo apt install ./PinShot-*.deb` installs everything it needs. With the `.tar.gz` you also need
`libxcb-cursor0` and `libxkbcommon-x11-0`.

**From source** (Python 3.9–3.12):

```
pip install -e .
python -m pinshot
```

## macOS permissions guide

macOS asks you to approve each feature once. If something does not work, a permission is almost always missing. macOS does
**not** show an error; the feature just does nothing (shortcuts never fire, screenshots are empty, recordings have no sound).

Open **System Settings → Privacy & Security** and switch **PinShot** on in these lists:

| Permission | What it is needed for | If it is missing |
|---|---|---|
| **Screen Recording** | Screenshots, screen recording and **system sound** | Empty or desktop-only images; no system sound |
| **Input Monitoring** | The global shortcuts | Shortcuts do nothing |
| **Accessibility** | The global shortcuts | Shortcuts do nothing |
| **Microphone** | Recording your voice | The recording has no microphone |

Steps:

1. Start PinShot. macOS may show a prompt for some permissions; click **Allow**.
2. For the others, open the list yourself: **Privacy & Security** → pick the permission → click `+` (or the switch) and
   add **PinShot**. PinShot also opens the right page for you when it notices a missing permission.
3. **Quit and start PinShot again.** macOS only applies new permissions to programs that start afterwards.

Good to know:

- Because the app is not notarized by Apple, the first launch needs a right-click → *Open*.
- If a permission is already switched on but still does not work (this can happen after you install an update), select
  PinShot in the list, remove it with `−`, add it again, and restart PinShot.
- System sound works on macOS 13 and newer with the Screen Recording permission. Older macOS needs the setup in *System sound on macOS* below.

## Start it

PinShot runs in the system tray and uses almost no CPU while it waits.

| System | How to start | Where to find it |
|---|---|---|
| Windows | Start menu → PinShot | Tray icon, bottom right (click `^` if hidden) |
| macOS | Applications or Launchpad → PinShot | Menu bar, top right |
| Linux | Application menu → PinShot, or run `pinshot` | Tray icon. GNOME hides it by default; the shortcuts still work |

## Shortcuts

You can change these in the settings.

| Action | Windows / Linux | macOS |
|---|---|---|
| Capture an area | `Ctrl+Alt+A` | `Control+Shift+A` |
| Capture the monitor under the cursor | `Ctrl+Alt+F` | `Control+Shift+F` |
| Capture the last area again | `Ctrl+Alt+R` | `Control+Shift+R` |
| Start / stop recording | `Ctrl+Alt+V` | `Control+Shift+V` |
| Pin the clipboard | `F3` | `Control+Shift+P` |

macOS uses different keys because `Option` changes the letter you type (`Option+A` gives "å"), so shortcuts built on it
cannot work, and `F3` belongs to Mission Control. Shortcuts only work after you grant the permissions below.

On macOS, `Ctrl` in the editor shortcuts below means `⌘ Command`.

## Taking a screenshot

1. Press `Ctrl+Alt+A` and drag over the area you want.
2. Draw on it if you like. The toolbar appears under the selection.
3. Press `Enter` to copy, then paste anywhere.

| Key | Action |
|---|---|
| `Enter` or double-click | Copy to the clipboard |
| `Ctrl+S` | Save to a file |
| `F3` or `Ctrl+P` | Pin to the screen |
| `O` | Read the text (OCR) |
| `Ctrl+A` | Select the whole monitor under the cursor |
| `V R E L A P H T N M B` | Select, rectangle, ellipse, line, arrow, pen, highlighter, text, number, mosaic, blur |
| `Shift` while drawing | Perfect squares and circles, straight lines |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `Del` | Delete the selected shape |
| Arrow keys | Nudge the selection or shape (`Shift` = 10 px) |
| `C` before you select | Copy the color under the cursor |
| `Esc` or right-click | Go back / cancel |

Double-click a text to edit it. Undo only covers what you draw, move or delete, not color or width changes.

## Pinned images

Pin an area with `F3`, or pin the clipboard (image, file or text) from the tray menu.

- Drag to move. Scroll, or drag an edge or corner, to resize (`Shift` for free resize).
- `Alt` + scroll changes transparency.
- Double-click or `Esc` closes it.
- Right-click for copy, save, OCR, flip, rotate, lock and more.

## Recording

1. Press `Ctrl+Alt+V`, or click the red record button in the capture toolbar.
2. Choose **MP4** or **GIF**, the frame rate and the size.
3. For MP4, turn on **System sound** and/or **Microphone**, and pick the microphone.
4. Drag an area, or choose *Record whole screen*.
5. Stop with the same shortcut, or with the small **Stop** bar that floats on your screen. You can drag that bar anywhere;
   it never appears in the video.

The file goes to your capture folder and is copied to the clipboard. A recording stops by itself after 10 minutes
(you can change this).

Good to know:

- MP4 keeps full color detail (4:4:4) for sharp text. A few players (QuickTime, Safari, some TVs) cannot play it; turn the
  option off in the panel if you want to share widely.
- GIF files get big quickly. 10–15 fps is usually enough.
- On macOS, system sound works on macOS 13 and newer (see *System sound on macOS* below).

### System sound on macOS

**macOS 13 (Ventura) or newer:** nothing to install. Tick **System sound** in the record panel. It uses the same
*Screen Recording* permission as screenshots (see the permissions guide above). Restart PinShot after you grant it.
The microphone is recorded through macOS directly, so your Mac's built-in microphone works.

**Older macOS:** macOS cannot give an app the sound it is playing, so a free virtual audio device, **BlackHole**, is needed:

1. Install **BlackHole 2ch**: `brew install blackhole-2ch`, or download it from
   [existential.audio/blackhole](https://existential.audio/blackhole/).
2. Open **Audio MIDI Setup** (Applications → Utilities). Click `+` at the bottom left → **Create Multi-Output Device**.
   Tick your speakers or headphones **and** *BlackHole 2ch*. Turn on *Drift Correction* for BlackHole.
3. In **System Settings → Sound → Output**, choose that Multi-Output Device.
4. In PinShot, open the record panel, tick **System sound** and press **↻**.

The panel has a **System sound guide** button that opens the BlackHole page and Audio MIDI Setup for you.

## Command line

The commands talk to the running PinShot. They are handy for desktop shortcuts, for example on Wayland.

```
pinshot --capture | --fullscreen | --repeat | --record | --stop-recording | --pin-clipboard | --settings | --quit
```

## Limitations

- No scrolling screenshots and no automatic window selection.
- The interface is in Turkish only for now.
- Wayland: screenshots work through the desktop portal, but global shortcuts and screen recording do not.
- No Windows on ARM build for now.
- With monitors at different scale factors, a selection that crosses the gap between them can include a black strip.

## Development

```
pip install -e ".[build,test]"
QT_QPA_PLATFORM=offscreen pytest
python tools/make_icons.py
pyinstaller packaging/pinshot.spec
python packaging/package.py --arch amd64      # or arm64
```

To publish a release, push a tag: `git tag v1.2.3 && git push origin v1.2.3`. The `Release` workflow builds all six
targets, tests each build, and creates the release with checksums.

Settings are in `~/.config/PinShot/config.json` on Linux (a `PinShot` folder in your user directory elsewhere). Logs are in
`~/.local/share/PinShot/logs`.

## Third-party software

Video recording runs [FFmpeg](https://ffmpeg.org/) as a separate program, taken from the
[imageio-ffmpeg](https://github.com/imageio/imageio-ffmpeg) package. That build is GPL-licensed; PinShot does not link to
it. OCR uses RapidOCR (Apache-2.0).

## Author

Created by **Nuri Can OZTURK** · [github.com/nuricanozturk01](https://github.com/nuricanozturk01)

## License

MIT, see [LICENSE](LICENSE). The bundled FFmpeg binary keeps its own license.
