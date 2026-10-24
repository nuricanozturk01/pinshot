"""Launch-at-login for Windows (registry), macOS (LaunchAgent) and Linux (XDG autostart)."""
from __future__ import annotations

import logging
import os
import plistlib
import sys

log = logging.getLogger(__name__)
APP_ID = "PinShot"


def _command() -> list[str]:
    if getattr(sys, "frozen", False):
        return [sys.executable]
    return [sys.executable, "-m", "pinshot"]


def set_enabled(enabled: bool) -> bool:
    try:
        if sys.platform == "win32":
            return _windows(enabled)
        if sys.platform == "darwin":
            return _mac(enabled)
        return _linux(enabled)
    except Exception as ex:  # noqa: BLE001
        log.error("autostart change failed: %s", ex)
        return False


def _windows(enabled: bool) -> bool:
    import subprocess
    import winreg
    cmd = subprocess.list2cmdline(_command())
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run", 0,
                        winreg.KEY_SET_VALUE) as k:
        if enabled:
            winreg.SetValueEx(k, APP_ID, 0, winreg.REG_SZ, cmd)
        else:
            try:
                winreg.DeleteValue(k, APP_ID)
            except FileNotFoundError:
                pass
    return True


def _mac(enabled: bool) -> bool:
    path = os.path.expanduser(f"~/Library/LaunchAgents/com.pinshot.app.plist")
    if enabled:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            plistlib.dump({"Label": "com.pinshot.app", "ProgramArguments": _command(), "RunAtLoad": True}, f)
    elif os.path.exists(path):
        os.remove(path)
    return True


def _linux(enabled: bool) -> bool:
    base = os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    path = os.path.join(base, "autostart", "pinshot.desktop")
    if enabled:
        os.makedirs(os.path.dirname(path), exist_ok=True)
        exec_line = " ".join(f'"{c}"' if " " in c else c for c in _command())
        with open(path, "w", encoding="utf-8") as f:
            f.write("[Desktop Entry]\nType=Application\nName=PinShot\nComment=Screenshot, annotate and pin\n"
                    f"Exec={exec_line}\nX-GNOME-Autostart-enabled=true\nTerminal=false\n")
    elif os.path.exists(path):
        os.remove(path)
    return True
