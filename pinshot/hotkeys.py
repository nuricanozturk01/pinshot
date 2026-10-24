"""Global hotkeys via pynput + Qt<->pynput key-string conversion."""
from __future__ import annotations

import logging
import sys

from PySide6.QtCore import QObject, Signal

log = logging.getLogger(__name__)
MAC = sys.platform == "darwin"
# Qt portable names -> pynput. On macOS Qt's "Ctrl" is Command and "Meta" is the Control key.
_MODS = {"Ctrl": "<cmd>" if MAC else "<ctrl>", "Meta": "<ctrl>" if MAC else "<cmd>",
         "Alt": "<alt>", "Shift": "<shift>"}
_KEYS = {"Return": "<enter>", "Enter": "<enter>", "Esc": "<esc>", "Escape": "<esc>", "Space": "<space>",
         "Tab": "<tab>", "Backspace": "<backspace>", "Del": "<delete>", "Delete": "<delete>", "Ins": "<insert>",
         "Home": "<home>", "End": "<end>", "PgUp": "<page_up>", "PgDown": "<page_down>", "Up": "<up>",
         "Down": "<down>", "Left": "<left>", "Right": "<right>", "Print": "<print_screen>",
         "SysReq": "<print_screen>", "Pause": "<pause>"}


def qt_to_pynput(seq: str) -> str:
    """'Ctrl+Alt+A' -> '<ctrl>+<alt>+a'. Returns '' for an empty / unsupported sequence."""
    seq = seq.split(",")[0].strip()          # QKeySequence chords: use the first only
    if not seq:
        return ""
    parts = seq.split("+")
    if seq.endswith("++"):
        parts = parts[:-2] + ["+"]
    out = []
    for part in parts[:-1]:
        if part not in _MODS:
            return ""
        out.append(_MODS[part])
    key = parts[-1]
    if key in _KEYS:
        out.append(_KEYS[key])
    elif len(key) > 1 and key[0] == "F" and key[1:].isdigit():
        out.append(f"<{key.lower()}>")
    elif len(key) == 1:
        out.append(key.lower())
    else:
        return ""
    return "+".join(out)


def pynput_to_qt(s: str) -> str:
    inv_m = {v: k for k, v in _MODS.items()}
    inv_k = {"<enter>": "Return", "<esc>": "Esc", "<space>": "Space", "<tab>": "Tab", "<backspace>": "Backspace",
             "<delete>": "Del", "<insert>": "Ins", "<home>": "Home", "<end>": "End", "<page_up>": "PgUp",
             "<page_down>": "PgDown", "<up>": "Up", "<down>": "Down", "<left>": "Left", "<right>": "Right",
             "<print_screen>": "Print", "<pause>": "Pause"}
    out = []
    for part in s.split("+"):
        if part in inv_m:
            out.append(inv_m[part])
        elif part in inv_k:
            out.append(inv_k[part])
        elif part.startswith("<f") and part[2:-1].isdigit():
            out.append(part[1:-1].upper())
        elif part:
            out.append(part.upper())
    return "+".join(out)


class HotkeyManager(QObject):
    triggered = Signal(str)
    failed = Signal(str)

    def __init__(self) -> None:
        super().__init__()
        self._listener = None

    def start(self, mapping: dict[str, str]) -> None:
        """mapping: pynput hotkey string -> action name."""
        self.stop()
        mapping = {k: v for k, v in mapping.items() if k}
        if not mapping:
            return
        try:
            from pynput import keyboard
            self._listener = keyboard.GlobalHotKeys(
                {k: (lambda a=a: self.triggered.emit(a)) for k, a in mapping.items()})
            self._listener.daemon = True
            self._listener.start()
        except Exception as ex:  # noqa: BLE001  (no X server, Wayland, missing macOS permission...)
            log.warning("global hotkeys unavailable: %s", ex)
            self._listener = None
            self.failed.emit(str(ex))

    def stop(self) -> None:
        if self._listener is not None:
            try:
                self._listener.stop()
            except Exception:  # noqa: BLE001
                pass
            self._listener = None
