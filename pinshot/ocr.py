"""Offline OCR (RapidOCR / ONNX, bundled models) with a non-blocking result window."""
from __future__ import annotations

import logging
import threading

from PySide6.QtCore import QPoint, QThread, QTimer, Qt, Signal
from PySide6.QtGui import QGuiApplication, QImage
from .memory import release_memory
from PySide6.QtWidgets import QHBoxLayout, QLabel, QPlainTextEdit, QPushButton, QVBoxLayout, QWidget

log = logging.getLogger(__name__)
_engine = None
_lock = threading.Lock()
_workers: set = set()
_unload_timer: QTimer | None = None
UNLOAD_AFTER_MS = 300_000      # free the ~100 MB engine after 5 idle minutes (a second OCR is instant before that)


class OcrUnavailable(RuntimeError):
    pass


def _to_array(img: QImage):
    import numpy as np
    im = img.convertToFormat(QImage.Format.Format_RGB888)
    w, h, bpl = im.width(), im.height(), im.bytesPerLine()
    buf = np.frombuffer(im.constBits(), dtype=np.uint8, count=h * bpl).reshape(h, bpl)
    return buf[:, : w * 3].reshape(h, w, 3).copy()


def _make_engine():
    """RapidOCR limited to a few threads: OCR must not saturate every core of the machine."""
    import os
    import tempfile
    from rapidocr_onnxruntime import RapidOCR  # type: ignore
    try:
        from rapidocr_onnxruntime.main import DEFAULT_CFG_PATH
        threads = max(1, min(4, (os.cpu_count() or 2) // 2))
        with open(DEFAULT_CFG_PATH, encoding="utf-8") as f:
            text = f.read()
        text = text.replace("intra_op_num_threads: &intra_nums -1", f"intra_op_num_threads: &intra_nums {threads}")
        text = text.replace("inter_op_num_threads: &inter_nums -1", "inter_op_num_threads: &inter_nums 1")
        path = os.path.join(tempfile.gettempdir(), f"pinshot-rapidocr-{threads}.yaml")
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
        return RapidOCR(config_path=path)
    except Exception:  # noqa: BLE001  (config layout changed: the default engine still works)
        log.warning("could not apply OCR thread limit", exc_info=True)
        return RapidOCR()


def unload_engine() -> None:
    global _engine
    with _lock:
        _engine = None
    release_memory()


def schedule_unload() -> None:
    """Main thread only: (re)start the idle timer that drops the engine."""
    global _unload_timer
    if _unload_timer is None:
        _unload_timer = QTimer()
        _unload_timer.setSingleShot(True)
        _unload_timer.timeout.connect(unload_engine)
    _unload_timer.start(UNLOAD_AFTER_MS)


def recognize(img: QImage) -> str:
    """Blocking OCR; call from a worker thread."""
    global _engine
    try:
        arr = _to_array(img)
        with _lock:
            if _engine is None:
                _engine = _make_engine()
            result, _ = _engine(arr)
        return "\n".join(r[1] for r in (result or []))
    except ImportError:
        pass
    try:  # optional fallback
        import pytesseract  # type: ignore
        from PIL import Image  # type: ignore
        im = img.convertToFormat(QImage.Format.Format_RGB888)
        pil = Image.frombuffer("RGB", (im.width(), im.height()), bytes(im.constBits()), "raw", "RGB",
                               im.bytesPerLine(), 1)
        return pytesseract.image_to_string(pil, lang="eng+tur").strip()
    except ImportError:
        raise OcrUnavailable("OCR motoru bulunamadı.\nKur:  pip install rapidocr_onnxruntime") from None


class _Worker(QThread):
    done = Signal(str)
    failed = Signal(str)

    def __init__(self, img: QImage):
        super().__init__()
        self.img = img

    def run(self) -> None:
        try:
            self.done.emit(recognize(self.img))
        except Exception as ex:  # noqa: BLE001
            log.exception("OCR failed")
            self.failed.emit(str(ex))


class OcrWindow(QWidget):
    _open: set["OcrWindow"] = set()

    def __init__(self, img: QImage, pos: QPoint | None):
        super().__init__(None, Qt.WindowType.Window | Qt.WindowType.WindowStaysOnTopHint)
        self.setWindowTitle("PinShot – OCR")
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
        self.resize(520, 360)
        lay = QVBoxLayout(self)
        self.edit = QPlainTextEdit()
        self.edit.setPlaceholderText("Metin tanınıyor…")
        self.edit.setReadOnly(True)
        self.status = QLabel("Metin tanınıyor…")
        row = QHBoxLayout()
        self.copy_btn = QPushButton("Kopyala")
        self.copy_btn.setEnabled(False)
        self.copy_btn.clicked.connect(self._copy)
        close = QPushButton("Kapat")
        close.clicked.connect(self.close)
        row.addWidget(self.status, 1)
        row.addWidget(self.copy_btn)
        row.addWidget(close)
        lay.addWidget(self.edit)
        lay.addLayout(row)
        if pos is not None:
            self.move(pos)
        OcrWindow._open.add(self)
        self.worker = _Worker(img)
        _workers.add(self.worker)                      # outlives the window if it is closed early
        self.worker.finished.connect(lambda w=self.worker: _workers.discard(w))
        self.worker.done.connect(self._on_done)
        self.worker.failed.connect(self._on_failed)
        # Normal priority on purpose: macOS throttles low-priority (background QoS) threads so hard that OCR can take
        # minutes. The engine already uses only a few threads, so the machine stays responsive.
        self.worker.start()
        self.show()
        self.raise_()                        # a menu-bar (agent) app on macOS does not come to the front by itself
        self.activateWindow()

    def _on_done(self, text: str) -> None:
        schedule_unload()
        self.edit.setReadOnly(False)
        if text.strip():
            self.edit.setPlainText(text)
            self.copy_btn.setEnabled(True)
            self._copy()
        else:
            self.status.setText("Metin bulunamadı.")

    def _on_failed(self, msg: str) -> None:
        schedule_unload()
        self.status.setText("OCR başarısız")
        self.edit.setPlainText(msg)

    def _copy(self) -> None:
        QGuiApplication.clipboard().setText(self.edit.toPlainText())
        self.status.setText(f"{len(self.edit.toPlainText())} karakter – panoya kopyalandı")

    def closeEvent(self, e) -> None:
        OcrWindow._open.discard(self)
        for sig in (self.worker.done, self.worker.failed):
            try:
                sig.disconnect()
            except (RuntimeError, TypeError):
                pass
        super().closeEvent(e)


def run_ocr(img: QImage, pos: QPoint | None = None) -> OcrWindow:
    return OcrWindow(img.copy(), pos)
