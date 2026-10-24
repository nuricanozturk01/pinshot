"""Pinned image windows: always on top, drag, resize by edges / wheel, flip, rotate, opacity, OCR."""
from __future__ import annotations

import logging
import os

from PySide6.QtCore import QPoint, QPointF, QRect, QRectF, QSize, Qt
from PySide6.QtGui import (QColor, QCursor, QGuiApplication, QImage, QPainter, QPen, QTextDocument,
                           QTransform)
from PySide6.QtWidgets import QApplication, QFileDialog, QMenu, QMessageBox, QWidget

from . import output

log = logging.getLogger(__name__)
_pins: list["PinWindow"] = []
EDGE = 8
MIN_SIDE = 40
CURSORS = {"l": Qt.CursorShape.SizeHorCursor, "r": Qt.CursorShape.SizeHorCursor,
           "t": Qt.CursorShape.SizeVerCursor, "b": Qt.CursorShape.SizeVerCursor,
           "tl": Qt.CursorShape.SizeFDiagCursor, "br": Qt.CursorShape.SizeFDiagCursor,
           "tr": Qt.CursorShape.SizeBDiagCursor, "bl": Qt.CursorShape.SizeBDiagCursor}


class PinWindow(QWidget):
    config = None   # set by the app: Config instance (save dir, border)

    def __init__(self, image: QImage, pos: QPoint | None = None, dpr: float = 1.0):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool)
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
        self.setAttribute(Qt.WidgetAttribute.WA_MacAlwaysShowToolWindow)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setMouseTracking(True)
        self.image = image
        self.dpr = max(1.0, dpr)
        self.border = True if PinWindow.config is None else bool(PinWindow.config["pin_border"])
        self.locked = False
        self._hover = False
        self._drag: QPoint | None = None
        self._resize: str | None = None
        self._g0 = QRect()
        self._press = QPoint()
        self.resize(self.natural_size())
        if pos is not None:
            self.move(pos)
        _pins.append(self)
        self.show()
        self.raise_()

    # ---------------------------------------------------------------- geometry
    def natural_size(self) -> QSize:
        return QSize(max(MIN_SIDE, round(self.image.width() / self.dpr)),
                     max(MIN_SIDE, round(self.image.height() / self.dpr)))

    def _zoom_about(self, factor: float, anchor_global: QPoint | None = None) -> None:
        g = self.geometry()
        w = min(max(MIN_SIDE, round(g.width() * factor)), 16000)
        h = min(max(MIN_SIDE, round(g.height() * factor)), 16000)
        a = anchor_global if anchor_global is not None else g.center()
        fx = (a.x() - g.x()) / max(1, g.width())
        fy = (a.y() - g.y()) / max(1, g.height())
        self.setGeometry(round(a.x() - fx * w), round(a.y() - fy * h), w, h)

    def set_percent(self, pct: int) -> None:
        n = self.natural_size()
        g = self.geometry()
        c = g.center()
        w, h = max(MIN_SIDE, n.width() * pct // 100), max(MIN_SIDE, n.height() * pct // 100)
        self.setGeometry(c.x() - w // 2, c.y() - h // 2, w, h)

    def _edge_at(self, p: QPoint) -> str:
        r, e = self.rect(), EDGE
        s = ("t" if p.y() < e else "b" if p.y() > r.height() - e else "") + \
            ("l" if p.x() < e else "r" if p.x() > r.width() - e else "")
        return s

    def _close_rect(self) -> QRect:
        return QRect(self.width() - 24, 6, 18, 18)

    # ---------------------------------------------------------------- painting
    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform)
        p.drawImage(self.rect(), self.image)
        if self.border:
            p.setPen(QPen(QColor(10, 132, 255, 210), 1))
            p.drawRect(self.rect().adjusted(0, 0, -1, -1))
        if self._hover:
            p.setRenderHint(QPainter.RenderHint.Antialiasing)
            r = self._close_rect()
            p.setPen(Qt.PenStyle.NoPen)
            p.setBrush(QColor(0, 0, 0, 170))
            p.drawEllipse(r)
            p.setPen(QPen(Qt.GlobalColor.white, 1.6))
            c = QRectF(r).center()
            p.drawLine(QPointF(c.x() - 4, c.y() - 4), QPointF(c.x() + 4, c.y() + 4))
            p.drawLine(QPointF(c.x() + 4, c.y() - 4), QPointF(c.x() - 4, c.y() + 4))

    def enterEvent(self, e) -> None:
        self._hover = True
        self.update()

    def leaveEvent(self, e) -> None:
        self._hover = False
        self.update()

    # ---------------------------------------------------------------- mouse
    def mousePressEvent(self, e) -> None:
        if e.button() != Qt.MouseButton.LeftButton:
            return
        pt = e.position().toPoint()
        if self._close_rect().contains(pt):
            self.close()
            return
        self._press = e.globalPosition().toPoint()
        self._g0 = self.geometry()
        edge = self._edge_at(pt)
        if edge:
            self._resize = edge
        elif not self.locked:
            if QGuiApplication.platformName() == "wayland" and self.windowHandle():
                self.windowHandle().startSystemMove()
                return
            self._drag = self._press - self._g0.topLeft()

    def mouseMoveEvent(self, e) -> None:
        g = e.globalPosition().toPoint()
        if self._resize:
            self._do_resize(g, bool(e.modifiers() & Qt.KeyboardModifier.ShiftModifier))
        elif self._drag is not None:
            self.move(g - self._drag)
        else:
            self.setCursor(CURSORS.get(self._edge_at(e.position().toPoint()), Qt.CursorShape.ArrowCursor))

    def mouseReleaseEvent(self, _) -> None:
        self._drag = None
        self._resize = None

    def mouseDoubleClickEvent(self, e) -> None:
        if not self._edge_at(e.position().toPoint()):
            self.close()

    def _do_resize(self, g: QPoint, free: bool) -> None:
        h = self._resize or ""
        g0, d = self._g0, g - self._press
        r = QRect(g0)
        if "l" in h:
            r.setLeft(min(g0.left() + d.x(), g0.right() - MIN_SIDE))
        if "r" in h:
            r.setRight(max(g0.right() + d.x(), g0.left() + MIN_SIDE))
        if "t" in h:
            r.setTop(min(g0.top() + d.y(), g0.bottom() - MIN_SIDE))
        if "b" in h:
            r.setBottom(max(g0.bottom() + d.y(), g0.top() + MIN_SIDE))
        if not free:                      # keep aspect ratio (hold Shift for free resize)
            if len(h) == 2:
                s = max(r.width() / g0.width(), r.height() / g0.height())
                w, hh = round(g0.width() * s), round(g0.height() * s)
                x = g0.x() + g0.width() - w if "l" in h else g0.x()
                y = g0.y() + g0.height() - hh if "t" in h else g0.y()
                r = QRect(x, y, w, hh)
            elif h in ("l", "r"):
                hh = round(g0.height() * r.width() / g0.width())
                r = QRect(r.x(), g0.center().y() - hh // 2, r.width(), hh)
            else:
                w = round(g0.width() * r.height() / g0.height())
                r = QRect(g0.center().x() - w // 2, r.y(), w, r.height())
        self.setGeometry(r)

    def wheelEvent(self, e) -> None:
        d = e.angleDelta().y() or e.angleDelta().x()   # Alt+wheel is horizontal on some platforms
        if not d:
            return
        mods = e.modifiers()
        if mods & Qt.KeyboardModifier.AltModifier or (
                mods & Qt.KeyboardModifier.ControlModifier and mods & Qt.KeyboardModifier.ShiftModifier):
            self.setWindowOpacity(min(1.0, max(0.1, self.windowOpacity() + (0.05 if d > 0 else -0.05))))
        else:                                           # plain wheel and Ctrl+wheel both resize
            self._zoom_about(1.08 ** (d / 120), e.globalPosition().toPoint())

    # ---------------------------------------------------------------- keyboard
    def keyPressEvent(self, e) -> None:
        k, ctrl = e.key(), bool(e.modifiers() & Qt.KeyboardModifier.ControlModifier)
        if k in (Qt.Key.Key_Escape, Qt.Key.Key_Delete):
            self.close()
        elif ctrl and k == Qt.Key.Key_C:
            self.copy()
        elif ctrl and k == Qt.Key.Key_S:
            self.save()
        elif k in (Qt.Key.Key_Plus, Qt.Key.Key_Equal):
            self._zoom_about(1.1)
        elif k == Qt.Key.Key_Minus:
            self._zoom_about(1 / 1.1)
        elif k == Qt.Key.Key_0:
            self.set_percent(100)
        else:
            super().keyPressEvent(e)

    # ---------------------------------------------------------------- actions
    def copy(self) -> None:
        QGuiApplication.clipboard().setImage(self.image)

    def save(self) -> None:
        d = PinWindow.config.save_dir() if PinWindow.config else os.path.expanduser("~")
        path, _ = QFileDialog.getSaveFileName(self, "Kaydet", os.path.join(d, output.timestamp_name()),
                                              "PNG (*.png);;JPEG (*.jpg);;WebP (*.webp)")
        if path:
            ok, err = output.save_image(self.image, path, PinWindow.config["quality"] if PinWindow.config else 92)
            if not ok:
                QMessageBox.warning(self, "PinShot", f"Kaydedilemedi:\n{err}")

    def ocr(self) -> None:
        from .ocr import run_ocr
        run_ocr(self.image, self.pos() + QPoint(20, 20))

    def _transform(self, t: QTransform, swap: bool = False) -> None:
        self.image = self.image.transformed(t, Qt.TransformationMode.SmoothTransformation)
        if swap:
            g = self.geometry()
            c = g.center()
            w, h = g.height(), g.width()
            self.setGeometry(c.x() - w // 2, c.y() - h // 2, w, h)
        self.update()

    def contextMenuEvent(self, e) -> None:
        m = QMenu(self)
        m.addAction("Kopyala\tCtrl+C", self.copy)
        m.addAction("Kaydet…\tCtrl+S", self.save)
        m.addAction("Metni tanı (OCR)", self.ocr)
        m.addSeparator()
        size = m.addMenu("Boyut")
        for pct in (25, 50, 75, 100, 150, 200, 300):
            size.addAction(f"%{pct}", lambda pct=pct: self.set_percent(pct))
        op = m.addMenu("Opaklık")
        for pct in (100, 80, 60, 40, 20):
            op.addAction(f"%{pct}", lambda pct=pct: self.setWindowOpacity(pct / 100))
        tr = m.addMenu("Dönüştür")
        tr.addAction("Yatay çevir", lambda: self._transform(QTransform().scale(-1, 1)))
        tr.addAction("Dikey çevir", lambda: self._transform(QTransform().scale(1, -1)))
        tr.addAction("Saat yönünde döndür", lambda: self._transform(QTransform().rotate(90), True))
        tr.addAction("Saat yönünün tersine döndür", lambda: self._transform(QTransform().rotate(-90), True))
        lock = m.addAction("Konumu kilitle")
        lock.setCheckable(True)
        lock.setChecked(self.locked)
        lock.toggled.connect(lambda v: setattr(self, "locked", v))
        bd = m.addAction("Kenarlık")
        bd.setCheckable(True)
        bd.setChecked(self.border)
        bd.toggled.connect(lambda v: (setattr(self, "border", v), self.update()))
        m.addSeparator()
        m.addAction("Kapat\tEsc", self.close)
        m.addAction("Tüm pinleri kapat", close_all)
        m.exec(e.globalPos())

    def closeEvent(self, e) -> None:
        if self in _pins:
            _pins.remove(self)
        super().closeEvent(e)


def close_all() -> None:
    for p in list(_pins):
        p.close()


def pin_count() -> int:
    return len(_pins)


def _center_on_cursor(size: QSize) -> tuple[QPoint, float]:
    scr = QGuiApplication.screenAt(QCursor.pos()) or QApplication.primaryScreen()
    g = scr.geometry()
    return QPoint(g.center().x() - size.width() // 2, g.center().y() - size.height() // 2), scr.devicePixelRatio()


def render_text(text: str) -> QImage:
    doc = QTextDocument()
    doc.setDefaultStyleSheet("body{color:#1c1c1e}")
    doc.setPlainText(text)
    f = doc.defaultFont()
    f.setPixelSize(15)
    doc.setDefaultFont(f)
    doc.setTextWidth(560)
    w, h = int(doc.idealWidth()) + 28, int(doc.size().height()) + 24
    img = QImage(w * 2, h * 2, QImage.Format.Format_RGB32)
    img.fill(QColor("#fafafa"))
    p = QPainter(img)
    p.scale(2, 2)
    p.translate(14, 12)
    doc.drawContents(p)
    p.end()
    return img


def pin_image(img: QImage) -> PinWindow:
    scr = QGuiApplication.screenAt(QCursor.pos()) or QApplication.primaryScreen()
    dpr = scr.devicePixelRatio()
    pos, _ = _center_on_cursor(QSize(round(img.width() / dpr), round(img.height() / dpr)))
    return PinWindow(img, pos, dpr)


def pin_clipboard() -> bool:
    """Pin image / file(s) / text currently on the clipboard. Returns False when nothing usable."""
    cb = QGuiApplication.clipboard()
    md = cb.mimeData()
    if md is None:
        return False
    if md.hasImage():
        img = cb.image()
        if not img.isNull():
            pin_image(img)
            return True
    if md.hasUrls():
        ok = False
        for u in md.urls():
            if u.isLocalFile():
                ok = pin_file(u.toLocalFile()) or ok
        if ok:
            return True
    if md.hasText() and md.text().strip():
        pin_image(render_text(md.text()))
        return True
    return False


def pin_file(path: str) -> bool:
    img = QImage(path)
    if img.isNull():
        log.warning("cannot pin %s", path)
        return False
    pin_image(img)
    return True
