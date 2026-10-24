"""Retina / HiDPI: Qt tags grabbed images with devicePixelRatio=2 and QPainter then works in HALF-size logical units.
On Linux (ratio 1) this never shows; on a Retina Mac screenshots collapsed into the top-left quarter and the recording
island was not hidden (a half-size copy appeared elsewhere). These tests tag images with a ratio on purpose."""
from PySide6.QtCore import QRect, QRectF
from PySide6.QtGui import QColor, QGuiApplication, QImage, QPainter, QPixmap

from pinshot.capture import Capture, compose_region


def retina_image(w, h, ratio=2.0):
    """A screen image as grabWindow() returns it on a Retina Mac: physical pixels + a devicePixelRatio tag."""
    img = QImage(int(w * ratio), int(h * ratio), QImage.Format.Format_RGB32)
    img.fill(QColor("red"))
    p = QPainter(img)
    p.fillRect(QRect(int(w * ratio) // 2, 0, int(w * ratio) // 2, int(h * ratio)), QColor("blue"))   # right half blue
    p.end()
    img.setDevicePixelRatio(ratio)
    return img


def test_compose_region_ignores_the_pixel_ratio_tag(qapp):
    scr = QGuiApplication.primaryScreen()
    cap = Capture(scr, retina_image(100, 60), QRect(0, 0, 100, 60), 2.0)         # 100x60 logical, 200x120 physical
    out = compose_region([cap], QRectF(0, 0, 100, 60))
    assert (out.width(), out.height()) == (200, 120)
    assert QColor(out.pixel(5, 5)) == QColor("red")
    assert QColor(out.pixel(195, 115)) == QColor("blue"), "content collapsed into the top-left quarter"
    assert QColor(out.pixel(150, 100)) == QColor("blue")


def test_compose_region_partial_area_on_retina(qapp):
    scr = QGuiApplication.primaryScreen()
    cap = Capture(scr, retina_image(100, 60), QRect(0, 0, 100, 60), 2.0)
    out = compose_region([cap], QRectF(40, 10, 40, 20))                          # spans the red/blue boundary at x=50
    assert (out.width(), out.height()) == (80, 40)
    assert QColor(out.pixel(5, 5)) == QColor("red") and QColor(out.pixel(75, 35)) == QColor("blue")


class FakeRetinaScreen:
    """Behaves like QScreen on a Retina Mac: grabWindow() returns a pixmap of device pixels tagged with ratio 2."""
    def __init__(self, w, h):
        self.w, self.h = w, h

    def geometry(self):
        return QRect(0, 0, self.w, self.h)

    def devicePixelRatio(self):
        return 2.0

    def grabWindow(self, _wid, x=0, y=0, w=-1, h=-1):
        w = self.w if w < 0 else w
        h = self.h if h < 0 else h
        pm = QPixmap(w * 2, h * 2)
        pm.fill(QColor("green"))
        pm.setDevicePixelRatio(2.0)
        return pm


def test_recording_frames_are_plain_pixels(qapp):
    from pinshot.recorder import Recorder
    rec = Recorder()
    screen = FakeRetinaScreen(100, 60)
    rec.region = QRect(10, 10, 50, 30)
    rec._parts = [(screen, QRect(10, 10, 50, 30))]
    rec.cursor = False
    img = rec._frame()
    assert (img.width(), img.height()) == (100, 60)                               # physical size
    assert img.devicePixelRatio() == 1.0, "painters must work in pixels, not half-size units"


def test_island_mask_patches_the_right_pixels_on_retina_frames(qapp):
    from pinshot.recorder import IslandMask, Recorder
    rec = Recorder()
    rec.region = QRect(0, 0, 100, 60)
    rec._parts = [(FakeRetinaScreen(100, 60), QRect(0, 0, 100, 60))]
    rec.cursor = False
    rec.exclude = lambda: QRect(20, 10, 30, 10)                                   # logical rect of the island
    clean = rec._frame()                                                          # frame before the island appears
    rec.exclude = None
    p = QPainter(clean)
    p.end()
    # a frame in which the island (red) covers its area in PHYSICAL pixels
    frame = rec._frame()
    p = QPainter(frame)
    p.fillRect(QRect(40, 20, 60, 20), QColor("red"))
    p.end()
    rec.exclude = lambda: QRect(20, 10, 30, 10)
    rec._mask = IslandMask()
    rec._mask.process(clean, None)
    out = rec._mask.process(frame, rec._island_in_image(frame))
    assert QColor(out.pixel(60, 30)) == QColor("green"), "island was not painted out where it actually is"
    assert QColor(out.pixel(5, 5)) == QColor("green")
    assert out.width() == 200 and out.height() == 120, "no half-size copy may be drawn elsewhere"
