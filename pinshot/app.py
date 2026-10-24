"""Tray application: hotkeys, single instance, capture flows. Entry point: `python -m pinshot` / `pinshot`."""
from __future__ import annotations

import gc
import logging
import logging.handlers
import os
import signal
import sys
import traceback

from PySide6.QtCore import QCoreApplication, QMimeData, QPoint, QRect, QRectF, QTimer, QUrl
from PySide6.QtGui import QAction, QCursor, QDesktopServices, QGuiApplication, QImage
from PySide6.QtWidgets import QApplication, QFileDialog, QMenu, QMessageBox, QSystemTrayIcon

from . import APP_NAME, __version__, output, pin
from .config import Config, log_dir
from .hotkeys import HotkeyManager
from .icons import app_icon, recording_icon
from .ipc import CommandServer, send_command
from .memory import release_memory, tune_malloc
# Rarely used parts (capture, editor, recording, settings) are imported on first use: a tray app that sits in the
# background for days should not keep their code and Qt widgets resident until they are needed.

log = logging.getLogger("pinshot")
HOTKEY_ACTIONS = {"hotkey_capture": "capture", "hotkey_fullscreen": "fullscreen",
                  "hotkey_repeat": "repeat", "hotkey_pin": "pin_clipboard",
                  "hotkey_record": "record"}
COMMANDS = {"--capture": "capture", "--fullscreen": "fullscreen", "--repeat": "repeat",
            "--pin-clipboard": "pin_clipboard", "--record": "record", "--stop-recording": "stop_record",
            "--settings": "settings", "--quit": "quit"}


def setup_logging() -> None:
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    fmt = logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s")
    try:
        fh = logging.handlers.RotatingFileHandler(os.path.join(log_dir(), "pinshot.log"), maxBytes=1_000_000,
                                                  backupCount=3, encoding="utf-8")
        fh.setFormatter(fmt)
        root.addHandler(fh)
    except OSError:
        pass
    if sys.stderr is not None:           # windowed PyInstaller builds have no stderr
        sh = logging.StreamHandler()
        sh.setFormatter(fmt)
        root.addHandler(sh)


class App:
    def __init__(self, qapp: QApplication):
        self.qapp = qapp
        self.cfg = Config()
        pin.PinWindow.config = self.cfg
        self.overlays: list = []
        self.settings = None
        self._recorder = None
        self.rec_frame = None
        self.rec_bar = None
        self.rec_toggle: QAction | None = None
        self.selectors: list = []
        self.last_output = ""
        self._clip_dirty = False
        self._setting_clip = False
        QGuiApplication.clipboard().dataChanged.connect(self._clip_changed)
        self.tray = QSystemTrayIcon(app_icon(), qapp)
        self.tray.setToolTip(APP_NAME)
        self._build_menu()
        self.tray.activated.connect(self._tray_clicked)
        self.tray.messageClicked.connect(self._reveal_output)
        self.tray.show()
        self.hotkeys = HotkeyManager()
        self.hotkeys.triggered.connect(self.dispatch)
        self.hotkeys.failed.connect(self._hotkeys_failed)
        self.reload_hotkeys()
        sys.excepthook = self._excepthook

    # ------------------------------------------------------------ UI
    def _build_menu(self) -> None:
        m = QMenu()
        self.menu = m

        def add(label: str, name: str, parent: QMenu | None = None):
            a = QAction(label, m)
            a.triggered.connect(lambda _=False, n=name: self.dispatch(n, True))
            (parent or m).addAction(a)

        add("Bölge yakala", "capture")
        delay = m.addMenu("Gecikmeli yakala")
        for s in (3, 5, 10):
            add(f"{s} saniye", f"delay:{s}", delay)
        add("Tam ekran yakala", "fullscreen")
        add("Son bölgeyi tekrarla", "repeat")
        m.addSeparator()
        add("Ekranı video olarak kaydet (GIF / MP4)…", "record")
        self.rec_toggle = m.actions()[-1]
        m.addSeparator()
        add("Panodan sabitle", "pin_clipboard")
        add("Dosyadan sabitle…", "pin_file")
        add("Tüm sabitlenenleri kapat", "close_pins")
        m.addSeparator()
        add("Kayıt klasörünü aç", "open_folder")
        add("Ayarlar…", "settings")
        add("Hakkında", "about")
        m.addSeparator()
        add("Çıkış", "quit")
        self.tray.setContextMenu(m)

    def _tray_clicked(self, reason) -> None:
        if reason == QSystemTrayIcon.ActivationReason.Trigger and sys.platform != "darwin":
            self.dispatch("capture")

    def notify(self, msg: str) -> None:
        self.tray.showMessage(APP_NAME, msg, app_icon(), 3000)

    def _hotkeys_failed(self, why: str) -> None:
        self.notify("Global kısayollar başlatılamadı (macOS: Erişilebilirlik izni; Wayland desteklenmez). "
                    "Tray menüsünü veya 'pinshot --capture' komutunu kullanın.")

    def reload_hotkeys(self) -> None:
        if sys.platform == "darwin":
            from . import macperm
            if macperm.input_monitoring_ok(request=True) is False or macperm.accessibility_ok() is False:
                # macOS gives no error: without these permissions the shortcuts simply never fire
                self.notify(macperm.SHORTCUTS_TEXT)
                if self.cfg["mac_permission_help_shown"] is False:
                    self.cfg["mac_permission_help_shown"] = True
                    self.cfg.save()
                    macperm.open_pane(macperm.INPUT_URL)
        self.hotkeys.start({self.cfg[k]: a for k, a in HOTKEY_ACTIONS.items()})

    # ------------------------------------------------------------ actions
    def dispatch(self, name: str, from_menu: bool = False) -> None:
        lag = 150 if from_menu else 40        # a tray menu needs time to disappear; a hotkey does not
        try:
            if name == "capture":
                self.capture(lag)
            elif name.startswith("delay:"):
                secs = int(name.split(":")[1])
                self.notify(f"{secs} saniye sonra yakalanacak")
                QTimer.singleShot(secs * 1000, lambda: self.capture(40))
            elif name == "fullscreen":
                QTimer.singleShot(lag + 50, self.capture_fullscreen)
            elif name == "repeat":
                QTimer.singleShot(lag + 50, self.capture_last_region)
            elif name == "record":
                self.toggle_recording(lag)
            elif name == "stop_record":
                self.stop_recording()
            elif name == "pin_clipboard":
                if not pin.pin_clipboard():
                    self.notify("Panoda sabitlenecek görsel veya metin yok")
            elif name == "pin_file":
                self.pin_file()
            elif name == "close_pins":
                pin.close_all()
            elif name == "open_folder":
                QDesktopServices.openUrl(QUrl.fromLocalFile(self.cfg.save_dir()))
            elif name == "settings":
                self.open_settings()
            elif name == "about":
                self.about()
            elif name == "quit":
                self.quit()
        except Exception:  # noqa: BLE001
            log.exception("action %s failed", name)
            self.notify(f"Hata: {name} başarısız. Ayrıntılar günlükte.")

    def _grab(self):
        from .capture import grab_screens, looks_blank, mac_screen_access
        if not mac_screen_access():
            self.notify("macOS: Sistem Ayarları › Gizlilik › Ekran Kaydı'nda PinShot'a izin verin, sonra yeniden başlatın.")
            return None
        caps = grab_screens()
        if caps and all(looks_blank(c.image) for c in caps):
            if sys.platform.startswith("linux") and QGuiApplication.platformName().startswith("wayland"):
                self.notify("Wayland: ekran yakalanamadı. xdg-desktop-portal / grim / spectacle gerekli.")
            else:
                self.notify("Ekran yakalanamadı (görüntü boş). İzinleri kontrol edin.")
            return None
        return caps

    def capture(self, lag: int = 150) -> None:
        if any(o.isVisible() for o in self.overlays if _alive(o)):
            return
        QTimer.singleShot(lag, self._capture_now)

    def _capture_now(self) -> None:
        caps = self._grab()
        if caps:
            from .overlay import start_overlays
            self.overlays = start_overlays(caps, self.cfg, self.notify)
            for o in self.overlays:
                o.closed.connect(self._overlay_closed)
                o.record.connect(self._schedule_recording)

    def _overlay_closed(self) -> None:
        self.overlays = []                     # drop the references so the screenshots can be freed
        QTimer.singleShot(500, release_memory)
        QTimer.singleShot(3000, release_memory)   # again after Qt's deferred widget deletion has run

    def _finish_image(self, img: QImage, what: str) -> None:
        QGuiApplication.clipboard().setImage(img)
        QTimer.singleShot(500, release_memory)
        msg = f"{what} panoya kopyalandı ({img.width()}×{img.height()})"
        if self.cfg["auto_save"]:
            path = output.unique_path(os.path.join(self.cfg.save_dir(), output.timestamp_name(self.cfg["format"])))
            ok, err = output.save_image(img, path, self.cfg["quality"])
            msg += f" · kaydedildi: {os.path.basename(path)}" if ok else f" · kaydedilemedi: {err}"
        self.notify(msg)

    def capture_fullscreen(self) -> None:
        caps = self._grab()
        if caps:
            from .capture import screen_under_cursor
            self._finish_image(screen_under_cursor(caps, QCursor.pos()).image, "Tam ekran")

    def capture_last_region(self) -> None:
        r = self.cfg["last_region"]
        if not r:
            self.notify("Önce bir bölge yakalayın")
            return
        caps = self._grab()
        if not caps:
            return
        from .capture import compose_region
        _name, x, y, w, h = r                                   # global desktop coordinates (may span monitors)
        img = compose_region(caps, QRectF(x, y, w, h))
        if img.isNull():
            self.notify("Son bölge geçersiz")
            return
        self._finish_image(img, "Son bölge")

    def pin_file(self) -> None:
        paths, _ = QFileDialog.getOpenFileNames(None, "Sabitlenecek görseller", "",
                                                "Görseller (*.png *.jpg *.jpeg *.bmp *.gif *.webp *.tif *.tiff *.svg)")
        for p in paths:
            if not pin.pin_file(p):
                self.notify(f"Açılamadı: {os.path.basename(p)}")

    def open_settings(self) -> None:
        if self.settings is not None:
            self.settings.raise_()
            self.settings.activateWindow()
            return
        from .settings_dialog import SettingsDialog
        self.settings = SettingsDialog(self.cfg)
        self.settings.applied.connect(self.reload_hotkeys)
        self.settings.finished.connect(lambda _: setattr(self, "settings", None))
        self.settings.show()

    # ------------------------------------------------------------ screen recording
    @property
    def recorder(self):
        if self._recorder is None:
            from .recorder import Recorder
            r = Recorder()
            r.tick.connect(self._rec_tick)
            r.finished.connect(self._rec_finished)
            r.failed.connect(self._rec_failed)
            r.warning.connect(self.notify)
            self._recorder = r
        return self._recorder

    def _recording_active(self) -> bool:
        return self._recorder is not None and self._recorder.active

    def toggle_recording(self, lag: int = 150) -> None:
        if self._recording_active():
            self.stop_recording()
        else:
            self.pick_recording_area(lag)

    def pick_recording_area(self, lag: int = 150) -> None:
        if self.selectors or any(_alive(o) and o.isVisible() for o in self.overlays):
            return
        from .recorder import find_ffmpeg
        if find_ffmpeg() is None:
            self.notify("Video kaydı için ffmpeg bulunamadı.")
            return
        if QGuiApplication.platformName().startswith("wayland"):
            self.notify("Wayland'de ekran kaydı desteklenmiyor. X11 / XWayland oturumu gerekir.")
            return
        QTimer.singleShot(lag, self._open_selectors)

    def _open_selectors(self) -> None:
        caps = self._grab()
        if not caps:
            return
        from .overlay import start_overlays
        from .record_ui import RecordSelector
        self.selectors = start_overlays(caps, self.cfg, self.notify, factory=RecordSelector)
        for o in self.selectors:
            o.chosen.connect(self._schedule_recording)
            o.closed.connect(self._selector_closed)

    def _selector_closed(self) -> None:
        self.selectors = []
        QTimer.singleShot(500, release_memory)

    def _schedule_recording(self, info: dict) -> None:
        QTimer.singleShot(350, lambda: self._start_recording(info))   # let the picker/overlay vanish first

    def _start_recording(self, info: dict) -> None:
        from .record_ui import RecordingFrame, RecordingIsland
        screen, region, fmt, fps = info["screen"], info["region"], info["fmt"], info["fps"]
        width = info.get("max_width")
        self._clip_dirty = False          # did the user copy something while we record?
        path = output.unique_path(os.path.join(self.cfg.save_dir(), output.timestamp_name(fmt)))
        if not self.recorder.start(screen, region, fmt, fps, path, int(self.cfg["record_max_seconds"]),
                                   bool(self.cfg["record_cursor"]), width,
                                   bool(info.get("audio_system")), bool(info.get("audio_mic")),
                                   str(info.get("mic_device") or ""), bool(info.get("hq444"))):
            return
        g = QRect(region)                                       # global desktop coordinates, may span monitors
        involved = [sc for sc in QGuiApplication.screens() if sc.geometry().intersects(g)] or [screen]
        union = QRect()
        for sc in involved:
            union = union.united(sc.geometry())
        self.rec_frame = RecordingFrame(g, union)
        self.rec_frame.show()
        island = RecordingIsland(fmt.upper(), min(fps, 50) if fmt == "gif" else fps)
        island.stop_requested.connect(self.stop_recording)
        island.moved.connect(self._island_moved)
        screens = [sc.geometry() for sc in QGuiApplication.screens()]
        saved = self.cfg["island_pos"]
        if saved and island.is_on_screen(QPoint(*saved), screens):
            island.move(QPoint(*saved))                      # the user placed it there last time
        else:
            island.place_default(g, union, [sc.geometry() for sc in QGuiApplication.screens() if sc not in involved])
        island.show()
        self.rec_bar = island
        # the island may be dragged over the recorded area: the recorder paints it out of the video
        self.recorder.exclude = lambda: island.frameGeometry() if island.isVisible() else None
        self.tray.setIcon(recording_icon())
        self.tray.setToolTip("PinShot – kayıt sürüyor")
        if self.rec_toggle:
            self.rec_toggle.setText("Kaydı durdur")
        # No "recording started" notification: on macOS it is a large banner at the top right that ends up IN the video
        # (and the island + red tray icon already show that a recording is running).

    def _island_moved(self, pos: QPoint) -> None:
        self.cfg["island_pos"] = [pos.x(), pos.y()]
        self.cfg.save()

    def _rec_tick(self, secs: int) -> None:
        if self.rec_bar:
            self.rec_bar.set_elapsed(secs)

    def stop_recording(self) -> None:
        if self._recording_active():
            self.recorder.stop()                # first stop capturing, so nothing we show can appear in the video
            self.notify("Kayıt kaydediliyor…")
        self._end_recording_ui()

    def _end_recording_ui(self) -> None:
        if self._recorder is not None:
            self._recorder.exclude = None            # the island is about to be destroyed
        for w in (self.rec_frame, self.rec_bar):
            if w is not None:
                w.close()
                w.deleteLater()
        self.rec_frame = self.rec_bar = None
        self.tray.setIcon(app_icon())
        self.tray.setToolTip(APP_NAME)
        if self.rec_toggle:
            self.rec_toggle.setText("Ekranı video olarak kaydet (GIF / MP4)…")
        QTimer.singleShot(1000, lambda: release_memory(True))

    def _clip_changed(self) -> None:
        if not self._setting_clip:
            self._clip_dirty = True

    def _rec_finished(self, path: str) -> None:
        self.last_output = path
        copied = not self._clip_dirty
        if copied:                    # paste the file into a chat / e-mail, unless the user copied something newer
            mime = QMimeData()
            mime.setUrls([QUrl.fromLocalFile(path)])
            self._setting_clip = True
            QGuiApplication.clipboard().setMimeData(mime)
            self._setting_clip = False
        size = os.path.getsize(path) / 1024 / 1024
        st = self._recorder.last_stats if self._recorder else {}
        extra = ""
        if st:
            achieved = st["frames"] / st["seconds"]
            target = min(st["fps"], 50) if st["fmt"] == "gif" else st["fps"]
            if achieved < target * 0.85:                      # be honest when the machine could not keep up
                extra = f" Bilgisayar {target} fps'i yakalayamadı (ortalama {achieved:.0f} fps)."
        where = "dosya panoya kopyalandı" if copied else "panonuza dokunulmadı"
        self.notify(f"Kayıt hazır ({size:.1f} MB), {where}.{extra} Klasörü açmak için tıkla.")
        release_memory(True)

    def _rec_failed(self, msg: str) -> None:
        self._end_recording_ui()
        self.notify(msg)

    def _reveal_output(self) -> None:
        if self.last_output:
            QDesktopServices.openUrl(QUrl.fromLocalFile(os.path.dirname(self.last_output)))

    def about(self) -> None:
        from PySide6.QtCore import Qt
        from PySide6.QtWidgets import QLabel
        box = QMessageBox()
        box.setWindowTitle(f"{APP_NAME} hakkında")
        box.setIconPixmap(app_icon().pixmap(64, 64))
        box.setTextFormat(Qt.TextFormat.RichText)
        box.setText(f"<h3>{APP_NAME} {__version__}</h3>"
                    "Created by <b>Nuri Can OZTURK</b><br>"
                    '<a href="https://github.com/nuricanozturk01">https://github.com/nuricanozturk01</a>'
                    f"<br><br><small>Ekran görüntüsü, çizim, OCR, ekrana sabitleme ve ekran kaydı.<br>"
                    f"Günlükler: {log_dir()}</small>")
        for lab in box.findChildren(QLabel):
            lab.setOpenExternalLinks(True)
            lab.setTextInteractionFlags(Qt.TextInteractionFlag.TextBrowserInteraction)
        box.exec()

    def quit(self) -> None:
        if self._recorder is not None:
            self._recorder.cancel()
        self.hotkeys.stop()
        pin.close_all()
        self.cfg.save()
        self.tray.hide()
        self.qapp.quit()

    def _excepthook(self, et, ev, tb) -> None:
        log.error("unhandled exception:\n%s", "".join(traceback.format_exception(et, ev, tb)))
        self.notify(f"Beklenmeyen hata: {ev}")


def _alive(w) -> bool:
    try:
        w.isVisible()
        return True
    except RuntimeError:
        return False


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv if argv is None else argv)
    if "--version" in argv:
        print(f"{APP_NAME} {__version__}")
        return 0
    if "--selftest" in argv:
        from .selftest import run
        i = argv.index("--selftest")
        return run(argv[i + 1] if i + 1 < len(argv) else None)
    tune_malloc()                      # before Qt/pynput start threads
    # PySide objects must be destroyed on the main thread, but Python's cyclic GC can run in any thread. A very high
    # threshold makes automatic runs rare; release_memory() collects on the main thread after every capture/recording.
    gc.set_threshold(50_000, 20, 20)
    setup_logging()
    QCoreApplication.setApplicationName(APP_NAME)
    QCoreApplication.setOrganizationName(APP_NAME)
    qapp = QApplication(argv)
    qapp.setQuitOnLastWindowClosed(False)
    qapp.setWindowIcon(app_icon())

    cmd = next((v for k, v in COMMANDS.items() if k in argv), "")
    if cmd or "--now" in argv:
        cmd = cmd or "capture"
    if send_command(cmd or "ping"):                        # another instance is already running
        return 0
    if cmd == "quit":
        return 0
    if not QSystemTrayIcon.isSystemTrayAvailable():
        log.warning("no system tray available; hotkeys and CLI commands still work")
    from .recorder import cleanup_stale_temp
    cleanup_stale_temp()
    server = CommandServer()
    if not server.listen():
        log.warning("IPC server could not listen; single-instance guard disabled")
    app = App(qapp)
    QTimer.singleShot(4000, lambda: release_memory(True))               # drop start-up garbage (matters most on Windows)
    server.command.connect(lambda c: app.dispatch(c) if c != "ping" else app.notify("PinShot zaten çalışıyor"))
    signal.signal(signal.SIGINT, lambda *_: app.quit())
    keepalive = QTimer()                                    # only for Ctrl+C in a terminal: no idle wake-ups otherwise
    if sys.stdin is not None and sys.stdin.isatty():
        keepalive.start(1000)
        keepalive.timeout.connect(lambda: None)
    if cmd:
        QTimer.singleShot(300, lambda: app.dispatch(cmd))
    code = qapp.exec()
    server.server.close()
    return code


if __name__ == "__main__":
    sys.exit(main())
