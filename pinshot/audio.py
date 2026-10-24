"""Optional audio for MP4 recordings: system sound (loopback) and/or microphone, via the `soundcard` library.

Each source streams raw PCM to a temp file (nothing accumulates in RAM) and is muxed into the video at the end.
System sound: WASAPI loopback on Windows, a PulseAudio/PipeWire monitor on Linux. macOS has no built-in loopback;
a virtual device such as BlackHole is used if installed.
"""
from __future__ import annotations

import logging
import os
import sys
import tempfile
import threading
import time
from dataclasses import dataclass

log = logging.getLogger(__name__)
RATE = 48000
BLOCK = 2048                      # ~43 ms per read


@dataclass
class Track:
    label: str
    path: str
    channels: int
    started: float = 0.0          # time.monotonic() when the device opened
    error: str = ""
    nbytes: int = 0


def _mic_hint() -> str:
    if sys.platform == "win32":
        return ("Mikrofon bulunamadı. Ayarlar → Sistem → Ses → Giriş bölümünden bir mikrofon etkinleştirin ve "
                "Gizlilik → Mikrofon iznini verin.")
    if sys.platform == "darwin":
        return "Mikrofon bulunamadı. Sistem Ayarları → Ses → Giriş bölümünden bir mikrofon seçin."
    return ("Mikrofon bulunamadı. Ses ayarlarında ses kartının giriş (mikrofon) profilini açın; Bluetooth kulaklıkta "
            "'Hands-free (HFP)' profilini seçin. Sonra Yenile'ye basın.")


def probe() -> dict:
    """What can be recorded right now: {'system': (ok, reason), 'mic': (ok, reason)}. Not cached: devices come and go."""
    res = {"system": (False, ""), "mic": (False, "")}
    try:
        import soundcard as sc  # type: ignore
    except Exception as ex:  # noqa: BLE001  (missing library, or no PulseAudio on Linux)
        why = f"Ses desteği yüklenemedi ({type(ex).__name__})."
        return {"system": (False, why), "mic": (False, why)}
    try:
        mics = [m for m in sc.all_microphones(include_loopback=True) if not getattr(m, "isloopback", False)]
        res["mic"] = (True, "") if mics else (False, _mic_hint())
    except Exception as ex:  # noqa: BLE001
        res["mic"] = (False, f"Mikrofon açılamadı ({type(ex).__name__}).")
    try:
        if _loopback_source(sc) is not None:
            res["system"] = (True, "")
        elif sys.platform == "darwin":
            res["system"] = (False, "macOS sistem sesi için sanal bir ses cihazı gerekir (ör. BlackHole).")
        else:
            res["system"] = (False, "Sistem sesi kaynağı bulunamadı.")
    except Exception as ex:  # noqa: BLE001
        res["system"] = (False, f"Sistem sesi açılamadı ({type(ex).__name__}).")
    return res


def open_sound_settings():
    """Open the OS sound settings. Returns the Popen handle (Linux), True (opened, no handle) or False."""
    import shutil
    import subprocess
    try:
        if sys.platform == "win32":
            os.startfile("ms-settings:sound")          # type: ignore[attr-defined]
            return True
        if sys.platform == "darwin":
            subprocess.Popen(["open", "x-apple.systempreferences:com.apple.preference.sound"])
            return True
        for cmd in (["pavucontrol", "--tab=4"], ["gnome-control-center", "sound"], ["mate-volume-control"],
                    ["cinnamon-settings", "sound"], ["systemsettings", "kcm_pulseaudio"], ["xfce4-mixer"]):
            if shutil.which(cmd[0]):
                return subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError:
        pass
    return False


def _loopback_source(sc):
    if sys.platform == "darwin":       # no native loopback: accept a known virtual device
        for m in sc.all_microphones():
            if any(k in m.name.lower() for k in ("blackhole", "loopback", "soundflower")):
                return m
        return None
    speaker = sc.default_speaker()
    return sc.get_microphone(speaker.name, include_loopback=True)


def list_microphones() -> list[str]:
    """Names of real input devices (loopback/monitor sources excluded)."""
    try:
        import soundcard as sc  # type: ignore
        return [m.name for m in sc.all_microphones(include_loopback=True) if not getattr(m, "isloopback", False)]
    except Exception:  # noqa: BLE001
        return []


def _mic_source(sc, name: str = ""):
    real = [m for m in sc.all_microphones(include_loopback=True) if not getattr(m, "isloopback", False)]
    if name:
        for m in real:
            if m.name == name:
                return m
    try:
        default = sc.default_microphone()
        if not getattr(default, "isloopback", False):
            return default
    except Exception:  # noqa: BLE001
        pass
    return real[0] if real else None


class AudioRecorder:
    def __init__(self, system: bool, mic: bool, mic_name: str = ""):
        self.mic_name = mic_name
        self.want = [("system", system), ("mic", mic)]
        self.tracks: list[Track] = []
        self._threads: list[threading.Thread] = []
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._files: dict[str, object] = {}
        self.warnings: list[str] = []

    def start(self) -> list[str]:
        """Open the devices (waits briefly so start times are known). Returns warnings for sources that failed."""
        for kind, on in self.want:
            if not on:
                continue
            path = os.path.join(tempfile.gettempdir(), f"pinshot-rec-{os.getpid()}-{kind}-{int(time.time()*1000)}.pcm")
            track = Track("Sistem sesi" if kind == "system" else "Mikrofon", path, 2)
            ready = threading.Event()
            th = threading.Thread(target=self._run, args=(kind, track, ready), name=f"pinshot-audio-{kind}", daemon=True)
            th.start()
            if not ready.wait(3.0) and not track.error:
                track.error = "cihaz zamanında açılamadı"
            if track.error:
                self.warnings.append(f"{track.label} kaydedilemedi: {track.error}")
            else:
                self.tracks.append(track)
                self._threads.append(th)
        return self.warnings

    def _run(self, kind: str, track: Track, ready: threading.Event) -> None:
        try:
            import numpy as np
            import soundcard as sc  # type: ignore
            src = _loopback_source(sc) if kind == "system" else _mic_source(sc, self.mic_name)
            if src is None:
                raise RuntimeError("kaynak bulunamadı")
            ch = max(1, min(2, len(src.channels) if hasattr(src.channels, "__len__") else int(src.channels)))
            track.channels = ch
            with open(track.path, "wb") as f, src.recorder(samplerate=RATE, channels=ch, blocksize=BLOCK) as rec:
                with self._lock:
                    self._files[track.path] = f
                track.started = time.monotonic()
                ready.set()
                while not self._stop.is_set():
                    data = rec.record(numframes=BLOCK)           # float32, shape (n, ch)
                    pcm = (np.clip(data, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()
                    with self._lock:
                        if f.closed:
                            break
                        f.write(pcm)
                        track.nbytes += len(pcm)
        except Exception as ex:  # noqa: BLE001
            track.error = str(ex) or type(ex).__name__
            log.warning("audio source %s failed: %s", kind, track.error)
        finally:
            ready.set()

    def request_stop(self) -> None:
        self._stop.set()

    def finish(self) -> list[Track]:
        """Stop (call off the UI thread: a silent WASAPI loopback can block a read until the timeout)."""
        self._stop.set()
        for th in self._threads:
            th.join(timeout=2.0)
        with self._lock:                                          # close files even if a read is still blocked
            for f in self._files.values():
                try:
                    f.close()
                except OSError:
                    pass
        good = [t for t in self.tracks if not t.error and t.nbytes > 0]
        self.discard([t for t in self.tracks if t not in good])
        return good

    @staticmethod
    def discard(tracks: list[Track]) -> None:
        for t in tracks:
            try:
                os.remove(t.path)
            except OSError:
                pass
