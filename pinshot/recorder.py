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
from PySide6.QtGui import QColor, QCursor, QGuiApplication, QImage, QPainter, QPen, QRegion, QScreen

log = logging.getLogger(__name__)
GIF_MAX_FPS = 50              # GIF stores delays in 1/100 s and browsers slow down anything under 20 ms
DEFAULT_WIDTH = {"gif": 1920, "mp4": 0}      # max output width in px (0 = original); ffmpeg does the scaling
QUEUE_FRAMES = 3


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


class IslandMask:
    """Hides one of our own windows from recorded frames.

    A screen grab contains every window, including the recording island. When it overlaps the recorded area, its
    pixels are replaced by what was there just before the island covered them; everything around it stays live."""

    def __init__(self) -> None:
        self._clean: QImage | None = None

    def process(self, img: QImage, hidden: QRect | None) -> QImage:
        if hidden is not None:
            hidden = hidden.intersected(img.rect())
        if hidden is None or hidden.isEmpty() or self._clean is None or self._clean.size() != img.size():
            self._clean = img                       # nothing to hide: remember this frame, no copy
            return img
        self._clean = QImage(self._clean)           # share + detach on paint: the old buffer may still be queued
        p = QPainter(self._clean)                   # refresh the "clean" copy everywhere except under the island
        p.setClipRegion(QRegion(img.rect()).subtracted(QRegion(hidden)))
        p.drawImage(0, 0, img)
        p.end()
        q = QPainter(img)
        q.drawImage(hidden, self._clean, hidden)    # patch the island area with the clean pixels
        q.end()
        return img


class Encoder:
    """Raw BGRA frames in -> MP4/GIF file out. push() never blocks; finish() blocks (call off the UI thread).

    Frames arrive at full resolution; any downscaling is done here by ffmpeg (lanczos), which is far sharper than
    scaling in Qt. Quality settings favour legible text: CRF 18, BT.709-tagged colour, even dimensions."""

    def __init__(self, exe: str, size: tuple[int, int], fps: int, fmt: str, out_path: str, max_width: int = 0,
                 with_audio: bool = False, hq444: bool = False):
        self.exe, self.size, self.fps, self.fmt, self.out = exe, size, fps, fmt, out_path
        self.temp = os.path.join(tempfile.gettempdir(), f"pinshot-rec-{os.getpid()}-{int(time.time()*1000)}.mp4")
        self._err_path = self.temp + ".log"
        self._err = open(self._err_path, "wb")
        self._q: queue.Queue = queue.Queue(maxsize=QUEUE_FRAMES)
        self.dropped = 0
        self.frames = 0
        self._failed = ""
        # video goes to a temp file first when it will be post-processed (GIF palette pass, or audio mux)
        target = out_path if (fmt == "mp4" and not with_audio) else self.temp
        crf, preset = ("18", "veryfast") if fmt == "mp4" and not with_audio else ("17", "veryfast") if fmt == "mp4" \
            else ("12", "ultrafast")
        scale = "scale=out_color_matrix=bt709:out_range=tv:flags=lanczos+accurate_rnd+full_chroma_int"
        if max_width:
            scale = (f"scale='min({int(max_width)},iw)':-2:out_color_matrix=bt709:out_range=tv:"
                     "flags=lanczos+accurate_rnd+full_chroma_int")
        threads = str(max(2, (os.cpu_count() or 4) // 2))
        # 4:2:0 halves the colour resolution: coloured text and UI edges blur and PSNR stalls near 39 dB whatever the
        # CRF. 4:4:4 keeps full colour detail (+10 dB at the same size) but some players (QuickTime/Safari, TVs,
        # phones) cannot open it, so it is a user choice. Temp files for GIFs always use it (never played).
        pix = "yuv444p" if (hq444 or fmt == "gif") else "yuv420p"
        cmd = [exe, "-hide_banner", "-loglevel", "error", "-y",
               "-f", "rawvideo", "-pix_fmt", "bgra", "-video_size", f"{size[0]}x{size[1]}",
               "-use_wallclock_as_timestamps", "1", "-i", "pipe:0",
               "-vf", f"fps={fps},crop=trunc(iw/2)*2:trunc(ih/2)*2,{scale},format={pix}",
               "-c:v", "libx264", "-preset", preset, "-crf", crf, "-threads", threads,
               "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]
        if target == out_path:
            cmd += ["-movflags", "+faststart"]
        cmd.append(target)
        self.proc = _run(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=self._err)
        self._writer = threading.Thread(target=self._write_loop, name="pinshot-rec-writer", daemon=True)
        self._writer.start()

    def push(self, data) -> bool:
        """`data`: bytes, or a QImage (preferred: no copy on the UI thread; keep it unmodified until written)."""
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
                stdin.write(data.constBits() if hasattr(data, "constBits") else data)
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

    def finish(self, tracks: list | None = None, t0: float = 0.0) -> tuple[bool, str]:
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
        elif ok and tracks:
            ok, err = self._mux_audio(tracks, t0)
        self._cleanup()
        if not ok:
            try:
                os.remove(self.out)
            except OSError:
                pass
        return ok, err

    def _to_gif(self) -> tuple[bool, str]:
        fps = min(self.fps, GIF_MAX_FPS)
        # one palette per frame (new=1): far better colours when the content changes, e.g. video or photos on screen
        vf = (f"fps={fps},split[a][b];[a]palettegen=max_colors=256:stats_mode=single[p];"
              "[b][p]paletteuse=new=1:dither=sierra2_4a:diff_mode=rectangle")
        proc = _run([self.exe, "-hide_banner", "-loglevel", "error", "-y", "-i", self.temp, "-vf", vf,
                     "-loop", "0", self.out], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            _, err = proc.communicate(timeout=600)
        except subprocess.TimeoutExpired:
            proc.kill()
            return False, "GIF conversion timed out"
        return proc.returncode == 0, err.decode("utf-8", "replace")[-600:].strip()

    def _video_duration(self) -> float:
        """Duration of the encoded temp video in seconds (parsed from ffmpeg's banner)."""
        import re
        r = subprocess.run([self.exe, "-hide_banner", "-i", self.temp], capture_output=True, text=True, timeout=30)
        m = re.search(r"Duration: (\d+):(\d+):([\d.]+)", r.stderr)
        if not m:
            return 0.0
        return int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))

    def _mux_audio(self, tracks: list, t0: float) -> tuple[bool, str]:
        """Add the recorded PCM tracks to the video (video stream is copied, not re-encoded).

        The audio is padded with silence to exactly the video length and the output is cut there. (Padding to
        infinity and relying on `-shortest` can hang ffmpeg, so the length is always finite.)"""
        from .audio import RATE
        dur = self._video_duration()
        if dur <= 0:
            return False, "could not read the video length"

        def build(fc: str) -> list[str]:
            cmd = [self.exe, "-hide_banner", "-loglevel", "error", "-y", "-i", self.temp]
            for t in tracks:
                off = t.started - t0                      # >0: audio opened after the first frame; <0: before it
                pre = ["-f", "s16le", "-ar", str(RATE), "-ac", str(t.channels)]
                if off > 0.001:
                    pre += ["-itsoffset", f"{off:.3f}"]
                elif off < -0.001:
                    pre += ["-ss", f"{-off:.3f}"]
                cmd += pre + ["-i", t.path]
            return cmd + ["-filter_complex", fc, "-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac",
                          "-b:a", "192k", "-ar", "48000", "-ac", "2", "-t", f"{dur:.3f}",
                          "-movflags", "+faststart", self.out]

        pad = f"apad=whole_dur={dur:.3f}"
        if len(tracks) == 1:
            attempts = [f"[1:a]{pad}[a]"]
        else:                                             # `normalize` needs ffmpeg >= 4.4: fall back if unsupported
            attempts = [f"[1:a][2:a]amix=inputs=2:duration=longest:normalize=0,alimiter=limit=0.97,{pad}[a]",
                        f"[1:a][2:a]amix=inputs=2:duration=longest,{pad}[a]"]
        err = ""
        for fc in attempts:
            try:
                r = subprocess.run(build(fc), stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=120)
            except subprocess.TimeoutExpired:
                err = "audio mux timed out"
                continue
            if r.returncode == 0:
                return True, ""
            err = r.stderr.decode("utf-8", "replace")[-600:].strip()
        return False, err


class Recorder(QObject):
    tick = Signal(int)            # elapsed whole seconds
    finished = Signal(str)        # output path
    failed = Signal(str)
    warning = Signal(str)         # non-fatal (e.g. the microphone could not be opened)

    def __init__(self) -> None:
        super().__init__()
        self.enc: Encoder | None = None
        self.screen: QScreen | None = None
        self.region = QRect()
        self.cursor = True
        self.max_seconds = 600
        self.fmt = "mp4"
        self.fps = 15
        self.max_width = 1920
        self.exclude = None              # callable -> QRect (global logical coords) of a window to hide, or None
        self._mask = IslandMask()
        self.last_stats: dict = {}
        self._audio = None
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
              max_seconds: int = 600, cursor: bool = True, max_width: int | None = None,
              audio_system: bool = False, audio_mic: bool = False, mic_device: str = "",
              hq444: bool = False) -> bool:
        exe = find_ffmpeg()
        if not exe:
            self.failed.emit("ffmpeg bulunamadı. Video kaydı için ffmpeg gerekir.")
            return False
        # `region` is in GLOBAL desktop coordinates and may span several monitors
        self.screen, self.region, self.cursor, self.max_seconds, self.fmt = screen, QRect(region), cursor, max_seconds, fmt
        self._parts = [(sc, sc.geometry().intersected(self.region)) for sc in QGuiApplication.screens()
                       if not sc.geometry().intersected(self.region).isEmpty()]
        self.fps = fps
        self.max_width = DEFAULT_WIDTH.get(fmt, 1920) if max_width is None else int(max_width)   # 0 = original size
        audio = None
        if fmt == "mp4" and (audio_system or audio_mic):       # GIFs have no sound
            from .audio import AudioRecorder
            audio = AudioRecorder(audio_system, audio_mic, mic_device)
            for w in audio.start():
                self.warning.emit(w)
            if not audio.tracks:                                 # every source failed: record video only
                audio = None
        first = self._frame()
        if first is None or first.isNull():
            if audio:
                audio.finish()
            self.failed.emit("Ekran yakalanamadı.")
            return False
        try:
            self.enc = Encoder(exe, (first.width(), first.height()), fps, fmt, out_path, self.max_width,
                               with_audio=audio is not None, hq444=hq444)
        except OSError as ex:
            if audio:
                audio.finish()
            self.failed.emit(f"Kodlayıcı başlatılamadı: {ex}")
            return False
        self._audio = audio
        self.enc.push(first)
        self._t0 = time.monotonic()
        self._timer.start(max(10, round(1000 / fps)))
        self._clock.start()
        return True

    def _grab_region(self) -> QImage:
        r = self.region
        if len(self._parts) == 1 and self._parts[0][1] == r:        # common case: one monitor, one grab
            sc = self._parts[0][0]
            g = sc.geometry()
            return sc.grabWindow(0, r.x() - g.x(), r.y() - g.y(), r.width(), r.height()).toImage() \
                .convertToFormat(QImage.Format.Format_RGB32)
        scale = max((sc.devicePixelRatio() for sc, _ in self._parts), default=1.0)   # stitch the monitors together
        out = QImage(max(1, round(r.width() * scale)), max(1, round(r.height() * scale)), QImage.Format.Format_RGB32)
        out.fill(0)
        p = QPainter(out)
        for sc, inter in self._parts:
            g = sc.geometry()
            img = sc.grabWindow(0, inter.x() - g.x(), inter.y() - g.y(), inter.width(), inter.height()).toImage()
            dx, dy = round((inter.x() - r.x()) * scale), round((inter.y() - r.y()) * scale)
            if abs(sc.devicePixelRatio() - scale) < 1e-6:
                p.drawImage(dx, dy, img)
            else:
                p.drawImage(QRect(dx, dy, round(inter.width() * scale), round(inter.height() * scale)), img)
        p.end()
        return out

    def _frame(self) -> QImage | None:
        img = self._grab_region()
        if self.exclude is not None and not img.isNull():
            img = self._mask.process(img, self._island_in_image(img))
        if self.cursor and not img.isNull():
            self._draw_cursor(img, self.region)
        return img

    def _island_in_image(self, img: QImage) -> QRect | None:
        """Island rectangle in this frame's pixel coordinates (None when it does not overlap the area)."""
        g = self.exclude()
        if g is None:
            return None
        local = g.translated(-self.region.topLeft())
        if not local.intersects(QRect(0, 0, self.region.width(), self.region.height())):
            return None
        k = img.width() / max(1, self.region.width())
        pad = 2
        return QRect(int((local.x() - pad) * k), int((local.y() - pad) * k),
                     int((local.width() + 2 * pad) * k) + 1, int((local.height() + 2 * pad) * k) + 1)

    @staticmethod
    def _draw_cursor(img: QImage, region: QRect) -> None:
        pos = QCursor.pos() - region.topLeft()
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
        enc.push(img)

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
        audio, self._audio = self._audio, None
        t0 = self._t0
        if audio:
            audio.request_stop()                     # stop capturing sound at the same moment as the video
        self.last_stats = {"frames": enc.frames, "dropped": enc.dropped,
                           "seconds": max(0.001, time.monotonic() - self._t0), "fps": self.fps, "fmt": self.fmt}

        def work() -> None:
            tracks = audio.finish() if audio else None
            ok, err = enc.finish(tracks, t0)
            if tracks:
                from .audio import AudioRecorder
                AudioRecorder.discard(tracks)
            if ok:
                self.finished.emit(enc.out)
            else:
                self.failed.emit(f"Kayıt başarısız: {err or 'bilinmeyen hata'}")
        threading.Thread(target=work, name="pinshot-rec-finish", daemon=True).start()

    def cancel(self) -> None:
        enc, self.enc = self.enc, None
        self._timer.stop()
        self._clock.stop()
        audio, self._audio = self._audio, None
        if audio:
            from .audio import AudioRecorder
            AudioRecorder.discard(audio.finish())
        if enc is not None:
            enc.abort()
