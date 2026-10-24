import gc
import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
os.environ.setdefault("PINSHOT_NO_AUDIO", "1")       # never enumerate real audio devices in tests (can hang on CI)

import pytest
from PySide6.QtCore import QPointF, Qt
from PySide6.QtGui import QColor, QGuiApplication, QImage, QPainter
from PySide6.QtTest import QTest
from PySide6.QtWidgets import QApplication

from pinshot.capture import Capture
from pinshot.config import Config
from pinshot.overlay import Overlay


@pytest.fixture(autouse=True)
def _collect_garbage_on_the_main_thread():
    """Qt objects may only be destroyed on the GUI (main) thread. Python's cyclic garbage collector can run in ANY thread;
    when it ran inside a worker thread of a later test it destroyed leftover widgets there and crashed the process
    (a sporadic segmentation fault). So garbage is collected here, on the main thread, after every test."""
    yield
    gc.collect()


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
