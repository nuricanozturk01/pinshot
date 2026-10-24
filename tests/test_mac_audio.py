"""macOS audio capture (ffmpeg avfoundation microphone + pinshot-audiotap helper), tested with fake programs."""
import os
import stat
import sys
import textwrap
import time

import numpy as np
import pytest

import pinshot.audio as audio
import pinshot.mac_audio as ma
from pinshot.audio import AudioRecorder

AVF_LISTING = """\
[AVFoundation indev @ 0x7f] AVFoundation video devices:
[AVFoundation indev @ 0x7f] [0] FaceTime HD Camera
[AVFoundation indev @ 0x7f] [1] Capture screen 0
[AVFoundation indev @ 0x7f] AVFoundation audio devices:
[AVFoundation indev @ 0x7f] [0] MacBook Pro Microphone
[AVFoundation indev @ 0x7f] [1] BlackHole 2ch
[AVFoundation indev @ 0x7f] [2] USB Headset
"""


@pytest.fixture(autouse=True)
def _no_gc_in_worker_threads():
    """These tests start many threads; keep the cyclic GC off so it cannot run (and destroy Qt objects) in them."""
    import gc
    gc.disable()
    yield
    gc.enable()
    gc.collect()


def wait_until(cond, timeout=15.0):
    end = time.time() + timeout
    while time.time() < end:
        if cond():
            return True
        time.sleep(0.05)
    return False


# The fake capture programs are executable scripts with a "#!" line, which Windows cannot run. The macOS capture code
# is never used on Windows anyway.
pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="macOS capture programs are not used on Windows")


def fake_program(tmp_path, body: str, name="fake") -> str:
    """An executable Python script standing in for ffmpeg / pinshot-audiotap."""
    path = tmp_path / name
    path.write_text(f"#!{sys.executable}\n" + textwrap.dedent(body))
    path.chmod(path.stat().st_mode | stat.S_IEXEC)
    return str(path)


def test_avfoundation_device_listing_is_parsed():
    devs = ma.parse_avf_audio_devices(AVF_LISTING)
    assert devs == [(0, "MacBook Pro Microphone"), (1, "BlackHole 2ch"), (2, "USB Headset")]   # video devices ignored


def test_virtual_devices_are_not_microphones(monkeypatch):
    monkeypatch.setattr(ma, "avf_audio_devices", lambda exe: ma.parse_avf_audio_devices(AVF_LISTING))
    assert ma.list_mics("ffmpeg") == ["MacBook Pro Microphone", "USB Headset"]
    assert ma.virtual_devices("ffmpeg") == ["BlackHole 2ch"]


# a helper that behaves like pinshot-audiotap: prints "started", appends PCM, stops on "q"
GOOD_HELPER = """
    import sys, time, struct, threading
    path = sys.argv[1]
    f = open(path, "wb")
    print("started", flush=True)
    stop = threading.Event()
    threading.Thread(target=lambda: (sys.stdin.readline(), stop.set()), daemon=True).start()
    while not stop.is_set():
        f.write(struct.pack("<hh", 8000, -8000) * 2400); f.flush(); time.sleep(0.05)
    f.close()
"""


def test_child_capture_starts_records_and_stops_on_q(tmp_path):
    helper = fake_program(tmp_path, GOOD_HELPER)
    path = str(tmp_path / "sys.pcm")
    cap = ma.ChildCapture([helper, path], path, ready_line="started")
    cap.wait_ready(10)
    assert cap.alive() and cap.started_at > 0
    assert wait_until(lambda: os.path.getsize(path) > 20000)
    t = time.time()
    cap.stop()
    assert not cap.alive() and time.time() - t < 6
    assert ma.file_peak(path) == 8000


def test_missing_screen_recording_permission_is_explained(tmp_path):
    helper = fake_program(tmp_path, """
        import sys
        sys.stderr.write("Screen Recording permission is missing\\n")
        sys.exit(2)
    """, "denied")
    path = str(tmp_path / "x.pcm")
    cap = ma.ChildCapture([helper, path], path, ready_line="started")
    with pytest.raises(RuntimeError) as ei:
        cap.wait_ready(10)
    assert "Ekran Kaydı izni" in str(ei.value)
    cap.stop()


def test_helper_that_never_starts_times_out_with_a_message(tmp_path):
    helper = fake_program(tmp_path, "import time\ntime.sleep(60)\n", "stuck")
    path = str(tmp_path / "y.pcm")
    cap = ma.ChildCapture([helper, path], path, ready_line="started")
    t = time.time()
    with pytest.raises(RuntimeError):
        cap.wait_ready(1.0)
    assert time.time() - t < 5
    cap.stop()


def test_system_sound_uses_the_helper_on_macos13(tmp_path, monkeypatch):
    helper = fake_program(tmp_path, GOOD_HELPER, "pinshot-audiotap")
    monkeypatch.setenv("PINSHOT_AUDIOTAP", helper)
    monkeypatch.setattr(ma, "macos_version", lambda: (14, 5))
    path = str(tmp_path / "s.pcm")
    cap = ma.start_system("ffmpeg", path)
    assert cap.alive() and wait_until(lambda: os.path.exists(path) and os.path.getsize(path) > 0)
    cap.stop()


def test_system_sound_falls_back_to_blackhole_when_the_helper_cannot_run(tmp_path, monkeypatch):
    monkeypatch.delenv("PINSHOT_AUDIOTAP", raising=False)
    monkeypatch.setattr(ma, "find_helper", lambda: None)
    monkeypatch.setattr(ma, "avf_audio_devices", lambda exe: ma.parse_avf_audio_devices(AVF_LISTING))
    # fake "ffmpeg": records into the file named last on its command line
    ffmpeg = fake_program(tmp_path, """
        import sys, time
        f = open(sys.argv[-1], "wb")
        while True:
            f.write(b"\\x10\\x00\\x10\\x00" * 4800); f.flush(); time.sleep(0.05)
    """, "ffmpeg")
    path = str(tmp_path / "bh.pcm")
    cap = ma.start_system(ffmpeg, path)
    assert "-i" in cap.cmd and cap.cmd[cap.cmd.index("-i") + 1] == ":1"                  # BlackHole is device 1
    cap.stop()


def test_system_sound_without_any_source_says_what_to_do(monkeypatch):
    monkeypatch.setattr(ma, "find_helper", lambda: None)
    monkeypatch.setattr(ma, "avf_audio_devices", lambda exe: [(0, "MacBook Pro Microphone")])
    with pytest.raises(RuntimeError) as ei:
        ma.start_system("ffmpeg", "/tmp/never.pcm")
    assert "BlackHole" in str(ei.value)


def test_microphone_picks_the_chosen_device_and_never_a_virtual_one(tmp_path, monkeypatch):
    monkeypatch.setattr(ma, "avf_audio_devices", lambda exe: ma.parse_avf_audio_devices(AVF_LISTING))
    ffmpeg = fake_program(tmp_path, """
        import sys, time
        f = open(sys.argv[-1], "wb")
        while True:
            f.write(b"\\x20\\x00\\x20\\x00" * 4800); f.flush(); time.sleep(0.05)
    """, "ffmpeg")
    cap = ma.start_mic(ffmpeg, "USB Headset", str(tmp_path / "m1.pcm"))
    assert cap.cmd[cap.cmd.index("-i") + 1] == ":2"
    cap.stop()
    cap = ma.start_mic(ffmpeg, "", str(tmp_path / "m2.pcm"))                                # default = first real mic
    assert cap.cmd[cap.cmd.index("-i") + 1] == ":0"
    cap.stop()


# ---- through AudioRecorder, the way the app uses it
class FakeCapture:
    def __init__(self, path, level=2000, alive_for=None):
        self.path, self.level, self.started_at = path, level, time.monotonic()
        self.proc = type("P", (), {"poll": lambda s: None})()
        self._on = True
        self._data = np.full(48000 * 2, level, dtype="<i2").tobytes()
        open(path, "wb").write(self._data)

    def alive(self):
        return self._on

    def explain(self, code):
        return "kapandı"

    def stop(self):
        self._on = False


def test_audio_recorder_on_macos_collects_both_sources(tmp_path, monkeypatch):
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    import pinshot.macperm as mp
    monkeypatch.setattr(mp, "mic_status", lambda: "authorized")
    monkeypatch.setattr(ma, "start_mic", lambda exe, name, path: FakeCapture(path, 3000))
    monkeypatch.setattr(ma, "start_system", lambda exe, path: FakeCapture(path, 1500))
    rec = AudioRecorder(True, True)
    assert rec.start() == []
    time.sleep(0.3)
    rec.request_stop()
    tracks = rec.finish()
    assert {t.label for t in tracks} == {"Sistem sesi", "Mikrofon"}
    assert all(t.nbytes > 0 and t.peak > 0 and t.channels == 2 for t in tracks)
    AudioRecorder.discard(tracks)


def test_audio_recorder_on_macos_reports_a_capture_that_dies(tmp_path, monkeypatch):
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    import pinshot.macperm as mp
    monkeypatch.setattr(mp, "mic_status", lambda: "authorized")
    dying = FakeCapture(str(tmp_path / "d.pcm"))
    dying._on = False
    monkeypatch.setattr(ma, "start_system", lambda exe, path: dying)
    rec = AudioRecorder(True, False)
    rec.start()
    time.sleep(0.4)
    rec.request_stop()
    rec.finish()
    assert any("kapandı" in (t.error or "") for t in rec.tracks) or rec.warnings


def test_probe_on_macos_describes_each_missing_piece(monkeypatch):
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    monkeypatch.delenv("PINSHOT_NO_AUDIO", raising=False)
    import pinshot.macperm as mp
    monkeypatch.setattr(ma, "list_mics", lambda exe: [])
    monkeypatch.setattr(ma, "find_helper", lambda: None)
    monkeypatch.setattr(ma, "virtual_devices", lambda exe: [])
    res = audio.probe()
    assert res["mic"][0] is False and "Mikrofon bulunamadı" in res["mic"][1]
    assert res["system"][0] is False and "macOS 13" in res["system"][1]
    # everything present
    monkeypatch.setattr(ma, "list_mics", lambda exe: ["MacBook Pro Microphone"])
    monkeypatch.setattr(mp, "mic_status", lambda: "authorized")
    monkeypatch.setattr(ma, "find_helper", lambda: "/x/pinshot-audiotap")
    monkeypatch.setattr(ma, "macos_version", lambda: (15, 0))
    import pinshot.capture as cap
    monkeypatch.setattr(cap, "mac_screen_access", lambda request=True: True)
    res = audio.probe()
    assert res["mic"] == (True, "") and res["system"] == (True, "")
    monkeypatch.setattr(cap, "mac_screen_access", lambda request=True: False)
    assert "Ekran Kaydı" in audio.probe()["system"][1]
