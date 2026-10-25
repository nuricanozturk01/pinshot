//! ffmpeg: locating it, the encoding command lines, and the encoder process fed through a bounded queue.

use crate::audio::Track;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const GIF_MAX_FPS: u32 = 50;
pub const QUEUE_FRAMES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fmt {
    Mp4,
    Gif,
}

#[derive(Clone, Debug)]
pub struct VideoOpts {
    pub fmt: Fmt,
    pub fps: u32,
    /// Maximum output width in pixels (0 = original); ffmpeg does the scaling.
    pub max_width: u32,
    pub hq444: bool,
    pub with_audio: bool,
    pub input: crate::PixFmt,
}

/// ffmpeg next to the program, or named by `PINSHOT_FFMPEG`, or on the PATH.
pub fn find_ffmpeg() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Some(p) = std::env::var_os("PINSHOT_FFMPEG")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(p);
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf());
            dirs.push(d.join("../Resources")); // macOS app bundle
            dirs.push(d.join("../lib/pinshot")); // Linux package
        }
    }
    for d in dirs {
        let c = d.join(name);
        if c.is_file() {
            return Some(c);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|c| c.is_file())
    })
}

/// Command that runs low priority (and without a console window on Windows).
pub fn command(exe: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new(exe);
        c.creation_flags(0x0000_4000 | 0x0800_0000); // BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("nice");
        c.args(["-n", "10"]).arg(exe);
        c
    }
}

pub fn video_args(
    size: (u32, u32),
    o: &VideoOpts,
    target: &Path,
    temp: &Path,
    cpus: usize,
) -> Vec<String> {
    let to_file = target == temp;
    let (crf, preset) = match (o.fmt, o.with_audio) {
        (Fmt::Mp4, false) => ("18", "veryfast"),
        (Fmt::Mp4, true) => ("17", "veryfast"),
        (Fmt::Gif, _) => ("12", "ultrafast"),
    };
    let tail = "out_color_matrix=bt709:out_range=tv:flags=lanczos+accurate_rnd+full_chroma_int";
    let scale = if o.max_width > 0 {
        format!("scale='min({},iw)':-2:{tail}", o.max_width)
    } else {
        format!("scale={tail}")
    };
    // 4:2:0 halves the colour resolution and blurs coloured text; 4:4:4 keeps it but some players cannot open it.
    // GIF temp files always use 4:4:4 (they are never played).
    let pix = if o.hq444 || o.fmt == Fmt::Gif {
        "yuv444p"
    } else {
        "yuv420p"
    };
    let threads = (cpus / 2).max(2).to_string();
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pix_fmt",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(o.input.ffmpeg().to_string());
    a.push("-video_size".to_string());
    a.push(format!("{}x{}", size.0, size.1));
    a.extend(["-use_wallclock_as_timestamps", "1", "-i", "pipe:0", "-vf"].map(String::from));
    a.push(format!(
        "fps={},crop=trunc(iw/2)*2:trunc(ih/2)*2,{scale},format={pix}",
        o.fps
    ));
    a.extend(
        [
            "-c:v", "libx264", "-preset", preset, "-crf", crf, "-threads", &threads,
        ]
        .map(String::from),
    );
    a.extend(
        [
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ]
        .map(String::from),
    );
    if (!to_file || (o.fmt == Fmt::Mp4 && !o.with_audio)) && target != temp {
        a.extend(["-movflags", "+faststart"].map(String::from));
    }
    a.push(target.to_string_lossy().into_owned());
    a
}

pub fn gif_args(temp: &Path, out: &Path, fps: u32) -> Vec<String> {
    let fps = fps.min(GIF_MAX_FPS);
    // one palette per frame: far better colours when the content changes
    let vf = format!(
        "fps={fps},split[a][b];[a]palettegen=max_colors=256:stats_mode=single[p];[b][p]paletteuse=new=1:dither=sierra2_4a:diff_mode=rectangle"
    );
    let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
        .map(String::from)
        .to_vec();
    a.push(temp.to_string_lossy().into_owned());
    a.extend([
        "-vf".to_string(),
        vf,
        "-loop".to_string(),
        "0".to_string(),
        out.to_string_lossy().into_owned(),
    ]);
    a
}

/// Audio mux command: copy the video, add the PCM tracks (padded to the video length).
pub fn mux_args(
    temp: &Path,
    tracks: &[Track],
    t0_unix: f64,
    dur: f64,
    out: &Path,
) -> Vec<Vec<String>> {
    let build = |fc: String| {
        let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-i"]
            .map(String::from)
            .to_vec();
        a.push(temp.to_string_lossy().into_owned());
        for t in tracks {
            let off = t.started_unix - t0_unix; // >0: audio opened after the first frame
            a.extend(["-f", "s16le", "-ar"].map(String::from));
            a.push(t.rate.to_string());
            a.push("-ac".into());
            a.push(t.channels.to_string());
            if off > 0.001 {
                a.extend(["-itsoffset".to_string(), format!("{off:.3}")]);
            } else if off < -0.001 {
                a.extend(["-ss".to_string(), format!("{:.3}", -off)]);
            }
            a.push("-i".into());
            a.push(t.path.to_string_lossy().into_owned());
        }
        a.extend(["-filter_complex".to_string(), fc]);
        a.extend(
            [
                "-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ar",
                "48000", "-ac", "2", "-t",
            ]
            .map(String::from),
        );
        a.push(format!("{dur:.3}"));
        a.extend(["-movflags", "+faststart"].map(String::from));
        a.push(out.to_string_lossy().into_owned());
        a
    };
    let pad = format!("apad=whole_dur={dur:.3}");
    if tracks.len() == 1 {
        vec![build(format!("[1:a]{pad}[a]"))]
    } else {
        // `normalize` needs ffmpeg >= 4.4: fall back if unsupported
        vec![
            build(format!(
                "[1:a][2:a]amix=inputs=2:duration=longest:normalize=0,alimiter=limit=0.97,{pad}[a]"
            )),
            build(format!("[1:a][2:a]amix=inputs=2:duration=longest,{pad}[a]")),
        ]
    }
}

pub struct Encoder {
    exe: PathBuf,
    child: Child,
    tx: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<Option<String>>>,
    frames: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    pending: Arc<AtomicUsize>,
    temp: PathBuf,
    err_path: PathBuf,
    out: PathBuf,
    opts: VideoOpts,
    started_unix: f64,
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn temp_path(suffix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pinshot-rec-{}-{ms}-{n}{suffix}",
        std::process::id()
    ))
}

/// Remove temp files from a crashed recording (the ffmpeg child exits on its own when its stdin closes).
pub fn cleanup_stale_temp(max_age_secs: u64) {
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("pinshot-rec-") {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs() > max_age_secs)
                .unwrap_or(false);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

impl Encoder {
    pub fn new(exe: &Path, size: (u32, u32), o: &VideoOpts, out: &Path) -> Result<Encoder, String> {
        let temp = temp_path(".mp4");
        let err_path = temp.with_extension("mp4.log");
        let target = if o.fmt == Fmt::Mp4 && !o.with_audio {
            out.to_path_buf()
        } else {
            temp.clone()
        };
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let args = video_args(size, o, &target, &temp, cpus);
        if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let err = File::create(&err_path).map_err(|e| e.to_string())?;
        let mut child = command(exe)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(err))
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let (tx, rx) = sync_channel::<Vec<u8>>(QUEUE_FRAMES);
        let pending = Arc::new(AtomicUsize::new(0));
        let pending_w = pending.clone();
        let writer = std::thread::Builder::new()
            .name("pinshot-rec-writer".into())
            .spawn(move || {
                let mut failed = None;
                for frame in rx {
                    let r = stdin.write_all(&frame);
                    pending_w.fetch_sub(1, Ordering::Relaxed);
                    if let Err(e) = r {
                        failed = Some(format!("encoder stopped: {e}"));
                        break;
                    }
                }
                failed
            })
            .map_err(|e| e.to_string())?;
        Ok(Encoder {
            exe: exe.to_path_buf(),
            child,
            tx: Some(tx),
            writer: Some(writer),
            frames: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            pending,
            temp,
            err_path,
            out: out.to_path_buf(),
            opts: o.clone(),
            started_unix: unix_now(),
        })
    }

    pub fn started_at(&self) -> f64 {
        self.started_unix
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn note_dropped(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// True when the encoder is behind: the caller should skip grabbing instead of queueing more.
    pub fn queue_full(&self) -> bool {
        self.pending.load(Ordering::Relaxed) >= QUEUE_FRAMES
    }

    /// Queue a frame without blocking. Returns false (and counts a drop) when the encoder is behind.
    pub fn push(&self, rgba: Vec<u8>) -> bool {
        let Some(tx) = &self.tx else { return false };
        self.pending.fetch_add(1, Ordering::Relaxed);
        match tx.try_send(rgba) {
            Ok(()) => {
                self.frames.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                self.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    fn stderr_tail(&self) -> String {
        let mut s = String::new();
        if let Ok(mut f) = File::open(&self.err_path) {
            let _ = f.read_to_string(&mut s);
        }
        let n = s.len().saturating_sub(600);
        s[n..].trim().to_string()
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.temp);
        let _ = std::fs::remove_file(&self.err_path);
    }

    pub fn abort(&mut self) {
        self.tx.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        self.cleanup();
        let _ = std::fs::remove_file(&self.out);
    }

    /// Close the input, wait for ffmpeg, then run the GIF palette pass or the audio mux. Blocks.
    pub fn finish(mut self, tracks: &[Track], t0_unix: f64) -> Result<(), String> {
        self.tx.take(); // closes the queue; the writer drains it and closes ffmpeg's stdin
        let writer_err = self.writer.take().and_then(|w| w.join().ok()).flatten();
        let deadline = Instant::now() + std::time::Duration::from_secs(120);
        let code = loop {
            match self.child.try_wait() {
                Ok(Some(s)) => break s.code().unwrap_or(-1),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                _ => {
                    let _ = self.child.kill();
                    break -1;
                }
            }
        };
        let mut result = if code == 0 && writer_err.is_none() && self.frames() > 0 {
            Ok(())
        } else {
            Err(writer_err.unwrap_or_else(|| self.stderr_tail()))
        };
        if result.is_ok() {
            result = match self.opts.fmt {
                Fmt::Gif => self.to_gif(),
                Fmt::Mp4 if !tracks.is_empty() => self.mux(tracks, t0_unix),
                Fmt::Mp4 => Ok(()),
            };
        }
        self.cleanup();
        if result.is_err() {
            let _ = std::fs::remove_file(&self.out);
        }
        result
    }

    fn to_gif(&self) -> Result<(), String> {
        let out = command(&self.exe)
            .args(gif_args(&self.temp, &self.out, self.opts.fps))
            .stdout(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(tail(&out.stderr))
        }
    }

    fn duration(&self) -> f64 {
        let Ok(out) = Command::new(&self.exe)
            .args(["-hide_banner", "-i"])
            .arg(&self.temp)
            .output()
        else {
            return 0.0;
        };
        parse_duration(&String::from_utf8_lossy(&out.stderr))
    }

    fn mux(&self, tracks: &[Track], t0_unix: f64) -> Result<(), String> {
        let dur = self.duration();
        if dur <= 0.0 {
            return Err("could not read the video length".into());
        }
        let mut err = String::new();
        for args in mux_args(&self.temp, tracks, t0_unix, dur, &self.out) {
            match Command::new(&self.exe)
                .args(&args)
                .stdout(Stdio::null())
                .output()
            {
                Ok(o) if o.status.success() => return Ok(()),
                Ok(o) => err = tail(&o.stderr),
                Err(e) => err = e.to_string(),
            }
        }
        Err(err)
    }
}

fn tail(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let n = s.len().saturating_sub(600);
    s[n..].trim().to_string()
}

/// `Duration: 00:01:02.50` in ffmpeg's banner → seconds.
pub fn parse_duration(banner: &str) -> f64 {
    let Some(i) = banner.find("Duration: ") else {
        return 0.0;
    };
    let t = banner[i + 10..].split(',').next().unwrap_or("").trim();
    let p: Vec<&str> = t.split(':').collect();
    if p.len() != 3 {
        return 0.0;
    }
    let f = |s: &str| s.parse::<f64>().unwrap_or(0.0);
    f(p[0]) * 3600.0 + f(p[1]) * 60.0 + f(p[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(fmt: Fmt, audio: bool, w: u32, hq: bool) -> VideoOpts {
        VideoOpts {
            fmt,
            fps: 30,
            max_width: w,
            hq444: hq,
            with_audio: audio,
            input: crate::PixFmt::Rgba,
        }
    }

    #[test]
    fn mp4_without_audio_writes_the_final_file_with_faststart() {
        let a = video_args(
            (1920, 1080),
            &opts(Fmt::Mp4, false, 0, false),
            Path::new("/o/out.mp4"),
            Path::new("/t/tmp.mp4"),
            8,
        );
        let s = a.join(" ");
        assert!(s.contains("-video_size 1920x1080") && s.contains("-pix_fmt rgba"));
        assert!(s.contains("-crf 18") && s.contains("format=yuv420p") && s.contains("+faststart"));
        assert_eq!(a.last().unwrap(), "/o/out.mp4");
        assert!(s.contains("-threads 4"));
        assert!(s.contains("fps=30,crop=trunc(iw/2)*2:trunc(ih/2)*2,scale=out_color_matrix=bt709"));
    }

    #[test]
    fn quality_options_change_the_pipeline() {
        let hq = video_args(
            (100, 100),
            &opts(Fmt::Mp4, false, 1280, true),
            Path::new("/o.mp4"),
            Path::new("/t.mp4"),
            2,
        )
        .join(" ");
        assert!(hq.contains("format=yuv444p") && hq.contains("scale='min(1280,iw)':-2"));
        let audio = video_args(
            (100, 100),
            &opts(Fmt::Mp4, true, 0, false),
            Path::new("/t.mp4"),
            Path::new("/t.mp4"),
            2,
        )
        .join(" ");
        assert!(
            audio.contains("-crf 17") && !audio.contains("faststart"),
            "temp file for the mux step"
        );
        let gif = video_args(
            (100, 100),
            &opts(Fmt::Gif, false, 1920, false),
            Path::new("/t.mp4"),
            Path::new("/t.mp4"),
            2,
        )
        .join(" ");
        assert!(
            gif.contains("-crf 12") && gif.contains("-preset ultrafast") && gif.contains("yuv444p")
        );
    }

    #[test]
    fn gif_pass_caps_the_frame_rate_and_uses_per_frame_palettes() {
        let s = gif_args(Path::new("/t.mp4"), Path::new("/o.gif"), 60).join(" ");
        assert!(
            s.contains("fps=50,")
                && s.contains("stats_mode=single")
                && s.contains("paletteuse=new=1")
                && s.contains("-loop 0")
        );
    }

    #[test]
    fn mux_aligns_tracks_and_falls_back_without_normalize() {
        let t = |label: &str, off: f64| Track {
            label: label.into(),
            path: PathBuf::from(format!("/{label}.pcm")),
            channels: 2,
            rate: 48000,
            started_unix: 100.0 + off,
            error: String::new(),
        };
        let one = mux_args(
            Path::new("/t.mp4"),
            &[t("sys", 0.5)],
            100.0,
            12.0,
            Path::new("/o.mp4"),
        );
        assert_eq!(one.len(), 1);
        let s = one[0].join(" ");
        assert!(
            s.contains("-itsoffset 0.500")
                && s.contains("apad=whole_dur=12.000")
                && s.contains("-t 12.000")
        );
        let two = mux_args(
            Path::new("/t.mp4"),
            &[t("a", -0.25), t("b", 0.0)],
            100.0,
            5.0,
            Path::new("/o.mp4"),
        );
        assert_eq!(two.len(), 2);
        assert!(
            two[0].join(" ").contains("normalize=0") && !two[1].join(" ").contains("normalize")
        );
        assert!(two[0].join(" ").contains("-ss 0.250"));
    }

    #[test]
    fn parses_the_banner_duration() {
        assert_eq!(parse_duration("  Duration: 00:01:02.50, start: 0.0"), 62.5);
        assert_eq!(parse_duration("nothing"), 0.0);
    }
}
