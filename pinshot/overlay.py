"""Capture overlay (one per screen): select a region, annotate, then copy / save / pin / OCR."""
from __future__ import annotations

import logging
import os
import sys

from PySide6.QtCore import QPoint, QPointF, QRect, QRectF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QCursor, QFont, QFontMetricsF, QGuiApplication, QImage, QPainter, QPen
from PySide6.QtWidgets import QFileDialog, QPlainTextEdit, QWidget

from . import output
from .annotate import History, Item, bounds, constrain, drag_handle, geom_state, handles, hit_test, paint_items
from .capture import Capture
from .config import Config
from .pin import PinWindow
from .toolbar import PROPS, SHORTCUTS, Toolbar

log = logging.getLogger(__name__)
HANDLE = 8
ACCENT = QColor("#0a84ff")
SEL_CURSORS = {"tl": Qt.CursorShape.SizeFDiagCursor, "br": Qt.CursorShape.SizeFDiagCursor,
               "tr": Qt.CursorShape.SizeBDiagCursor, "bl": Qt.CursorShape.SizeBDiagCursor,
               "tc": Qt.CursorShape.SizeVerCursor, "bc": Qt.CursorShape.SizeVerCursor,
               "ml": Qt.CursorShape.SizeHorCursor, "mr": Qt.CursorShape.SizeHorCursor}


class InlineEditor(QPlainTextEdit):
    """Borderless text box shown on the canvas while typing a text annotation."""
    committed = Signal()

    def keyPressEvent(self, e) -> None:
        plain_enter = e.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter) \
            and not e.modifiers() & Qt.KeyboardModifier.ShiftModifier
        if plain_enter or e.key() == Qt.Key.Key_Escape:
            self.committed.emit()
        else:
            super().keyPressEvent(e)

    def focusOutEvent(self, e) -> None:
        super().focusOutEvent(e)
        self.committed.emit()


def _show(w: QWidget) -> None:
    # macOS: showFullScreen() creates a separate Space; a frameless window is enough there.
    w.show() if sys.platform == "darwin" else w.showFullScreen()


class Overlay(QWidget):
    closed = Signal()
    started = Signal()          # user began a selection on this screen
    done = Signal(str)          # status text for a tray notification

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
        self.items: list[Item] = []
        self.history = History(self.items)
        self.tool = "select"
        self.selected: Item | None = None
        self.color = QColor(cfg["color"])
        self.pen_w = int(cfg["pen_width"])
        self.fill = False
        self.dashed = False
        self.font_px = int(cfg["font_px"])

        self._mode = ""
        self._anchor = QPointF()
        self._cur: Item | None = None
        self._mouse = QPointF()
        self._last = QPointF()
        self._grab_off = QPointF()
        self._sel_handle = ""
        self._spec: tuple = (None, None)
        self._before = None
        self._editor: InlineEditor | None = None
        self._edit_item: Item | None = None
        self._edit_pos = QPointF()
        self._edit_before = None
        self._hex = True
        self._finished = False

        self.bar = Toolbar(self)
        self.bar.setCursor(Qt.CursorShape.ArrowCursor)
        self.bar.hide()
        self.bar.tool_changed.connect(self.set_tool)
        self.bar.color_changed.connect(self._on_color)
        self.bar.width_changed.connect(self._on_width)
        self.bar.fill_changed.connect(self._on_fill)
        self.bar.dash_changed.connect(self._on_dash)
        self.bar.font_changed.connect(self._on_font)
        self.bar.action.connect(self._on_action)

    # ================================================================ toolbar sync
    def _sync_bar(self) -> None:
        it = self.selected if self.tool == "select" else None
        kind = it.kind if it else (None if self.tool == "select" else self.tool)
        self.bar.configure(kind)
        if it:
            self.bar.set_values(it.color, it.width, it.fill, it.dashed, it.font_px)
        else:
            self.bar.set_values(self.color, self.pen_w, self.fill, self.dashed, self.font_px)
        self.bar.set_tool(self.tool)
        self.bar.set_history(self.history.can_undo, self.history.can_redo, self.selected is not None)
        self._place_bar()

    def _place_bar(self) -> None:
        if not self.sel or self._mode in ("sel-new", "sel-move", "sel-resize"):
            self.bar.hide()
            return
        self.bar.adjustSize()
        w, h = self.bar.width(), self.bar.height()
        r = self.sel.toRect()
        x = max(8, min(r.right() - w + 1, self.width() - w - 8))
        y = r.bottom() + 10
        if y + h > self.height() - 8:
            y = r.top() - h - 10
        if y < 8:
            y = max(8, r.bottom() - h - 10)
        self.bar.move(x, y)
        self.bar.show()
        self.bar.raise_()

    def set_tool(self, name: str) -> None:
        self._commit_text()
        self.tool = name
        if name != "select":
            self.selected = None
        self.setCursor(Qt.CursorShape.ArrowCursor if name == "select" else Qt.CursorShape.CrossCursor)
        self._sync_bar()
        self.update()

    def _target(self) -> Item | None:
        """Item that property changes apply to (select mode only). Property edits are not undoable."""
        return self.selected if self.tool == "select" else None

    def _on_color(self, c: QColor) -> None:
        self.color = QColor(c)
        self.cfg["color"] = c.name()
        if (t := self._target()) and "color" in PROPS.get(t.kind, ()):
            t.color = QColor(c)
        self._restyle_editor()
        self.update()

    def _on_width(self, v: int) -> None:
        if (t := self._target()) and "width" in PROPS.get(t.kind, ()):
            t.width = v
        else:
            self.pen_w = v
            self.cfg["pen_width"] = v
        self.update()

    def _on_fill(self, v: bool) -> None:
        if t := self._target():
            t.fill = v
        else:
            self.fill = v
        self.update()

    def _on_dash(self, v: bool) -> None:
        if t := self._target():
            t.dashed = v
        else:
            self.dashed = v
        self.update()

    def _on_font(self, v: int) -> None:
        if (t := self._target()) and t.kind == "text":
            t.font_px = v
        else:
            self.font_px = v
            self.cfg["font_px"] = v
        self._restyle_editor()
        self.update()

    def _on_action(self, name: str) -> None:
        {"undo": self.undo, "redo": self.redo, "delete": self.delete_selected, "pin": self.pin,
         "ocr": self.ocr, "save": self.save, "copy": self.copy, "cancel": self.close}[name]()

    # ================================================================ painting
    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.drawImage(self.rect(), self.cap.image)
        dim = QColor(0, 0, 0, 120)
        if not self.sel:
            p.fillRect(self.rect(), dim)
            if self._mode == "":
                self._draw_hint(p)
        else:
            s = self.sel.toRect()
            p.fillRect(QRect(0, 0, self.width(), max(0, s.top())), dim)
            p.fillRect(QRect(0, s.bottom() + 1, self.width(), self.height()), dim)
            p.fillRect(QRect(0, s.top(), max(0, s.left()), s.height()), dim)
            p.fillRect(QRect(s.right() + 1, s.top(), self.width(), s.height()), dim)
            p.save()
            p.setClipRect(s)
            paint_items(p, self.items + ([self._cur] if self._cur else []), self.cap.image, self.cap.scale,
                        skip=self._edit_item)
            p.restore()
            self._draw_selected_item(p)
            p.setPen(QPen(ACCENT, 1))
            p.setBrush(Qt.BrushStyle.NoBrush)
            p.drawRect(s.adjusted(0, 0, -1, -1))
            self._draw_sel_handles(p)
            self._draw_size_label(p, s)
        if self.sel is None or self._mode == "sel-new":
            self._draw_loupe(p)

    def _draw_pill(self, p: QPainter, text: str, x: int, y: int) -> QRect:
        f = QFont()
        f.setPixelSize(12)
        p.setFont(f)
        r = QRect(x, y, p.fontMetrics().horizontalAdvance(text) + 16, 22)
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor(20, 20, 22, 215))
        p.drawRoundedRect(r, 6, 6)
        p.setPen(Qt.GlobalColor.white)
        p.drawText(r, Qt.AlignmentFlag.AlignCenter, text)
        return r

    def _draw_hint(self, p: QPainter) -> None:
        txt = "Alanı seçmek için sürükle  ·  Ctrl+A tüm ekran  ·  C renk kopyala  ·  Esc iptal"
        f = QFont()
        f.setPixelSize(12)
        w = QFontMetricsF(f).horizontalAdvance(txt) + 16
        self._draw_pill(p, txt, int((self.width() - w) / 2), 24)

    def _draw_size_label(self, p: QPainter, s: QRect) -> None:
        sc = self.cap.scale
        txt = f"{round(self.sel.width() * sc)} × {round(self.sel.height() * sc)}"
        y = s.top() - 28 if s.top() > 32 else s.top() + 6
        self._draw_pill(p, txt, max(4, s.left()), y)

    def _draw_sel_handles(self, p: QPainter) -> None:
        for r in self._sel_handles().values():
            p.setPen(QPen(ACCENT, 1))
            p.setBrush(Qt.GlobalColor.white)
            p.drawRect(r)

    def _draw_selected_item(self, p: QPainter) -> None:
        it = self.selected
        if not it or self.tool != "select" or it is self._edit_item:
            return
        p.save()
        p.setPen(QPen(ACCENT, 1, Qt.PenStyle.DashLine))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRect(bounds(it))
        for pos, _ in handles(it):
            p.setPen(QPen(ACCENT, 1))
            p.setBrush(Qt.GlobalColor.white)
            p.drawEllipse(pos, 4.5, 4.5)
        p.restore()

    def _pixel_at(self, pt: QPointF) -> QColor:
        x, y = int(pt.x() * self.cap.scale), int(pt.y() * self.cap.scale)
        img = self.cap.image
        return QColor(img.pixel(x, y)) if 0 <= x < img.width() and 0 <= y < img.height() else QColor(0, 0, 0)

    def _color_text(self, c: QColor) -> str:
        return c.name().upper() if self._hex else f"rgb({c.red()}, {c.green()}, {c.blue()})"

    def _draw_loupe(self, p: QPainter) -> None:
        n, z = 15, 8
        sc = self.cap.scale
        cx, cy = int(self._mouse.x() * sc), int(self._mouse.y() * sc)
        img = QImage(n, n, QImage.Format.Format_RGB32)
        img.fill(0)
        src = self.cap.image
        for i in range(n):
            for j in range(n):
                x, y = cx - n // 2 + i, cy - n // 2 + j
                if 0 <= x < src.width() and 0 <= y < src.height():
                    img.setPixel(i, j, src.pixel(x, y))
        size = n * z
        pos = QPoint(int(self._mouse.x()) + 22, int(self._mouse.y()) + 22)
        if pos.x() + size + 4 > self.width():
            pos.setX(int(self._mouse.x()) - 22 - size)
        if pos.y() + size + 56 > self.height():
            pos.setY(int(self._mouse.y()) - 58 - size)
        box = QRect(pos, QSize(size, size))
        p.drawImage(box, img)
        p.setPen(QPen(ACCENT, 2))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRect(box)
        p.setPen(QPen(Qt.GlobalColor.white, 1))
        p.drawRect(QRect(pos.x() + (n // 2) * z, pos.y() + (n // 2) * z, z, z))
        self._draw_pill(p, self._color_text(self._pixel_at(self._mouse)), pos.x(), pos.y() + size + 4)
        self._draw_pill(p, f"{cx}, {cy}   C kopyala · Shift biçim", pos.x(), pos.y() + size + 28)

    # ================================================================ geometry helpers
    def _sel_handles(self) -> dict[str, QRect]:
        s = self.sel
        xs = {"l": s.left(), "c": s.center().x(), "r": s.right()}
        ys = {"t": s.top(), "m": s.center().y(), "b": s.bottom()}
        return {yk + xk: QRect(int(x) - HANDLE // 2, int(y) - HANDLE // 2, HANDLE, HANDLE)
                for yk, y in ys.items() for xk, x in xs.items() if (xk, yk) != ("c", "m")}

    def _sel_handle_at(self, pt: QPointF) -> str:
        if not self.sel:
            return ""
        for k, r in self._sel_handles().items():
            if r.adjusted(-4, -4, 4, 4).contains(pt.toPoint()):
                return k
        return ""

    def _item_at(self, pt: QPointF) -> Item | None:
        if not self.sel or not self.sel.contains(pt):
            return None
        return next((it for it in reversed(self.items) if hit_test(it, pt)), None)

    def _item_handle_at(self, pt: QPointF):
        if self.selected:
            for pos, spec in handles(self.selected):
                if abs(pos.x() - pt.x()) <= 8 and abs(pos.y() - pt.y()) <= 8:
                    return spec
        return None

    def _clamp_rect(self, r: QRectF) -> QRectF:
        return r.intersected(QRectF(self.rect()))

    # ================================================================ mouse
    def mousePressEvent(self, e) -> None:
        pt = e.position()
        if self._editor:
            self._commit_text()
            return
        if e.button() == Qt.MouseButton.RightButton:
            self._right_click()
            return
        if e.button() != Qt.MouseButton.LeftButton:
            return
        if not self.sel:
            self._mode, self._anchor = "sel-new", pt
            self.sel = QRectF(pt, pt)
            self.started.emit()
            return
        h = self._sel_handle_at(pt)
        if h:
            self._mode, self._sel_handle = "sel-resize", h
            self._place_bar()
            return
        if self.tool == "select":
            spec = self._item_handle_at(pt)
            if spec:
                self._mode, self._spec, self._before = "item-handle", spec, geom_state(self.selected)
                return
            it = self._item_at(pt)
            if it:
                self.selected = it
                self._mode, self._last, self._before = "item-move", pt, geom_state(it)
                self._sync_bar()
                self.update()
                return
            if self.sel.contains(pt):
                self.selected = None
                self._mode, self._grab_off = "sel-move", pt - self.sel.topLeft()
                self._sync_bar()
            return
        if not self.sel.contains(pt):
            return
        if self.tool == "text":
            self._begin_text(pt)
            return
        if self.tool == "number":
            self._add(self._new_item("number", [pt]))
            self.update()
            return
        pts = [pt] if self.tool in ("pen", "marker") else [pt, pt]
        self._mode, self._anchor = "draw", pt
        self._cur = self._new_item(self.tool, pts)

    def mouseMoveEvent(self, e) -> None:
        pt = e.position()
        self._mouse = pt
        shift = bool(e.modifiers() & Qt.KeyboardModifier.ShiftModifier)
        m = self._mode
        if m == "sel-new":
            self.sel = self._clamp_rect(QRectF(self._anchor, pt).normalized())
        elif m == "sel-move":
            tl = pt - self._grab_off
            tl.setX(min(max(0, tl.x()), self.width() - self.sel.width()))
            tl.setY(min(max(0, tl.y()), self.height() - self.sel.height()))
            self.sel.moveTo(tl)
        elif m == "sel-resize":
            r, h = QRectF(self.sel), self._sel_handle
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
        elif m == "draw" and self._cur:
            if self._cur.kind in ("pen", "marker"):
                self._cur.pts.append(pt)
            else:
                self._cur.pts[-1] = constrain(self._cur.kind, self._anchor, pt) if shift else pt
        elif m == "item-move" and self.selected:
            self.selected.translate(pt.x() - self._last.x(), pt.y() - self._last.y())
            self._last = pt
        elif m == "item-handle" and self.selected:
            drag_handle(self.selected, self._spec, pt, shift)
        else:
            self._update_cursor(pt)
        self.update()

    def mouseReleaseEvent(self, e) -> None:
        m, self._mode = self._mode, ""
        if m == "sel-new":
            if self.sel.width() < 4 or self.sel.height() < 4:
                self.sel = None
        elif m == "draw" and self._cur:
            it, self._cur = self._cur, None
            r = it.rect()
            if it.kind in ("pen", "marker", "line", "arrow") or r.width() >= 3 or r.height() >= 3:
                self._add(it)
        elif m in ("item-move", "item-handle") and self.selected and self._before:
            self.history.modified(self.selected, self._before)
        self._sync_bar()
        self.update()

    def mouseDoubleClickEvent(self, e) -> None:
        pt = e.position()
        it = self._item_at(pt)
        if it and it.kind == "text" and self.tool == "select":
            self._begin_text(it.pts[0], it)
        elif self.sel and self.tool == "select" and not it and self.sel.contains(pt):
            self.copy()

    def _right_click(self) -> None:
        if self.tool != "select":
            self.set_tool("select")
        elif self.selected:
            self.selected = None
            self._sync_bar()
        elif self.sel:
            self.sel, self.selected = None, None
            self.items.clear()
            self.history = History(self.items)
            self.bar.hide()
        else:
            self.close()
        self.update()

    def _update_cursor(self, pt: QPointF) -> None:
        cur = Qt.CursorShape.CrossCursor
        if self.sel:
            h = self._sel_handle_at(pt)
            if h:
                cur = SEL_CURSORS[h]
            elif self.tool == "select":
                if self._item_handle_at(pt):
                    cur = Qt.CursorShape.PointingHandCursor
                elif self._item_at(pt):
                    cur = Qt.CursorShape.SizeAllCursor
                elif self.sel.contains(pt):
                    cur = Qt.CursorShape.OpenHandCursor
                else:
                    cur = Qt.CursorShape.ArrowCursor
            elif not self.sel.contains(pt):
                cur = Qt.CursorShape.ArrowCursor
            elif self.tool == "text":
                cur = Qt.CursorShape.IBeamCursor
        self.setCursor(cur)

    # ================================================================ items
    def _new_item(self, kind: str, pts: list[QPointF]) -> Item:
        return Item(kind, QColor(self.color), self.pen_w, pts, "", self.fill, self.dashed, self.font_px)

    def _add(self, it: Item) -> None:
        self.items.append(it)
        self.history.added(it)
        self._sync_bar()

    def delete_selected(self) -> None:
        it = self.selected
        if it and it in self.items:
            idx = self.items.index(it)
            self.items.remove(it)
            self.history.deleted(it, idx)
            self.selected = None
            self._sync_bar()
            self.update()

    def undo(self) -> None:
        self._commit_text()
        it = self.history.undo()
        if it is self.selected and it not in self.items:
            self.selected = None
        self._sync_bar()
        self.update()

    def redo(self) -> None:
        self._commit_text()
        self.history.redo()
        self._sync_bar()
        self.update()

    # ================================================================ inline text
    def _begin_text(self, pos: QPointF, item: Item | None = None) -> None:
        self._commit_text()
        self._edit_item = item
        self._edit_pos = QPointF(item.pts[0]) if item else QPointF(pos)
        self._edit_before = geom_state(item) if item else None
        ed = InlineEditor(self)
        ed.setFrameStyle(0)
        ed.setLineWrapMode(QPlainTextEdit.LineWrapMode.NoWrap)
        ed.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        ed.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        ed.document().setDocumentMargin(0)
        ed.setPlainText(item.text if item else "")
        self._editor = ed
        self._restyle_editor()
        ed.textChanged.connect(self._fit_editor)
        ed.committed.connect(self._commit_text)
        ed.show()
        ed.setFocus()
        if item:
            ed.selectAll()
        self._fit_editor()
        self.update()

    def _editor_look(self) -> tuple[QColor, int]:
        it = self._edit_item
        return (it.color, it.font_px) if it else (self.color, self.font_px)

    def _restyle_editor(self) -> None:
        if not self._editor:
            return
        color, px = self._editor_look()
        f = QFont()
        f.setPixelSize(px)
        self._editor.setFont(f)
        self._editor.setStyleSheet(f"QPlainTextEdit{{background:rgba(255,255,255,40);color:{color.name()};"
                                   f"border:1px dashed {color.name()};padding:0}}")
        self._fit_editor()

    def _fit_editor(self) -> None:
        ed = self._editor
        if not ed:
            return
        _, px = self._editor_look()
        f = QFont()
        f.setPixelSize(px)
        fm = QFontMetricsF(f)
        lines = ed.toPlainText().split("\n")
        w = max([fm.horizontalAdvance(ln) for ln in lines] + [px * 3])
        h = max(fm.lineSpacing() * len(lines), px * 1.3)
        ed.setGeometry(int(self._edit_pos.x()) - 1, int(self._edit_pos.y()) - 1, int(w) + 10, int(h) + 4)

    def _commit_text(self) -> None:
        ed = self._editor
        if ed is None:
            return
        self._editor = None                      # guard: focus-out re-emits while hiding
        text = ed.toPlainText().rstrip()
        ed.hide()
        ed.deleteLater()
        it, self._edit_item = self._edit_item, None
        if it:
            if text:
                it.text = text
                self.history.modified(it, self._edit_before)
            elif it in self.items:
                idx = self.items.index(it)
                self.items.remove(it)
                self.history.deleted(it, idx)
                self.selected = None
        elif text:
            new = self._new_item("text", [QPointF(self._edit_pos)])
            new.text = text
            self._add(new)
            self.selected, self.tool = new, "select"
        self.setFocus()
        self._sync_bar()
        self.update()

    # ================================================================ keyboard
    def keyPressEvent(self, e) -> None:
        k, m = e.key(), e.modifiers()
        ctrl = bool(m & Qt.KeyboardModifier.ControlModifier)
        shift = bool(m & Qt.KeyboardModifier.ShiftModifier)
        arrows = (Qt.Key.Key_Left, Qt.Key.Key_Right, Qt.Key.Key_Up, Qt.Key.Key_Down)
        if k == Qt.Key.Key_Escape:
            if self.tool != "select":
                self.set_tool("select")
            elif self.selected:
                self.selected = None
                self._sync_bar()
                self.update()
            else:
                self.close()
        elif k == Qt.Key.Key_Shift and not self.sel:
            self._hex = not self._hex
            self.update()
        elif k == Qt.Key.Key_A and ctrl:
            self.sel = QRectF(self.rect())
            self.started.emit()
            self._sync_bar()
            self.update()
        elif not self.sel:
            if k == Qt.Key.Key_C and not ctrl:
                QGuiApplication.clipboard().setText(self._color_text(self._pixel_at(self._mouse)))
                self._finish(f"Renk kopyalandı: {self._color_text(self._pixel_at(self._mouse))}")
        elif k in (Qt.Key.Key_Return, Qt.Key.Key_Enter) or (ctrl and k == Qt.Key.Key_C):
            self.copy()
        elif ctrl and k == Qt.Key.Key_Z and not shift:
            self.undo()
        elif ctrl and (k == Qt.Key.Key_Y or (k == Qt.Key.Key_Z and shift)):
            self.redo()
        elif ctrl and k == Qt.Key.Key_S:
            self.save()
        elif k in (Qt.Key.Key_Delete, Qt.Key.Key_Backspace):
            self.delete_selected()
        elif k == Qt.Key.Key_F3:
            self.pin()
        elif k in arrows:
            step = 10 if shift else 1
            dx = {Qt.Key.Key_Left: -step, Qt.Key.Key_Right: step}.get(k, 0)
            dy = {Qt.Key.Key_Up: -step, Qt.Key.Key_Down: step}.get(k, 0)
            if self.selected and self.tool == "select":
                before = geom_state(self.selected)
                self.selected.translate(dx, dy)
                self.history.modified(self.selected, before)
            else:
                self.sel.translate(dx, dy)
                self.sel = self._clamp_rect(self.sel)
                self._place_bar()
            self.update()
        elif not ctrl and (ch := e.text().lower()):
            if ch in SHORTCUTS:
                self.set_tool(SHORTCUTS[ch])
            elif ch == "o":
                self.ocr()

    # ================================================================ output
    def render(self) -> QImage:
        s, sc = self.sel, self.cap.scale
        phys = QRect(round(s.x() * sc), round(s.y() * sc), max(1, round(s.width() * sc)),
                     max(1, round(s.height() * sc)))
        out = self.cap.image.copy(phys)
        p = QPainter(out)
        p.scale(sc, sc)
        p.translate(-phys.x() / sc, -phys.y() / sc)   # aligned to the rounded physical crop
        paint_items(p, self.items, self.cap.image, sc)
        p.end()
        return out

    def _finish(self, msg: str = "") -> None:
        if self._finished:
            return
        self._finished = True
        if self.sel:
            r, g = self.sel, self.cap.geometry
            self.cfg["last_region"] = [self.cap.screen.name(), r.x() + g.x(), r.y() + g.y(), r.width(), r.height()]
        self.cfg.save()
        if msg:
            self.done.emit(msg)
        self.close()

    def _autosave(self, img: QImage) -> str:
        if not self.cfg["auto_save"]:
            return ""
        path = output.unique_path(os.path.join(self.cfg.save_dir(), output.timestamp_name(self.cfg["format"])))
        ok, err = output.save_image(img, path, self.cfg["quality"])
        return f" · kaydedildi: {os.path.basename(path)}" if ok else f" · kaydedilemedi: {err}"

    def copy(self) -> None:
        if not self.sel:
            return
        self._commit_text()
        img = self.render()
        QGuiApplication.clipboard().setImage(img)
        self._finish(f"Panoya kopyalandı ({img.width()}×{img.height()}){self._autosave(img)}")

    def save(self) -> None:
        if not self.sel:
            return
        self._commit_text()
        img = self.render()
        default = os.path.join(self.cfg.save_dir(), output.timestamp_name(self.cfg["format"]))
        self.hide()
        path, _ = QFileDialog.getSaveFileName(None, "Kaydet", default,
                                              "PNG (*.png);;JPEG (*.jpg *.jpeg);;WebP (*.webp)")
        if not path:                      # cancelled: back to editing, nothing lost
            self._reshow()
            return
        ok, err = output.save_image(img, path, self.cfg["quality"])
        if not ok:
            self.done.emit(f"Kaydedilemedi: {err}")
            self._reshow()
            return
        QGuiApplication.clipboard().setImage(img)
        self._finish(f"Kaydedildi: {path}")

    def _reshow(self) -> None:
        _show(self)
        self.raise_()
        self.activateWindow()
        self.setFocus()

    def pin(self) -> None:
        if not self.sel:
            return
        self._commit_text()
        img = self.render()
        pos = self.mapToGlobal(self.sel.topLeft().toPoint())
        self._finish()
        PinWindow(img, pos, self.cap.scale)

    def ocr(self) -> None:
        if not self.sel:
            return
        self._commit_text()
        img = self.render()
        pos = self.mapToGlobal(self.sel.topLeft().toPoint())
        self._finish()
        from .ocr import run_ocr
        run_ocr(img, pos)

    def closeEvent(self, e) -> None:
        self.cap.image = QImage()          # release the full-resolution screenshot immediately
        self.closed.emit()
        super().closeEvent(e)


def start_overlays(caps: list[Capture], cfg: Config, done_cb=lambda msg: None, factory=None) -> list:
    """One frameless overlay per screen; the first one the user starts on wins, the rest close."""
    overlays = [(factory or Overlay)(c, cfg) for c in caps]

    def keep_only(active: Overlay) -> None:
        for o in [x for x in overlays if x is not active]:
            overlays.remove(o)
            try:
                o.closed.disconnect()
            except (RuntimeError, TypeError):
                pass
            o.close()

    def on_closed() -> None:
        for o in list(overlays):
            overlays.remove(o)
            try:
                o.closed.disconnect()
            except (RuntimeError, TypeError):
                pass
            o.close()

    for o in overlays:
        o.started.connect(lambda o=o: keep_only(o))
        o.closed.connect(on_closed)
        o.done.connect(done_cb)
        _show(o)
        o.raise_()
    pos = QCursor.pos()
    target = next((o for o in overlays if o.cap.geometry.contains(pos)), overlays[0])
    target.activateWindow()
    target.setFocus()
    return overlays
