"""Screen grabbing. One Capture per screen (own DPR) so mixed-DPI multi-monitor setups stay exact."""
from __future__ import annotations

import ctypes
import logging
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass

from PySide6.QtCore import QEventLoop, QObject, QRect, QTimer, QUrl, Slot
from PySide6.QtGui import QGuiApplication, QImage, QScreen

log = logging.getLogger(__name__)


@dataclass
class Capture:
    screen: QScreen
    image: QImage        # physical pixels
    geometry: QRect      # screen rect in logical desktop coordinates
    scale: float         # physical / logical


def looks_blank(img: QImage) -> bool:
    """True when the grab is a single flat colour (permission denied / Wayland returns black)."""
    if img.isNull():
        return True
    w, h = img.width(), img.height()
    first = img.pixel(0, 0)
    for i in range(1, 24):
        for j in range(1, 24):
            if img.pixel(w * i // 24 % w, h * j // 24 % h) != first:
                return False
    return True


def mac_screen_access(request: bool = True) -> bool:
    if sys.platform != "darwin":
        return True
    try:
        cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        cg.CGPreflightScreenCaptureAccess.restype = ctypes.c_bool
        if cg.CGPreflightScreenCaptureAccess():
            return True
        if request:
            cg.CGRequestScreenCaptureAccess.restype = ctypes.c_bool
            cg.CGRequestScreenCaptureAccess()
        return False
    except (OSError, AttributeError):
        return True   # API unavailable (old macOS): let the capture attempt decide


def _from_pixmaps() -> list[Capture]:
    out = []
    for s in QGuiApplication.screens():
        img = s.grabWindow(0).toImage().convertToFormat(QImage.Format.Format_RGB32)
        g = s.geometry()
        scale = img.width() / g.width() if g.width() else s.devicePixelRatio()
        out.append(Capture(s, img, g, scale))
    return out


def _split_desktop_image(img: QImage) -> list[Capture]:
    screens = QGuiApplication.screens()
    virt = QRect()
    for s in screens:
        virt = virt.united(s.geometry())
    scale = img.width() / virt.width()
    out = []
    for s in screens:
        g = s.geometry()
        r = QRect(round((g.x() - virt.x()) * scale), round((g.y() - virt.y()) * scale),
                  round(g.width() * scale), round(g.height() * scale))
        out.append(Capture(s, img.copy(r).convertToFormat(QImage.Format.Format_RGB32), g, scale))
    return out


# ------------------------------------------------------------------ Wayland
class _PortalWaiter(QObject):
    def __init__(self) -> None:
        super().__init__()
        self.uri = ""
        self.loop = QEventLoop()

    @Slot("QDBusMessage")
    def response(self, msg) -> None:
        args = msg.arguments()
        if args and args[0] == 0 and len(args) > 1 and isinstance(args[1], dict):
            self.uri = str(args[1].get("uri", ""))
        self.loop.quit()


def _grab_portal() -> QImage | None:
    """xdg-desktop-portal Screenshot (works on GNOME/KDE/wlroots Wayland)."""
    try:
        from PySide6.QtCore import SLOT
        from PySide6.QtDBus import QDBusConnection, QDBusMessage
    except ImportError:
        return None
    bus = QDBusConnection.sessionBus()
    if not bus.isConnected():
        return None
    token = f"pinshot{os.getpid()}_{int(time.time() * 1000)}"
    sender = bus.baseService().lstrip(":").replace(".", "_")
    path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    waiter = _PortalWaiter()
    if not bus.connect("org.freedesktop.portal.Desktop", path, "org.freedesktop.portal.Request", "Response",
                       waiter, SLOT("response(QDBusMessage)")):
        return None
    msg = QDBusMessage.createMethodCall("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
                                        "org.freedesktop.portal.Screenshot", "Screenshot")
    msg.setArguments(["", {"handle_token": token, "interactive": False}])
    reply = bus.call(msg, timeout=5000)
    if reply.type() == QDBusMessage.MessageType.ErrorMessage:
        log.warning("portal screenshot error: %s", reply.errorMessage())
        return None
    QTimer.singleShot(20000, waiter.loop.quit)
    waiter.loop.exec()
    if not waiter.uri:
        return None
    local = QUrl(waiter.uri).toLocalFile()
    img = QImage(local)
    try:
        os.remove(local)
    except OSError:
        pass
    return None if img.isNull() else img


def _grab_cli_tool() -> QImage | None:
    """Fallback for Wayland compositors without a working portal."""
    fd, tmp = tempfile.mkstemp(suffix=".png")
    os.close(fd)
    attempts = [("grim", ["grim", tmp]), ("spectacle", ["spectacle", "-b", "-n", "-f", "-o", tmp]),
                ("gnome-screenshot", ["gnome-screenshot", "-f", tmp])]
    try:
        for exe, cmd in attempts:
            if shutil.which(exe):
                try:
                    subprocess.run(cmd, check=True, timeout=10, capture_output=True)
                    img = QImage(tmp)
                    if not img.isNull():
                        return img
                except (subprocess.SubprocessError, OSError):
                    continue
    finally:
        try:
            os.remove(tmp)
        except OSError:
            pass
    return None


def grab_screens() -> list[Capture]:
    if QGuiApplication.platformName().startswith("wayland"):
        img = _grab_portal() or _grab_cli_tool()
        if img is not None:
            return _split_desktop_image(img)
        log.warning("wayland capture failed; falling back to Qt grab (likely blank)")
    return _from_pixmaps()


def screen_under_cursor(caps: list[Capture], pos) -> Capture:
    return next((c for c in caps if c.geometry.contains(pos)), caps[0])


def compose_region(caps: list[Capture], rect) -> QImage:
    """One image of a desktop rectangle (logical coords) that may span several monitors.

    The output uses the highest scale factor among the monitors involved, so nothing is downsampled; a monitor with a
    lower scale is upscaled. With a single monitor this is an exact pixel copy."""
    from PySide6.QtCore import QRectF
    from PySide6.QtGui import QPainter
    rect = QRectF(rect)
    parts = []
    for c in caps:
        inter = QRectF(c.geometry).intersected(rect)
        if inter.width() > 0 and inter.height() > 0 and not c.image.isNull():
            parts.append((c, inter))
    scale = max((c.scale for c, _ in parts), default=1.0)
    out = QImage(max(1, round(rect.width() * scale)), max(1, round(rect.height() * scale)), QImage.Format.Format_RGB32)
    out.fill(0)
    p = QPainter(out)
    p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
    for c, inter in parts:
        src = QRect(round((inter.x() - c.geometry.x()) * c.scale), round((inter.y() - c.geometry.y()) * c.scale),
                    max(1, round(inter.width() * c.scale)), max(1, round(inter.height() * c.scale)))
        dx, dy = round((inter.x() - rect.x()) * scale), round((inter.y() - rect.y()) * scale)
        if abs(c.scale - scale) < 1e-6:                    # same scale: exact copy, no resampling blur
            p.drawImage(dx, dy, c.image, src.x(), src.y(), src.width(), src.height())
        else:
            p.drawImage(QRect(dx, dy, max(1, round(inter.width() * scale)), max(1, round(inter.height() * scale))),
                        c.image, src)
    p.end()
    return out
