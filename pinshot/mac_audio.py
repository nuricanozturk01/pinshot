"""macOS audio capture, done by small child processes instead of an audio library.

- Microphone: ffmpeg's `avfoundation` input (the most widely used way to record a Mac microphone).
- System sound: `pinshot-audiotap`, a tiny Swift program built on ScreenCaptureKit (macOS 13+). If it is not available
  (older macOS), a virtual device such as BlackHole is recorded through avfoundation too.

Every capture writes raw signed 16-bit little-endian PCM, 48 kHz, stereo, to a file: the same format the other
platforms produce, so the muxing code does not care where it came from.
"""
from __future__ import annotations

import logging
import os
import re
import subprocess
import sys
import tempfile
import threading
import time

log = logging.getLogger(__name__)
VIRTUAL_HINTS = ("blackhole", "loopback", "soundflower", "multi-output", "aggregate", "zoomaudio", "teams")
START_TIMEOUT = 15.0


# ---------------------------------------------------------------- device list (ffmpeg avfoundation)
def parse_avf_audio_devices(stderr_text: str) -> list[tuple[int, str]]:
    """Audio devices from `ffmpeg -f avfoundation -list_devices true -i ""` (it prints them to stderr)."""
    devices: list[tuple[int, str]] = []
    in_audio = False
    for line in stderr_text.splitlines():
        if "AVFoundation audio devices" in line:
            in_audio = True
        elif "AVFoundation video devices" in line:
            in_audio = False
        elif in_audio:
            m = re.search(r"\[(\d+)\]\s+(.+?)\s*$", line)
            if m:
                devices.append((int(m.group(1)), m.group(2)))
    return devices


def avf_audio_devices(exe: str) -> list[tuple[int, str]]:
    try:
        r = subprocess.run([exe, "-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""],
                           capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.TimeoutExpired) as ex:
        log.warning("could not list avfoundation devices: %s", ex)
        return []
    return parse_avf_audio_devices(r.stderr)


def is_virtual(name: str) -> bool:
    return any(h in name.lower() for h in VIRTUAL_HINTS)


def list_mics(exe: str) -> list[str]:
    """Real input devices (virtual loopback devices are not microphones)."""
    return [n for _, n in avf_audio_devices(exe) if not is_virtual(n)]


def virtual_devices(exe: str) -> list[str]:
    return [n for _, n in avf_audio_devices(exe) if any(h in n.lower() for h in ("blackhole", "loopback", "soundflower"))]


def macos_version() -> tuple[int, ...]:
    import platform
    try:
        return tuple(int(x) for x in platform.mac_ver()[0].split(".")[:2])
    except ValueError:
        return (0, 0)


# ---------------------------------------------------------------- the helper program
def find_helper() -> str | None:
    """pinshot-audiotap next to the app (several places, depending on how the app was bundled), or in the repo."""
    names = ["pinshot-audiotap"]
    here = os.path.dirname(os.path.abspath(sys.argv[0] if sys.argv and sys.argv[0] else __file__))
    roots = [os.environ.get("PINSHOT_AUDIOTAP_DIR", ""), getattr(sys, "_MEIPASS", ""), os.path.dirname(sys.executable),
             here, os.path.join(os.path.dirname(sys.executable), "..", "Resources"),
             os.path.join(os.path.dirname(sys.executable), "..", "Frameworks"),
             os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "packaging", "bin")]
    env_file = os.environ.get("PINSHOT_AUDIOTAP")
    for cand in ([env_file] if env_file else []) + [os.path.join(r, n) for r in roots if r for n in names]:
        if cand and os.path.isfile(cand) and os.access(cand, os.X_OK):
            return os.path.abspath(cand)
    return None


class ChildCapture:
    """A capture program writing PCM into `path`; stopped by sending "q" on stdin (ends on EOF too)."""

    def __init__(self, cmd: list[str], path: str, ready_line: str | None):
        self.cmd, self.path, self.ready_line = cmd, path, ready_line
        self._err_path = path + ".err"
        self._err = open(self._err_path, "wb")
        self.proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self._err)
        self.started_at = 0.0
        self._ready = threading.Event()
        threading.Thread(target=self._watch, name="pinshot-capture-watch", daemon=True).start()

    def _watch(self) -> None:
        """Ready = the helper printed its ready line, or (ffmpeg) the output file started to grow."""
        try:
            if self.ready_line is not None:
                for raw in self.proc.stdout:                       # blocks until a line or the process ends
                    if raw.decode("utf-8", "replace").strip() == self.ready_line:
                        break
                else:
                    return
                self.started_at = time.monotonic()
                self._ready.set()
                return
            end = time.monotonic() + START_TIMEOUT
            while time.monotonic() < end and self.proc.poll() is None:
                if os.path.exists(self.path) and os.path.getsize(self.path) > 0:
                    self.started_at = time.monotonic()
                    self._ready.set()
                    return
                time.sleep(0.02)
        except (OSError, ValueError):
            pass

    def wait_ready(self, timeout: float = START_TIMEOUT) -> None:
        """Raise RuntimeError with the reason if the capture did not start."""
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self._ready.wait(0.05):
                return
            if self.proc.poll() is not None:
                break
        if self._ready.is_set():
            return
        code = self.proc.poll()
        raise RuntimeError(self.explain(code))

    def explain(self, code: int | None) -> str:
        err = self.stderr_text()
        if code == 2:
            return "Ekran Kaydı izni verilmemiş. Sistem Ayarları → Gizlilik ve Güvenlik → Ekran Kaydı'nda PinShot'u açıp yeniden başlatın."
        if code == 65:
            return "Sistem sesi için macOS 13 veya üstü gerekir."
        if code is None:
            return "Ses yakalama zamanında başlamadı." + (f" ({err})" if err else "")
        return (err or f"Ses yakalama programı {code} koduyla kapandı.")

    def alive(self) -> bool:
        return self.proc.poll() is None

    def stderr_text(self) -> str:
        try:
            self._err.flush()
            with open(self._err_path, "rb") as f:
                return f.read()[-400:].decode("utf-8", "replace").strip()
        except OSError:
            return ""

    def stop(self) -> None:
        try:
            if self.proc.poll() is None:
                try:
                    self.proc.stdin.write(b"q\n")
                    self.proc.stdin.flush()
                    self.proc.stdin.close()
                except (OSError, ValueError):
                    pass
                try:
                    self.proc.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    self.proc.terminate()
                    try:
                        self.proc.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        self.proc.kill()
                        self.proc.wait(timeout=3)
        finally:
            try:
                self._err.close()
            except OSError:
                pass
            for p in (self._err_path,):
                try:
                    os.remove(p)
                except OSError:
                    pass


# ---------------------------------------------------------------- starting the captures
def _avf_capture(exe: str, index: int, path: str) -> ChildCapture:
    cmd = [exe, "-hide_banner", "-loglevel", "error", "-y", "-f", "avfoundation",
           "-i", f":{index}", "-ar", "48000", "-ac", "2", "-f", "s16le", path]
    return ChildCapture(cmd, path, ready_line=None)


def start_mic(exe: str, name: str, path: str) -> ChildCapture:
    devices = avf_audio_devices(exe)
    real = [(i, n) for i, n in devices if not is_virtual(n)]
    if not real:
        raise RuntimeError("Mikrofon bulunamadı.")
    pick = next(((i, n) for i, n in real if n == name), real[0])
    log.info("recording microphone %r (avfoundation index %s)", pick[1], pick[0])
    cap = _avf_capture(exe, pick[0], path)
    cap.wait_ready()
    return cap


def start_system(exe: str, path: str) -> ChildCapture:
    helper = find_helper()
    if helper and macos_version() >= (13, 0):
        cap = ChildCapture([helper, path], path, ready_line="started")
        try:
            cap.wait_ready()
            return cap
        except RuntimeError as ex:
            reason = str(ex)
            cap.stop()
            fallback = virtual_devices(exe)
            if not fallback:
                raise RuntimeError(reason) from None
            log.warning("ScreenCaptureKit capture failed (%s): using the virtual device %r", reason, fallback[0])
    devices = [(i, n) for i, n in avf_audio_devices(exe) if any(h in n.lower() for h in ("blackhole", "loopback", "soundflower"))]
    if not devices:
        raise RuntimeError("Sistem sesi kaynağı bulunamadı (macOS 13+ ya da BlackHole gerekir).")
    cap = _avf_capture(exe, devices[0][0], path)
    cap.wait_ready()
    return cap


def file_peak(path: str) -> int:
    """Loudest sample (0..32767) of a PCM file, read in chunks (0 = pure silence)."""
    import numpy as np
    peak = 0
    try:
        with open(path, "rb") as f:
            while True:
                chunk = f.read(1 << 20)
                if not chunk:
                    break
                data = np.frombuffer(chunk[: len(chunk) // 2 * 2], dtype="<i2")
                if data.size:
                    peak = max(peak, int(np.abs(data.astype("int32")).max()))
    except OSError:
        pass
    return peak
