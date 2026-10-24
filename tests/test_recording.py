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
    assert enc._q.qsize() <= 2 and enc.dropped > 0 and not all(results)
    enc.abort()
    assert not os.path.exists(str(tmp_path / "b.mp4"))


def test_abort_removes_everything(tmp_path):
    enc, out = _record(tmp_path, "mp4", frames=5)
    enc.abort()
    assert not os.path.exists(out) and not os.path.exists(enc.temp)
    assert enc.proc.poll() is not None                     # ffmpeg process is gone
