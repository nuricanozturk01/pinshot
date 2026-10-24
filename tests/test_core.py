import math

import pytest
from PySide6.QtCore import QPoint, QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QImage, QPainter
from PySide6.QtTest import QTest

from pinshot.annotate import History, Item, apply_geom, geom_state, hit_test
from pinshot.config import Config
from pinshot.hotkeys import pynput_to_qt, qt_to_pynput

from conftest import drag


def P(x, y):
    return QPoint(x, y)


def select_region(o):
    drag(o, P(100, 100), P(400, 300))
    assert o.sel is not None and o.sel.toRect().size().width() == 300
    assert o.bar.isVisible()


def test_config_roundtrip(tmp_path):
    c = Config(str(tmp_path / "c.json"))
    c["color"] = "#00ff00"
    c.save()
    assert Config(str(tmp_path / "c.json"))["color"] == "#00ff00"
    (tmp_path / "bad.json").write_text("{not json")
    assert Config(str(tmp_path / "bad.json"))["format"] == "png"      # corrupt file -> defaults


def test_hotkey_conversion():
    assert qt_to_pynput("Ctrl+Alt+A") in ("<ctrl>+<alt>+a", "<cmd>+<alt>+a")
    assert qt_to_pynput("F3") == "<f3>"
    assert qt_to_pynput("") == ""
    assert pynput_to_qt("<f3>") == "F3"


def test_selection_and_drawing(overlay):
    o = overlay
    select_region(o)
    o.set_tool("rect")
    drag(o, P(150, 150), P(250, 220))
    assert len(o.items) == 1 and o.items[0].kind == "rect"
    o.undo()
    assert not o.items
    o.redo()
    assert len(o.items) == 1


def test_undo_ignores_property_changes(overlay):
    o = overlay
    select_region(o)
    o.set_tool("rect")
    drag(o, P(150, 150), P(250, 220))
    o.set_tool("select")
    drag(o, P(150, 185), P(150, 185))              # click on left edge -> select
    assert o.selected is o.items[0]
    before = len(o.history._undo)
    o.bar.slider.setValue(11)                       # width change on selected item
    o.bar.color_changed.emit(QColor("#00ff00"))
    assert o.items[0].width == 11 and o.items[0].color == QColor("#00ff00")
    assert len(o.history._undo) == before           # not an undo step
    o.undo()                                        # undoes the drawing itself, not the width
    assert not o.items


def test_move_item_and_undo(overlay):
    o = overlay
    select_region(o)
    o.set_tool("rect")
    drag(o, P(150, 150), P(250, 220))
    o.set_tool("select")
    drag(o, P(150, 185), P(190, 205))               # grab left edge, drag (+40,+20)
    r = o.items[0].rect()
    assert (round(r.x()), round(r.y())) == (190, 170)
    o.undo()
    assert round(o.items[0].rect().x()) == 150


def test_resize_item_with_handle(overlay):
    o = overlay
    select_region(o)
    o.set_tool("rect")
    drag(o, P(150, 150), P(250, 220))
    o.set_tool("select")
    drag(o, P(150, 185), P(150, 185))
    drag(o, P(250, 220), P(300, 260))               # bottom-right handle
    r = o.items[0].rect()
    assert (round(r.right()), round(r.bottom())) == (300, 260) and round(r.x()) == 150


def test_text_is_movable_and_editable(overlay):
    o = overlay
    select_region(o)
    o.set_tool("text")
    QTest.mouseClick(o, Qt.MouseButton.LeftButton, Qt.KeyboardModifier.NoModifier, P(150, 150))
    assert o._editor is not None
    o._editor.setPlainText("Merhaba")
    o._commit_text()
    t = o.items[-1]
    assert t.kind == "text" and t.text == "Merhaba" and o.selected is t and o.tool == "select"
    x0 = t.pts[0].x()
    drag(o, P(160, 160), P(200, 190))
    assert round(t.pts[0].x() - x0) == 40
    o._begin_text(t.pts[0], t)                      # edit existing
    o._editor.setPlainText("Değişti")
    o._commit_text()
    assert t.text == "Değişti"
    o.undo()
    assert t.text == "Merhaba"


def test_render_contains_annotation(overlay):
    o = overlay
    select_region(o)
    o.items.append(Item("rect", QColor("red"), 3, [QPointF(120, 120), QPointF(200, 200)], fill=True))
    img = o.render()
    assert img.width() == 300 and img.height() == 200
    assert QColor(img.pixel(40, 40)) == QColor("red")      # (160,160) in overlay coords
    assert QColor(img.pixel(280, 180)) == QColor("white")


def test_render_matches_hidpi_scale(overlay):
    o = overlay
    o.cap.scale = 2.0
    o.cap.image = o.cap.image.scaled(o.cap.image.size() * 2)
    o.sel = QRectF(10, 10, 100, 50)
    img = o.render()
    assert (img.width(), img.height()) == (200, 100)


def test_mosaic_and_blur_do_not_crash(overlay):
    o = overlay
    select_region(o)
    for kind in ("mosaic", "blur"):
        o.items.append(Item(kind, QColor("red"), 5, [QPointF(120, 120), QPointF(180, 180)]))
    assert not o.render().isNull()


def test_hit_testing():
    r = Item("rect", QColor("red"), 3, [QPointF(0, 0), QPointF(100, 100)])
    assert hit_test(r, QPointF(0, 50)) and not hit_test(r, QPointF(50, 50))
    r.fill = True
    assert hit_test(r, QPointF(50, 50))
    ln = Item("line", QColor("red"), 3, [QPointF(0, 0), QPointF(100, 0)])
    assert hit_test(ln, QPointF(50, 4)) and not hit_test(ln, QPointF(50, 30))
    e = Item("ellipse", QColor("red"), 3, [QPointF(0, 0), QPointF(100, 100)])
    assert hit_test(e, QPointF(50, 0)) and not hit_test(e, QPointF(50, 50))


def test_pin_resize_keeps_aspect(qapp):
    from pinshot.pin import PinWindow, close_all
    img = QImage(200, 100, QImage.Format.Format_RGB32)
    img.fill(QColor("green"))
    w = PinWindow(img, QPoint(50, 50), 1.0)
    w.show()
    g0 = w.geometry()
    w._zoom_about(2.0)
    assert abs(w.width() / w.height() - 2.0) < 0.05 and w.width() == 400
    w.resize(200, 100)
    w._press, w._g0, w._resize = QPoint(0, 0), w.geometry(), "br"
    w._do_resize(QPoint(100, 10), False)
    assert abs(w.width() / w.height() - 2.0) < 0.05 and w.width() > 200
    w._resize = None
    close_all()


def test_pin_text_and_clipboard(qapp):
    from pinshot import pin
    from PySide6.QtGui import QGuiApplication
    QGuiApplication.clipboard().setText("hello pin")
    assert pin.pin_clipboard()
    assert pin.pin_count() >= 1
    pin.close_all()


def test_ocr_reads_text(qapp):
    pytest.importorskip("rapidocr_onnxruntime")
    from PySide6.QtGui import QFont
    from pinshot.ocr import recognize
    img = QImage(500, 120, QImage.Format.Format_RGB32)
    img.fill(QColor("white"))
    p = QPainter(img)
    f = QFont()
    f.setPixelSize(44)
    p.setFont(f)
    p.drawText(20, 80, "Hello PinShot")
    p.end()
    assert "PinShot" in recognize(img).replace(" ", "") or "Hello" in recognize(img)
