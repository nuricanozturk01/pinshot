"""Screen-recording UI: area/screen picker (one per monitor) and the on-screen recording indicator."""
from __future__ import annotations

import sys

from PySide6.QtCore import QPointF, QRect, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QCursor, QFont, QPainter, QPen, QRegion
from PySide6.QtWidgets import (QComboBox, QFrame, QHBoxLayout, QLabel, QPushButton, QToolButton, QWidget)

from .capture import Capture
from .config import Config
from .icons import icon

ACCENT = QColor("#0a84ff")
HANDLE = 8
CSS = """
QFrame#panel{background:#262629;border:1px solid #3a3a3f;border-radius:10px}
QLabel{color:#d0d0d5;font-size:12px}
QComboBox{background:#1b1b1d;color:white;border:1px solid #3a3a3f;border-radius:5px;padding:3px 8px;min-width:54px}
QComboBox QAbstractItemView{background:#1b1b1d;color:white;selection-background-color:#0a84ff}
QPushButton{background:#3a3a3f;color:white;border:0;border-radius:6px;padding:6px 12px}
QPushButton:hover{background:#4a4a50}
QPushButton:disabled{background:#2e2e32;color:#777}
QPushButton#go{background:#e5342a}
QPushButton#go:hover{background:#ff453a}
QPushButton#go:disabled{background:#5a2a27;color:#999}
QToolButton{background:transparent;border:0;border-radius:6px;padding:5px}
QToolButton:hover{background:#3e3e44}
"""


def _show(w: QWidget) -> None:
    w.show() if sys.platform == "darwin" else w.showFullScreen()


class RecordSelector(QWidget):
    """Frozen-desktop picker: drag an area, or press the button to record this whole monitor."""
    closed = Signal()
    started = Signal()
    done = Signal(str)
    chosen = Signal(object)        # dict: screen, region (screen-relative logical QRect), fmt, fps

    def __init__(self, cap: Capture, cfg: Config):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint)
        self.cap, self.cfg = cap, cfg
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setMouseTracking(True)
        self.setCursor(Qt.CursorShape.CrossCursor)
        self.setScreen(cap.screen)
        self.setGeometry(cap.geometry)
        self.sel: QRectF | None = None
        self._mode = ""
        self._anchor = QPointF()
        self._grab = QPointF()
        self._handle = ""

        p = QFrame(self)
        p.setObjectName("panel")
        p.setStyleSheet(CSS)
        p.setCursor(Qt.CursorShape.ArrowCursor)
        lay = QHBoxLayout(p)
        lay.setContentsMargins(12, 8, 12, 8)
        lay.setSpacing(8)
        name = cap.screen.name() or "Ekran"
        lay.addWidget(QLabel(f"<b>{name}</b> · {cap.geometry.width()}×{cap.geometry.height()}"))
        self.fmt = QComboBox()
        self.fmt.addItems(["MP4", "GIF"])
        self.fmt.setCurrentText(str(cfg["record_format"]).upper())
        self.fps = QComboBox()
        for v in (10, 15, 24, 30):
            self.fps.addItem(f"{v} fps", v)
        i = self.fps.findData(int(cfg["record_fps"]))
        self.fps.setCurrentIndex(i if i >= 0 else 1)
        self.fmt.currentTextChanged.connect(self._fmt_changed)
        lay.addWidget(self.fmt)
        lay.addWidget(self.fps)
        self.full_btn = QPushButton("Tüm ekranı kaydet")
        self.full_btn.clicked.connect(self._record_full)
        self.start_btn = QPushButton("● Seçimi kaydet")
        self.start_btn.setObjectName("go")
        self.start_btn.setEnabled(False)
        self.start_btn.clicked.connect(self._record_selection)
        cancel = QToolButton()
        cancel.setIcon(icon("close"))
        cancel.setToolTip("İptal (Esc)")
        cancel.clicked.connect(self.close)
        for w in (self.full_btn, self.start_btn, cancel):
            lay.addWidget(w)
        for w in (self.fmt, self.fps, self.full_btn, self.start_btn, cancel):
            w.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        p.adjustSize()
        p.move((cap.geometry.width() - p.width()) // 2, 28)
        self.panel = p
        self._fmt_changed(self.fmt.currentText())

    def _fmt_changed(self, text: str) -> None:
        gif = text == "GIF"
        self.fps.setEnabled(True)
        if gif and self.fps.currentData() > 15:
            self.fps.setCurrentIndex(self.fps.findData(15))     # GIFs above 15 fps only grow in size

    # ---- painting
    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.drawImage(self.rect(), self.cap.image)
        dim = QColor(0, 0, 0, 125)
        if not self.sel:
            p.fillRect(self.rect(), dim)
            self._hint(p)
            return
        s = self.sel.toRect()
        p.fillRect(QRect(0, 0, self.width(), max(0, s.top())), dim)
        p.fillRect(QRect(0, s.bottom() + 1, self.width(), self.height()), dim)
        p.fillRect(QRect(0, s.top(), max(0, s.left()), s.height()), dim)
        p.fillRect(QRect(s.right() + 1, s.top(), self.width(), s.height()), dim)
        p.setPen(QPen(QColor("#ff453a"), 2))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRect(s.adjusted(0, 0, -1, -1))
        for r in self._handles().values():
            p.setPen(QPen(QColor("#ff453a"), 1))
            p.setBrush(Qt.GlobalColor.white)
            p.drawRect(r)
        sc = self.cap.scale
        self._pill(p, f"{round(self.sel.width() * sc)} × {round(self.sel.height() * sc)}",
                   max(4, s.left()), s.top() - 28 if s.top() > 32 else s.top() + 6)

    def _pill(self, p: QPainter, text: str, x: int, y: int) -> None:
        f = QFont()
        f.setPixelSize(12)
        p.setFont(f)
        r = QRect(x, y, p.fontMetrics().horizontalAdvance(text) + 16, 22)
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor(20, 20, 22, 215))
        p.drawRoundedRect(r, 6, 6)
        p.setPen(Qt.GlobalColor.white)
        p.drawText(r, Qt.AlignmentFlag.AlignCenter, text)

    def _hint(self, p: QPainter) -> None:
        txt = "Kaydedilecek alanı sürükleyin  ·  Enter: bu ekranın tamamı  ·  Esc: iptal"
        f = QFont()
        f.setPixelSize(12)
        w = QFontMetricsF_width(f, txt) + 16
        self._pill(p, txt, int((self.width() - w) / 2), self.panel.y() + self.panel.height() + 14)

    # ---- selection handling
    def _handles(self) -> dict[str, QRect]:
        s = self.sel
        xs = {"l": s.left(), "c": s.center().x(), "r": s.right()}
        ys = {"t": s.top(), "m": s.center().y(), "b": s.bottom()}
        return {yk + xk: QRect(int(x) - HANDLE // 2, int(y) - HANDLE // 2, HANDLE, HANDLE)
                for yk, y in ys.items() for xk, x in xs.items() if (xk, yk) != ("c", "m")}

    def _handle_at(self, pt: QPointF) -> str:
        if self.sel:
            for k, r in self._handles().items():
                if r.adjusted(-4, -4, 4, 4).contains(pt.toPoint()):
                    return k
        return ""

    def _clamp(self, r: QRectF) -> QRectF:
        return r.intersected(QRectF(self.rect()))

    def mousePressEvent(self, e) -> None:
        pt = e.position()
        if e.button() == Qt.MouseButton.RightButton:
            if self.sel:
                self.sel = None
                self.start_btn.setEnabled(False)
                self.update()
            else:
                self.close()
            return
        if e.button() != Qt.MouseButton.LeftButton:
            return
        h = self._handle_at(pt)
        if h:
            self._mode, self._handle = "resize", h
        elif self.sel and self.sel.contains(pt):
            self._mode, self._grab = "move", pt - self.sel.topLeft()
        else:
            self._mode, self._anchor = "new", pt
            self.sel = QRectF(pt, pt)
            self.started.emit()

    def mouseMoveEvent(self, e) -> None:
        pt = e.position()
        m = self._mode
        if m == "new":
            self.sel = self._clamp(QRectF(self._anchor, pt).normalized())
        elif m == "move":
            tl = pt - self._grab
            tl.setX(min(max(0, tl.x()), self.width() - self.sel.width()))
            tl.setY(min(max(0, tl.y()), self.height() - self.sel.height()))
            self.sel.moveTo(tl)
        elif m == "resize":
            r, h = QRectF(self.sel), self._handle
            x, y = min(max(0, pt.x()), self.width()), min(max(0, pt.y()), self.height())
            if "l" in h:
                r.setLeft(x)
            if "r" in h:
                r.setRight(x)
            if "t" in h:
                r.setTop(y)
            if "b" in h:
                r.setBottom(y)
            self.sel = r.normalized()
        elif self.sel:
            self.setCursor(Qt.CursorShape.SizeAllCursor if self.sel.contains(pt) or self._handle_at(pt)
                           else Qt.CursorShape.CrossCursor)
        self.update()

    def mouseReleaseEvent(self, e) -> None:
        if self._mode == "new" and (self.sel.width() < 16 or self.sel.height() < 16):
            self.sel = None
        self._mode = ""
        self.start_btn.setEnabled(self.sel is not None)
        self.update()

    def mouseDoubleClickEvent(self, e) -> None:
        if self.sel and self.sel.contains(e.position()):
            self._record_selection()

    def keyPressEvent(self, e) -> None:
        if e.key() == Qt.Key.Key_Escape:
            self.close()
        elif e.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter):
            self._record_selection() if self.sel else self._record_full()

    # ---- result
    def _emit(self, region: QRect) -> None:
        fmt = self.fmt.currentText().lower()
        fps = int(self.fps.currentData())
        self.cfg["record_format"], self.cfg["record_fps"] = fmt, fps
        self.cfg.save()
        self.hide()
        self.chosen.emit({"screen": self.cap.screen, "region": region, "fmt": fmt, "fps": fps})
        self.close()

    def _record_full(self) -> None:
        self._emit(QRect(0, 0, self.cap.geometry.width(), self.cap.geometry.height()))

    def _record_selection(self) -> None:
        if self.sel:
            self._emit(self.sel.toRect())

    def closeEvent(self, e) -> None:
        from PySide6.QtGui import QImage
        self.cap.image = QImage()
        self.closed.emit()
        super().closeEvent(e)


def QFontMetricsF_width(font: QFont, text: str) -> float:
    from PySide6.QtGui import QFontMetricsF
    return QFontMetricsF(font).horizontalAdvance(text)


class RecordingFrame(QWidget):
    """Red outline just *outside* the recorded area, so it is visible but never part of the video."""

    def __init__(self, global_rect: QRect, margin: int = 3):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool | Qt.WindowType.WindowTransparentForInput)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setAttribute(Qt.WidgetAttribute.WA_MacAlwaysShowToolWindow)
        outer = global_rect.adjusted(-margin, -margin, margin, margin)
        self.setGeometry(outer)
        inner = QRect(margin, margin, global_rect.width(), global_rect.height())
        self.setMask(QRegion(QRect(0, 0, outer.width(), outer.height())).subtracted(QRegion(inner)))

    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.fillRect(self.rect(), QColor("#ff3b30"))


class RecordingBar(QFrame):
    """Small floating stop button with a blinking dot and a timer."""
    stop_requested = Signal()

    def __init__(self):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_MacAlwaysShowToolWindow)
        self.setObjectName("panel")
        self.setStyleSheet(CSS)
        lay = QHBoxLayout(self)
        lay.setContentsMargins(12, 6, 8, 6)
        self.dot = QLabel("●")
        self.dot.setStyleSheet("color:#ff3b30;font-size:14px")
        self.time = QLabel("00:00")
        self.time.setStyleSheet("color:white;font-size:13px;min-width:42px")
        stop = QPushButton("Durdur")
        stop.setObjectName("go")
        stop.clicked.connect(self.stop_requested)
        lay.addWidget(self.dot)
        lay.addWidget(self.time)
        lay.addWidget(stop)
        self.adjustSize()

    def set_elapsed(self, secs: int) -> None:
        self.time.setText(f"{secs // 60:02d}:{secs % 60:02d}")
        self.dot.setVisible(secs % 2 == 0)          # blink once a second; no timer of its own

    def place_outside(self, region: QRect, screen_geo: QRect) -> bool:
        """Put the bar where it will not be recorded. False when the region leaves no room."""
        w, h = self.width(), self.height()
        x = min(max(screen_geo.left() + 8, region.center().x() - w // 2), screen_geo.right() - w - 8)
        if region.bottom() + 8 + h <= screen_geo.bottom():
            y = region.bottom() + 8
        elif region.top() - 8 - h >= screen_geo.top():
            y = region.top() - 8 - h
        else:
            return False
        self.move(x, y)
        return True
