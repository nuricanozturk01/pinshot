"""Floating editor toolbar: tool row + context-sensitive property row."""
from __future__ import annotations

from PySide6.QtCore import QSize, Qt, Signal
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (QColorDialog, QFrame, QHBoxLayout, QLabel, QPushButton, QSlider, QSpinBox,
                               QToolButton, QVBoxLayout, QWidget)

from .icons import icon

TOOLS = [("select", "Seç / Taşı (V)"), ("rect", "Dikdörtgen (R)"), ("ellipse", "Elips (E)"),
         ("line", "Çizgi (L)"), ("arrow", "Ok (A)"), ("pen", "Kalem (P)"), ("marker", "Fosforlu kalem (H)"),
         ("text", "Metin (T)"), ("number", "Numara (N)"), ("mosaic", "Mozaik (M)"), ("blur", "Bulanıklaştır (B)")]
SHORTCUTS = {"v": "select", "r": "rect", "e": "ellipse", "l": "line", "a": "arrow", "p": "pen", "h": "marker",
             "t": "text", "n": "number", "m": "mosaic", "b": "blur"}
SWATCHES = ["#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#0a84ff", "#af52de", "#ffffff", "#000000"]

# which property controls each tool shows
PROPS = {"rect": ("color", "width", "fill", "dash"), "ellipse": ("color", "width", "fill", "dash"),
         "line": ("color", "width", "dash"), "arrow": ("color", "width", "dash"),
         "pen": ("color", "width"), "marker": ("color", "width"), "text": ("color", "font"),
         "number": ("color", "width"), "mosaic": ("width",), "blur": ("width",)}
WIDTH_LABEL = {"mosaic": "Güç", "blur": "Güç", "number": "Boyut"}

CSS = """
QFrame#bar{background:#262629;border:1px solid #3a3a3f;border-radius:10px}
QToolButton{background:transparent;border:0;border-radius:6px;padding:5px}
QToolButton:hover{background:#3e3e44}
QToolButton:checked{background:#0a84ff}
QToolButton:disabled{background:transparent}
QLabel{color:#d0d0d5;font-size:12px}
QSpinBox{background:#1b1b1d;color:white;border:1px solid #3a3a3f;border-radius:4px;padding:1px 3px}
QSlider::groove:horizontal{height:4px;background:#4a4a50;border-radius:2px}
QSlider::handle:horizontal{background:white;width:12px;margin:-5px 0;border-radius:6px}
QToolTip{background:#111;color:white;border:1px solid #444;padding:3px}
QFrame#sep{background:#45454b}
"""


class Swatch(QPushButton):
    def __init__(self, color: str):
        super().__init__()
        self.hex = color
        self.setFixedSize(20, 20)
        self.setCheckable(True)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self._style(False)

    def _style(self, on: bool) -> None:
        border = "#ffffff" if on else "#55555b"
        self.setStyleSheet(f"QPushButton{{background:{self.hex};border:{3 if on else 1}px solid {border};"
                           f"border-radius:10px}}")

    def set_on(self, on: bool) -> None:
        self.setChecked(on)
        self._style(on)


def _sep() -> QFrame:
    f = QFrame()
    f.setObjectName("sep")
    f.setFixedSize(1, 22)
    return f


def _btn(name: str, tip: str, checkable: bool = False) -> QToolButton:
    b = QToolButton()
    b.setIcon(icon(name))
    b.setIconSize(QSize(20, 20))
    b.setToolTip(tip)
    b.setCheckable(checkable)
    b.setCursor(Qt.CursorShape.PointingHandCursor)
    b.setFocusPolicy(Qt.FocusPolicy.NoFocus)   # keep keyboard focus on the overlay
    return b


class Toolbar(QFrame):
    tool_changed = Signal(str)
    color_changed = Signal(QColor)
    width_changed = Signal(int)
    fill_changed = Signal(bool)
    dash_changed = Signal(bool)
    font_changed = Signal(int)
    action = Signal(str)      # undo redo delete pin ocr record save copy cancel

    def __init__(self, parent: QWidget):
        super().__init__(parent)
        self.setObjectName("bar")
        self.setStyleSheet(CSS)
        self.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self._color = QColor("#ff3b30")
        lay = QVBoxLayout(self)
        lay.setContentsMargins(8, 6, 8, 6)
        lay.setSpacing(6)

        # ---- property row
        self.props = QWidget()
        pl = QHBoxLayout(self.props)
        pl.setContentsMargins(2, 0, 2, 0)
        pl.setSpacing(5)
        self.color_box = QWidget()
        cl = QHBoxLayout(self.color_box)
        cl.setContentsMargins(0, 0, 0, 0)
        cl.setSpacing(5)
        self.swatches = [Swatch(c) for c in SWATCHES]
        for s in self.swatches:
            s.clicked.connect(lambda _=False, s=s: self._set_color(QColor(s.hex)))
            cl.addWidget(s)
        more = _btn("plus", "Özel renk…")
        more.clicked.connect(self._custom_color)
        cl.addWidget(more)
        pl.addWidget(self.color_box)
        self.width_box = QWidget()
        wl = QHBoxLayout(self.width_box)
        wl.setContentsMargins(0, 0, 0, 0)
        self.width_label = QLabel("Kalınlık")
        self.slider = QSlider(Qt.Orientation.Horizontal)
        self.slider.setRange(1, 20)
        self.slider.setFixedWidth(90)
        self.slider.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        self.slider.valueChanged.connect(self.width_changed)
        wl.addWidget(self.width_label)
        wl.addWidget(self.slider)
        pl.addWidget(self.width_box)
        self.fill_btn = _btn("fill", "Dolgu", True)
        self.fill_btn.toggled.connect(self.fill_changed)
        self.dash_btn = _btn("dash", "Kesik çizgi", True)
        self.dash_btn.toggled.connect(self.dash_changed)
        pl.addWidget(self.fill_btn)
        pl.addWidget(self.dash_btn)
        self.font_box = QWidget()
        fl = QHBoxLayout(self.font_box)
        fl.setContentsMargins(0, 0, 0, 0)
        self.font_spin = QSpinBox()
        self.font_spin.setRange(8, 200)
        self.font_spin.setSuffix(" px")
        self.font_spin.setFocusPolicy(Qt.FocusPolicy.ClickFocus)
        self.font_spin.valueChanged.connect(self.font_changed)
        fl.addWidget(QLabel("Yazı"))
        fl.addWidget(self.font_spin)
        pl.addWidget(self.font_box)
        pl.addStretch(1)
        lay.addWidget(self.props)

        # ---- main row
        row = QHBoxLayout()
        row.setSpacing(2)
        self.tool_btns: dict[str, QToolButton] = {}
        for name, tip in TOOLS:
            b = _btn(name, tip, True)
            b.clicked.connect(lambda _=False, n=name: self.tool_changed.emit(n))
            row.addWidget(b)
            self.tool_btns[name] = b
        row.addWidget(_sep())
        self.undo_btn = _btn("undo", "Geri al (Ctrl+Z)")
        self.redo_btn = _btn("redo", "Yinele (Ctrl+Y)")
        self.del_btn = _btn("trash", "Seçiliyi sil (Del)")
        for b, n in ((self.undo_btn, "undo"), (self.redo_btn, "redo"), (self.del_btn, "delete")):
            b.clicked.connect(lambda _=False, n=n: self.action.emit(n))
            row.addWidget(b)
        row.addWidget(_sep())
        for n, tip in (("pin", "Ekrana sabitle (F3)"), ("ocr", "Metni tanı – OCR (O)"),
                       ("record", "Bu alanı video olarak kaydet (MP4 / GIF)"), ("save", "Kaydet… (Ctrl+S)")):
            b = _btn(n, tip)
            b.clicked.connect(lambda _=False, n=n: self.action.emit(n))
            row.addWidget(b)
        row.addWidget(_sep())
        cancel = _btn("close", "İptal (Esc)")
        cancel.clicked.connect(lambda: self.action.emit("cancel"))
        ok = _btn("check", "Panoya kopyala (Enter)")
        ok.setStyleSheet("QToolButton{background:#30a14e}QToolButton:hover{background:#3cc05f}")
        ok.clicked.connect(lambda: self.action.emit("copy"))
        row.addWidget(cancel)
        row.addWidget(ok)
        lay.addLayout(row)
        self.configure(None)

    # ---- state sync
    def _set_color(self, c: QColor) -> None:
        self.set_color(c)
        self.color_changed.emit(c)

    def _custom_color(self) -> None:
        c = QColorDialog.getColor(self._color, self.window(), "Renk")
        if c.isValid():
            self._set_color(c)

    def set_color(self, c: QColor) -> None:
        self._color = QColor(c)
        for s in self.swatches:
            s.set_on(s.hex.lower() == c.name().lower())

    def set_values(self, color: QColor, width: int, fill: bool, dashed: bool, font_px: int) -> None:
        for w in (self.slider, self.fill_btn, self.dash_btn, self.font_spin):
            w.blockSignals(True)
        self.set_color(color)
        self.slider.setValue(width)
        self.fill_btn.setChecked(fill)
        self.dash_btn.setChecked(dashed)
        self.font_spin.setValue(font_px)
        for w in (self.slider, self.fill_btn, self.dash_btn, self.font_spin):
            w.blockSignals(False)

    def set_tool(self, tool: str) -> None:
        for n, b in self.tool_btns.items():
            b.setChecked(n == tool)

    def configure(self, kind: str | None) -> None:
        """Show only the property controls relevant to `kind` (None hides the row)."""
        show = PROPS.get(kind or "", ())
        self.color_box.setVisible("color" in show)
        self.width_box.setVisible("width" in show)
        self.fill_btn.setVisible("fill" in show)
        self.dash_btn.setVisible("dash" in show)
        self.font_box.setVisible("font" in show)
        self.width_label.setText(WIDTH_LABEL.get(kind or "", "Kalınlık"))
        self.props.setVisible(bool(show))
        self.adjustSize()

    def set_history(self, can_undo: bool, can_redo: bool, can_delete: bool) -> None:
        self.undo_btn.setEnabled(can_undo)
        self.redo_btn.setEnabled(can_redo)
        self.del_btn.setEnabled(can_delete)
