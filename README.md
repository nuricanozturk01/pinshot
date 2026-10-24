# PinShot

Capture part of your screen, mark it up, copy it, or keep it floating above your other windows. Works on Windows, macOS and Linux.

Taking a screenshot is easy. The work that comes after it is not: you want to point at an error message, put a design next to
your code, copy a table from a PDF, or grab text that cannot be selected. PinShot turns these small jobs into one shortcut:
capture, annotate, paste. Or pin the image and keep it in front of you while you work.

## What it does

**Capture and annotate**
- Select a region, then move it or resize it by its handles.
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

## Usage

Default shortcuts (you can change them in the settings):

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+A` | Capture a region |
| `Ctrl+Alt+F` | Capture the screen under the cursor |
| `Ctrl+Alt+R` | Capture the last region again |
| `F3` | Pin the clipboard content |

On macOS, use `Cmd` instead of `Ctrl`.

**In the capture screen**

| Key | Action |
|---|---|
| Drag | Select a region. Resize it with the handles, or move it by dragging inside |
| `Enter` / double-click | Copy to the clipboard |
| `Ctrl+S` | Save to a file |
| `F3` | Pin to the screen |
| `O` | Recognize text (OCR) |
| `Ctrl+A` | Select the whole screen |
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
pinshot --capture | --fullscreen | --repeat | --pin-clipboard | --settings | --quit | --version
```

Commands are sent to the running instance. If PinShot is not running yet, it starts first.

## Limitations

- No scrolling (long) screenshots, no GIF or video recording.
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

## Credits

The idea comes from desktop screenshot tools like PixPin. The code was written independently. The interface uses
[Qt](https://www.qt.io/) (PySide6), OCR uses [RapidOCR](https://github.com/RapidAI/RapidOCR) and ONNX Runtime, and global
shortcuts use [pynput](https://github.com/moses-palmer/pynput).

## License

MIT, see [LICENSE](LICENSE).
