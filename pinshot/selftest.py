"""`pinshot --selftest <file>`: headless sanity check of a *built* binary (used by CI before publishing).

Verifies that Qt, the bundled fonts/plugins, the renderer and the offline OCR models were really packaged.
"""
from __future__ import annotations

import os
import sys
import tempfile
import traceback


def make_text_image(text: str = "Hello PinShot", size: int = 46):
    """White image with black text drawn by Pillow's bundled font (independent of system / Qt fonts)."""
    from PIL import Image, ImageDraw, ImageFont
    from PySide6.QtGui import QImage
    font = ImageFont.load_default(size=size)
    probe = ImageDraw.Draw(Image.new("RGB", (1, 1)))
    l, t, r, b = probe.textbbox((0, 0), text, font=font)
    im = Image.new("RGB", (r - l + 60, b - t + 60), "white")
    ImageDraw.Draw(im).text((30 - l, 30 - t), text, fill="black", font=font)
    qi = QImage(im.tobytes(), im.width, im.height, im.width * 3, QImage.Format.Format_RGB888)
    return qi.copy()


def ocr_diagnostics(img) -> str:
    """Raw engine behaviour for several input variants; only used to explain a failing OCR check."""
    out: list[str] = []

    def step(name: str, fn) -> None:
        try:
            out.append(f"  {name}: {fn()}")
        except Exception as ex:  # noqa: BLE001
            out.append(f"  {name}: EXC {type(ex).__name__}: {ex}")

    import io
    import numpy as np
    from .ocr import _to_array
    arr = _to_array(img)
    from rapidocr_onnxruntime import RapidOCR
    eng = RapidOCR()

    def mod(name: str) -> str:
        import importlib
        m = importlib.import_module(name)
        return getattr(m, "__version__", "?")

    for n in ("cv2", "onnxruntime", "numpy", "shapely", "pyclipper", "PIL"):
        step(f"version {n}", lambda n=n: mod(n))
    step("onnx providers", lambda: __import__("onnxruntime").get_available_providers())
    step("array", lambda: f"{arr.shape} {arr.dtype} min={arr.min()} max={arr.max()} dark={(arr < 128).sum()} "
                          f"contiguous={arr.flags['C_CONTIGUOUS']}")
    step("ndarray", lambda: eng(arr)[0])
    step("ndarray det-only", lambda: eng(arr, use_det=True, use_cls=False, use_rec=False)[0])
    step("ndarray x2", lambda: eng(np.ascontiguousarray(np.kron(arr, np.ones((2, 2, 1), dtype=np.uint8))))[0])
    step("bgr", lambda: eng(np.ascontiguousarray(arr[:, :, ::-1]))[0])

    def from_png():
        from PIL import Image
        buf = io.BytesIO()
        Image.fromarray(arr).save(buf, "PNG")
        return eng(buf.getvalue())[0]
    step("png bytes", from_png)

    def from_file():
        import tempfile, os
        from PIL import Image
        f = os.path.join(tempfile.gettempdir(), "pinshot-ocr-diag.png")
        Image.fromarray(arr).save(f)
        return eng(f)[0]
    step("png file", from_file)
    step("model dir", lambda: sorted(__import__("os").listdir(__import__("os").path.join(
        __import__("os").path.dirname(__import__("rapidocr_onnxruntime").__file__), "models"))))
    return "\n".join(out)


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
        import platform
        try:
            import rapidocr_onnxruntime  # noqa: F401
        except ImportError:
            if sys.platform == "win32" and platform.machine().upper() == "ARM64":
                return "skipped (no onnxruntime wheels for Windows ARM64)"
            raise                                   # a missing engine anywhere else is a packaging bug
        from .ocr import recognize
        img = make_text_image()
        text = recognize(img).replace(" ", "").lower()
        if not ("pinshot" in text or "hello" in text):
            raise AssertionError(f"unexpected OCR output: {text!r}\nDIAGNOSTICS:\n{ocr_diagnostics(img)}")
        return "ocr"

    def video() -> str:
        import platform
        import time
        from .recorder import Encoder, find_ffmpeg
        exe = find_ffmpeg()
        if exe is None:
            if sys.platform == "win32" and platform.machine().upper() == "ARM64":
                return "skipped (no bundled ffmpeg for Windows ARM64)"
            raise AssertionError("ffmpeg is missing from the build")
        w, h = 161, 91                                     # odd size on purpose: must be handled
        res = []
        for fmt, magic in (("mp4", b"ftyp"), ("gif", b"GIF8")):
            out = os.path.join(tempfile.gettempdir(), f"pinshot-selftest.{fmt}")
            enc = Encoder(exe, (w, h), 15, fmt, out)
            for i in range(20):
                enc.push(bytes([(i * 12) % 256, 90, 200, 255]) * (w * h))
                time.sleep(1 / 15)
            ok, err = enc.finish()
            assert ok, f"{fmt} encode failed: {err}"
            with open(out, "rb") as f:
                head = f.read(12)
            os.remove(out)
            assert magic in head, f"{fmt}: unexpected file header {head!r}"
            res.append(fmt)
        return "+".join(res)

    def audio() -> str:
        import math
        import struct
        import time
        import platform
        from .audio import RATE, Track, probe
        from .recorder import Encoder, find_ffmpeg
        probe()                                             # must not raise, with or without devices
        exe = find_ffmpeg()
        if exe is None:
            if sys.platform == "win32" and platform.machine().upper() == "ARM64":
                return "skipped (no bundled ffmpeg)"
            raise AssertionError("ffmpeg is missing from the build")
        pcm = os.path.join(tempfile.gettempdir(), "pinshot-selftest.pcm")
        with open(pcm, "wb") as f:
            f.write(b"".join(struct.pack("<hh", *([int(9000 * math.sin(2 * math.pi * 440 * i / RATE))] * 2))
                             for i in range(RATE)))
        out = os.path.join(tempfile.gettempdir(), "pinshot-selftest-audio.mp4")
        enc = Encoder(exe, (160, 90), 15, "mp4", out, 0, with_audio=True, hq444=True)
        t0 = time.monotonic()
        for i in range(15):
            enc.push(bytes([(i * 15) % 256, 80, 190, 255]) * (160 * 90))
            time.sleep(1 / 15)
        ok, err = enc.finish([Track("t", pcm, 2, started=t0)], t0)
        os.remove(pcm)
        assert ok, f"audio mux failed: {err}"
        import subprocess
        info = subprocess.run([exe, "-hide_banner", "-i", out], capture_output=True, text=True).stderr
        os.remove(out)
        assert "Audio: aac" in info and "Video: h264" in info, info[-300:]
        return "mux"

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
                     ("video", video), ("audio", audio), ("hotkeys", hotkeys)):
        check(name, fn)
    text = "\n".join(lines) + f"\nRESULT {'OK' if ok else 'FAILED'}\n"
    if outfile:
        with open(outfile, "w", encoding="utf-8") as f:
            f.write(text)
    if sys.stdout is not None:
        print(text)
    del app
    return 0 if ok else 1
