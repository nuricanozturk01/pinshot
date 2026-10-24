"""JSON-backed user settings stored in the platform config directory."""
from __future__ import annotations

import json
import logging
import os
import sys
import tempfile
from typing import Any

from PySide6.QtCore import QStandardPaths

log = logging.getLogger(__name__)

def default_hotkeys(platform: str | None = None) -> dict[str, str]:
    """macOS: Option turns the key into another character ("Option+A" types "å"), so the global-hotkey library never
    sees an "a" and the shortcut never fires; F3 is Mission Control. Use Control+Shift there."""
    if (platform or sys.platform) == "darwin":
        return {"hotkey_capture": "<ctrl>+<shift>+a", "hotkey_fullscreen": "<ctrl>+<shift>+f",
                "hotkey_repeat": "<ctrl>+<shift>+r", "hotkey_record": "<ctrl>+<shift>+v",
                "hotkey_pin": "<ctrl>+<shift>+p"}
    return {"hotkey_capture": "<ctrl>+<alt>+a", "hotkey_fullscreen": "<ctrl>+<alt>+f",
            "hotkey_repeat": "<ctrl>+<alt>+r", "hotkey_record": "<ctrl>+<alt>+v", "hotkey_pin": "<f3>"}


CONFIG_VERSION = 2          # 2: macOS default shortcuts changed (see default_hotkeys)

DEFAULTS: dict[str, Any] = {
    "config_version": CONFIG_VERSION,
    "save_dir": "",                          # "" -> ~/Pictures/PinShot
    "format": "png",                         # png | jpg | webp
    "quality": 92,
    "auto_save": False,                      # save every capture to save_dir
    **default_hotkeys(),
    "record_format": "mp4",                  # mp4 | gif
    "record_fps": 30,                        # MP4 frame rate
    "record_fps_gif": 15,                    # GIF frame rate (files grow quickly with fps)
    "record_quality_444": True,              # full-resolution colour (sharp coloured text); some players cannot open it
    "record_audio_system": False,            # MP4 only
    "record_audio_mic": False,
    "record_mic_device": "",                 # "" = system default microphone
    "record_max_seconds": 600,               # hard stop: protects disk and CPU from a forgotten recording
    "record_cursor": True,
    "record_gif_width": 1920,                # max output width in px; frames are scaled down while recording
    "record_video_width": 0,                 # 0 = original size
    "island_pos": None,                      # where the user last dragged the recording island
    "color": "#ff3b30",
    "pen_width": 3,
    "font_px": 28,
    "pin_border": True,
    "autostart": False,
    "mac_permission_help_shown": False,     # open the privacy pane for the shortcuts only once
    "last_region": None,                     # [screen_name, x, y, w, h] logical
}


def config_dir() -> str:
    base = QStandardPaths.writableLocation(QStandardPaths.StandardLocation.GenericConfigLocation)
    d = os.path.join(base or os.path.expanduser("~"), "PinShot")
    os.makedirs(d, exist_ok=True)
    return d


def log_dir() -> str:
    base = QStandardPaths.writableLocation(QStandardPaths.StandardLocation.GenericDataLocation)
    d = os.path.join(base or os.path.expanduser("~"), "PinShot", "logs")
    os.makedirs(d, exist_ok=True)
    return d


class Config:
    def __init__(self, path: str | None = None):
        self.path = path or os.path.join(config_dir(), "config.json")
        self.data: dict[str, Any] = dict(DEFAULTS)
        self.load()

    def load(self) -> None:
        try:
            with open(self.path, encoding="utf-8") as f:
                stored = json.load(f)
            if isinstance(stored, dict):
                if sys.platform == "darwin" and int(stored.get("config_version", 1)) < 2:
                    # old macOS builds saved Option-based shortcuts that never worked: drop them so the new defaults apply
                    stored = {k: v for k, v in stored.items() if not k.startswith("hotkey_")}
                self.data.update({k: v for k, v in stored.items() if k in DEFAULTS and k != "config_version"})
        except FileNotFoundError:
            pass
        except (OSError, ValueError) as ex:
            log.warning("config unreadable, using defaults: %s", ex)

    def save(self) -> None:
        try:
            fd, tmp = tempfile.mkstemp(dir=os.path.dirname(self.path), suffix=".tmp")
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                json.dump(self.data, f, indent=2)
            os.replace(tmp, self.path)  # atomic: never leaves a half-written file
        except OSError as ex:
            log.error("cannot save config: %s", ex)

    def __getitem__(self, k: str) -> Any:
        return self.data[k]

    def __setitem__(self, k: str, v: Any) -> None:
        self.data[k] = v

    def save_dir(self) -> str:
        d = self.data["save_dir"] or os.path.join(os.path.expanduser("~"), "Pictures", "PinShot")
        try:
            os.makedirs(d, exist_ok=True)
        except OSError:
            d = os.path.expanduser("~")
        return d
