# PinShot

Screenshot, annotate, OCR and pin-to-screen for **Windows, macOS and Linux** (amd64 + arm64). Python + Qt (PySide6).

## Use
Run it; it lives in the system tray. Default shortcuts (change in *Settings*):

| Shortcut | Action |
|---|---|
| `Ctrl+Alt+A` | Capture a region |
| `Ctrl+Alt+F` | Capture the screen under the cursor |
| `Ctrl+Alt+R` | Repeat the last region |
| `F3` | Pin the clipboard (image, files or text) |

**Capture overlay** – drag a region; move/resize it with its handles; arrow keys nudge (Shift = 10 px); `Ctrl+A` selects
the whole screen; `C` copies the colour under the cursor (`Shift` switches HEX/RGB).
Tools: select/move (`V`), rectangle (`R`), ellipse (`E`), line (`L`), arrow (`A`), pen (`P`), highlighter (`H`),
text (`T`), numbers (`N`), mosaic (`M`), blur (`B`). Every drawn item can be selected, moved, resized (handles) and
restyled; double-click text to edit it. `Shift` while drawing makes squares / 15° lines.
`Ctrl+Z` / `Ctrl+Y` undo/redo canvas edits only (style tweaks are not undo steps). `Enter`/✓ copies, `Ctrl+S` saves,
`F3` pins, `O` runs OCR, `Esc` cancels (right-click steps back).

**Pins** – drag to move; wheel or `Ctrl`+wheel or edge/corner drag to resize (aspect kept, `Shift` = free);
`Alt`+wheel (or `Ctrl+Shift`+wheel) for opacity; double-click or `Esc` closes; right-click for copy, save, OCR, size,
opacity, flip/rotate, lock, border.

**OCR** is fully offline (RapidOCR / ONNX models are bundled).

**CLI** (talks to the running instance – use it for desktop shortcuts, e.g. on Wayland):
`pinshot --capture | --fullscreen | --repeat | --pin-clipboard | --settings | --quit | --version`

## Install
Download from the **Releases** page:
- Windows: `*-windows-amd64-setup.exe` or portable `.zip` (arm64 builds are experimental)
- macOS: `.dmg` (Apple Silicon = arm64, Intel = amd64). Not notarised: right-click > Open the first time, then allow
  *Screen Recording* and *Accessibility* (global hotkeys) in System Settings.
- Linux: `sudo apt install ./PinShot-*.deb`, or the `.tar.gz` (needs `libxcb-cursor0`, `libxkbcommon-x11-0`).
  Wayland: screenshots use xdg-desktop-portal (or grim/spectacle/gnome-screenshot); global hotkeys are not
  possible there – bind a desktop shortcut to `pinshot --capture`.

From source: `pip install -e .` then `python -m pinshot`.

## Develop / release
```
pip install -e ".[build,test]"
QT_QPA_PLATFORM=offscreen pytest
python tools/make_icons.py && pyinstaller packaging/pinshot.spec && python packaging/package.py --arch amd64
```
Release: `git tag v1.0.0 && git push origin v1.0.0` – `.github/workflows/release.yml` builds all six targets, self-tests
every binary, and publishes the GitHub Release with checksums. Config: `~/.config/PinShot/config.json`
(`%APPDATA%`/`~/Library/Preferences` equivalents); logs in the `PinShot/logs` data folder.
