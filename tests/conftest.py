import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

import pytest
from PySide6.QtCore import QPointF, Qt
from PySide6.QtGui import QColor, QGuiApplication, QImage, QPainter
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from pinshot.capture import Capture
from pinshot.config import Config
from pinshot.overlay import Overlay


@pytest.fixture(scope="session")
def qapp():
    app = QApplication.instance() or QApplication([])
    yield app


@pytest.fixture
def overlay(qapp, tmp_path):
    cfg = Config(str(tmp_path / "config.json"))
    scr = QGuiApplication.primaryScreen()
    g = scr.geometry()
    img = QImage(g.width(), g.height(), QImage.Format.Format_RGB32)
    img.fill(QColor("white"))
    p = QPainter(img)
    p.fillRect(50, 50, 100, 100, QColor("blue"))
    p.end()
    o = Overlay(Capture(scr, img, g, 1.0), cfg)
    o.show()
    yield o
    o.close()


def drag(w, a, b, button=Qt.MouseButton.LeftButton):
    QTest.mousePress(w, button, Qt.KeyboardModifier.NoModifier, a.toPoint() if isinstance(a, QPointF) else a)
    QTest.mouseMove(w, b.toPoint() if isinstance(b, QPointF) else b)
    QTest.mouseRelease(w, button, Qt.KeyboardModifier.NoModifier, b.toPoint() if isinstance(b, QPointF) else b)
