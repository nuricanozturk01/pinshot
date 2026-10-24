"""Saving helpers shared by overlay, full-screen capture and pins."""
from __future__ import annotations

import datetime
import logging
import os

from PySide6.QtGui import QImage, QImageWriter

log = logging.getLogger(__name__)
FORMATS = ("png", "jpg", "webp")


def timestamp_name(fmt: str = "png") -> str:
    return datetime.datetime.now().strftime("PinShot_%Y%m%d_%H%M%S") + "." + fmt


def unique_path(path: str) -> str:
    base, ext = os.path.splitext(path)
    n, cand = 1, path
    while os.path.exists(cand):
        n += 1
        cand = f"{base}_{n}{ext}"
    return cand


def save_image(img: QImage, path: str, quality: int = 92) -> tuple[bool, str]:
    """Write `img` honouring the extension. Returns (ok, error message)."""
    ext = os.path.splitext(path)[1].lstrip(".").lower() or "png"
    if ext == "jpeg":
        ext = "jpg"
    try:
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    except OSError as ex:
        return False, str(ex)
    w = QImageWriter(path, ext.encode())
    if ext in ("jpg", "webp"):
        w.setQuality(int(quality))
    out = img.convertToFormat(QImage.Format.Format_RGB888) if ext == "jpg" else img
    if not w.write(out):
        log.error("save failed %s: %s", path, w.errorString())
        return False, w.errorString()
    return True, ""
