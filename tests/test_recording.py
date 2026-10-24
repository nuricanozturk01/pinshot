import os
import time

import pytest

from pinshot.recorder import Encoder, find_ffmpeg

pytestmark = pytest.mark.skipif(find_ffmpeg() is None, reason="no ffmpeg available")


def _record(tmp_path, fmt, w=161, h=91, frames=20, fps=15):
    out = str(tmp_path / f"t.{fmt}")
    enc = Encoder(find_ffmpeg(), (w, h), fps, fmt, out)
    for i in range(frames):
        assert enc.push(bytes([(i * 12) % 256, 90, 200, 255]) * (w * h)) or enc.dropped
        time.sleep(1 / fps)
    return enc, out


@pytest.mark.parametrize("fmt,magic", [("mp4", b"ftyp"), ("gif", b"GIF8")])
def test_encoder_writes_valid_file_and_cleans_up(tmp_path, fmt, magic):
    enc, out = _record(tmp_path, fmt)
    ok, err = enc.finish()
    assert ok, err
    assert magic in open(out, "rb").read(12)
    assert not os.path.exists(enc.temp) and not os.path.exists(enc._err_path)   # temp files removed


def test_queue_is_bounded(tmp_path):
    enc = Encoder(find_ffmpeg(), (64, 64), 15, "mp4", str(tmp_path / "b.mp4"))
    frame = b"\x00" * (64 * 64 * 4)
    results = [enc.push(frame) for _ in range(500)]       # far faster than the encoder can consume
    from pinshot.recorder import QUEUE_FRAMES
    assert enc._q.qsize() <= QUEUE_FRAMES and enc.dropped > 0 and not all(results)
    enc.abort()
    assert not os.path.exists(str(tmp_path / "b.mp4"))


def test_abort_removes_everything(tmp_path):
    enc, out = _record(tmp_path, "mp4", frames=5)
    enc.abort()
    assert not os.path.exists(out) and not os.path.exists(enc.temp)
    assert enc.proc.poll() is not None                     # ffmpeg process is gone


def _sine_pcm(path, seconds=1.5, freq=440, channels=2):
    import math
    import struct
    n = int(48000 * seconds)
    with open(path, "wb") as f:
        f.write(b"".join(struct.pack("<" + "h" * channels, *([int(12000 * math.sin(2 * math.pi * freq * i / 48000))] * channels))
                         for i in range(n)))


def _streams(path):
    import subprocess
    out = subprocess.run([find_ffmpeg(), "-hide_banner", "-i", path], capture_output=True, text=True).stderr
    return [line.split("Stream")[1] for line in out.splitlines() if "Stream #" in line]


@pytest.mark.parametrize("n_tracks", [1, 2])
def test_mp4_gets_audio_stream_from_recorded_tracks(tmp_path, n_tracks):
    from pinshot.audio import Track
    out = str(tmp_path / "a.mp4")
    enc = Encoder(find_ffmpeg(), (161, 91), 15, "mp4", out, 0, with_audio=True)
    t0 = time.monotonic()
    for i in range(20):
        enc.push(bytes([(i * 12) % 256, 90, 200, 255]) * (161 * 91))
        time.sleep(1 / 15)
    tracks = []
    for i in range(n_tracks):
        p = str(tmp_path / f"t{i}.pcm")
        _sine_pcm(p, freq=440 * (i + 1))
        tracks.append(Track("x", p, 2, started=t0 + 0.05 * i))       # one opened slightly after the first frame
    ok, err = enc.finish(tracks, t0)
    assert ok, err
    kinds = " ".join(_streams(out))
    assert "Video: h264" in kinds and "Audio: aac" in kinds
    assert not os.path.exists(enc.temp)


def test_mp4_without_audio_has_no_audio_stream(tmp_path):
    enc, out = _record(tmp_path, "mp4")
    ok, err = enc.finish()
    assert ok, err
    assert "Audio" not in " ".join(_streams(out))


def test_video_is_tagged_bt709_and_even_sized(tmp_path):
    enc, out = _record(tmp_path, "mp4", w=161, h=91)               # odd input size
    ok, err = enc.finish()
    assert ok, err
    info = " ".join(_streams(out))
    assert "160x90" in info and "bt709" in info


def test_hq444_keeps_full_colour_resolution(tmp_path):
    out = str(tmp_path / "h.mp4")
    enc = Encoder(find_ffmpeg(), (160, 90), 15, "mp4", out, 0, with_audio=False, hq444=True)
    for i in range(15):
        enc.push(bytes([(i * 12) % 256, 90, 200, 255]) * (160 * 90))
        time.sleep(1 / 15)
    ok, err = enc.finish()
    assert ok, err
    assert "yuv444p" in " ".join(_streams(out))


def test_default_mp4_stays_player_friendly_420(tmp_path):
    enc, out = _record(tmp_path, "mp4")
    ok, err = enc.finish()
    assert ok, err
    assert "yuv420p" in " ".join(_streams(out))


def test_encoder_accepts_qimage_frames_without_copying(tmp_path):
    from PySide6.QtGui import QColor, QImage
    out = str(tmp_path / "q.mp4")
    enc = Encoder(find_ffmpeg(), (160, 90), 15, "mp4", out)
    for i in range(12):
        img = QImage(160, 90, QImage.Format.Format_RGB32)
        img.fill(QColor.fromHsv(i * 20, 200, 220))
        assert enc.push(img) or enc.dropped
        time.sleep(1 / 15)
    ok, err = enc.finish()
    assert ok, err
    assert os.path.getsize(out) > 500
