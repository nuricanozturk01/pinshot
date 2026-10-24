"""macOS microphone permission (TCC) via AVFoundation.

Without permission macOS does NOT raise an error: the input stream is silent or delivers nothing, and the audio library
then waits forever. So the permission is checked and requested explicitly, and a denial is reported to the user.
On other platforms, or when PyObjC is missing, everything reports "unknown" and recording is simply attempted."""
from __future__ import annotations

import logging
import sys
import threading

log = logging.getLogger(__name__)
_STATUS = {0: "not_determined", 1: "restricted", 2: "denied", 3: "authorized"}
PRIVACY_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"


def _av():
    if sys.platform != "darwin":
        return None
    try:
        import AVFoundation  # type: ignore
        return AVFoundation
    except Exception as ex:  # noqa: BLE001
        log.info("AVFoundation unavailable (%s): microphone permission cannot be queried", ex)
        return None


def mic_status() -> str:
    """'authorized' | 'denied' | 'restricted' | 'not_determined' | 'unknown' (not macOS / no PyObjC)."""
    av = _av()
    if av is None:
        return "unknown"
    try:
        return _STATUS.get(int(av.AVCaptureDevice.authorizationStatusForMediaType_(av.AVMediaTypeAudio)), "unknown")
    except Exception as ex:  # noqa: BLE001
        log.warning("microphone status query failed: %s", ex)
        return "unknown"


def request_mic(on_done=None) -> bool:
    """Show the system permission prompt (non-blocking). `on_done(granted: bool)` runs on an arbitrary thread."""
    av = _av()
    if av is None:
        return False

    def handler(granted) -> None:
        log.info("microphone permission answer: %s", bool(granted))
        if on_done:
            on_done(bool(granted))
    try:
        av.AVCaptureDevice.requestAccessForMediaType_completionHandler_(av.AVMediaTypeAudio, handler)
        return True
    except Exception as ex:  # noqa: BLE001
        log.warning("microphone permission request failed: %s", ex)
        return False


DENIED_TEXT = ("Mikrofon izni verilmemiş. Sistem Ayarları → Gizlilik ve Güvenlik → Mikrofon bölümünde PinShot'u açın, "
               "sonra PinShot'u yeniden başlatın.")


# ---- global shortcuts: Input Monitoring (listen to keys) and Accessibility
INPUT_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
ACCESS_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
SCREEN_URL = "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"

SHORTCUTS_TEXT = ("Kısayollar için izin gerekli: Sistem Ayarları → Gizlilik ve Güvenlik → Giriş İzleme ve "
                  "Erişilebilirlik bölümlerinde PinShot'u açın, sonra PinShot'u yeniden başlatın.")


def _lib(path: str):
    import ctypes
    return ctypes.CDLL(path)


def input_monitoring_ok(request: bool = False) -> bool | None:
    """True/False on macOS 10.15+, None when it cannot be determined (older macOS / not macOS)."""
    if sys.platform != "darwin":
        return None
    try:
        import ctypes
        cg = _lib("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
        cg.CGPreflightListenEventAccess.restype = ctypes.c_bool
        ok = bool(cg.CGPreflightListenEventAccess())
        if not ok and request:
            cg.CGRequestListenEventAccess.restype = ctypes.c_bool
            cg.CGRequestListenEventAccess()               # shows the system prompt once
        return ok
    except (OSError, AttributeError):
        return None


def accessibility_ok() -> bool | None:
    if sys.platform != "darwin":
        return None
    try:
        import ctypes
        ax = _lib("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices")
        ax.AXIsProcessTrusted.restype = ctypes.c_bool
        return bool(ax.AXIsProcessTrusted())
    except (OSError, AttributeError):
        return None


def shortcuts_permitted() -> bool:
    """False only when macOS says a required permission is missing (unknown counts as fine: just try)."""
    return input_monitoring_ok() is not False and accessibility_ok() is not False


def open_pane(url: str) -> bool:
    import subprocess
    try:
        subprocess.Popen(["open", url])
        return True
    except OSError:
        return False
