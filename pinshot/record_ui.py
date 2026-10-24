"""Screen-recording UI: area/screen picker (one per monitor) and the on-screen recording indicator."""
from __future__ import annotations

import sys

from PySide6.QtCore import QPoint, QPointF, QRect, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QCursor, QFont, QGuiApplication, QPainter, QPen, QRegion
from PySide6.QtWidgets import (QCheckBox, QComboBox, QFrame, QHBoxLayout, QLabel, QPushButton, QToolButton,
                               QVBoxLayout, QWidget)

from .capture import Capture
from .config import Config
from .overlay import Session, _shared
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
QCheckBox{color:#d0d0d5;font-size:12px}
QCheckBox:disabled{color:#77777d}
QPushButton#go{background:#e5342a}
QPushButton#go:hover{background:#ff453a}
QPushButton#go:disabled{background:#5a2a27;color:#999}
QToolButton{background:transparent;border:0;border-radius:6px;padding:5px}
QToolButton:hover{background:#3e3e44}
"""


def _show(w: QWidget) -> None:
    w.show() if sys.platform == "darwin" else w.showFullScreen()


def hide_while_settings_open(owner, proc, on_back) -> None:
    """The overlays are full-screen and always-on-top, so an external settings window would open *behind* them.
    Hide them while it is open and bring everything back (plus a fresh device scan) when it closes."""
    for o in list(owner.s.overlays):
        o.hide()
    if proc is None:                                    # no process handle (Windows/macOS): come back after a pause
        poll = lambda: False                            # noqa: E731
        QTimer.singleShot(15000, lambda: _back(owner, on_back))
        return
    timer = QTimer(owner)
    timer.setInterval(400)

    def check() -> None:
        if proc.poll() is not None:
            timer.stop()
            _back(owner, on_back)
    timer.timeout.connect(check)
    timer.start()


def _back(owner, on_back) -> None:
    try:
        for o in list(owner.s.overlays):
            (o._reshow if hasattr(o, "_reshow") else o.show)()
        owner.activateWindow()
        on_back()
    except RuntimeError:                                # closed in the meantime
        pass


def persist_choice(cfg: Config, v: dict) -> None:
    """Remember the last recording choices (frame rate and size are kept per format)."""
    fmt = v["fmt"]
    cfg["record_format"] = fmt
    cfg["record_fps_gif" if fmt == "gif" else "record_fps"] = v["fps"]
    cfg["record_gif_width" if fmt == "gif" else "record_video_width"] = v["max_width"]
    if fmt == "mp4":                                   # a GIF choice must not erase the audio setup
        cfg["record_quality_444"] = bool(v.get("hq444", True))
        cfg["record_audio_system"], cfg["record_audio_mic"] = v["audio_system"], v["audio_mic"]
        cfg["record_mic_device"] = v["mic_device"]


class RecordOptions(QWidget):
    """Format, frame rate, size and audio choices; shared by the area picker and the capture toolbar popup."""
    SIZES = {"GIF": (("Boyut: 640", 640), ("Boyut: 960", 960), ("Boyut: 1280", 1280), ("Boyut: 1920", 1920),
                     ("Boyut: orijinal", 0)),
             "MP4": (("Boyut: 1280", 1280), ("Boyut: 1920", 1920), ("Boyut: 2560", 2560), ("Boyut: orijinal", 0))}
    DEFAULT_MIC = "Varsayılan mikrofon"
    settings_launched = Signal(object)        # Popen handle (or None): the owner hides its always-on-top windows
    _perm_answered = Signal()                 # macOS permission prompt was answered (emitted from another thread)

    def __init__(self, cfg: Config):
        super().__init__()
        from . import audio
        self.cfg = cfg
        self._probe = audio.probe()
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        row1 = QHBoxLayout()
        row1.setSpacing(8)
        self.fmt = QComboBox()
        self.fmt.addItems(["MP4", "GIF"])
        self.fmt.setCurrentText(str(cfg["record_format"]).upper())
        self.fps = QComboBox()
        self.size = QComboBox()
        self.size.setToolTip("Çıktının en fazla genişliği. \"Orijinal\" en keskin sonucu verir; küçültme dosyayı ve CPU'yu azaltır.")
        for w in (self.fmt, self.fps, self.size):
            row1.addWidget(w)
        row1.addStretch(1)
        lay.addLayout(row1)
        self.hq_cb = QCheckBox("Maksimum kalite (4:4:4 renk)")
        self.hq_cb.setChecked(bool(cfg["record_quality_444"]))
        self.hq_cb.setToolTip("Renkli yazı ve arayüz kenarları keskin kalır (özellikle 2K/4K ekranlarda).\n"
                              "Bazı oynatıcılarda (QuickTime, Safari, bazı TV ve telefonlar) açılmayabilir; "
                              "paylaşmak için kapatın.")
        lay.addWidget(self.hq_cb)
        row2 = QHBoxLayout()
        row2.setSpacing(10)
        self.sys_cb = QCheckBox("Sistem sesi")
        self.mic_cb = QCheckBox("Mikrofon")
        self.mic_dev = QComboBox()
        self.mic_dev.setMinimumWidth(190)
        self.mic_dev.addItem(self.DEFAULT_MIC, "")
        for name in audio.list_microphones():
            self.mic_dev.addItem(name, name)
        i = self.mic_dev.findData(str(cfg["record_mic_device"]))
        self.mic_dev.setCurrentIndex(i if i >= 0 else 0)
        self.refresh_btn = QToolButton()
        self.refresh_btn.setText("↻")
        self.refresh_btn.setToolTip("Ses cihazlarını yeniden tara")
        self.refresh_btn.clicked.connect(self.reload_audio)
        for w in (self.sys_cb, self.mic_cb, self.mic_dev, self.refresh_btn):
            row2.addWidget(w)
        row2.addStretch(1)
        lay.addLayout(row2)
        # a visible explanation (a tooltip is easy to miss) when a source is unavailable
        self.audio_note = QLabel()
        self.audio_note.setWordWrap(True)
        self.audio_note.setMaximumWidth(520)
        self.audio_note.setStyleSheet("color:#ffb340;font-size:12px")
        self.settings_btn = QPushButton("Ses ayarlarını aç")
        self.settings_btn.clicked.connect(self._open_sound_settings)
        lay.addWidget(self.audio_note)
        lay.addWidget(self.settings_btn, 0, Qt.AlignmentFlag.AlignLeft)
        self.sys_cb.setChecked(bool(cfg["record_audio_system"]))
        self.mic_cb.setChecked(bool(cfg["record_audio_mic"]))
        for w in self.findChildren(QWidget):
            w.setFocusPolicy(Qt.FocusPolicy.NoFocus)         # keep keyboard focus on the overlay (Esc / Enter)
        self.fmt.currentTextChanged.connect(self._format_changed)
        self.mic_cb.toggled.connect(self._mic_toggled)
        self._perm_answered.connect(self.reload_audio)
        self._fill_fps(self.fmt.currentText())
        self._fill_size(self.fmt.currentText())
        self._sync_audio()

    def _fill_fps(self, fmt: str) -> None:
        """MP4 goes up to 60 fps; GIF tops out at 50 (format limit: delays are stored in 1/100 s)."""
        rates = (10, 15, 24, 30, 50) if fmt == "GIF" else (10, 15, 24, 30, 60)
        want = int(self.cfg["record_fps_gif" if fmt == "GIF" else "record_fps"])
        self.fps.blockSignals(True)
        self.fps.clear()
        for v in rates:
            self.fps.addItem(f"{v} fps", v)
        self.fps.setCurrentIndex(min(range(len(rates)), key=lambda i: abs(rates[i] - want)))
        self.fps.blockSignals(False)

    def _fill_size(self, fmt: str) -> None:
        want = int(self.cfg["record_gif_width" if fmt == "GIF" else "record_video_width"])
        self.size.blockSignals(True)
        self.size.clear()
        for label, v in self.SIZES[fmt]:
            self.size.addItem(label, v)
        i = self.size.findData(want)
        self.size.setCurrentIndex(i if i >= 0 else len(self.SIZES[fmt]) - 1)
        self.size.blockSignals(False)

    def _format_changed(self, text: str) -> None:
        self._fill_fps(text)
        self._fill_size(text)
        self._sync_audio()

    def _mic_toggled(self, on: bool) -> None:
        if on and sys.platform == "darwin":
            from . import macperm
            if macperm.mic_status() == "not_determined":        # show the system prompt now, while the UI stays responsive
                macperm.request_mic(lambda granted: self._perm_answered.emit())
        self._sync_audio()

    def _open_sound_settings(self) -> None:
        from . import audio
        res = audio.open_sound_settings()
        if not res:
            self.audio_note.setText(self.audio_note.text() + "\nAyarlar açılamadı: lütfen sistem ses ayarlarını elle açın.")
            return
        self.settings_launched.emit(res if hasattr(res, "poll") else None)

    def reload_audio(self) -> None:
        """Re-scan the devices (a microphone may have been plugged in or enabled meanwhile)."""
        from . import audio
        self._probe = audio.probe()
        keep = str(self.mic_dev.currentData() or self.cfg["record_mic_device"])
        self.mic_dev.blockSignals(True)
        self.mic_dev.clear()
        self.mic_dev.addItem(self.DEFAULT_MIC, "")
        for name in audio.list_microphones():
            self.mic_dev.addItem(name, name)
        i = self.mic_dev.findData(keep)
        self.mic_dev.setCurrentIndex(i if i >= 0 else 0)
        self.mic_dev.blockSignals(False)
        self._sync_audio()
        w = self.window()
        if w is not None:
            w.adjustSize() if isinstance(w, QFrame) else None

    def _sync_audio(self) -> None:
        """Audio only exists for MP4, and only for sources this machine actually has."""
        mp4 = self.fmt.currentText() == "MP4"
        self.hq_cb.setVisible(mp4)                      # GIFs are palette-based: this only matters for MP4
        sys_ok, sys_why = self._probe["system"]
        mic_ok, mic_why = self._probe["mic"]
        no_gif = "GIF dosyalarında ses olmaz"
        self.sys_cb.setEnabled(mp4 and sys_ok)
        self.sys_cb.setToolTip("" if mp4 and sys_ok else (no_gif if not mp4 else sys_why))
        self.mic_cb.setEnabled(mp4 and mic_ok)
        self.mic_cb.setToolTip("" if mp4 and mic_ok else (no_gif if not mp4 else mic_why))
        self.mic_dev.setEnabled(mp4 and mic_ok and self.mic_cb.isChecked())
        self.mic_dev.setToolTip("Kullanılacak mikrofon")
        notes = [] if not mp4 else [why for ok, why in ((sys_ok, sys_why), (mic_ok, mic_why)) if not ok and why]
        self.audio_note.setText("\n".join(notes))
        self.audio_note.setVisible(bool(notes))
        self.settings_btn.setVisible(bool(notes) and not mic_ok)
        self.refresh_btn.setVisible(mp4)
        self.mic_dev.setVisible(mp4)

    def values(self) -> dict:
        mp4 = self.fmt.currentText() == "MP4"
        return {"fmt": self.fmt.currentText().lower(), "fps": int(self.fps.currentData()),
                "max_width": int(self.size.currentData()),
                "hq444": mp4 and self.hq_cb.isChecked(),
                "audio_system": mp4 and self.sys_cb.isEnabled() and self.sys_cb.isChecked(),
                "audio_mic": mp4 and self.mic_cb.isEnabled() and self.mic_cb.isChecked(),
                "mic_device": str(self.mic_dev.currentData() or "")}


class RecordPopup(QFrame):
    """Options + Record button shown under the capture toolbar (the red record button)."""
    chosen = Signal(dict)
    settings_launched = Signal(object)

    def __init__(self, parent: QWidget, cfg: Config):
        super().__init__(parent)
        self.setObjectName("panel")
        self.setStyleSheet(CSS)
        self.setCursor(Qt.CursorShape.ArrowCursor)
        self.opts = RecordOptions(cfg)
        self.opts.settings_launched.connect(self.settings_launched)
        self.cfg = cfg
        lay = QVBoxLayout(self)
        lay.setContentsMargins(12, 10, 12, 10)
        lay.setSpacing(10)
        title = QLabel("<b>Seçili alanı kaydet</b>")
        lay.addWidget(title)
        lay.addWidget(self.opts)
        row = QHBoxLayout()
        row.addStretch(1)
        cancel = QPushButton("Vazgeç")
        go = QPushButton("● Kaydı başlat")
        go.setObjectName("go")
        cancel.clicked.connect(self.close)
        go.clicked.connect(self._go)
        for b in (cancel, go):
            b.setFocusPolicy(Qt.FocusPolicy.NoFocus)
            row.addWidget(b)
        lay.addLayout(row)
        self.adjustSize()

    def _go(self) -> None:
        v = self.opts.values()
        persist_choice(self.cfg, v)
        self.chosen.emit(v)


class RecordSelector(QWidget):
    """Frozen-desktop picker: drag an area, or press the button to record this whole monitor."""
    closed = Signal()
    started = Signal()
    done = Signal(str)
    chosen = Signal(object)        # dict: screen, region (GLOBAL desktop QRect, may span monitors), fmt, fps, ...

    sel = _shared("sel")
    _mode = _shared("mode")

    def __init__(self, cap: Capture, cfg: Config, session: Session | None = None):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint)
        self.cap, self.cfg = cap, cfg
        self.s = session or Session([cap], cfg)
        self.s.overlays.append(self)
        self.origin = cap.geometry.topLeft()
        self.setAttribute(Qt.WidgetAttribute.WA_DeleteOnClose)
        self.setFocusPolicy(Qt.FocusPolicy.StrongFocus)
        self.setMouseTracking(True)
        self.setCursor(Qt.CursorShape.CrossCursor)
        self.setScreen(cap.screen)
        self.setGeometry(cap.geometry)
        self._anchor = QPointF()
        self._grab = QPointF()
        self._handle = ""
        self.cursor_target = None     # set by start_overlays (monitor under the cursor)

        p = QFrame(self)
        p.setObjectName("panel")
        p.setStyleSheet(CSS)
        p.setCursor(Qt.CursorShape.ArrowCursor)
        outer = QVBoxLayout(p)
        outer.setContentsMargins(12, 10, 12, 10)
        outer.setSpacing(10)
        lay = QHBoxLayout()
        lay.setSpacing(8)
        name = cap.screen.name() or "Ekran"
        lay.addWidget(QLabel(f"<b>{name}</b> · {cap.geometry.width()}×{cap.geometry.height()}"))
        lay.addStretch(1)
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
            w.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        outer.addLayout(lay)
        self.opts = RecordOptions(cfg)
        self.opts.settings_launched.connect(self._settings_launched)
        outer.addWidget(self.opts)
        p.adjustSize()
        p.move((cap.geometry.width() - p.width()) // 2, 28)
        self.panel = p

    # ---- painting
    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.drawImage(self.rect(), self.cap.image)
        p.fillRect(self.rect(), QColor(0, 0, 0, 125))
        if not self.sel:
            self._hint(p)
            return
        origin = QPointF(self.origin)
        vis = self.sel.translated(-origin).intersected(QRectF(self.rect()))
        if not vis.isEmpty():
            p.save()
            p.setClipRect(vis)
            p.drawImage(self.rect(), self.cap.image)              # undim the part of the selection on this monitor
            p.restore()
        p.save()
        p.translate(-origin)                                       # global desktop coordinates
        p.setPen(QPen(QColor("#ff453a"), 2))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRect(self.sel.adjusted(0, 0, -1, -1))
        for r in self._handles().values():
            p.setPen(QPen(QColor("#ff453a"), 1))
            p.setBrush(Qt.GlobalColor.white)
            p.drawRect(r)
        p.restore()
        tl = self.sel.topLeft() - origin
        if QRectF(self.rect()).contains(tl + QPointF(1, 1)):       # label on the monitor holding the top-left corner
            sc = self.cap.scale
            self._pill(p, f"{round(self.sel.width() * sc)} × {round(self.sel.height() * sc)}",
                       max(4, int(tl.x())), int(tl.y()) - 28 if tl.y() > 32 else int(tl.y()) + 6)

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
        return r.intersected(self.s.bounds)

    def _settings_launched(self, proc) -> None:
        hide_while_settings_open(self, proc, lambda: self.opts.reload_audio())

    def _gp(self, e) -> QPointF:
        """Mouse position in global desktop coordinates.

        While a button is held the pointer can leave this monitor but events keep coming to this window, with
        coordinates in *this window's* scale. On monitors with different scale factors those are wrong, so outside
        the window the real cursor position (already in global logical coordinates) is used instead."""
        local = e.position()
        if QRectF(self.rect()).contains(local):
            return local + QPointF(self.origin)
        return QPointF(QCursor.pos())

    def _reshow(self) -> None:
        self.show() if sys.platform == "darwin" else self.showFullScreen()
        self.raise_()

    def _repaint(self) -> None:
        self.s.update_all()
        for o in self.s.overlays:                  # the Record button is enabled on every monitor's panel
            try:
                o.start_btn.setEnabled(o.sel is not None and self._mode == "")
            except RuntimeError:
                pass

    def mousePressEvent(self, e) -> None:
        pt = self._gp(e)
        if e.button() == Qt.MouseButton.RightButton:
            if self.sel:
                self.sel = None
                self._repaint()
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
        pt = self._gp(e)
        m = self._mode
        if m == "new":
            self.sel = self._clamp(QRectF(self._anchor, pt).normalized())
        elif m == "move":
            b = self.s.bounds
            tl = pt - self._grab
            tl.setX(min(max(b.left(), tl.x()), b.right() - self.sel.width()))
            tl.setY(min(max(b.top(), tl.y()), b.bottom() - self.sel.height()))
            self.sel.moveTo(tl)
        elif m == "resize":
            r, h = QRectF(self.sel), self._handle
            b = self.s.bounds
            x, y = min(max(b.left(), pt.x()), b.right()), min(max(b.top(), pt.y()), b.bottom())
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
        self._repaint()

    def mouseReleaseEvent(self, e) -> None:
        if self._mode == "new" and (self.sel.width() < 16 or self.sel.height() < 16):
            self.sel = None
        self._mode = ""
        self._repaint()

    def mouseDoubleClickEvent(self, e) -> None:
        if self.sel and self.sel.contains(self._gp(e)):
            self._record_selection()

    def keyPressEvent(self, e) -> None:
        if e.key() == Qt.Key.Key_Escape:
            self.close()
        elif e.key() in (Qt.Key.Key_Return, Qt.Key.Key_Enter):
            if self.sel:
                self._record_selection()
            else:
                target = (self.cursor_target() if self.cursor_target else None) or self
                target._record_full()        # monitor under the mouse, not the one holding keyboard focus

    # ---- result
    def _emit(self, region: QRect) -> None:
        v = self.opts.values()
        persist_choice(self.cfg, v)
        self.cfg.save()
        self.hide()
        screen = QGuiApplication.screenAt(region.center()) or self.cap.screen
        self.chosen.emit({"screen": screen, "region": region, **v})
        self.close()

    def _record_full(self) -> None:
        self._emit(QRect(self.cap.geometry))                    # this whole monitor, in global coordinates

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
    """Red outline around the recorded area, drawn only where it is *outside* the video and *on the same monitor*.

    Sides that touch the screen edge get no line (it would land on the neighbouring monitor or inside the video), so a
    full-screen recording has no frame at all."""

    def __init__(self, global_rect: QRect, screen_geo: QRect, margin: int = 3):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool | Qt.WindowType.WindowTransparentForInput)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents)
        self.setAttribute(Qt.WidgetAttribute.WA_MacAlwaysShowToolWindow)
        outer = global_rect.adjusted(-margin, -margin, margin, margin).intersected(screen_geo)
        self.setGeometry(outer)
        inner = global_rect.translated(-outer.topLeft())
        ring = QRegion(QRect(0, 0, outer.width(), outer.height())).subtracted(QRegion(inner))
        self.visible_ring = not ring.isEmpty()
        if self.visible_ring:
            self.setMask(ring)

    def show(self) -> None:
        if self.visible_ring:          # an empty mask would mean "no mask": the whole window would be painted
            super().show()

    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.fillRect(self.rect(), QColor("#ff3b30"))


class RecordingIsland(QWidget):
    """Floating "island" shown while recording: blinking dot, timer, stop button. Drag it anywhere.

    The recorder removes it from the video if it overlaps the recorded area, so it can sit anywhere."""
    stop_requested = Signal()
    moved = Signal(QPoint)        # emitted when a drag ends (global position)

    W, H = 196, 40

    def __init__(self, fmt: str = "MP4", fps: int = 15):
        super().__init__(None, Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint
                         | Qt.WindowType.Tool | Qt.WindowType.WindowDoesNotAcceptFocus)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        self.setAttribute(Qt.WidgetAttribute.WA_MacAlwaysShowToolWindow)
        self.setMouseTracking(True)
        self.setFixedSize(self.W, self.H)
        self.label = f"{fmt} · {fps}"
        self._secs = 0
        self._drag: QPoint | None = None
        # pill-shaped mask (works without a compositing window manager)
        r = self.H
        self.setMask(QRegion(r // 2, 0, self.W - r, self.H) | QRegion(0, 0, r, r, QRegion.RegionType.Ellipse)
                     | QRegion(self.W - r, 0, r, r, QRegion.RegionType.Ellipse))

    def _stop_rect(self) -> QRect:
        return QRect(self.W - 34, 6, 28, 28)

    def set_elapsed(self, secs: int) -> None:
        self._secs = secs
        self.update()

    def paintEvent(self, _) -> None:
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.setPen(QPen(QColor("#3a3a3f"), 1))
        p.setBrush(QColor("#1c1c1e"))
        p.drawRoundedRect(QRectF(0.5, 0.5, self.W - 1, self.H - 1), self.H / 2, self.H / 2)
        if self._secs % 2 == 0:                                   # blinks once a second, no timer of its own
            p.setPen(Qt.PenStyle.NoPen)
            p.setBrush(QColor("#ff3b30"))
            p.drawEllipse(QPointF(22, self.H / 2), 5.5, 5.5)
        f = QFont()
        f.setPixelSize(14)
        f.setBold(True)
        p.setFont(f)
        p.setPen(Qt.GlobalColor.white)
        p.drawText(QRect(36, 0, 56, self.H), Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft,
                   f"{self._secs // 60:02d}:{self._secs % 60:02d}")
        f.setBold(False)
        f.setPixelSize(11)
        p.setFont(f)
        p.setPen(QColor("#9a9aa0"))
        p.drawText(QRect(92, 0, 70, self.H), Qt.AlignmentFlag.AlignVCenter | Qt.AlignmentFlag.AlignLeft, self.label)
        sr = self._stop_rect()
        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor("#e5342a"))
        p.drawEllipse(sr)
        p.setBrush(Qt.GlobalColor.white)
        p.drawRoundedRect(QRectF(sr.center().x() - 5, sr.center().y() - 5, 10, 10), 2, 2)

    def mousePressEvent(self, e) -> None:
        if e.button() != Qt.MouseButton.LeftButton:
            return
        if self._stop_rect().contains(e.position().toPoint()):
            self.stop_requested.emit()
            return
        self._drag = e.globalPosition().toPoint() - self.frameGeometry().topLeft()
        self.setCursor(Qt.CursorShape.ClosedHandCursor)

    def mouseMoveEvent(self, e) -> None:
        if self._drag is not None:
            self.move(e.globalPosition().toPoint() - self._drag)
        else:
            self.setCursor(Qt.CursorShape.PointingHandCursor if self._stop_rect().contains(e.position().toPoint())
                           else Qt.CursorShape.OpenHandCursor)

    def mouseReleaseEvent(self, e) -> None:
        if self._drag is not None:
            self._drag = None
            self.setCursor(Qt.CursorShape.OpenHandCursor)
            self.moved.emit(self.pos())

    def place_default(self, region: QRect, screen_geo: QRect, other_screens: list[QRect]) -> None:
        """Pick a start position that is *not* in the video when possible: next to the area, else on another monitor."""
        w, h = self.W, self.H
        x = min(max(screen_geo.left() + 8, region.center().x() - w // 2), screen_geo.right() - w - 8)
        if region.bottom() + 8 + h <= screen_geo.bottom():
            self.move(x, region.bottom() + 8)
        elif region.top() - 8 - h >= screen_geo.top():
            self.move(x, region.top() - 8 - h)
        elif other_screens:
            g = other_screens[0]
            self.move(g.center().x() - w // 2, g.top() + 24)
        else:                                                       # nowhere else: inside, the recorder hides it
            self.move(region.center().x() - w // 2, region.top() + 16)

    def is_on_screen(self, pos: QPoint, screens: list[QRect]) -> bool:
        return any(g.contains(pos) and g.contains(pos + QPoint(self.W - 1, self.H - 1)) for g in screens)
