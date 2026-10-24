"""Screen recording to MP4 or GIF.

Frames are grabbed on the Qt thread, pushed through a *bounded* queue to a writer thread, and streamed into a
separate low-priority ffmpeg process. Nothing accumulates in RAM (a slow encoder just drops frames), and GIFs are
produced from a temporary MP4 with a palette pass, so memory stays flat however long the recording is.
"""
from __future__ import annotations

import logging
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time

from PySide6.QtCore import QObject, QPoint, QRect, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QCursor, QImage, QPainter, QPen, QScreen

log = logging.getLogger(__name__)
GIF_MAX_FPS = 15
GIF_MAX_WIDTH = 960
MP4_MAX_WIDTH = 1920          # frames are shrunk at capture time: keeps RAM and CPU low on 4K/5K screens
QUEUE_FRAMES = 2


def find_ffmpeg() -> str | None:
    env = os.environ.get("PINSHOT_FFMPEG")
    if env and os.path.isfile(env):
        return env
    try:
        import imageio_ffmpeg  # type: ignore
        exe = imageio_ffmpeg.get_ffmpeg_exe()
        if os.path.isfile(exe):
            return exe
    except Exception:  # noqa: BLE001
        pass
    return shutil.which("ffmpeg")


def _spawn_args() -> tuple[list[str], dict]:
    """Run ffmpeg at low priority and (on Windows) without a console window."""
    if sys.platform == "win32":
        return [], {"creationflags": 0x00004000 | 0x08000000}      # BELOW_NORMAL | CREATE_NO_WINDOW
    nice = shutil.which("nice")
    return ([nice, "-n", "10"] if nice else []), {}


def _run(cmd: list[str], **kw) -> subprocess.Popen:
    prefix, extra = _spawn_args()
    return subprocess.Popen(prefix + cmd, **extra, **kw)


def cleanup_stale_temp(max_age_s: int = 86_400) -> None:
    """Remove temp files left behind by a crashed recording (the ffmpeg child exits on its own: its stdin closes)."""
    tmp, now = tempfile.gettempdir(), time.time()
    try:
        names = os.listdir(tmp)
    except OSError:
        return
    for n in names:
        if n.startswith("pinshot-rec-"):
            path = os.path.join(tmp, n)
            try:
                if now - os.path.getmtime(path) > max_age_s:
                    os.remove(path)
            except OSError:
                pass


class Encoder:
    """Raw BGRA frames in -> MP4/GIF file out. push() never blocks; finish() blocks (call off the UI thread)."""

    def __init__(self, exe: str, size: tuple[int, int], fps: int, fmt: str, out_path: str):
        self.exe, self.size, self.fps, self.fmt, self.out = exe, size, fps, fmt, out_path
        self.temp = os.path.join(tempfile.gettempdir(), f"pinshot-rec-{os.getpid()}-{int(time.time()*1000)}.mp4")
        self._err_path = self.temp + ".log"
        self._err = open(self._err_path, "wb")
        self._q: queue.Queue = queue.Queue(maxsize=QUEUE_FRAMES)
        self.dropped = 0
        self.frames = 0
        self._failed = ""
        target = out_path if fmt == "mp4" else self.temp
        crf, preset = ("24", "veryfast") if fmt == "mp4" else ("16", "ultrafast")
        cmd = [exe, "-hide_banner", "-loglevel", "error", "-y",
               "-f", "rawvideo", "-pix_fmt", "bgra", "-video_size", f"{size[0]}x{size[1]}",
               "-use_wallclock_as_timestamps", "1", "-i", "pipe:0",
               "-vf", f"fps={fps},crop=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
               "-c:v", "libx264", "-preset", preset, "-crf", crf, "-threads", "2"]
        if fmt == "mp4":
            cmd += ["-movflags", "+faststart"]
        cmd.append(target)
        self.proc = _run(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=self._err)
        self._writer = threading.Thread(target=self._write_loop, name="pinshot-rec-writer", daemon=True)
        self._writer.start()

    def push(self, data: bytes) -> bool:
        try:
            self._q.put_nowait(data)
            self.frames += 1
            return True
        except queue.Full:
            self.dropped += 1
            return False

    def _write_loop(self) -> None:
        stdin = self.proc.stdin
        while True:
            data = self._q.get()
            if data is None:
                break
            try:
                stdin.write(data)
            except (BrokenPipeError, OSError, ValueError) as ex:
                self._failed = f"encoder stopped: {ex}"
                break
        try:
            stdin.close()
        except OSError:
            pass

    def _cleanup(self) -> None:
        try:
            self._err.close()
        except OSError:
            pass
        for p in (self.temp, self._err_path):
            try:
                os.remove(p)
            except OSError:
                pass

    def _stderr_tail(self) -> str:
        try:
            with open(self._err_path, "rb") as f:
                return f.read()[-600:].decode("utf-8", "replace").strip()
        except OSError:
            return ""

    def abort(self) -> None:
        try:
            self._q.put_nowait(None)
        except queue.Full:
            pass
        try:
            self.proc.kill()
            self.proc.wait(timeout=5)             # reap it: no zombie process left behind
        except (OSError, subprocess.TimeoutExpired):
            pass
        self._writer.join(timeout=5)
        self._cleanup()
        try:
            os.remove(self.out)
        except OSError:
            pass

    def finish(self) -> tuple[bool, str]:
        self._q.put(None)
        self._writer.join(timeout=30)
        try:
            code = self.proc.wait(timeout=120)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            code = -1
        ok, err = code == 0 and not self._failed and self.frames > 0, self._failed or self._stderr_tail()
        if ok and self.fmt == "gif":
            ok, err = self._to_gif()
        self._cleanup()
        if not ok:
            try:
                os.remove(self.out)
            except OSError:
                pass
        return ok, err

    def _to_gif(self) -> tuple[bool, str]:
        fps = min(self.fps, GIF_MAX_FPS)
        vf = (f"fps={fps},scale='min({GIF_MAX_WIDTH},iw)':-2:flags=lanczos,split[a][b];"
              "[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle")
        proc = _run([self.exe, "-hide_banner", "-loglevel", "error", "-y", "-i", self.temp, "-vf", vf,
                     "-loop", "0", self.out], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            _, err = proc.communicate(timeout=600)
        except subprocess.TimeoutExpired:
            proc.kill()
            return False, "GIF conversion timed out"
        return proc.returncode == 0, err.decode("utf-8", "replace")[-600:].strip()


class Recorder(QObject):
    tick = Signal(int)            # elapsed whole seconds
    finished = Signal(str)        # output path
    failed = Signal(str)

    def __init__(self) -> None:
        super().__init__()
        self.enc: Encoder | None = None
        self.screen: QScreen | None = None
        self.region = QRect()
        self.cursor = True
        self.max_seconds = 600
        self.fmt = "mp4"
        self._timer = QTimer(self)
        self._timer.timeout.connect(self._grab)
        self._clock = QTimer(self)
        self._clock.setInterval(1000)
        self._clock.timeout.connect(self._on_clock)
        self._t0 = 0.0

    @property
    def active(self) -> bool:
        return self.enc is not None

    def start(self, screen: QScreen, region: QRect, fmt: str, fps: int, out_path: str,
              max_seconds: int = 600, cursor: bool = True) -> bool:
        exe = find_ffmpeg()
        if not exe:
            self.failed.emit("ffmpeg bulunamadı. Video kaydı için ffmpeg gerekir.")
            return False
        self.screen, self.region, self.cursor, self.max_seconds, self.fmt = screen, region, cursor, max_seconds, fmt
        first = self._frame()
        if first is None or first.isNull():
            self.failed.emit("Ekran yakalanamadı.")
            return False
        try:
            self.enc = Encoder(exe, (first.width(), first.height()), fps, fmt, out_path)
        except OSError as ex:
            self.failed.emit(f"Kodlayıcı başlatılamadı: {ex}")
            return False
        self.enc.push(bytes(first.constBits()))
        self._t0 = time.monotonic()
        self._timer.start(max(10, round(1000 / fps)))
        self._clock.start()
        return True

    def _frame(self) -> QImage | None:
        r, s = self.region, self.screen
        pm = s.grabWindow(0, r.x(), r.y(), r.width(), r.height())
        img = pm.toImage().convertToFormat(QImage.Format.Format_RGB32)
        limit = GIF_MAX_WIDTH if self.fmt == "gif" else MP4_MAX_WIDTH
        if img.width() > limit:
            img = img.scaledToWidth(limit, Qt.TransformationMode.FastTransformation)
        if self.cursor and not img.isNull():
            self._draw_cursor(img, s, r)
        return img

    @staticmethod
    def _draw_cursor(img: QImage, screen: QScreen, region: QRect) -> None:
        pos = QCursor.pos() - screen.geometry().topLeft() - region.topLeft()
        dpr = img.width() / max(1, region.width())
        x, y = pos.x() * dpr, pos.y() * dpr
        if not (0 <= x < img.width() and 0 <= y < img.height()):
            return
        p = QPainter(img)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.setPen(QPen(QColor(255, 255, 255, 230), 2 * dpr))
        p.setBrush(QColor(255, 59, 48, 120))
        p.drawEllipse(QPoint(round(x), round(y)), round(9 * dpr), round(9 * dpr))
        p.end()

    def _grab(self) -> None:
        enc = self.enc
        if enc is None or enc._q.full():                 # encoder is behind: skip, never queue up
            if enc is not None:
                enc.dropped += 1
            return
        img = self._frame()
        if img is None or (img.width(), img.height()) != enc.size:
            return
        enc.push(bytes(img.constBits()))

    def _on_clock(self) -> None:
        elapsed = int(time.monotonic() - self._t0)
        self.tick.emit(elapsed)
        if elapsed >= self.max_seconds:
            self.stop()

    def stop(self) -> None:
        enc, self.enc = self.enc, None
        if enc is None:
            return
        self._timer.stop()
        self._clock.stop()

        def work() -> None:
            ok, err = enc.finish()
            if ok:
                self.finished.emit(enc.out)
            else:
                self.failed.emit(f"Kayıt başarısız: {err or 'bilinmeyen hata'}")
        threading.Thread(target=work, name="pinshot-rec-finish", daemon=True).start()

    def cancel(self) -> None:
        enc, self.enc = self.enc, None
        self._timer.stop()
        self._clock.stop()
        if enc is not None:
            enc.abort()
