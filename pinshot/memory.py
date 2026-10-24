"""Hand freed memory back to the OS (glibc/Windows keep it in the process after big images are released)."""
from __future__ import annotations

import ctypes
import gc
import sys


def release_memory() -> None:
    gc.collect()
    try:
        if sys.platform.startswith("linux"):
            ctypes.CDLL("libc.so.6").malloc_trim(0)
        elif sys.platform == "darwin":
            ctypes.CDLL("/usr/lib/libSystem.B.dylib").malloc_zone_pressure_relief(None, 0)
        elif sys.platform == "win32":
            k32 = ctypes.windll.kernel32
            k32.GetCurrentProcess.restype = ctypes.c_void_p
            k32.SetProcessWorkingSetSize(ctypes.c_void_p(k32.GetCurrentProcess()),
                                         ctypes.c_size_t(-1), ctypes.c_size_t(-1))
    except (OSError, AttributeError):
        pass    # purely an optimisation


def tune_malloc() -> None:
    """glibc only: keep the resident set small for a long-running tray app. Call before threads start.

    - few arenas: every thread otherwise gets its own heap that is never given back
    - fixed mmap threshold: big buffers (screenshots, video frames) are always mmap'd, so freeing them returns the
      memory to the OS at once instead of leaving it in a heap (the default threshold grows dynamically)
    - low trim threshold: free memory at the top of the heap is returned promptly
    """
    if not sys.platform.startswith("linux"):
        return
    try:
        libc = ctypes.CDLL("libc.so.6")
        for param, value in ((-8, 2),            # M_ARENA_MAX
                             (-3, 256 * 1024),   # M_MMAP_THRESHOLD
                             (-1, 128 * 1024)):  # M_TRIM_THRESHOLD
            libc.mallopt(ctypes.c_int(param), ctypes.c_int(value))
    except (OSError, AttributeError):
        pass
