"""Annotation model: items, hit-testing, editing handles and rendering.

All coordinates are logical (widget) coordinates of the capture overlay; `base` is the
physical-pixel screenshot and `scale` is physical/logical.
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field

from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import (QColor, QFont, QFontMetricsF, QImage, QPainter, QPainterPath, QPen,
                           QPolygonF)

TWO_POINT = ("rect", "ellipse", "mosaic", "blur")
LINE_LIKE = ("line", "arrow")
HIT_TOL = 6.0


@dataclass
class Item:
    kind: str
    color: QColor
    width: int = 3
    pts: list[QPointF] = field(default_factory=list)
    text: str = ""
    fill: bool = False
    dashed: bool = False
    font_px: int = 28

    def clone(self) -> "Item":
        return Item(self.kind, QColor(self.color), self.width, [QPointF(p) for p in self.pts],
                    self.text, self.fill, self.dashed, self.font_px)

    def rect(self) -> QRectF:
        return QRectF(self.pts[0], self.pts[-1]).normalized()

    def translate(self, dx: float, dy: float) -> None:
        self.pts = [QPointF(p.x() + dx, p.y() + dy) for p in self.pts]


# ---------------------------------------------------------------- geometry
def _font(it: Item) -> QFont:
    f = QFont()
    f.setPixelSize(max(6, it.font_px))
    return f


def text_rect(it: Item) -> QRectF:
    r = QFontMetricsF(_font(it)).boundingRect(QRectF(0, 0, 10000, 10000), Qt.AlignmentFlag.AlignLeft
                                              | Qt.AlignmentFlag.AlignTop, it.text or " ")
    return QRectF(it.pts[0].x(), it.pts[0].y(), max(r.width(), 4), max(r.height(), 4))


def number_radius(it: Item) -> float:
    return 9 + it.width * 2


def bounds(it: Item) -> QRectF:
    pad = it.width / 2 + 2
    if it.kind in TWO_POINT:
        return it.rect().adjusted(-pad, -pad, pad, pad)
    if it.kind in LINE_LIKE:
        pad += 4 + it.width * 2 if it.kind == "arrow" else 0
        return QRectF(it.pts[0], it.pts[-1]).normalized().adjusted(-pad, -pad, pad, pad)
    if it.kind in ("pen", "marker"):
        xs, ys = [p.x() for p in it.pts], [p.y() for p in it.pts]
        pad = it.width * (2 if it.kind == "marker" else 0.5) + 2
        return QRectF(QPointF(min(xs) - pad, min(ys) - pad), QPointF(max(xs) + pad, max(ys) + pad))
    if it.kind == "text":
        return text_rect(it).adjusted(-3, -3, 3, 3)
    r = number_radius(it)
    c = it.pts[0]
    return QRectF(c.x() - r, c.y() - r, 2 * r, 2 * r)


def _seg_dist(p: QPointF, a: QPointF, b: QPointF) -> float:
    dx, dy = b.x() - a.x(), b.y() - a.y()
    ln = dx * dx + dy * dy
    t = 0.0 if ln == 0 else max(0.0, min(1.0, ((p.x() - a.x()) * dx + (p.y() - a.y()) * dy) / ln))
    return math.hypot(p.x() - (a.x() + t * dx), p.y() - (a.y() + t * dy))


def hit_test(it: Item, pt: QPointF, tol: float = HIT_TOL) -> bool:
    if it.kind in ("rect", "mosaic", "blur", "ellipse"):
        r = it.rect()
        outer = r.adjusted(-tol, -tol, tol, tol)
        if it.kind in ("mosaic", "blur") or it.fill:
            return outer.contains(pt)
        if not outer.contains(pt):
            return False
        inner = r.adjusted(tol, tol, -tol, -tol)
        if it.kind == "rect":
            return not (inner.width() > 0 and inner.height() > 0 and inner.contains(pt))
        # ellipse outline: compare normalised radius
        cx, cy = r.center().x(), r.center().y()
        rx, ry = max(r.width() / 2, 1e-6), max(r.height() / 2, 1e-6)
        d = math.hypot((pt.x() - cx) / rx, (pt.y() - cy) / ry)
        return abs(d - 1) * min(rx, ry) <= tol
    if it.kind in LINE_LIKE:
        return _seg_dist(pt, it.pts[0], it.pts[-1]) <= tol + it.width / 2
    if it.kind in ("pen", "marker"):
        lim = tol + it.width * (2 if it.kind == "marker" else 0.5)
        if len(it.pts) == 1:
            return math.hypot(pt.x() - it.pts[0].x(), pt.y() - it.pts[0].y()) <= lim
        return any(_seg_dist(pt, a, b) <= lim for a, b in zip(it.pts, it.pts[1:]))
    return bounds(it).contains(pt)


# ---------------------------------------------------------------- handles
Handle = tuple[int | None, int | None]   # (point index owning x, point index owning y)


def handles(it: Item) -> list[tuple[QPointF, Handle]]:
    """Editable handles: (position, spec). Rect-like items get 4 corners, lines get 2 ends."""
    if it.kind in TWO_POINT:
        a, b = it.pts[0], it.pts[-1]
        xl, xr = (0, len(it.pts) - 1) if a.x() <= b.x() else (len(it.pts) - 1, 0)
        yt, yb = (0, len(it.pts) - 1) if a.y() <= b.y() else (len(it.pts) - 1, 0)
        r = it.rect()
        return [(r.topLeft(), (xl, yt)), (r.topRight(), (xr, yt)),
                (r.bottomLeft(), (xl, yb)), (r.bottomRight(), (xr, yb))]
    if it.kind in LINE_LIKE:
        return [(it.pts[0], (0, 0)), (it.pts[-1], (len(it.pts) - 1, len(it.pts) - 1))]
    return []


def drag_handle(it: Item, spec: Handle, pt: QPointF, square: bool = False) -> None:
    xi, yi = spec
    if it.kind in TWO_POINT and square and xi is not None and yi is not None:
        last = len(it.pts) - 1
        ox, oy = it.pts[last if xi == 0 else 0].x(), it.pts[last if yi == 0 else 0].y()
        side = max(abs(pt.x() - ox), abs(pt.y() - oy))
        pt = QPointF(ox + math.copysign(side, pt.x() - ox or 1), oy + math.copysign(side, pt.y() - oy or 1))
    if xi is not None:
        it.pts[xi] = QPointF(pt.x(), it.pts[xi].y())
    if yi is not None:
        it.pts[yi] = QPointF(it.pts[yi].x(), pt.y())


def constrain(kind: str, start: QPointF, pt: QPointF) -> QPointF:
    """Shift-key behaviour: squares/circles for 2-point shapes, 15° steps for lines."""
    dx, dy = pt.x() - start.x(), pt.y() - start.y()
    if kind in TWO_POINT:
        side = max(abs(dx), abs(dy))
        return QPointF(start.x() + math.copysign(side, dx or 1), start.y() + math.copysign(side, dy or 1))
    if kind in LINE_LIKE:
        ang = round(math.atan2(dy, dx) / (math.pi / 12)) * (math.pi / 12)
        ln = math.hypot(dx, dy)
        return QPointF(start.x() + ln * math.cos(ang), start.y() + ln * math.sin(ang))
    return pt


# ---------------------------------------------------------------- rendering
def _arrow_head(p: QPainter, a: QPointF, b: QPointF, width: int, color: QColor) -> None:
    ang = math.atan2(b.y() - a.y(), b.x() - a.x())
    size = 10 + width * 2.5
    left = QPointF(b.x() - size * math.cos(ang - 0.45), b.y() - size * math.sin(ang - 0.45))
    right = QPointF(b.x() - size * math.cos(ang + 0.45), b.y() - size * math.sin(ang + 0.45))
    p.setBrush(color)
    p.setPen(Qt.PenStyle.NoPen)
    p.drawPolygon(QPolygonF([b, left, right]))


def _obscure(base: QImage, r: QRectF, scale: float, kind: str, strength: int) -> QImage:
    phys = QRectF(r.x() * scale, r.y() * scale, r.width() * scale, r.height() * scale).toAlignedRect()
    crop = base.copy(phys)
    if crop.isNull() or crop.width() < 1 or crop.height() < 1:
        return crop
    w, h = crop.width(), crop.height()
    if kind == "mosaic":
        k = max(6, strength * 3)
        small = crop.scaled(max(1, w // k), max(1, h // k), Qt.AspectRatioMode.IgnoreAspectRatio,
                            Qt.TransformationMode.FastTransformation)
        return small.scaled(w, h, Qt.AspectRatioMode.IgnoreAspectRatio, Qt.TransformationMode.FastTransformation)
    k = max(3, strength * 2)
    out = crop
    for _ in range(2):   # two smooth down/up passes approximate a gaussian blur
        small = out.scaled(max(1, w // k), max(1, h // k), Qt.AspectRatioMode.IgnoreAspectRatio,
                           Qt.TransformationMode.SmoothTransformation)
        out = small.scaled(w, h, Qt.AspectRatioMode.IgnoreAspectRatio, Qt.TransformationMode.SmoothTransformation)
    return out


def paint_item(p: QPainter, it: Item, base: QImage, scale: float, number: int = 0) -> None:
    p.save()
    p.setRenderHint(QPainter.RenderHint.Antialiasing)
    pen = QPen(it.color, it.width, Qt.PenStyle.DashLine if it.dashed else Qt.PenStyle.SolidLine,
               Qt.PenCapStyle.RoundCap, Qt.PenJoinStyle.RoundJoin)
    brush = QColor(it.color) if it.fill else Qt.BrushStyle.NoBrush
    if it.kind in ("rect", "ellipse"):
        p.setPen(pen)
        p.setBrush(brush)
        (p.drawRect if it.kind == "rect" else p.drawEllipse)(it.rect())
    elif it.kind in LINE_LIKE:
        a, b = it.pts[0], it.pts[-1]
        p.setPen(pen)
        p.drawLine(a, b)
        if it.kind == "arrow" and a != b:
            _arrow_head(p, a, b, it.width, it.color)
    elif it.kind in ("pen", "marker"):
        path = QPainterPath(it.pts[0])
        for pt in it.pts[1:]:
            path.lineTo(pt)
        if len(it.pts) == 1:
            path.lineTo(QPointF(it.pts[0].x() + 0.01, it.pts[0].y()))
        if it.kind == "marker":
            c = QColor(it.color)
            c.setAlpha(100)
            pen = QPen(c, it.width * 4, Qt.PenStyle.SolidLine, Qt.PenCapStyle.FlatCap, Qt.PenJoinStyle.RoundJoin)
        p.setPen(pen)
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawPath(path)
    elif it.kind == "text":
        p.setFont(_font(it))
        p.setPen(it.color)
        p.drawText(text_rect(it), Qt.AlignmentFlag.AlignLeft | Qt.AlignmentFlag.AlignTop, it.text)
    elif it.kind == "number":
        r, c = number_radius(it), it.pts[0]
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(it.color)
        p.drawEllipse(c, r, r)
        f = QFont()
        f.setBold(True)
        f.setPixelSize(max(6, int(r * 1.15)))
        p.setFont(f)
        lum = 0.299 * it.color.red() + 0.587 * it.color.green() + 0.114 * it.color.blue()
        p.setPen(Qt.GlobalColor.black if lum > 160 else Qt.GlobalColor.white)
        p.drawText(QRectF(c.x() - r, c.y() - r, 2 * r, 2 * r), Qt.AlignmentFlag.AlignCenter, str(number))
    elif it.kind in ("mosaic", "blur"):
        r = it.rect()
        if r.width() >= 1 and r.height() >= 1:
            img = _obscure(base, r, scale, it.kind, it.width)
            if not img.isNull():
                p.drawImage(r, img)
    p.restore()


def paint_items(p: QPainter, items: list[Item], base: QImage, scale: float, skip: Item | None = None) -> None:
    n = 0
    for it in items:
        if it.kind == "number":
            n += 1
        if it is not skip:
            paint_item(p, it, base, scale, n)


def number_of(items: list[Item], target: Item) -> int:
    n = 0
    for it in items:
        if it.kind == "number":
            n += 1
        if it is target:
            return n
    return n


# ---------------------------------------------------------------- undo / redo
GeomState = tuple[list[QPointF], str]


def geom_state(it: Item) -> GeomState:
    """Only geometry + text are tracked: property tweaks (width, colour...) are never undone."""
    return [QPointF(p) for p in it.pts], it.text


def apply_geom(it: Item, st: GeomState) -> None:
    it.pts = [QPointF(p) for p in st[0]]
    it.text = st[1]


class History:
    """Undo/redo of canvas operations: add, delete, move/resize/edit."""

    def __init__(self, items: list[Item]):
        self.items = items
        self._undo: list[tuple] = []
        self._redo: list[tuple] = []

    @property
    def can_undo(self) -> bool:
        return bool(self._undo)

    @property
    def can_redo(self) -> bool:
        return bool(self._redo)

    def _push(self, op: tuple) -> None:
        self._undo.append(op)
        self._redo.clear()

    def added(self, it: Item) -> None:
        self._push(("add", it, len(self.items) - 1))

    def deleted(self, it: Item, index: int) -> None:
        self._push(("del", it, index))

    def modified(self, it: Item, before: GeomState) -> None:
        after = geom_state(it)
        if before[1] != after[1] or [(p.x(), p.y()) for p in before[0]] != [(p.x(), p.y()) for p in after[0]]:
            self._push(("mod", it, before, after))

    def undo(self) -> Item | None:
        if not self._undo:
            return None
        op = self._undo.pop()
        self._redo.append(op)
        kind, it = op[0], op[1]
        if kind == "add":
            if it in self.items:
                self.items.remove(it)
        elif kind == "del":
            self.items.insert(min(op[2], len(self.items)), it)
        else:
            apply_geom(it, op[2])
        return it

    def redo(self) -> Item | None:
        if not self._redo:
            return None
        op = self._redo.pop()
        self._undo.append(op)
        kind, it = op[0], op[1]
        if kind == "add":
            self.items.insert(min(op[2], len(self.items)), it)
        elif kind == "del":
            if it in self.items:
                self.items.remove(it)
        else:
            apply_geom(it, op[3])
        return it
