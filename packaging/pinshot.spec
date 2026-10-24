# PyInstaller spec shared by Windows / macOS / Linux (amd64 + arm64). Build: pyinstaller packaging/pinshot.spec
import os
import sys

from PyInstaller.utils.hooks import collect_all

ROOT = os.path.abspath(os.path.join(os.path.dirname(SPEC), ".."))
sys.path.insert(0, ROOT)
from pinshot import __version__  # noqa: E402

datas, binaries, hiddenimports = [], [], []
for pkg in ("rapidocr_onnxruntime", "onnxruntime", "imageio_ffmpeg", "soundcard"):   # OCR models + native libs, ffmpeg binary
    try:
        d, b, h = collect_all(pkg)
        datas += d
        binaries += b
        hiddenimports += h
    except Exception as ex:                               # engine not installable on this platform
        print(f"WARNING: {pkg} not bundled: {ex}")

hiddenimports += ["_cffi_backend", "pynput.keyboard._xorg", "pynput.mouse._xorg", "pynput.keyboard._win32",
                  "pynput.mouse._win32", "pynput.keyboard._darwin", "pynput.mouse._darwin"]
excludes = ["tkinter", "matplotlib", "scipy", "pandas", "PyQt5", "PyQt6", "PySide2"]

a = Analysis([os.path.join(ROOT, "packaging", "entry.py")], pathex=[ROOT], binaries=binaries, datas=datas,
             hiddenimports=hiddenimports, excludes=excludes, noarchive=False)
pyz = PYZ(a.pure)

icon = {"win32": "icon.ico", "darwin": "icon.icns"}.get(sys.platform)
icon = os.path.join(ROOT, "assets", icon) if icon and os.path.exists(os.path.join(ROOT, "assets", icon)) else None

exe = EXE(pyz, a.scripts, [], exclude_binaries=True, name="PinShot", console=False, icon=icon,
          strip=False, upx=False)
coll = COLLECT(exe, a.binaries, a.datas, strip=False, upx=False, name="PinShot")

if sys.platform == "darwin":
    app = BUNDLE(coll, name="PinShot.app", icon=icon, bundle_identifier="com.pinshot.app", version=__version__,
                 info_plist={
                     "CFBundleName": "PinShot", "CFBundleDisplayName": "PinShot",
                     "CFBundleShortVersionString": __version__, "CFBundleVersion": __version__,
                     "LSUIElement": True,                  # menu-bar app, no Dock icon
                     "NSHighResolutionCapable": True,
                     "NSScreenCaptureUsageDescription": "PinShot needs Screen Recording access to take screenshots.",
                     "NSMicrophoneUsageDescription": "PinShot records your microphone when you turn it on for a screen recording.",
                     "NSAppleEventsUsageDescription": "PinShot uses global shortcuts.",
                 })
