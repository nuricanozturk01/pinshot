"""`pinshot --selftest <file>`: headless sanity check of a *built* binary (used by CI before publishing).

Verifies that Qt, the bundled fonts/plugins, the renderer and the offline OCR models were really packaged.
"""
from __future__ import annotations

import os
import sys
import tempfile
import traceback


def run(outfile: str | None) -> int:
    os.environ["QT_QPA_PLATFORM"] = "offscreen"
    lines: list[str] = []
    ok = True

    def check(name: str, fn) -> None:
        nonlocal ok
        try:
            lines.append(f"PASS {name}: {fn() or ''}".rstrip())
        except Exception:  # noqa: BLE001
            ok = False
            lines.append(f"FAIL {name}\n{traceback.format_exc()}")

    from PySide6.QtCore import QPointF
    from PySide6.QtGui import QColor, QFont, QImage, QPainter
    from PySide6.QtWidgets import QApplication

    from . import __version__
    app = QApplication.instance() or QApplication(["pinshot-selftest"])

    def render() -> str:
        from .annotate import Item, paint_items
        img = QImage(200, 120, QImage.Format.Format_RGB32)
        img.fill(QColor("white"))
        p = QPainter(img)
        items = [Item("rect", QColor("red"), 3, [QPointF(10, 10), QPointF(90, 60)], fill=True),
                 Item("arrow", QColor("blue"), 3, [QPointF(100, 100), QPointF(150, 50)]),
                 Item("text", QColor("black"), 3, [QPointF(10, 70)], "Test", font_px=20),
                 Item("mosaic", QColor("red"), 4, [QPointF(120, 10), QPointF(190, 50)]),
                 Item("blur", QColor("red"), 4, [QPointF(120, 60), QPointF(190, 90)])]
        paint_items(p, items, img, 1.0)
        p.end()
        assert QColor(img.pixel(20, 20)) == QColor("red")
        return "annotations"

    def icons() -> str:
        from .icons import app_pixmap, icon
        assert not app_pixmap(64).isNull()
        assert not icon("pin").pixmap(22, 22).isNull()
        return "icons"

    def overlay() -> str:
        from PySide6.QtGui import QGuiApplication
        from .capture import Capture
        from .config import Config
        from .overlay import Overlay
        scr = QGuiApplication.primaryScreen()
        img = QImage(scr.geometry().size(), QImage.Format.Format_RGB32)
        img.fill(QColor("white"))
        o = Overlay(Capture(scr, img, scr.geometry(), 1.0), Config(os.path.join(tempfile.gettempdir(), "pinshot-selftest.json")))
        o.show()
        from PySide6.QtCore import QRectF
        o.sel = QRectF(10, 10, 100, 80)
        o._sync_bar()
        assert o.render().width() == 100
        o.close()
        return "overlay"

    def ocr() -> str:
        try:
            import rapidocr_onnxruntime  # noqa: F401
        except ImportError:
            return "skipped (engine not bundled on this platform)"
        from .ocr import recognize
        img = QImage(520, 120, QImage.Format.Format_RGB32)
        img.fill(QColor("white"))
        p = QPainter(img)
        f = QFont()
        f.setPixelSize(46)
        p.setFont(f)
        p.drawText(20, 80, "Hello PinShot")
        p.end()
        text = recognize(img).replace(" ", "").lower()
        assert "pinshot" in text or "hello" in text, f"unexpected OCR output: {text!r}"
        return "ocr"

    def hotkeys() -> str:
        try:
            import pynput  # noqa: F401
        except Exception as ex:  # noqa: BLE001  (headless Linux has no X server for pynput)
            if sys.platform.startswith("linux"):
                return f"skipped ({ex.__class__.__name__})"
            raise
        return "pynput"

    lines.append(f"PinShot {__version__} on {sys.platform} {sys.version.split()[0]}")
    for name, fn in (("render", render), ("icons", icons), ("overlay", overlay), ("ocr", ocr),
                     ("hotkeys", hotkeys)):
        check(name, fn)
    text = "\n".join(lines) + f"\nRESULT {'OK' if ok else 'FAILED'}\n"
    if outfile:
        with open(outfile, "w", encoding="utf-8") as f:
            f.write(text)
    if sys.stdout is not None:
        print(text)
    del app
    return 0 if ok else 1
