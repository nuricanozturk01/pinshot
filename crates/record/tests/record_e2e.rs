//! End-to-end: synthetic frames → real ffmpeg → playable MP4 / GIF.

use pinshot_record::ffmpeg::{find_ffmpeg, parse_duration, Fmt};
use pinshot_record::{Event, Frame, Options, PixFmt, Recorder};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

fn run(fmt: Fmt, ext: &str, system_audio: bool) -> Option<(PathBuf, f64)> {
    let ffmpeg = find_ffmpeg()?;
    let out = std::env::temp_dir().join(format!("pinshot_e2e_{}_{ext}.{ext}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let mut n = 0u32;
    let opts = Options {
        fmt,
        fps: 15,
        max_width: 0,
        hq444: false,
        max_seconds: 30,
        audio_system: system_audio,
        audio_mic: false,
        mic_device: String::new(),
        out: out.clone(),
    };
    let mut rec = Recorder::start(
        opts,
        move || {
            n += 1;
            let (w, h) = (160u32, 90u32);
            let mut rgba = vec![255u8; (w * h * 4) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i] = ((x + n * 5) % 256) as u8;
                    rgba[i + 1] = (y * 2) as u8;
                    rgba[i + 2] = (n * 7 % 256) as u8;
                }
            }
            Some(Frame {
                width: w,
                height: h,
                format: PixFmt::Rgba,
                data: rgba,
            })
        },
        move |e| {
            let _ = tx.lock().unwrap().send(e);
        },
    )
    .expect("recording starts");
    std::thread::sleep(Duration::from_millis(2200));
    rec.stop();
    loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("finishes") {
            Event::Finished(path, stats) => {
                assert!(stats.frames >= 15, "frames: {}", stats.frames);
                let banner = std::process::Command::new(&ffmpeg)
                    .args(["-hide_banner", "-i"])
                    .arg(&path)
                    .output()
                    .unwrap();
                return Some((
                    path,
                    parse_duration(&String::from_utf8_lossy(&banner.stderr)),
                ));
            }
            Event::Failed(e) => panic!("recording failed: {e}"),
            _ => {}
        }
    }
}

#[test]
fn records_an_mp4() {
    let Some((path, dur)) = run(Fmt::Mp4, "mp4", false) else {
        return eprintln!("ffmpeg not available: skipped");
    };
    assert!(dur > 1.0 && dur < 4.0, "duration {dur}");
    assert!(std::fs::metadata(&path).unwrap().len() > 1000);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn records_a_gif() {
    let Some((path, dur)) = run(Fmt::Gif, "gif", false) else {
        return eprintln!("ffmpeg not available: skipped");
    };
    assert!(dur > 1.0, "duration {dur}");
    let head = std::fs::read(&path).unwrap();
    assert_eq!(&head[..3], b"GIF");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn records_an_mp4_with_system_sound_when_pulse_is_available() {
    if std::env::var_os("PINSHOT_AUDIO_E2E").is_none() {
        return eprintln!(
            "set PINSHOT_AUDIO_E2E=1 on a machine with PulseAudio/PipeWire to run this"
        );
    }
    let Some((path, dur)) = run(Fmt::Mp4, "audio.mp4", true) else {
        return;
    };
    assert!(dur > 1.0);
    let ffmpeg = find_ffmpeg().unwrap();
    let banner = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&banner.stderr).contains("Audio:"),
        "an audio stream was muxed in"
    );
    std::fs::remove_file(path).unwrap();
}
