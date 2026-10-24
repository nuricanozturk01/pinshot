"""Tray application: hotkeys, single instance, capture flows. Entry point: `python -m pinshot` / `pinshot`."""
from __future__ import annotations

import logging
import logging.handlers
import os
import signal
import sys
import traceback

from PySide6.QtCore import QCoreApplication, QPoint, QTimer, QUrl
from PySide6.QtGui import QAction, QCursor, QDesktopServices, QGuiApplication, QImage
from PySide6.QtWidgets import QApplication, QFileDialog, QMenu, QMessageBox, QSystemTrayIcon

from . import APP_NAME, __version__, output, pin
from .capture import Capture, grab_screens, looks_blank, mac_screen_access, screen_under_cursor
from .config import Config, log_dir
from .hotkeys import HotkeyManager
from .icons import app_icon
from .ipc import CommandServer, send_command
from .overlay import start_overlays
from .settings_dialog import SettingsDialog

log = logging.getLogger("pinshot")
HOTKEY_ACTIONS = {"hotkey_capture": "capture", "hotkey_fullscreen": "fullscreen",
                  "hotkey_repeat": "repeat", "hotkey_pin": "pin_clipboard"}
COMMANDS = {"--capture": "capture", "--fullscreen": "fullscreen", "--repeat": "repeat",
            "--pin-clipboard": "pin_clipboard", "--settings": "settings", "--quit": "quit"}


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
        self.settings: SettingsDialog | None = None
        self.tray = QSystemTrayIcon(app_icon(), qapp)
        self.tray.setToolTip(APP_NAME)
        self._build_menu()
        self.tray.activated.connect(self._tray_clicked)
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
            a.triggered.connect(lambda _=False, n=name: self.dispatch(n))
            (parent or m).addAction(a)

        add("Bölge yakala", "capture")
        delay = m.addMenu("Gecikmeli yakala")
        for s in (3, 5, 10):
            add(f"{s} saniye", f"delay:{s}", delay)
        add("Tam ekran yakala", "fullscreen")
        add("Son bölgeyi tekrarla", "repeat")
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
        self.hotkeys.start({self.cfg[k]: a for k, a in HOTKEY_ACTIONS.items()})

    # ------------------------------------------------------------ actions
    def dispatch(self, name: str) -> None:
        try:
            if name == "capture":
                self.capture()
            elif name.startswith("delay:"):
                secs = int(name.split(":")[1])
                self.notify(f"{secs} saniye sonra yakalanacak")
                QTimer.singleShot(secs * 1000, self.capture)
            elif name == "fullscreen":
                QTimer.singleShot(200, self.capture_fullscreen)
            elif name == "repeat":
                QTimer.singleShot(200, self.capture_last_region)
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
                QMessageBox.about(None, APP_NAME, f"{APP_NAME} {__version__}\nEkran görüntüsü, çizim, OCR ve "
                                                  f"ekrana sabitleme.\nGünlükler: {log_dir()}")
            elif name == "quit":
                self.quit()
        except Exception:  # noqa: BLE001
            log.exception("action %s failed", name)
            self.notify(f"Hata: {name} başarısız. Ayrıntılar günlükte.")

    def _grab(self) -> list[Capture] | None:
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

    def capture(self) -> None:
        if any(o.isVisible() for o in self.overlays if _alive(o)):
            return
        QTimer.singleShot(150, self._capture_now)       # let menus / hotkey UI disappear first

    def _capture_now(self) -> None:
        caps = self._grab()
        if caps:
            self.overlays = start_overlays(caps, self.cfg, self.notify)

    def _finish_image(self, img: QImage, what: str) -> None:
        QGuiApplication.clipboard().setImage(img)
        msg = f"{what} panoya kopyalandı ({img.width()}×{img.height()})"
        if self.cfg["auto_save"]:
            path = output.unique_path(os.path.join(self.cfg.save_dir(), output.timestamp_name(self.cfg["format"])))
            ok, err = output.save_image(img, path, self.cfg["quality"])
            msg += f" · kaydedildi: {os.path.basename(path)}" if ok else f" · kaydedilemedi: {err}"
        self.notify(msg)

    def capture_fullscreen(self) -> None:
        caps = self._grab()
        if caps:
            self._finish_image(screen_under_cursor(caps, QCursor.pos()).image, "Tam ekran")

    def capture_last_region(self) -> None:
        r = self.cfg["last_region"]
        if not r:
            self.notify("Önce bir bölge yakalayın")
            return
        caps = self._grab()
        if not caps:
            return
        name, x, y, w, h = r
        cap = next((c for c in caps if c.screen.name() == name), None) or caps[0]
        rect = QPoint(round((x - cap.geometry.x()) * cap.scale), round((y - cap.geometry.y()) * cap.scale))
        img = cap.image.copy(rect.x(), rect.y(), round(w * cap.scale), round(h * cap.scale))
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
        self.settings = SettingsDialog(self.cfg)
        self.settings.applied.connect(self.reload_hotkeys)
        self.settings.finished.connect(lambda _: setattr(self, "settings", None))
        self.settings.show()

    def quit(self) -> None:
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
    server = CommandServer()
    if not server.listen():
        log.warning("IPC server could not listen; single-instance guard disabled")
    app = App(qapp)
    server.command.connect(lambda c: app.dispatch(c) if c != "ping" else app.notify("PinShot zaten çalışıyor"))
    signal.signal(signal.SIGINT, lambda *_: app.quit())
    keepalive = QTimer()                                    # lets Python handle SIGINT while Qt runs
    keepalive.start(500)
    keepalive.timeout.connect(lambda: None)
    if cmd:
        QTimer.singleShot(300, lambda: app.dispatch(cmd))
    code = qapp.exec()
    server.server.close()
    return code


if __name__ == "__main__":
    sys.exit(main())
