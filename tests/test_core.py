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
    from pinshot.ocr import recognize
    from pinshot.selftest import make_text_image
    img = make_text_image()
    text = recognize(img).replace(" ", "").lower()
    if not ("pinshot" in text or "hello" in text):
        from pinshot.selftest import ocr_diagnostics
        pytest.fail(f"OCR returned {text!r} for a {img.width()}x{img.height()} image\n{ocr_diagnostics(img)}")


def test_ctrl_a_selects_screen_under_cursor(qapp, overlay):
    """Keyboard focus may sit on another monitor: Ctrl+A must act on the overlay returned by cursor_target."""
    other = overlay
    redirected = []
    other.cursor_target = lambda: (redirected.append(1) or other)
    QTest.keyClick(other, Qt.Key.Key_A, Qt.KeyboardModifier.ControlModifier)
    assert redirected and other.sel is not None
    assert other.sel.toRect().size() == other.rect().size()


def test_record_request_emits_region_and_format(overlay):
    o = overlay
    select_region(o)
    got = []
    o.record.connect(got.append)
    o.request_record("gif")
    assert got and got[0]["fmt"] == "gif" and got[0]["region"].width() == 300
    assert got[0]["screen"] is o.cap.screen


def test_record_request_rejects_tiny_area(overlay):
    from PySide6.QtCore import QRectF
    o = overlay
    o.sel = QRectF(10, 10, 5, 5)
    got = []
    o.record.connect(got.append)
    o.request_record("mp4")
    assert not got


def test_record_request_uses_chosen_fps_and_remembers_it(overlay):
    o = overlay
    select_region(o)
    got = []
    o.record.connect(got.append)
    o.request_record("mp4", 60)
    assert got[0]["fps"] == 60 and o.cfg["record_fps"] == 60 and o.cfg["record_format"] == "mp4"


def test_gif_fps_is_capped_at_50(overlay):
    o = overlay
    select_region(o)
    got = []
    o.record.connect(got.append)
    o.request_record("gif", 60)
    assert got[0]["fps"] == 50


def test_record_popup_offers_audio_and_format_choices(overlay):
    from pinshot.record_ui import RecordPopup
    o = overlay
    select_region(o)
    o.cfg["record_format"], o.cfg["record_fps"] = "mp4", 30
    pop = RecordPopup(o, o.cfg)
    opts = pop.opts
    assert [opts.fps.itemData(i) for i in range(opts.fps.count())] == [10, 15, 24, 30, 60]
    opts.fmt.setCurrentText("GIF")
    assert [opts.fps.itemData(i) for i in range(opts.fps.count())] == [10, 15, 24, 30, 50]
    assert not opts.sys_cb.isEnabled() and not opts.mic_cb.isEnabled()       # GIFs have no sound
    assert "GIF" in opts.mic_cb.toolTip()
    opts.fmt.setCurrentText("MP4")
    got = []
    o.record.connect(got.append)
    pop.chosen.connect(lambda v: o.request_record(v["fmt"], v["fps"], max_width=v["max_width"]))
    opts.fps.setCurrentIndex(opts.fps.findData(60))
    pop._go()
    assert got and got[0]["fps"] == 60 and got[0]["fmt"] == "mp4"
    assert o.cfg["record_fps"] == 60


def test_record_options_remember_audio_choice_per_mp4_only(overlay):
    from pinshot.record_ui import persist_choice
    c = overlay.cfg
    persist_choice(c, {"fmt": "mp4", "fps": 30, "max_width": 0, "audio_system": True, "audio_mic": True,
                       "mic_device": "USB Mic"})
    persist_choice(c, {"fmt": "gif", "fps": 15, "max_width": 1920, "audio_system": False, "audio_mic": False,
                       "mic_device": ""})
    assert c["record_audio_system"] and c["record_audio_mic"] and c["record_mic_device"] == "USB Mic"   # GIF keeps them
    assert c["record_fps"] == 30 and c["record_fps_gif"] == 15


def test_record_request_carries_audio_settings(overlay):
    o = overlay
    select_region(o)
    got = []
    o.record.connect(got.append)
    o.request_record("mp4", 30, max_width=0, audio_system=True, audio_mic=True, mic_device="USB Mic")
    r = got[0]
    assert r["audio_system"] and r["audio_mic"] and r["mic_device"] == "USB Mic" and r["max_width"] == 0


def test_gif_request_never_carries_audio(overlay):
    o = overlay
    select_region(o)
    got = []
    o.record.connect(got.append)
    o.request_record("gif", 15, audio_system=True, audio_mic=True)
    assert not got[0]["audio_system"] and not got[0]["audio_mic"]       # no sound in GIFs


def test_loupe_only_on_monitor_with_cursor(overlay, monkeypatch):
    """Every monitor has its own overlay; only the one under the real cursor may draw the magnifier."""
    from PySide6.QtCore import QPoint
    import pinshot.overlay as ov
    o = overlay
    g = o.cap.geometry
    calls = []
    orig = ov.Overlay._draw_loupe
    monkeypatch.setattr(ov.QCursor, "pos", staticmethod(lambda: QPoint(g.x() + 50, g.y() + 60)))
    assert o._cursor_local() == QPointF(50, 60)
    monkeypatch.setattr(ov.QCursor, "pos", staticmethod(lambda: QPoint(g.right() + 500, g.y())))   # another monitor
    assert o._cursor_local() is None
    drawn = []
    from PySide6.QtGui import QImage, QPainter
    img = QImage(o.size(), QImage.Format.Format_RGB32)
    p = QPainter(img)
    o._draw_pill = lambda *a, **k: drawn.append(a)        # the loupe labels go through _draw_pill
    orig(o, p)
    p.end()
    assert not drawn                                      # nothing drawn: cursor is elsewhere


def test_record_options_explain_missing_microphone(overlay, monkeypatch):
    """No microphone: the reason must be visible (not just a tooltip) and a way to fix it offered."""
    import pinshot.audio as audio
    from pinshot.record_ui import RecordOptions
    monkeypatch.setattr(audio, "probe", lambda: {"system": (True, ""), "mic": (False, "Mikrofon bulunamadı. TEST-İPUCU")})
    monkeypatch.setattr(audio, "list_microphones", lambda: [])
    opts = RecordOptions(overlay.cfg)
    opts.show()
    assert not opts.mic_cb.isEnabled() and opts.sys_cb.isEnabled()
    assert "TEST-İPUCU" in opts.audio_note.text() and opts.audio_note.isVisible()
    assert opts.settings_btn.isVisible() and opts.refresh_btn.isVisible()
    # a microphone appears (plugged in / enabled): Yenile picks it up
    monkeypatch.setattr(audio, "probe", lambda: {"system": (True, ""), "mic": (True, "")})
    monkeypatch.setattr(audio, "list_microphones", lambda: ["USB Mic", "Headset Mic"])
    opts.refresh_btn.click()
    assert opts.mic_cb.isEnabled() and not opts.audio_note.isVisible()
    assert [opts.mic_dev.itemText(i) for i in range(opts.mic_dev.count())] == [opts.DEFAULT_MIC, "USB Mic", "Headset Mic"]
    opts.mic_cb.setChecked(True)
    opts.mic_dev.setCurrentIndex(2)
    v = opts.values()
    assert v["audio_mic"] and v["mic_device"] == "Headset Mic"


def test_record_options_gif_hides_audio_controls(overlay):
    from pinshot.record_ui import RecordOptions
    opts = RecordOptions(overlay.cfg)
    opts.show()
    opts.fmt.setCurrentText("GIF")
    assert not opts.mic_dev.isVisible() and not opts.hq_cb.isVisible() and not opts.audio_note.isVisible()
    assert not opts.values()["audio_mic"] and not opts.values()["hq444"]


def test_compose_region_stitches_two_monitors_exactly(qapp):
    """A selection that spans two monitors is built from each monitor's own pixels, with no resampling."""
    from PySide6.QtCore import QRect, QRectF
    from PySide6.QtGui import QColor, QGuiApplication, QImage
    from pinshot.capture import Capture, compose_region
    scr = QGuiApplication.primaryScreen()
    left_img = QImage(100, 80, QImage.Format.Format_RGB32)
    left_img.fill(QColor("red"))
    right_img = QImage(120, 80, QImage.Format.Format_RGB32)
    right_img.fill(QColor("blue"))
    caps = [Capture(scr, left_img, QRect(0, 0, 100, 80), 1.0), Capture(scr, right_img, QRect(100, 0, 120, 80), 1.0)]
    out = compose_region(caps, QRectF(60, 10, 100, 40))                 # 40 px from the left monitor, 60 from the right
    assert (out.width(), out.height()) == (100, 40)
    assert QColor(out.pixel(5, 5)) == QColor("red") and QColor(out.pixel(39, 20)) == QColor("red")
    assert QColor(out.pixel(40, 20)) == QColor("blue") and QColor(out.pixel(99, 39)) == QColor("blue")


def test_compose_region_uses_highest_scale_when_monitors_differ(qapp):
    from PySide6.QtCore import QRect, QRectF
    from PySide6.QtGui import QColor, QGuiApplication, QImage
    from pinshot.capture import Capture, compose_region
    scr = QGuiApplication.primaryScreen()
    lo = QImage(100, 100, QImage.Format.Format_RGB32)
    lo.fill(QColor("red"))
    hi = QImage(200, 200, QImage.Format.Format_RGB32)               # same 100x100 logical area at scale 2
    hi.fill(QColor("blue"))
    caps = [Capture(scr, lo, QRect(0, 0, 100, 100), 1.0), Capture(scr, hi, QRect(100, 0, 100, 100), 2.0)]
    out = compose_region(caps, QRectF(50, 0, 100, 50))
    assert (out.width(), out.height()) == (200, 100)                  # output at the sharper monitor's scale


def test_selection_is_shared_and_global_across_overlays(qapp, tmp_path):
    """Two overlays on adjacent monitors share one selection in global coordinates."""
    from PySide6.QtCore import QRect, QRectF
    from PySide6.QtGui import QColor, QGuiApplication, QImage
    from pinshot.capture import Capture
    from pinshot.overlay import Overlay, Session
    scr = QGuiApplication.primaryScreen()
    cfg = Config(str(tmp_path / "c.json"))
    def cap(x):
        im = QImage(300, 200, QImage.Format.Format_RGB32)
        im.fill(QColor("gray"))
        return Capture(scr, im, QRect(x, 0, 300, 200), 1.0)
    caps = [cap(0), cap(300)]
    sess = Session(caps, cfg)
    a, b = Overlay(caps[0], cfg, sess), Overlay(caps[1], cfg, sess)
    sess.sel = QRectF(250, 20, 120, 80)                                # starts on A, ends on B
    assert a.sel == b.sel and b.sel.right() == 370
    assert sess.host() is b                                            # toolbar goes where the bottom-right corner is
    assert sess.bounds == QRectF(0, 0, 600, 200)
    out = a.render()
    assert (out.width(), out.height()) == (120, 80)
    a.close()
    b.close()


def test_save_dialog_is_not_covered_by_overlays_on_other_monitors(qapp, tmp_path, monkeypatch):
    """Ctrl+S: every monitor's overlay must be hidden while the file dialog is open (they are always-on-top)."""
    from PySide6.QtCore import QRect, QRectF
    from PySide6.QtGui import QColor, QGuiApplication, QImage
    import pinshot.overlay as ov
    from pinshot.capture import Capture
    scr = QGuiApplication.primaryScreen()
    cfg = Config(str(tmp_path / "c.json"))
    def cap(x):
        im = QImage(300, 200, QImage.Format.Format_RGB32)
        im.fill(QColor("gray"))
        return Capture(scr, im, QRect(x, 0, 300, 200), 1.0)
    caps = [cap(0), cap(300)]
    a, b = ov.start_overlays(caps, cfg)
    a.sel = QRectF(10, 10, 100, 80)
    seen = {}

    def fake_dialog(*args, **kwargs):
        seen["visible_during_dialog"] = [o.isVisible() for o in (a, b)]
        return "", ""                                         # user cancels

    monkeypatch.setattr(ov.QFileDialog, "getSaveFileName", staticmethod(fake_dialog))
    a.save()
    assert seen["visible_during_dialog"] == [False, False]      # nothing may stay above the dialog
    assert a.isVisible() and b.isVisible()                      # cancelled: everything comes back, selection intact
    assert a.sel is not None
    a.close()


def test_pointer_position_outside_window_uses_real_cursor(overlay, monkeypatch):
    """Mixed-scale monitors: while dragging out of this window, event coordinates use *this* window's scale and are
    wrong; the real cursor position must be used instead."""
    from PySide6.QtCore import QPoint, QPointF
    import pinshot.overlay as ov

    class Ev:
        def __init__(self, p):
            self._p = p

        def position(self):
            return self._p

    o = overlay
    inside = QPointF(40, 50)
    assert o._gp(Ev(inside)) == inside + QPointF(o.origin)                       # normal case: exact local position
    far = QPointF(o.width() + 900, o.height() + 700)                              # pointer is on another monitor
    monkeypatch.setattr(ov.QCursor, "pos", staticmethod(lambda: QPoint(3904, 1120)))
    assert o._gp(Ev(far)) == QPointF(3904, 1120)


def test_macos_default_shortcuts_avoid_option_and_f3():
    """Option+A types "å" on macOS, so the shortcut library never sees "a"; F3 is Mission Control."""
    from pinshot.config import default_hotkeys
    mac = default_hotkeys("darwin")
    assert all("<alt>" not in v and v != "<f3>" for v in mac.values())
    assert len(set(mac.values())) == len(mac)                              # no duplicates
    assert default_hotkeys("linux")["hotkey_capture"] == "<ctrl>+<alt>+a"
    assert default_hotkeys("win32")["hotkey_pin"] == "<f3>"


def test_old_macos_config_gets_the_new_shortcuts(tmp_path, monkeypatch):
    import json
    import pinshot.config as cfgmod
    path = tmp_path / "c.json"
    path.write_text(json.dumps({"hotkey_capture": "<ctrl>+<alt>+a", "hotkey_pin": "<f3>", "color": "#00ff00"}))
    monkeypatch.setattr(cfgmod.sys, "platform", "darwin")
    monkeypatch.setattr(cfgmod, "DEFAULTS", {**cfgmod.DEFAULTS, **cfgmod.default_hotkeys("darwin")})
    c = cfgmod.Config(str(path))
    assert c["hotkey_capture"] == "<ctrl>+<shift>+a" and c["hotkey_pin"] == "<ctrl>+<shift>+p"
    assert c["color"] == "#00ff00"                                          # other settings are kept


def test_shortcut_permission_checks_are_harmless_off_macos():
    import pinshot.macperm as mp
    import sys as _sys
    if _sys.platform != "darwin":
        assert mp.input_monitoring_ok() is None and mp.accessibility_ok() is None
        assert mp.shortcuts_permitted() is True


def test_ocr_window_runs_the_engine_in_its_worker_thread(qapp):
    """The path a user triggers (window + QThread + Qt event loop), not just the bare function."""
    pytest.importorskip("rapidocr_onnxruntime")
    from pinshot.selftest import run_ocr_window
    text = run_ocr_window().replace(" ", "").lower()
    assert "pinshot" in text or "hello" in text


def test_ocr_error_is_shown_to_the_user(qapp, monkeypatch):
    """When OCR fails the window must say why (a silent failure reads as "OCR does not work")."""
    import time
    from pinshot import ocr
    from pinshot.selftest import make_text_image

    def boom(_img):
        raise RuntimeError("model could not be loaded")

    monkeypatch.setattr(ocr, "recognize", boom)
    win = ocr.OcrWindow(make_text_image(), None)
    end = time.time() + 20
    while time.time() < end and win.status.text() != "OCR başarısız":
        qapp.processEvents()
        time.sleep(0.02)
    assert win.status.text() == "OCR başarısız"
    assert "model could not be loaded" in win.edit.toPlainText()
    win.close()


def test_record_popup_follows_its_content(overlay, monkeypatch):
    """The options panel must grow and shrink with its content (it was sized once and then cut off)."""
    import pinshot.audio as audio
    from pinshot.record_ui import RecordPopup
    monkeypatch.setattr(audio, "probe", lambda: {"system": (True, ""), "mic": (False, "Mikrofon bulunamadı. " + "uzun açıklama " * 12)})
    monkeypatch.setattr(audio, "list_microphones", lambda: [])
    o = overlay
    o.resize(900, 700)
    pop = RecordPopup(o, o.cfg)
    pop.move(40, 40)
    pop.show()
    for _ in range(5):
        QTest.qWait(10)
    h_mp4 = pop.height()
    assert pop.opts.audio_note.isVisible() and h_mp4 >= pop.opts.audio_note.height() + 100
    pop.opts.fmt.setCurrentText("GIF")
    for _ in range(5):
        QTest.qWait(10)
    assert pop.height() < h_mp4, "panel must shrink when the audio options are hidden"
    pop.opts.fmt.setCurrentText("MP4")
    for _ in range(5):
        QTest.qWait(10)
    assert pop.height() == h_mp4, "and grow back"
    assert pop.geometry().bottom() <= o.height() and pop.geometry().right() <= o.width(), "must stay inside the overlay"
    pop.close()
