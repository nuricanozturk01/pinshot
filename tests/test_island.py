import pytest
from PySide6.QtCore import QPoint, QRect, Qt
from PySide6.QtGui import QColor, QImage
from PySide6.QtTest import QTest

from pinshot.record_ui import RecordingIsland
from pinshot.recorder import IslandMask


def _img(color, w=200, h=120):
    im = QImage(w, h, QImage.Format.Format_RGB32)
    im.fill(QColor(color))
    return im


def test_island_mask_replaces_only_the_island_area_with_clean_pixels():
    m = IslandMask()
    box = QRect(50, 40, 60, 30)
    m.process(_img("blue"), None)                          # clean frame: nothing to hide
    frame = _img("green")                                  # new content everywhere...
    from PySide6.QtGui import QPainter
    p = QPainter(frame)
    p.fillRect(box, QColor("red"))                         # ...plus the island drawn over it
    p.end()
    out = m.process(frame, box)
    assert QColor(out.pixel(80, 55)) == QColor("blue")     # island area shows the last clean pixels
    assert QColor(out.pixel(5, 5)) == QColor("green")      # the rest stays live
    assert QColor(out.pixel(190, 110)) == QColor("green")


def test_island_mask_keeps_background_fresh_around_a_stationary_island():
    m = IslandMask()
    box = QRect(10, 10, 40, 20)
    m.process(_img("blue"), None)
    m.process(_img("green"), box)
    out = m.process(_img("yellow"), QRect(100, 60, 40, 20))   # island moved: its new spot must not be stale blue
    assert QColor(out.pixel(110, 70)) == QColor("yellow") or QColor(out.pixel(110, 70)) == QColor("green")
    assert QColor(out.pixel(110, 70)) != QColor("blue")


def test_island_mask_is_a_noop_without_overlap():
    m = IslandMask()
    a = _img("blue")
    assert m.process(a, None) is a
    assert m.process(_img("red"), QRect(500, 500, 10, 10)).pixel(5, 5) == _img("red").pixel(5, 5)


def test_island_stop_button_and_drag(qapp):
    isl = RecordingIsland("MP4", 60)
    isl.move(100, 100)
    isl.show()
    stops, moves = [], []
    isl.stop_requested.connect(lambda: stops.append(1))
    isl.moved.connect(moves.append)
    QTest.mouseClick(isl, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, isl._stop_rect().center())
    assert stops == [1] and not moves                      # clicking stop does not start a drag
    QTest.mousePress(isl, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, QPoint(60, 20))
    QTest.mouseMove(isl, QPoint(160, 70))
    QTest.mouseRelease(isl, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, QPoint(160, 70))
    assert isl.pos() != QPoint(100, 100) and len(moves) == 1
    isl.close()


def test_island_default_placement(qapp):
    isl = RecordingIsland()
    screen = QRect(0, 0, 1920, 1080)
    isl.place_default(QRect(100, 100, 800, 400), screen, [])
    assert isl.pos().y() > 500                                     # below the area
    isl.place_default(QRect(0, 0, 1920, 1080), screen, [QRect(1920, 0, 1280, 720)])
    assert isl.pos().x() >= 1920                                   # full screen: moved to the other monitor
    isl.place_default(QRect(0, 0, 1920, 1080), screen, [])
    assert QRect(0, 0, 1920, 1080).contains(isl.pos())             # single monitor: inside (recorder hides it)
    isl.close()


def test_recording_frame_never_leaves_its_monitor(qapp):
    from pinshot.record_ui import RecordingFrame
    screen = QRect(1920, 0, 2560, 1440)            # a monitor with neighbours on both sides
    full = RecordingFrame(QRect(screen), screen)
    assert not full.visible_ring                    # full screen: no frame at all (it would spill onto the neighbours)
    inner = RecordingFrame(QRect(2000, 100, 800, 600), screen)
    assert inner.visible_ring and screen.contains(inner.geometry())
    touching = RecordingFrame(QRect(1920, 100, 800, 600), screen)   # flush with the left edge
    assert touching.visible_ring and touching.geometry().left() >= screen.left()
    for f in (full, inner, touching):
        f.deleteLater()
