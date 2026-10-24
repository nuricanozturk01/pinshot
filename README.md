<p align="center">
  <img src="assets/icon.png" alt="PinShot icon" width="128" height="128">
</p>

<h1 align="center">PinShot</h1>

Capture part of your screen, mark it up, copy it, or keep it floating above your other windows. You can also record the screen as
an MP4 or a GIF. Works on Windows, macOS and Linux.

Taking a screenshot is easy. The work that comes after it is not: you want to point at an error message, put a design next to
your code, copy a table from a PDF, or grab text that cannot be selected. PinShot turns these small jobs into one shortcut:
capture, annotate, paste. Or pin the image and keep it in front of you while you work.

## What it does

**Capture and annotate**
- Select a region, then move it or resize it by its handles. With several monitors the region can start on one
  monitor and continue onto the next; the result is stitched together pixel for pixel.
- Tools: rectangle, ellipse, line, arrow, pen, highlighter, text, numbered markers, mosaic and blur.
- Everything you draw stays an object. You can select it, move it, resize it by its corners and change its colour or
  width later. Double-click a text to edit it.
- Undo and redo only cover what you do on the canvas (drawing, deleting, moving). Changing a width or colour is not an undo step.
- A magnifier shows the colour under the cursor, and one key copies it.
- Works with several monitors, including different scale factors.

**Pin to screen**
- Turn a capture, a clipboard image, a file or some text into an always-on-top window.
- Resize with the mouse wheel or by dragging an edge or corner. You can also change opacity, flip, rotate and lock the position.

**OCR**
- Reads text from an image. The model ships with the app, so it works offline and your image never leaves your computer.

**Screen recording**
- Record a region or a whole monitor as **MP4** or **GIF**. With several monitors, each one has its own picker, so you
  choose the screen by clicking on it.
- Frame rates: 10, 15, 24, 30 and 60 fps for MP4. GIF goes up to 50 fps (the format cannot go faster), but GIF files grow
  quickly with the frame rate, so 10–15 fps is usually the best choice there.
- Sound for MP4: record the **system sound**, the **microphone**, or both. Pick the microphone from a list; the options
  are in the record panel (both `Ctrl+Alt+V` and the red record button in the capture toolbar). macOS needs a virtual device
  such as BlackHole for system sound.
- Quality first: full resolution by default, 4:4:4 colour for sharp coloured text (switch it off for maximum player
  compatibility; QuickTime, Safari and some TVs cannot play 4:4:4), and a separate colour palette for every GIF frame.
- Light on resources: frames are streamed to a separate low-priority encoder, so memory stays flat however long you record.
  You can limit the output width to save CPU and disk space, but the default is the original size.
- A red frame around the area (outside the video) and a small stop bar show that you are recording. A recording stops
  by itself after the time limit you set (10 minutes by default).
- The finished file is copied to the clipboard, so you can paste it into a chat or an e-mail.

**Everyday use**
- Lives in the system tray. Shortcuts can be changed in the settings.
- Delayed capture (3, 5 or 10 seconds), full-screen capture and "repeat last region".
- Optional auto-save of every capture to a folder (PNG, JPEG or WebP).
- Can be controlled from the command line, which is handy for desktop shortcuts.

## Install

Download the file for your system from the [Releases](../../releases) page.

| System | File |
|---|---|
| Windows (x64) | `PinShot-…-windows-amd64-setup.exe`, or the portable `.zip` |
| Windows (ARM64) | `PinShot-…-windows-arm64-…` (experimental) |
| macOS (Apple Silicon) | `PinShot-…-macos-arm64.dmg` |
| macOS (Intel) | `PinShot-…-macos-amd64.dmg` |
| Linux (x86-64 / ARM64) | `PinShot-…-linux-amd64.deb`, `…-linux-arm64.deb`, or the `.tar.gz` |

You can check your download against `SHA256SUMS.txt`.

**macOS:** The app is ad-hoc signed, but not notarized by Apple. On the first launch, right-click it and choose Open. Then
go to *System Settings → Privacy & Security* and allow **Screen Recording**. Allow **Accessibility** as well if you want
global shortcuts. Without these permissions macOS returns an empty image.

**Linux:** `sudo apt install ./PinShot-*.deb` also installs the dependencies. If you use the `.tar.gz`, you need
`libxcb-cursor0` and `libxkbcommon-x11-0`. On Wayland, screenshots are taken through xdg-desktop-portal (or grim,
spectacle or gnome-screenshot). Wayland does not allow global shortcuts, so bind a shortcut in your desktop settings to
`pinshot --capture` instead.

**From source:** you need Python 3.9–3.12.

```
pip install -e .
python -m pinshot
```

## Starting PinShot

PinShot is a tray app: after you start it, it waits in the background and uses almost no CPU.

| System | How to start it | Where it lives |
|---|---|---|
| Windows | Start menu → **PinShot** (the installer can also add it to start-up) | Tray icon at the bottom right. If you do not see it, click the `^` arrow |
| macOS | Open **PinShot** from Applications or Launchpad | Menu bar icon at the top right (no Dock icon) |
| Linux | Application menu → **PinShot**, or run `pinshot` in a terminal | Tray icon. GNOME has no tray by default: install the *AppIndicator* extension, or just use the shortcuts |

To start it automatically at login, turn on *Settings → Start at login*.

## Usage

Default shortcuts. You can change all of them in the settings.

| Action | Windows / Linux | macOS |
|---|---|---|
| Capture a region | `Ctrl+Alt+A` | `Control+Option+A` |
| Capture the screen under the cursor | `Ctrl+Alt+F` | `Control+Option+F` |
| Capture the last region again | `Ctrl+Alt+R` | `Control+Option+R` |
| Start / stop screen recording | `Ctrl+Alt+V` | `Control+Option+V` |
| Pin the clipboard content | `F3` | `F3` |

You can also click the tray icon, or use its menu. On macOS, global shortcuts need the *Accessibility* permission.
On Wayland there are no global shortcuts, so bind a key in your desktop settings to `pinshot --capture`
(or `--record`).

In the editor and on pinned windows, `Ctrl` means **`⌘ Command`** on macOS (for example `⌘Z` to undo, `⌘+wheel` to
resize a pin), and `Alt` means `Option`.

**Recording a video**

You can also select an area in the normal capture screen and click the red record button in the toolbar.

1. Press the recording shortcut (or choose *Record screen* in the tray menu). Every monitor shows a picker.
2. On the monitor you want, choose **MP4** or **GIF** and the frame rate. Then drag an area and press *Record selection*, or
   press *Record whole screen* (or `Enter`).
3. To stop, press the same shortcut again, click *Stop* on the small bar, or use the tray menu.

The file is saved in your capture folder and copied to the clipboard. Click the notification to open the folder.

**In the capture screen**

| Key | Action |
|---|---|
| Drag | Select a region. Resize it with the handles, or move it by dragging inside |
| `Enter` / double-click | Copy to the clipboard |
| `Ctrl+S` | Save to a file |
| `F3` | Pin to the screen |
| `O` | Recognize text (OCR) |
| Red record button | Record the selected area as MP4 or GIF instead of taking a still |
| `Ctrl+A` | Select the whole monitor under the mouse cursor |
| Arrow keys | Move the region or the selected object by 1 px (`Shift` for 10 px) |
| `V R E L A P H T N M B` | Select, rectangle, ellipse, line, arrow, pen, highlighter, text, number, mosaic, blur |
| `Shift` while drawing | Square or circle, and lines in 15° steps |
| `Del` | Delete the selected object |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `C` (before selecting) | Copy the colour under the cursor (`Shift` switches between HEX and RGB) |
| `Esc` / right-click | Go back one step / cancel |

**On a pinned window:** the wheel or `Ctrl`+wheel resizes it, `Alt`+wheel changes the opacity, and a double-click or `Esc`
closes it. The right-click menu has copy, save, OCR, size, rotate, lock and border options.

**Command line**

```
pinshot --capture | --fullscreen | --repeat | --record | --stop-recording | --pin-clipboard | --settings | --quit | --version
```

Commands are sent to the running instance. If PinShot is not running yet, it starts first.

## Limitations

- The interface language is Turkish only for now.
- No scrolling (long) screenshots. Screen recordings have no sound.
- Screen recording does not work on Wayland yet (the compositor does not allow it). Use an X11 session, Windows or macOS.
- No window detection (selecting a window automatically with the mouse).
- On Windows ARM64 there is no ready-made package for the OCR engine, so OCR does not work there. That build is experimental.
- On macOS, the full-screen overlay may not cover the menu bar.

## Development

```
pip install -e ".[build,test]"
QT_QPA_PLATFORM=offscreen pytest
python tools/make_icons.py
pyinstaller packaging/pinshot.spec
python packaging/package.py --arch amd64      # or arm64
```

Project layout:

```
pinshot/       the application (overlay, annotate, pin, ocr, capture, settings)
packaging/     PyInstaller spec, packaging script, Windows installer script
tools/         icon generation, version stamping
tests/         pytest (Qt offscreen)
.github/       CI and release workflows
```

**Making a release:** run `git tag v1.2.3 && git push origin v1.2.3`. The `release.yml` workflow builds all six targets and
runs each build once (`--selftest`). Then it creates the release page with the files and their checksums.

Settings and logs are stored in a `PinShot/` folder in your user directory. On Linux, the settings file is
`~/.config/PinShot/config.json` and the logs are in `~/.local/share/PinShot/logs`.

## Third-party software

Video recording runs [FFmpeg](https://ffmpeg.org/) as a separate program, taken from the
[imageio-ffmpeg](https://github.com/imageio/imageio-ffmpeg) package. That build is licensed under the GPL; PinShot does not
link to it. OCR models come from RapidOCR (Apache-2.0).

## Author

Created by **Nuri Can OZTURK** · [github.com/nuricanozturk01](https://github.com/nuricanozturk01)

## License

MIT, see [LICENSE](LICENSE). The bundled FFmpeg binary keeps its own license.
