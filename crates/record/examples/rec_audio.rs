//! Manual check: records 4 s of synthetic video with system sound and/or microphone and prints what ended up in the file.
//! usage: cargo run -p pinshot-record --example rec_audio -- [system] [mic] [out.mp4]
use pinshot_record::ffmpeg::{find_ffmpeg, Fmt};
use pinshot_record::{Event, Frame, Options, PixFmt, Recorder};
use std::sync::mpsc;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::path::PathBuf::from(
        args.iter()
            .find(|a| a.ends_with(".mp4"))
            .map_or("/tmp/rec_audio.mp4", String::as_str),
    );
    let ffmpeg = find_ffmpeg().expect("ffmpeg");
    let (tx, rx) = mpsc::channel();
    let opts = Options {
        fmt: Fmt::Mp4,
        fps: 15,
        max_width: 0,
        hq444: false,
        max_seconds: 30,
        audio_system: args.iter().any(|a| a == "system"),
        audio_mic: args.iter().any(|a| a == "mic"),
        mic_device: String::new(),
        out: out.clone(),
    };
    let mut n = 0u32;
    let mut rec = Recorder::start(
        opts,
        move || {
            n += 1;
            Some(Frame {
                width: 160,
                height: 90,
                format: PixFmt::Rgba,
                data: vec![(n % 255) as u8; 160 * 90 * 4],
            })
        },
        move |e| {
            let _ = tx.send(e);
        },
    )
    .expect("start");
    std::thread::sleep(Duration::from_secs(4));
    rec.stop();
    loop {
        match rx.recv_timeout(Duration::from_secs(60)).expect("finish") {
            Event::Finished(p, stats) => {
                println!("finished {} frames {}", p.display(), stats.frames);
                break;
            }
            Event::Failed(e) => panic!("failed: {e}"),
            Event::Warning(w) => println!("warning: {w}"),
            Event::Tick(_) => {}
        }
    }
    let r = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&out)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .unwrap();
    for l in String::from_utf8_lossy(&r.stderr).lines() {
        if l.contains("Stream #") || l.contains("mean_volume") || l.contains("max_volume") {
            println!("{l}");
        }
    }
}
