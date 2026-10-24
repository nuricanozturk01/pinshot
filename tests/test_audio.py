"""AudioRecorder against fake `soundcard` devices (no sound hardware needed)."""
import sys
import threading
import time
import types

import numpy as np
import pytest

from pinshot.audio import AudioRecorder


class FakeRecorder:
    def __init__(self, dev, channels, log):
        self.dev, self.channels, self.log = dev, channels, log

    def __enter__(self):
        self.log.append(("open", self.dev.id))
        return self

    def __exit__(self, *a):
        self.log.append(("close", self.dev.id))

    def record(self, numframes):
        time.sleep(numframes / 48000)
        if self.dev.dead:
            raise RuntimeError("stream disconnected")
        return np.full((numframes, self.channels), self.dev.level, dtype="float32")


class FakeDevice:
    def __init__(self, id, name, level=0.25, loop=False, channels=2):
        self.id, self.name, self.level, self.isloopback, self.dead = id, name, level, loop, False
        self.channels = list(range(channels))
        self.log = None

    def recorder(self, samplerate, channels, blocksize):
        return FakeRecorder(self, channels, self.log)


class FakeSC(types.ModuleType):
    def __init__(self):
        super().__init__("soundcard")
        self.log = []
        self.sink = "sink-a2dp"                       # id of the current default output
        self.mic = FakeDevice("mic1", "Headset Mic", 0.5)
        self.monitors = {"sink-a2dp": FakeDevice("sink-a2dp.monitor", "Monitor A", 0.2, True),
                         "sink-hfp": FakeDevice("sink-hfp.monitor", "Monitor B", 0.3, True)}
        for d in [self.mic, *self.monitors.values()]:
            d.log = self.log

    def default_speaker(self):
        return types.SimpleNamespace(id=self.sink, name=self.sink)

    def get_microphone(self, id, include_loopback=False):
        """Like the real library: matches by NAME, mics first. (This is what picked the headset mic.)"""
        for m in self.all_microphones(include_loopback):
            if m.name == id or m.id == id:
                return m
        raise IndexError(id)

    def all_microphones(self, include_loopback=False):
        return [self.mic] + (list(self.monitors.values()) if include_loopback else [])

    def default_microphone(self):
        return self.mic


@pytest.fixture
def fake(monkeypatch):
    import pinshot.audio as audio
    sc = FakeSC()
    monkeypatch.setitem(sys.modules, "soundcard", sc)
    monkeypatch.setattr(audio, "_platform", lambda: "linux")      # the fake behaves like PulseAudio on every runner
    return sc


def wait_until(cond, timeout=15.0, step=0.05):
    """Poll instead of sleeping a fixed time: CI machines (especially macOS runners) are slow and uneven."""
    end = time.time() + timeout
    while time.time() < end:
        if cond():
            return True
        time.sleep(step)
    return False


def _peaks(track):
    data = np.frombuffer(open(track.path, "rb").read(), dtype="<i2")
    return data


def test_records_mic_and_system(fake):
    rec = AudioRecorder(True, True)
    assert rec.start() == []
    assert wait_until(lambda: all(t.nbytes > 0 for t in rec.tracks)), "both sources must produce audio"
    rec.request_stop()
    tracks = rec.finish()
    assert {t.label for t in tracks} == {"Sistem sesi", "Mikrofon"}
    assert all(t.nbytes > 0 for t in tracks)
    AudioRecorder.discard(tracks)


def test_microphone_is_opened_before_system_sound(fake):
    """Opening a Bluetooth mic switches the headset to HFP and replaces the output: system sound must come second."""
    rec = AudioRecorder(True, True)
    rec.start()
    rec.request_stop()
    AudioRecorder.discard(rec.finish())
    opens = [d for kind, d in fake.log if kind == "open"]
    assert opens[0] == "mic1" and opens[1].startswith("sink-")


def test_system_sound_follows_the_default_output_when_it_changes(fake):
    """Headset switches A2DP -> HFP while recording: the old sink vanishes, capture must continue on the new one."""
    rec = AudioRecorder(True, False)
    rec.start()
    assert wait_until(lambda: rec.tracks and rec.tracks[0].nbytes > 0)
    fake.monitors["sink-a2dp"].dead = True            # old output disappears...
    fake.sink = "sink-hfp"                            # ...and a new default output appears
    assert wait_until(lambda: "sink-hfp.monitor" in [d for k, d in fake.log if k == "open"]), "must reopen on the new output"
    n = rec.tracks[0].nbytes
    assert wait_until(lambda: rec.tracks[0].nbytes > n + 48000 * 4 * 0.3), "audio must keep flowing on the new device"
    rec.request_stop()
    tracks = rec.finish()
    assert tracks, "recording must survive the switch"
    assert (np.abs(_peaks(tracks[0])) > 0.28 * 32767).any()        # samples from the new device (level 0.3) are in the file
    AudioRecorder.discard(tracks)


def test_system_sound_recovers_from_a_dropped_stream(fake):
    rec = AudioRecorder(True, False)
    rec.start()
    assert wait_until(lambda: rec.tracks and rec.tracks[0].nbytes > 0)
    dev = fake.monitors["sink-a2dp"]
    dev.dead = True
    time.sleep(1.0)                                    # the stream stays broken for a while (several retries)
    n = rec.tracks[0].nbytes
    dev.dead = False                                   # device comes back
    assert wait_until(lambda: rec.tracks[0].nbytes > n + 48000 * 4 * 0.3), "recording must resume by itself"
    rec.request_stop()
    tracks = rec.finish()
    assert tracks
    AudioRecorder.discard(tracks)


def test_unavailable_source_is_reported_not_raised(fake, monkeypatch):
    monkeypatch.setattr(fake, "all_microphones", lambda include_loopback=False: [])
    # like PulseAudio with no real input: the "default microphone" is just a monitor source
    monkeypatch.setattr(fake, "default_microphone", lambda: fake.monitors["sink-a2dp"])
    rec = AudioRecorder(False, True)
    warnings = rec.start()
    assert warnings and "Mikrofon" in warnings[0]
    rec.finish()


def test_system_sound_never_picks_the_microphone_when_names_collide(fake):
    """Bluetooth Hands-free: the headset's mic and its output carry the same name. System sound must still use the
    output's monitor, not the microphone (that produced silence)."""
    fake.mic = FakeDevice("bluez_input.X.0", "soundcore Space One Pro", 0.0)          # silent mic, same name
    fake.mic.log = fake.log
    fake.sink = "bluez_output.X.1"
    fake.default_speaker = lambda: types.SimpleNamespace(id=fake.sink, name="soundcore Space One Pro")
    mon = FakeDevice("bluez_output.X.1.monitor", "Monitor of soundcore Space One Pro", 0.4, True)
    mon.log = fake.log
    fake.monitors = {"bluez_output.X.1": mon}
    from pinshot.audio import _loopback_source
    assert _loopback_source(fake) is mon
    rec = AudioRecorder(True, False)
    rec.start()
    assert wait_until(lambda: rec.tracks and rec.tracks[0].nbytes > 0)
    rec.request_stop()
    tracks = rec.finish()
    assert tracks
    assert (np.abs(_peaks(tracks[0])) > 0.3 * 32767).any()                    # real playback level, not the silent mic
    AudioRecorder.discard(tracks)


def test_system_sound_unavailable_when_only_microphones_exist(fake):
    from pinshot.audio import _loopback_source
    fake.monitors = {}
    assert _loopback_source(fake) is None


def test_device_scan_never_blocks_the_caller(monkeypatch):
    """A hanging audio backend (CoreAudio on a CI Mac, stuck PulseAudio) must not freeze the UI or the tests."""
    import pinshot.audio as audio
    monkeypatch.delenv("PINSHOT_NO_AUDIO", raising=False)
    monkeypatch.setattr(audio, "SCAN_TIMEOUT", 0.5)

    class Hang(types.ModuleType):
        def all_microphones(self, include_loopback=False):
            time.sleep(30)

        def default_speaker(self):
            time.sleep(30)

    monkeypatch.setitem(sys.modules, "soundcard", Hang("soundcard"))
    t = time.time()
    res = audio.probe()
    names = audio.list_microphones()
    assert time.time() - t < 3, "the scan must give up quickly"
    assert res["mic"][0] is False and res["system"][0] is False and "zaman aşımı" in res["mic"][1]
    assert names == []


def test_audio_can_be_switched_off_by_environment(monkeypatch):
    import pinshot.audio as audio
    monkeypatch.setenv("PINSHOT_NO_AUDIO", "1")
    assert audio.probe()["mic"][0] is False and audio.list_microphones() == []


def test_macos_system_sound_needs_a_virtual_device(fake, monkeypatch):
    """macOS has no built-in loopback: only a known virtual device (BlackHole, ...) counts as system sound."""
    import pinshot.audio as audio
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    assert audio._loopback_source(fake) is None
    hole = FakeDevice("bh", "BlackHole 2ch", 0.2)
    hole.log = fake.log
    monkeypatch.setattr(fake, "all_microphones", lambda include_loopback=False: [fake.mic, hole])
    assert audio._loopback_source(fake) is hole


def test_windows_loopback_is_matched_by_the_speaker_id(fake, monkeypatch):
    import pinshot.audio as audio
    monkeypatch.setattr(audio, "_platform", lambda: "win32")
    fake.sink = "{0.0.0.00000000}.{abc}"
    loop = FakeDevice("{0.0.0.00000000}.{abc}", "Speakers (Realtek)", 0.3, True)    # WASAPI loopback shares the id
    loop.log = fake.log
    other = FakeDevice("{0.0.1.00000000}.{def}", "Speakers (Realtek)", 0.9, True)    # same name, another device
    other.log = fake.log
    monkeypatch.setattr(fake, "all_microphones", lambda include_loopback=False: [fake.mic, other, loop])
    monkeypatch.setattr(fake, "default_speaker", lambda: types.SimpleNamespace(id=fake.sink, name="Speakers (Realtek)"))
    assert audio._loopback_source(fake) is loop


# ---------------------------------------------------------------- macOS microphone permission
def _fake_avfoundation(monkeypatch, status):
    import types as t
    av = t.ModuleType("AVFoundation")
    av.AVMediaTypeAudio = "soun"
    calls = {"requested": 0}

    class Dev:
        @staticmethod
        def authorizationStatusForMediaType_(_):
            return status["v"]

        @staticmethod
        def requestAccessForMediaType_completionHandler_(_, handler):
            calls["requested"] += 1
            handler(True)
    av.AVCaptureDevice = Dev
    monkeypatch.setitem(sys.modules, "AVFoundation", av)
    return calls


def test_macperm_maps_status_codes(monkeypatch):
    import pinshot.macperm as mp
    monkeypatch.setattr(mp.sys, "platform", "darwin")
    status = {"v": 0}
    _fake_avfoundation(monkeypatch, status)
    for code, name in ((0, "not_determined"), (1, "restricted"), (2, "denied"), (3, "authorized")):
        status["v"] = code
        assert mp.mic_status() == name


def test_macperm_unknown_when_not_macos():
    import pinshot.macperm as mp
    if sys.platform != "darwin":
        assert mp.mic_status() == "unknown"
        assert mp.request_mic() is False


def test_denied_microphone_permission_is_explained_not_silently_ignored(fake, monkeypatch):
    import pinshot.audio as audio
    import pinshot.mac_audio as ma
    import pinshot.macperm as mp
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    monkeypatch.setattr(mp, "mic_status", lambda: "denied")
    monkeypatch.setattr(ma, "list_mics", lambda exe: ["MacBook Pro Microphone"])
    monkeypatch.delenv("PINSHOT_NO_AUDIO", raising=False)
    res = audio.probe()
    assert res["mic"][0] is False and "Gizlilik" in res["mic"][1]
    rec = AudioRecorder(False, True)
    warnings = rec.start()
    assert warnings and "izni" in warnings[0]
    rec.finish()


def test_first_use_triggers_the_permission_prompt_instead_of_recording_silence(fake, monkeypatch):
    import pinshot.audio as audio
    import pinshot.macperm as mp
    monkeypatch.setattr(audio, "_platform", lambda: "darwin")
    monkeypatch.setattr(mp, "mic_status", lambda: "not_determined")
    asked = []
    monkeypatch.setattr(mp, "request_mic", lambda on_done=None: asked.append(1) or True)
    rec = AudioRecorder(False, True)
    warnings = rec.start()
    assert asked and warnings and "bekleniyor" in warnings[0]
    rec.finish()


def test_silent_microphone_is_reported_after_the_recording(fake):
    fake.mic.level = 0.0                                   # permission denied on macOS looks exactly like this
    rec = AudioRecorder(False, True)
    rec.start()
    assert wait_until(lambda: rec.tracks and rec.tracks[0].nbytes > 0)
    rec.request_stop()
    tracks = rec.finish()
    assert tracks and any("sessiz" in w for w in rec.end_warnings)
    AudioRecorder.discard(tracks)


def test_microphone_that_never_delivers_data_is_reported(fake):
    class Mute(FakeRecorder):
        def record(self, numframes):
            time.sleep(60)                                 # like soundcard's record() waiting forever

    fake.mic.recorder = lambda samplerate, channels, blocksize: Mute(fake.mic, channels, fake.log)
    rec = AudioRecorder(False, True)
    rec.start()
    time.sleep(0.3)
    t = time.time()
    rec.request_stop()
    tracks = rec.finish()
    assert time.time() - t < 6, "stopping must not wait for a blocked device"
    assert not tracks and any("hiç ses verisi" in w for w in rec.end_warnings)
