"""JSON-backed user settings stored in the platform config directory."""
from __future__ import annotations

import json
import logging
import os
import tempfile
from typing import Any

from PySide6.QtCore import QStandardPaths

log = logging.getLogger(__name__)

DEFAULTS: dict[str, Any] = {
    "save_dir": "",                          # "" -> ~/Pictures/PinShot
    "format": "png",                         # png | jpg | webp
    "quality": 92,
    "auto_save": False,                      # save every capture to save_dir
    "hotkey_capture": "<ctrl>+<alt>+a",
    "hotkey_fullscreen": "<ctrl>+<alt>+f",
    "hotkey_repeat": "<ctrl>+<alt>+r",
    "hotkey_pin": "<f3>",
    "hotkey_record": "<ctrl>+<alt>+v",
    "record_format": "mp4",                  # mp4 | gif
    "record_fps": 15,
    "record_max_seconds": 600,               # hard stop: protects disk and CPU from a forgotten recording
    "record_cursor": True,
    "color": "#ff3b30",
    "pen_width": 3,
    "font_px": 28,
    "pin_border": True,
    "autostart": False,
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
                self.data.update({k: v for k, v in stored.items() if k in DEFAULTS})
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
