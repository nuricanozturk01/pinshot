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
    peak: int = 0                 # loudest sample seen (0..32767): 0 means pure silence


def _platform() -> str:
    """_platform(), behind a function so tests can pin the platform (the macOS/Windows/Linux paths differ)."""
    return sys.platform


def _with_timeout(fn, seconds: float, fallback):
    """Run `fn` in a helper thread and give up after `seconds`.

    Enumerating audio devices can block for a very long time (CoreAudio on a machine without audio hardware, a stuck
    PulseAudio, a COM call on Windows). The UI and the tests must never wait on that."""
    box: list = []

    def work() -> None:
        try:
            box.append(fn())
        except Exception as ex:  # noqa: BLE001
            log.warning("audio scan failed: %s", ex)

    th = threading.Thread(target=work, name="pinshot-audio-scan", daemon=True)
    th.start()
    th.join(seconds)
    return box[0] if box else fallback


def audio_disabled() -> bool:
    return bool(os.environ.get("PINSHOT_NO_AUDIO"))          # CI / machines where scanning audio devices hangs


def _mic_hint() -> str:
    if _platform() == "win32":
        return ("Mikrofon bulunamadı. Ayarlar → Sistem → Ses → Giriş bölümünden bir mikrofon etkinleştirin ve "
                "Gizlilik → Mikrofon iznini verin.")
    if _platform() == "darwin":
        return "Mikrofon bulunamadı. Sistem Ayarları → Ses → Giriş bölümünden bir mikrofon seçin."
    return ("Mikrofon bulunamadı. Ses ayarlarında ses kartının giriş (mikrofon) profilini açın; Bluetooth kulaklıkta "
            "'Hands-free (HFP)' profilini seçin. Sonra Yenile'ye basın.")


SCAN_TIMEOUT = 3.0


def probe() -> dict:
    """What can be recorded right now: {'system': (ok, reason), 'mic': (ok, reason)}. Not cached: devices come and go.
    Never blocks longer than SCAN_TIMEOUT."""
    if audio_disabled():
        why = "Ses devre dışı."
        return {"system": (False, why), "mic": (False, why)}
    why = "Ses cihazları taranamadı (zaman aşımı). Yenile'ye basın."
    return _with_timeout(_probe_now, SCAN_TIMEOUT, {"system": (False, why), "mic": (False, why)})


def _probe_now() -> dict:
    res = {"system": (False, ""), "mic": (False, "")}
    try:
        import soundcard as sc  # type: ignore
    except Exception as ex:  # noqa: BLE001  (missing library, or no PulseAudio on Linux)
        why = f"Ses desteği yüklenemedi ({type(ex).__name__})."
        return {"system": (False, why), "mic": (False, why)}
    try:
        mics = [m for m in sc.all_microphones(include_loopback=True) if not getattr(m, "isloopback", False)]
        res["mic"] = (True, "") if mics else (False, _mic_hint())
        if mics and _platform() == "darwin":
            from . import macperm
            if macperm.mic_status() in ("denied", "restricted"):
                res["mic"] = (False, macperm.DENIED_TEXT)
    except Exception as ex:  # noqa: BLE001
        res["mic"] = (False, f"Mikrofon açılamadı ({type(ex).__name__}).")
    try:
        if _loopback_source(sc) is not None:
            res["system"] = (True, "")
        elif _platform() == "darwin":
            res["system"] = (False, "macOS sistem sesi için sanal bir ses cihazı gerekir (ör. BlackHole).")
        else:
            res["system"] = (False, "Sistem sesi kaynağı bulunamadı.")
    except Exception as ex:  # noqa: BLE001
        res["system"] = (False, f"Sistem sesi açılamadı ({type(ex).__name__}).")
    return res


def open_privacy_settings() -> bool:
    """macOS: open Privacy & Security → Microphone."""
    import subprocess
    from .macperm import PRIVACY_URL
    try:
        subprocess.Popen(["open", PRIVACY_URL])
        return True
    except OSError:
        return False


def open_sound_settings():
    """Open the OS sound settings. Returns the Popen handle (Linux), True (opened, no handle) or False."""
    import shutil
    import subprocess
    try:
        if _platform() == "win32":
            os.startfile("ms-settings:sound")          # type: ignore[attr-defined]
            return True
        if _platform() == "darwin":
            from . import macperm
            if macperm.mic_status() in ("denied", "restricted"):       # the sound pane would not help: it is a privacy setting
                return open_privacy_settings()
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
    """The source that carries what the default output plays.

    Never use `get_microphone(speaker.name, include_loopback=True)`: it matches by *name*, and a Bluetooth headset in
    Hands-free mode calls both its microphone and its output "<headset name>", so it returned the microphone and the
    "system sound" was silent. Choose among real loopback sources, by id first."""
    if _platform() == "darwin":       # no native loopback: accept a known virtual device
        for m in sc.all_microphones():
            if any(k in m.name.lower() for k in ("blackhole", "loopback", "soundflower")):
                return m
        return None
    speaker = sc.default_speaker()
    loops = [m for m in sc.all_microphones(include_loopback=True) if getattr(m, "isloopback", False)]
    sid, sname = str(speaker.id), str(speaker.name)
    for test in (lambda m: str(m.id) == sid + ".monitor",              # PulseAudio / PipeWire convention
                 lambda m: str(m.id) == sid,                           # WASAPI loopback shares the speaker's id
                 lambda m: m.name == "Monitor of " + sname,
                 lambda m: m.name == sname):
        for m in loops:
            if test(m):
                return m
    return None


def list_microphones() -> list[str]:
    """Names of real input devices (loopback/monitor sources excluded). Never blocks longer than SCAN_TIMEOUT."""
    if audio_disabled():
        return []

    def scan() -> list[str]:
        import soundcard as sc  # type: ignore
        return [m.name for m in sc.all_microphones(include_loopback=True) if not getattr(m, "isloopback", False)]
    return _with_timeout(scan, SCAN_TIMEOUT, [])


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
        # Microphone first: opening a Bluetooth headset's mic switches it from A2DP to Hands-free (HFP), which replaces
        # the output device. System sound must be opened after that, on the device that exists then.
        self.want = [("mic", mic), ("system", system)]
        self.tracks: list[Track] = []
        self._threads: list[threading.Thread] = []
        self._stop = threading.Event()
        self._lock = threading.Lock()
        self._files: dict[str, object] = {}
        self.warnings: list[str] = []          # found while opening the devices
        self.end_warnings: list[str] = []      # found when the recording ended (no data / pure silence)

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
        """Record one source into its PCM file. Reopens the device if it disappears or (system sound) the default
        output changes, e.g. when a Bluetooth headset switches profile in the middle of a recording."""
        failures = 0
        try:
            import numpy as np
            import soundcard as sc  # type: ignore
            if kind == "mic" and _platform() == "darwin":
                from . import macperm
                status = macperm.mic_status()
                if status in ("denied", "restricted"):
                    raise RuntimeError(macperm.DENIED_TEXT)
                if status == "not_determined":                 # the prompt is (re)shown; do not record silence meanwhile
                    macperm.request_mic()
                    raise RuntimeError("Mikrofon izni bekleniyor. İzin verdikten sonra yeniden kaydedin.")
            f = open(track.path, "wb")
            with self._lock:
                self._files[track.path] = f
            if kind == "system":
                self._settle(sc)
            while not self._stop.is_set():
                try:
                    src = _loopback_source(sc) if kind == "system" else _mic_source(sc, self.mic_name)
                    if src is None:
                        raise RuntimeError("kaynak bulunamadı")
                    ch = max(1, min(2, len(src.channels) if hasattr(src.channels, "__len__") else int(src.channels)))
                    if not track.started:
                        track.channels = ch
                    opened_for = self._default_output_id(sc) if kind == "system" else None
                    with src.recorder(samplerate=RATE, channels=track.channels, blocksize=BLOCK) as rec:
                        if not track.started:
                            track.started = time.monotonic()
                        ready.set()
                        failures = 0
                        last_check = time.monotonic()
                        while not self._stop.is_set():
                            data = rec.record(numframes=BLOCK)           # float32, shape (n, ch)
                            pcm = (np.clip(data, -1.0, 1.0) * 32767.0).astype("<i2").tobytes()
                            with self._lock:
                                if f.closed:
                                    return
                                f.write(pcm)
                                track.nbytes += len(pcm)
                                track.peak = max(track.peak, int(np.abs(data).max() * 32767))
                            if kind == "system" and time.monotonic() - last_check > 1.0:
                                last_check = time.monotonic()
                                if self._default_output_id(sc) != opened_for:
                                    log.info("default output changed: reopening the system-sound source")
                                    break                                # leave the `with`, reopen on the new device
                except Exception as ex:  # noqa: BLE001
                    failures += 1
                    if not track.started and failures >= 2:              # never opened: report it
                        raise
                    if failures > 25:
                        raise
                    log.warning("audio %s interrupted (%s); retrying", kind, ex)
                    self._stop.wait(0.4)
        except Exception as ex:  # noqa: BLE001
            track.error = str(ex) or type(ex).__name__
            log.warning("audio source %s failed: %s", kind, track.error)
        finally:
            ready.set()

    @staticmethod
    def _default_output_id(sc) -> str:
        try:
            return sc.default_speaker().id
        except Exception:  # noqa: BLE001
            return ""

    def _settle(self, sc, quiet: float = 0.7, limit: float = 3.0) -> None:
        """Wait until the default output stops changing (a headset profile switch takes a moment)."""
        end = time.monotonic() + limit
        last, since = self._default_output_id(sc), time.monotonic()
        while time.monotonic() < end and not self._stop.is_set():
            time.sleep(0.1)
            cur = self._default_output_id(sc)
            if cur != last:
                last, since = cur, time.monotonic()
            elif time.monotonic() - since >= quiet:
                return

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
        for t in self.tracks:
            if t.error:
                continue
            if t.nbytes == 0:
                tail = " Mikrofon izni verildi mi?" if t.label == "Mikrofon" else ""
                self.end_warnings.append(f"{t.label}: cihazdan hiç ses verisi gelmedi.{tail}")
            elif t.peak == 0 and t.label == "Mikrofon":
                self.end_warnings.append("Mikrofon tamamen sessiz kaydedildi: yanlış cihaz seçili ya da izin verilmemiş olabilir.")
        self.discard([t for t in self.tracks if t not in good])
        return good

    @staticmethod
    def discard(tracks: list[Track]) -> None:
        for t in tracks:
            try:
                os.remove(t.path)
            except OSError:
                pass
