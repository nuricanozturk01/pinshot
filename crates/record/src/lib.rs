//! Screen recording to MP4 or GIF.
//!
//! The caller supplies a frame source (a closure that grabs the region). Frames go through a small bounded queue
//! into a separate low-priority ffmpeg process, so nothing accumulates in RAM (a slow encoder just drops frames).
//! GIFs are produced from a temporary MP4 with a palette pass; audio tracks are muxed in at the end.

pub mod audio;
pub mod ffmpeg;

use ffmpeg::{Encoder, Fmt, VideoOpts};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Pixel layout of a frame. `Bgr0` is what X11, GDI and CoreGraphics deliver (the fourth byte is ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixFmt {
    Rgba,
    Bgr0,
}

impl PixFmt {
    pub fn ffmpeg(self) -> &'static str {
        match self {
            PixFmt::Rgba => "rgba",
            PixFmt::Bgr0 => "bgr0",
        }
    }
}

/// One captured frame: tightly packed pixels.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: PixFmt,
    pub data: Vec<u8>,
}

pub struct Options {
    pub fmt: Fmt,
    pub fps: u32,
    pub max_width: u32,
    pub hq444: bool,
    pub max_seconds: u32,
    pub audio_system: bool,
    pub audio_mic: bool,
    pub mic_device: String,
    pub out: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub frames: u64,
    pub dropped: u64,
    pub seconds: f64,
    pub fps: u32,
    pub gif: bool,
}

pub enum Event {
    Tick(u32),
    Warning(String),
    Finished(PathBuf, Stats),
    Failed(String),
}

pub struct Recorder {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Start recording. `grab` is called from the capture thread once per frame; it must return frames of constant
    /// size (a different size is skipped). `on_event` is called from background threads.
    pub fn start(
        opts: Options,
        mut grab: impl FnMut() -> Option<Frame> + Send + 'static,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Recorder, String> {
        let exe = ffmpeg::find_ffmpeg().ok_or_else(|| "ffmpeg not found".to_string())?;
        let on_event = Arc::new(on_event);
        let with_audio = opts.fmt == Fmt::Mp4 && (opts.audio_system || opts.audio_mic);
        let mut audio = None;
        if with_audio {
            let (rec, warnings) = audio::AudioRecorder::start(
                &exe,
                opts.audio_system,
                opts.audio_mic,
                &opts.mic_device,
            );
            for w in warnings {
                on_event(Event::Warning(w));
            }
            if rec.has_tracks() {
                audio = Some(rec);
            }
        }
        let first = grab().ok_or_else(|| "could not capture the screen".to_string());
        let first = match first {
            Ok(f) => f,
            Err(e) => {
                if let Some(a) = audio {
                    audio::AudioRecorder::discard(a.finish());
                }
                return Err(e);
            }
        };
        let vopts = VideoOpts {
            fmt: opts.fmt,
            fps: opts.fps,
            max_width: opts.max_width,
            hq444: opts.hq444,
            with_audio: audio.is_some(),
            input: first.format,
        };
        let enc = match Encoder::new(&exe, (first.width, first.height), &vopts, &opts.out) {
            Ok(e) => e,
            Err(e) => {
                if let Some(a) = audio {
                    audio::AudioRecorder::discard(a.finish());
                }
                return Err(e);
            }
        };
        enc.push(first.data);
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let (stop2, cancel2) = (stop.clone(), cancel.clone());
        let size = (first.width, first.height);
        let (fps, max_seconds, gif, out) = (
            opts.fps.max(1),
            opts.max_seconds,
            opts.fmt == Fmt::Gif,
            opts.out.clone(),
        );
        let thread = std::thread::Builder::new()
            .name("pinshot-record".into())
            .spawn(move || {
                let enc = Arc::new(Mutex::new(Some(enc)));
                let t0 = Instant::now();
                let period = Duration::from_secs_f64(1.0 / fps as f64);
                let mut next = t0 + period;
                let mut last_tick = 0u32;
                let mut audio = audio;
                while !stop2.load(Ordering::Relaxed) {
                    let now = Instant::now();
                    if now < next {
                        std::thread::sleep((next - now).min(Duration::from_millis(50)));
                        continue;
                    }
                    next += period;
                    if next < now {
                        next = now + period; // fell behind: do not try to catch up with a burst
                    }
                    let guard = enc.lock().expect("encoder lock");
                    let Some(e) = guard.as_ref() else { break };
                    if e.queue_full() {
                        e.note_dropped();
                    } else if let Some(f) = grab() {
                        if (f.width, f.height) == size {
                            e.push(f.data);
                        }
                    }
                    drop(guard);
                    let secs = t0.elapsed().as_secs() as u32;
                    if secs != last_tick {
                        last_tick = secs;
                        on_event(Event::Tick(secs));
                        if secs >= max_seconds {
                            break;
                        }
                    }
                }
                let seconds = t0.elapsed().as_secs_f64().max(0.001);
                let Some(mut e) = enc.lock().expect("encoder lock").take() else {
                    return;
                };
                if cancel2.load(Ordering::Relaxed) {
                    if let Some(a) = audio.take() {
                        audio::AudioRecorder::discard(a.finish());
                    }
                    e.abort();
                    return;
                }
                if let Some(a) = audio.as_mut() {
                    a.request_stop(); // stop the sound at the same moment as the video
                }
                let stats = Stats {
                    frames: e.frames(),
                    dropped: e.dropped(),
                    seconds,
                    fps,
                    gif,
                };
                let tracks = audio.map(|a| a.finish());
                if let Some(t) = &tracks {
                    for w in audio::silence_warnings(t) {
                        on_event(Event::Warning(w));
                    }
                }
                let t0_unix = e.started_at();
                match e.finish(tracks.as_deref().unwrap_or(&[]), t0_unix) {
                    Ok(()) => on_event(Event::Finished(out, stats)),
                    Err(err) => on_event(Event::Failed(err)),
                }
                if let Some(t) = tracks {
                    audio::AudioRecorder::discard(t);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Recorder {
            stop,
            cancel,
            thread: Some(thread),
        })
    }

    /// Stop capturing; the file is finished in the background and `Event::Finished` follows.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Stop and throw everything away.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
