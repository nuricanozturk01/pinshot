"""Vector icons drawn with QPainter, so no emoji/font dependency on any OS."""
from __future__ import annotations

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QFont, QIcon, QPainter, QPainterPath, QPen, QPixmap, QPolygonF

S = 24.0


def _pts(*xy: float) -> QPolygonF:
    return QPolygonF([QPointF(xy[i], xy[i + 1]) for i in range(0, len(xy), 2)])


def _undo(p: QPainter, c: QColor) -> None:
    path = QPainterPath(QPointF(5, 9))
    path.cubicTo(10, 4, 19, 7, 19, 15)
    p.drawPath(path)
    p.drawPolyline(_pts(9, 5, 5, 9, 9, 13))


def _text(p: QPainter, c: QColor, s: str, px: int, bold: bool = True) -> None:
    f = QFont()
    f.setPixelSize(px)
    f.setBold(bold)
    p.setFont(f)
    p.drawText(QRectF(0, 0, S, S), Qt.AlignmentFlag.AlignCenter, s)


def _draw(name: str, p: QPainter, c: QColor) -> None:
    nb = Qt.BrushStyle.NoBrush
    if name == "select":
        p.setBrush(c)
        p.drawPolygon(_pts(6, 3, 6, 18, 10, 14, 13, 20, 15, 19, 12, 13, 17, 13))
    elif name == "rect":
        p.drawRect(QRectF(4, 6, 16, 12))
    elif name == "ellipse":
        p.drawEllipse(QRectF(3.5, 5.5, 17, 13))
    elif name == "line":
        p.drawLine(QPointF(5, 19), QPointF(19, 5))
    elif name == "arrow":
        p.drawLine(QPointF(5, 19), QPointF(17, 7))
        p.setBrush(c)
        p.drawPolygon(_pts(20, 4, 12, 6, 18, 12))
    elif name == "pen":
        p.drawPolygon(_pts(4, 20, 5, 15, 16, 4, 20, 8, 9, 19))
    elif name == "marker":
        p.setBrush(QColor(c.red(), c.green(), c.blue(), 110))
        p.drawPolygon(_pts(6, 17, 4, 14, 14, 4, 19, 9, 9, 19))
        p.drawLine(QPointF(3, 21), QPointF(21, 21))
    elif name == "text":
        _text(p, c, "T", 20)
    elif name == "number":
        p.drawEllipse(QRectF(3.5, 3.5, 17, 17))
        _text(p, c, "1", 12)
    elif name == "mosaic":
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(c)
        for i in range(3):
            for j in range(3):
                if (i + j) % 2 == 0:
                    p.drawRect(QRectF(4 + i * 5.3, 4 + j * 5.3, 5.3, 5.3))
    elif name == "blur":
        p.drawEllipse(QRectF(4, 4, 16, 16))
        p.setBrush(QColor(c.red(), c.green(), c.blue(), 120))
        p.drawEllipse(QRectF(8, 8, 8, 8))
    elif name == "undo":
        _undo(p, c)
    elif name == "redo":
        p.translate(S, 0)
        p.scale(-1, 1)
        _undo(p, c)
    elif name == "pin":
        p.setBrush(nb)
        p.drawPolygon(_pts(8, 3, 16, 3, 15, 11, 9, 11))
        p.drawLine(QPointF(6, 11), QPointF(18, 11))
        p.drawLine(QPointF(12, 11), QPointF(12, 21))
    elif name == "ocr":
        for (x, y, dx, dy) in [(4, 4, 1, 1), (20, 4, -1, 1), (4, 20, 1, -1), (20, 20, -1, -1)]:
            p.drawPolyline(_pts(x, y + 4 * dy, x, y, x + 4 * dx, y))
        _text(p, c, "A", 11)
    elif name == "save":
        p.drawRect(QRectF(4, 4, 16, 16))
        p.drawRect(QRectF(8, 4, 8, 5))
        p.drawRect(QRectF(7, 13, 10, 7))
    elif name == "close":
        p.drawLine(QPointF(6, 6), QPointF(18, 18))
        p.drawLine(QPointF(18, 6), QPointF(6, 18))
    elif name == "check":
        p.drawPolyline(_pts(5, 13, 10, 18, 19, 7))
    elif name == "copy":
        p.drawRect(QRectF(8, 8, 11, 12))
        p.drawPolyline(_pts(5, 16, 5, 4, 14, 4))
    elif name == "trash":
        p.drawLine(QPointF(5, 7), QPointF(19, 7))
        p.drawPolygon(_pts(7, 7, 8, 20, 16, 20, 17, 7))
        p.drawPolyline(_pts(9, 7, 9, 4, 15, 4, 15, 7))
    elif name == "fill":
        p.setBrush(c)
        p.drawRect(QRectF(5, 5, 14, 14))
    elif name == "dash":
        pen = QPen(c, 2)
        pen.setDashPattern([2, 2])
        p.setPen(pen)
        p.drawLine(QPointF(3, 12), QPointF(21, 12))
    elif name == "flipx":
        p.drawLine(QPointF(12, 3), QPointF(12, 21))
        p.drawPolygon(_pts(9, 6, 9, 18, 3, 18))
        p.drawPolygon(_pts(15, 6, 15, 18, 21, 18))
    elif name == "plus":
        p.drawLine(QPointF(12, 5), QPointF(12, 19))
        p.drawLine(QPointF(5, 12), QPointF(19, 12))
    elif name == "settings":
        p.drawEllipse(QRectF(8, 8, 8, 8))
        for a in range(0, 360, 45):
            p.save()
            p.translate(12, 12)
            p.rotate(a)
            p.drawLine(QPointF(0, -7), QPointF(0, -10))
            p.restore()


def icon(name: str, color: str = "#ffffff", size: int = 22) -> QIcon:
    ic = QIcon()
    for mode, col in ((QIcon.Mode.Normal, color), (QIcon.Mode.Disabled, "#777777")):
        pm = QPixmap(size * 2, size * 2)
        pm.fill(Qt.GlobalColor.transparent)
        p = QPainter(pm)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.scale(size * 2 / S, size * 2 / S)
        c = QColor(col)
        pen = QPen(c, 1.8, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap, Qt.PenJoinStyle.RoundJoin)
        p.setPen(pen)
        p.setBrush(Qt.BrushStyle.NoBrush)
        _draw(name, p, c)
        p.end()
        pm.setDevicePixelRatio(2)
        ic.addPixmap(pm, mode)
    return ic


def app_icon(size: int = 256) -> QIcon:
    return QIcon(app_pixmap(size))


def app_pixmap(size: int = 256) -> QPixmap:
    pm = QPixmap(size, size)
    pm.fill(Qt.GlobalColor.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    s = size / 100
    p.scale(s, s)
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(QColor("#0a84ff"))
    p.drawRoundedRect(QRectF(4, 4, 92, 92), 22, 22)
    p.setPen(QPen(QColor("white"), 6, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap, Qt.PenJoinStyle.RoundJoin))
    for (x, y, dx, dy) in [(24, 24, 1, 1), (76, 24, -1, 1), (24, 76, 1, -1), (76, 76, -1, -1)]:
        p.drawPolyline(_pts(x, y + 16 * dy, x, y, x + 16 * dx, y))
    p.setPen(Qt.PenStyle.NoPen)
    p.setBrush(QColor("white"))
    p.drawEllipse(QPointF(50, 50), 9, 9)
    p.end()
    return pm


def recording_icon(size: int = 256) -> QIcon:
    """App icon with a red recording dot, shown in the tray while a recording runs."""
    pm = app_pixmap(size)
    p = QPainter(pm)
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    p.setPen(QPen(QColor("white"), size * 0.04))
    p.setBrush(QColor("#ff3b30"))
    r = size * 0.19
    p.drawEllipse(QPointF(size * 0.74, size * 0.26), r, r)
    p.end()
    return QIcon(pm)
