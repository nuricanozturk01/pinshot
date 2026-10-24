"""Settings window."""
from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QKeySequence
from PySide6.QtWidgets import (QCheckBox, QComboBox, QDialog, QDialogButtonBox, QFileDialog, QFormLayout, QGroupBox,
                               QHBoxLayout, QKeySequenceEdit, QLabel, QLineEdit, QPushButton, QSpinBox,
                               QVBoxLayout)

from . import __version__, autostart
from .config import DEFAULTS, Config
from .hotkeys import pynput_to_qt, qt_to_pynput

HOTKEY_FIELDS = [("hotkey_capture", "Bölge yakala"), ("hotkey_fullscreen", "Tam ekran yakala"),
                 ("hotkey_repeat", "Son bölgeyi tekrarla"), ("hotkey_pin", "Panodan sabitle")]


class SettingsDialog(QDialog):
    applied = Signal()

    def __init__(self, cfg: Config, parent=None):
        super().__init__(parent)
        self.cfg = cfg
        self.setWindowTitle("PinShot – Ayarlar")
        self.setMinimumWidth(460)
        root = QVBoxLayout(self)

        box = QGroupBox("Kayıt")
        f = QFormLayout(box)
        self.dir = QLineEdit()
        browse = QPushButton("Gözat…")
        browse.clicked.connect(self._browse)
        row = QHBoxLayout()
        row.addWidget(self.dir, 1)
        row.addWidget(browse)
        f.addRow("Klasör", row)
        self.fmt = QComboBox()
        self.fmt.addItems(["png", "jpg", "webp"])
        f.addRow("Biçim", self.fmt)
        self.quality = QSpinBox()
        self.quality.setRange(1, 100)
        f.addRow("JPEG/WebP kalitesi", self.quality)
        self.auto = QCheckBox("Her yakalamayı otomatik klasöre kaydet")
        f.addRow(self.auto)
        root.addWidget(box)

        keys = QGroupBox("Kısayollar")
        kf = QFormLayout(keys)
        self.keys: dict[str, QKeySequenceEdit] = {}
        for k, label in HOTKEY_FIELDS:
            e = QKeySequenceEdit()
            e.setMaximumSequenceLength(1)
            self.keys[k] = e
            kf.addRow(label, e)
        note = QLabel("Wayland'de global kısayollar çalışmaz; masaüstü ayarlarından <code>pinshot --capture</code> "
                      "komutuna kısayol atayın.")
        note.setWordWrap(True)
        note.setTextFormat(Qt.TextFormat.RichText)
        kf.addRow(note)
        root.addWidget(keys)

        misc = QGroupBox("Genel")
        mf = QFormLayout(misc)
        self.start = QCheckBox("Oturum açıldığında başlat")
        self.border = QCheckBox("Sabitlenen görsellerde kenarlık göster")
        mf.addRow(self.start)
        mf.addRow(self.border)
        root.addWidget(misc)

        bb = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel
                              | QDialogButtonBox.StandardButton.RestoreDefaults)
        bb.accepted.connect(self._ok)
        bb.rejected.connect(self.reject)
        bb.button(QDialogButtonBox.StandardButton.RestoreDefaults).clicked.connect(lambda: self._load(DEFAULTS))
        root.addWidget(bb)
        root.addWidget(QLabel(f"<small>PinShot {__version__}</small>"))
        self._load(cfg.data)

    def _load(self, d) -> None:
        self.dir.setText(d["save_dir"])
        self.dir.setPlaceholderText(self.cfg.save_dir() if not d["save_dir"] else "")
        self.fmt.setCurrentText(d["format"])
        self.quality.setValue(int(d["quality"]))
        self.auto.setChecked(bool(d["auto_save"]))
        for k, e in self.keys.items():
            e.setKeySequence(QKeySequence(pynput_to_qt(d[k])))
        self.start.setChecked(bool(d["autostart"]))
        self.border.setChecked(bool(d["pin_border"]))

    def _browse(self) -> None:
        d = QFileDialog.getExistingDirectory(self, "Klasör seç", self.dir.text() or self.cfg.save_dir())
        if d:
            self.dir.setText(d)

    def _ok(self) -> None:
        c = self.cfg
        c["save_dir"] = self.dir.text().strip()
        c["format"] = self.fmt.currentText()
        c["quality"] = self.quality.value()
        c["auto_save"] = self.auto.isChecked()
        seen: set[str] = set()
        for k, e in self.keys.items():
            v = qt_to_pynput(e.keySequence().toString(QKeySequence.SequenceFormat.PortableText))
            c[k] = v if v not in seen else ""      # duplicates would shadow each other
            if v:
                seen.add(v)
        c["pin_border"] = self.border.isChecked()
        want = self.start.isChecked()
        if want != c["autostart"] or want:
            autostart.set_enabled(want)
        c["autostart"] = want
        c.save()
        self.applied.emit()
        self.accept()
