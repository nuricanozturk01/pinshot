//! Audio for MP4 recordings: system sound and/or microphone, captured to raw PCM files and muxed at the end.
//!
//! None of this depends on what the bundled ffmpeg can capture (its static builds lack most input devices):
//!
//! * Linux: `parec` (PulseAudio and PipeWire) on the default sink's monitor and on a microphone; ffmpeg's `pulse`
//!   input is only the fallback.
//! * macOS: microphone through cpal (CoreAudio); system sound through the bundled `pinshot-audiotap` helper
//!   (ScreenCaptureKit, macOS 13+), or a virtual device such as BlackHole read through cpal.
//! * Windows: cpal (WASAPI): loopback of the default output, and a microphone.
//!
//! Every capture writes interleaved signed 16-bit little-endian PCM; the track records its rate and channel count.
#![cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "the child-process capture programs are used on Linux and macOS; Windows uses cpal"
    )
)]

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const RATE: u32 = 48_000;

#[derive(Clone, Debug)]
pub struct Track {
    pub label: String,
    pub path: PathBuf,
    pub channels: u16,
    pub rate: u32,
    /// Wall-clock time (seconds since the epoch) when the device started delivering audio.
    pub started_unix: f64,
    pub error: String,
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn audio_disabled() -> bool {
    std::env::var_os("PINSHOT_NO_AUDIO").is_some()
}

/// Loudest sample (0..32767) in a 16-bit PCM file; 0 means pure silence.
pub fn file_peak(path: &Path) -> i32 {
    let Ok(mut f) = File::open(path) else {
        return 0;
    };
    let mut buf = vec![0u8; 1 << 16];
    let mut peak = 0i32;
    while let Ok(n) = f.read(&mut buf) {
        if n == 0 {
            break;
        }
        for s in buf[..n / 2 * 2].as_chunks::<2>().0 {
            peak = peak.max((i16::from_le_bytes([s[0], s[1]]) as i32).abs());
        }
    }
    peak
}

/// Messages for sources that recorded nothing or only silence.
pub fn silence_warnings(tracks: &[Track]) -> Vec<String> {
    let mut out = Vec::new();
    for t in tracks {
        let size = std::fs::metadata(&t.path).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            out.push(format!("{}: no audio was recorded", t.label));
        } else if file_peak(&t.path) == 0 {
            out.push(format!("{}: only silence was recorded", t.label));
        }
    }
    out
}

fn pcm_path(kind: &str) -> PathBuf {
    crate::ffmpeg::temp_path(&format!("-{kind}.pcm"))
}

// ---------------------------------------------------------------- capture programs
/// A program that writes PCM into `path`; it is stopped by writing "q" to its stdin (it also ends on EOF).
struct ChildCapture {
    child: Child,
    err_path: PathBuf,
    /// The program has no way to be told to stop (it writes raw PCM, so killing it loses nothing).
    kill_on_stop: bool,
}

impl ChildCapture {
    /// Spawn and wait until it delivers data: `ready_line` on its stdout, or (when `None`) the output file growing.
    fn start(
        mut cmd: Command,
        path: &Path,
        ready_line: Option<&str>,
        timeout: Duration,
    ) -> Result<(ChildCapture, f64), String> {
        let err_path = path.with_extension("pcm.err");
        let err = File::create(&err_path).map_err(|e| e.to_string())?;
        cmd.stdin(Stdio::piped()).stderr(Stdio::from(err));
        cmd.stdout(if ready_line.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let ready = Arc::new(AtomicBool::new(false));
        if let (Some(line), Some(out)) = (ready_line, child.stdout.take()) {
            let (flag, line) = (ready.clone(), line.to_string());
            std::thread::spawn(move || {
                for l in BufReader::new(out).lines().map_while(Result::ok) {
                    if l.trim() == line {
                        flag.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
        let mut cap = ChildCapture {
            child,
            err_path,
            kill_on_stop: false,
        };
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            let up = if ready_line.is_some() {
                ready.load(Ordering::Relaxed)
            } else {
                std::fs::metadata(path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false)
            };
            if up {
                return Ok((cap, unix_now()));
            }
            if let Ok(Some(status)) = cap.child.try_wait() {
                return Err(cap.explain(status.code()));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let why = cap.explain(None);
        cap.stop();
        Err(why)
    }

    fn stderr_text(&self) -> String {
        let s = std::fs::read_to_string(&self.err_path).unwrap_or_default();
        let n = s.len().saturating_sub(400);
        s[n..].trim().to_string()
    }

    fn explain(&self, code: Option<i32>) -> String {
        let err = self.stderr_text();
        match code {
            Some(2) => "Screen Recording permission is missing. Allow PinShot in System Settings → Privacy & Security → Screen Recording and restart it.".into(),
            Some(65) => "System sound needs macOS 13 or newer.".into(),
            None => format!("the audio capture did not start in time{}", if err.is_empty() { String::new() } else { format!(" ({err})") }),
            Some(c) => if err.is_empty() { format!("the audio capture program exited with code {c}") } else { err },
        }
    }

    fn request_stop(&mut self) {
        if self.kill_on_stop {
            let _ = self.child.kill();
            return;
        }
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"q\n");
        }
    }

    fn stop(&mut self) {
        self.request_stop();
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.child.try_wait() {
                let _ = std::fs::remove_file(&self.err_path);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.err_path);
    }
}

// ---------------------------------------------------------------- Linux (PulseAudio / PipeWire)
/// `(name, description)` for each source of `pactl list sources`.
pub fn parse_pactl_sources(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(n) = l.strip_prefix("Name: ") {
            name = Some(n.to_string());
        } else if let (Some(d), Some(n)) = (l.strip_prefix("Description: "), name.take()) {
            out.push((n, d.to_string()));
        }
    }
    out
}

fn pactl(args: &[&str]) -> Option<String> {
    let o = Command::new("pactl")
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Real input devices as `(source name, description)`; monitor sources are not microphones.
pub fn pulse_mics() -> Vec<(String, String)> {
    let text = pactl(&["list", "sources"]).unwrap_or_default();
    parse_pactl_sources(&text)
        .into_iter()
        .filter(|(n, _)| !n.ends_with(".monitor"))
        .collect()
}

/// The source that carries what the default output plays.
pub fn pulse_system_source() -> Option<String> {
    let sink = pactl(&["get-default-sink"])
        .filter(|s| !s.is_empty())
        .or_else(|| {
            pactl(&["info"]).and_then(|t| {
                t.lines().find_map(|l| {
                    l.strip_prefix("Default Sink: ")
                        .map(|s| s.trim().to_string())
                })
            })
        })?;
    Some(format!("{sink}.monitor"))
}

#[cfg_attr(
    not(target_os = "linux"),
    allow(dead_code, reason = "PulseAudio is Linux only")
)]
fn pulse_default_mic(mics: &[(String, String)]) -> Option<String> {
    let def = pactl(&["get-default-source"]).filter(|s| !s.is_empty() && !s.ends_with(".monitor"));
    def.filter(|d| mics.iter().any(|(n, _)| n == d))
        .or_else(|| mics.first().map(|(n, _)| n.clone()))
}

/// Record PulseAudio/PipeWire source `src` as raw PCM into `path`. `parec` (shipped with `pactl`) is preferred: the
/// ffmpeg builds that get bundled with the app have no `pulse` input at all.
#[cfg(target_os = "linux")]
fn start_pulse_capture(
    ffmpeg: &Path,
    src: &str,
    path: &Path,
) -> Result<(ChildCapture, f64), String> {
    let has_parec = Command::new("parec")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let timeout = Duration::from_secs(5);
    if has_parec {
        let mut cmd = Command::new("parec");
        cmd.arg(format!("--device={src}"))
            .args(["--format=s16le", "--rate=48000", "--channels=2"])
            .arg(path);
        match ChildCapture::start(cmd, path, None, timeout) {
            Ok((mut cap, started)) => {
                cap.kill_on_stop = true;
                return Ok((cap, started));
            }
            Err(e) => log::warn!("parec could not capture {src}: {e}; trying ffmpeg"),
        }
        let _ = std::fs::remove_file(path);
    }
    let cmd = ffmpeg_capture_cmd(ffmpeg, &["-f", "pulse", "-i", src], path);
    ChildCapture::start(cmd, path, None, timeout)
        .map_err(|e| format!("{e} (install `pulseaudio-utils` for audio recording)"))
}

fn ffmpeg_capture_cmd(exe: &Path, input: &[&str], out: &Path) -> Command {
    let mut c = Command::new(exe);
    c.args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(input)
        .args(["-ar", "48000", "-ac", "2", "-f", "s16le"])
        .arg(out);
    c
}

// ---------------------------------------------------------------- macOS
const VIRTUAL_HINTS: [&str; 4] = ["blackhole", "loopback", "soundflower", "multi-output"];

/// A virtual audio device that can carry system sound (macOS before 13).
#[cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only the macOS capture path looks for virtual devices"
    )
)]
pub fn is_virtual(name: &str) -> bool {
    let n = name.to_lowercase();
    VIRTUAL_HINTS.iter().any(|h| n.contains(h)) || n.contains("aggregate")
}

#[cfg_attr(
    not(target_os = "macos"),
    expect(dead_code, reason = "used by the macOS capture path")
)]
fn audiotap_helper() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("PINSHOT_AUDIOTAP_DIR") {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.extend([
                d.to_path_buf(),
                d.join("../Resources"),
                d.join("../Frameworks"),
            ]);
        }
    }
    if let Some(f) = std::env::var_os("PINSHOT_AUDIOTAP")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(f);
    }
    dirs.into_iter()
        .map(|d| d.join("pinshot-audiotap"))
        .find(|p| p.is_file())
}

// ---------------------------------------------------------------- Windows and macOS microphones (cpal)
#[cfg(any(windows, target_os = "macos"))]
mod native {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{Device, SampleFormat};
    use std::sync::mpsc;

    pub struct Capture {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        pub rate: u32,
        pub channels: u16,
    }

    pub fn mic_names() -> Vec<String> {
        cpal::default_host()
            .input_devices()
            .map(|d| d.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    /// Record `device` (an input device, or an output device for loopback) to `path`.
    pub fn start(device: Device, loopback: bool, path: &Path) -> Result<Capture, String> {
        let cfg = if loopback {
            device.default_output_config()
        } else {
            device.default_input_config()
        }
        .map_err(|e| e.to_string())?;
        let (rate, channels, fmt) = (cfg.sample_rate().0, cfg.channels(), cfg.sample_format());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let stop2 = stop.clone();
        let path = path.to_path_buf();
        let config: cpal::StreamConfig = cfg.into();
        let thread = std::thread::spawn(move || {
            let err_fn = |e| log::warn!("audio stream error: {e}");
            let built = match fmt {
                SampleFormat::F32 => device.build_input_stream(
                    &config,
                    {
                        let tx = tx.clone();
                        move |d: &[f32], _: &_| {
                            let _ = tx.send(
                                d.iter()
                                    .flat_map(|s| {
                                        ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()
                                    })
                                    .collect(),
                            );
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => device.build_input_stream(
                    &config,
                    {
                        let tx = tx.clone();
                        move |d: &[i16], _: &_| {
                            let _ = tx.send(d.iter().flat_map(|s| s.to_le_bytes()).collect());
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::U16 => device.build_input_stream(
                    &config,
                    {
                        let tx = tx.clone();
                        move |d: &[u16], _: &_| {
                            let _ = tx.send(
                                d.iter()
                                    .flat_map(|s| ((*s as i32 - 32768) as i16).to_le_bytes())
                                    .collect(),
                            );
                        }
                    },
                    err_fn,
                    None,
                ),
                other => Err(cpal::BuildStreamError::StreamConfigNotSupported).inspect_err(|_| {
                    log::warn!("unsupported sample format {other:?}");
                }),
            };
            let stream = match built.and_then(|s| {
                s.play()
                    .map(|_| s)
                    .map_err(|_| cpal::BuildStreamError::DeviceNotAvailable)
            }) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut file = match File::create(&path) {
                Ok(f) => std::io::BufWriter::new(f),
                Err(_) => return,
            };
            while !stop2.load(Ordering::Relaxed) {
                if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                    let _ = file.write_all(&chunk);
                }
            }
            drop(stream);
            while let Ok(chunk) = rx.try_recv() {
                let _ = file.write_all(&chunk);
            }
            let _ = file.flush();
        });
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Capture {
                stop,
                thread: Some(thread),
                rate,
                channels,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("the audio device did not open in time".into()),
        }
    }

    impl Capture {
        pub fn stop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    #[cfg(windows)]
    pub fn default_output() -> Option<Device> {
        cpal::default_host().default_output_device()
    }

    /// An input device that carries system sound (BlackHole and similar), for macOS before 13.
    #[cfg(target_os = "macos")]
    pub fn find_virtual_input() -> Option<Device> {
        cpal::default_host()
            .input_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|n| super::is_virtual(&n)))
    }

    pub fn find_mic(name: &str) -> Option<Device> {
        let host = cpal::default_host();
        if !name.is_empty() {
            if let Some(d) = host
                .input_devices()
                .ok()?
                .find(|d| d.name().map(|n| n == name).unwrap_or(false))
            {
                return Some(d);
            }
        }
        host.default_input_device()
    }
}

// ---------------------------------------------------------------- the recorder
enum Capturing {
    Child(ChildCapture),
    #[cfg(any(windows, target_os = "macos"))]
    Cpal(native::Capture),
}

struct Source {
    track: Track,
    cap: Capturing,
}

pub struct AudioRecorder {
    sources: Vec<Source>,
}

/// Names of microphones the user can choose from.
pub fn list_microphones(ffmpeg: Option<&Path>) -> Vec<String> {
    if audio_disabled() {
        return Vec::new();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = ffmpeg;
        pulse_mics().into_iter().map(|(_, d)| d).collect()
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let _ = ffmpeg;
        native::mic_names()
    }
}

/// What can be recorded right now: `(system sound ok, reason if not)`, `(microphone ok, reason if not)`.
pub struct Probe {
    pub system: (bool, String),
    pub mic: (bool, String),
}

pub fn probe(ffmpeg: Option<&Path>) -> Probe {
    if audio_disabled() {
        let why = "Audio is disabled.".to_string();
        return Probe {
            system: (false, why.clone()),
            mic: (false, why),
        };
    }
    if ffmpeg.is_none() && !cfg!(windows) {
        let why = "ffmpeg was not found: audio recording is unavailable.".to_string();
        return Probe {
            system: (false, why.clone()),
            mic: (false, why),
        };
    }
    let mics = list_microphones(ffmpeg);
    let mic = if mics.is_empty() {
        (
            false,
            "No microphone found. Enable an input device in the system sound settings.".to_string(),
        )
    } else {
        (true, String::new())
    };
    #[cfg(target_os = "linux")]
    let system = if pulse_system_source().is_some() {
        (true, String::new())
    } else {
        (false, "No system sound source found.".to_string())
    };
    #[cfg(target_os = "macos")]
    let system = if audiotap_helper().is_some() || native::find_virtual_input().is_some() {
        (true, String::new())
    } else {
        (
            false,
            "System sound needs macOS 13 or newer, or a virtual device such as BlackHole."
                .to_string(),
        )
    };
    #[cfg(windows)]
    let system = if native::default_output().is_some() {
        (true, String::new())
    } else {
        (false, "No output device found.".to_string())
    };
    Probe { system, mic }
}

impl AudioRecorder {
    /// Open the sources. Returns the recorder and warnings for sources that could not be opened.
    pub fn start(
        ffmpeg: &Path,
        system: bool,
        mic: bool,
        mic_device: &str,
    ) -> (AudioRecorder, Vec<String>) {
        let mut rec = AudioRecorder {
            sources: Vec::new(),
        };
        let mut warnings = Vec::new();
        // Microphone first: opening a Bluetooth headset's mic switches it from A2DP to Hands-free, which replaces
        // the output device; system sound must then be opened on the device that exists afterwards.
        for (kind, on) in [("mic", mic), ("system", system)] {
            if !on {
                continue;
            }
            let label = if kind == "system" {
                "System sound"
            } else {
                "Microphone"
            };
            match rec.open(ffmpeg, kind, mic_device) {
                Ok(src) => rec.sources.push(src),
                Err(e) => warnings.push(format!("{label} could not be recorded: {e}")),
            }
        }
        (rec, warnings)
    }

    pub fn has_tracks(&self) -> bool {
        !self.sources.is_empty()
    }

    fn open(&self, ffmpeg: &Path, kind: &str, mic_device: &str) -> Result<Source, String> {
        let path = pcm_path(kind);
        let label = if kind == "system" {
            "System sound"
        } else {
            "Microphone"
        }
        .to_string();
        let track = |started: f64, rate: u32, channels: u16| Track {
            label: label.clone(),
            path: path.clone(),
            channels,
            rate,
            started_unix: started,
            error: String::new(),
        };
        #[cfg(target_os = "linux")]
        {
            let src = if kind == "system" {
                pulse_system_source().ok_or("no system sound source found")?
            } else {
                let mics = pulse_mics();
                if mics.is_empty() {
                    return Err("no microphone found".into());
                }
                mics.iter()
                    .find(|(_, d)| d == mic_device)
                    .map(|(n, _)| n.clone())
                    .or_else(|| pulse_default_mic(&mics))
                    .ok_or("no microphone found")?
            };
            let (cap, started) = start_pulse_capture(ffmpeg, &src, &path)?;
            Ok(Source {
                track: track(started, RATE, 2),
                cap: Capturing::Child(cap),
            })
        }
        #[cfg(target_os = "macos")]
        {
            let _ = ffmpeg;
            if kind == "mic" {
                let dev = native::find_mic(mic_device).ok_or("no microphone found")?;
                let cap = native::start(dev, false, &path)?;
                let (rate, channels) = (cap.rate, cap.channels);
                return Ok(Source {
                    track: track(unix_now(), rate, channels),
                    cap: Capturing::Cpal(cap),
                });
            }
            let helper_result = audiotap_helper().map(|h| {
                let mut c = Command::new(h);
                c.arg(&path);
                ChildCapture::start(c, &path, Some("started"), Duration::from_secs(15))
            });
            match helper_result {
                Some(Ok((cap, started))) => Ok(Source {
                    track: track(started, RATE, 2),
                    cap: Capturing::Child(cap),
                }),
                other => {
                    let reason = match other {
                        Some(Err(e)) => e,
                        _ => "the system sound helper is missing".to_string(),
                    };
                    let dev = native::find_virtual_input().ok_or(reason)?;
                    let cap = native::start(dev, false, &path)?;
                    let (rate, channels) = (cap.rate, cap.channels);
                    Ok(Source {
                        track: track(unix_now(), rate, channels),
                        cap: Capturing::Cpal(cap),
                    })
                }
            }
        }
        #[cfg(windows)]
        {
            let _ = ffmpeg;
            let (dev, loopback) = if kind == "system" {
                (native::default_output().ok_or("no output device")?, true)
            } else {
                (
                    native::find_mic(mic_device).ok_or("no microphone found")?,
                    false,
                )
            };
            let cap = native::start(dev, loopback, &path)?;
            let (rate, channels) = (cap.rate, cap.channels);
            Ok(Source {
                track: track(unix_now(), rate, channels),
                cap: Capturing::Cpal(cap),
            })
        }
    }

    /// Ask the captures to stop (used right when the video stops, so both end at the same moment).
    pub fn request_stop(&mut self) {
        for s in &mut self.sources {
            #[cfg_attr(
                not(any(windows, target_os = "macos")),
                expect(
                    irrefutable_let_patterns,
                    reason = "Cpal exists on Windows and macOS only"
                )
            )]
            if let Capturing::Child(c) = &mut s.cap {
                c.request_stop();
            }
        }
    }

    /// Stop everything and return the tracks that produced a file.
    pub fn finish(mut self) -> Vec<Track> {
        let mut out = Vec::new();
        for s in &mut self.sources {
            match &mut s.cap {
                Capturing::Child(c) => c.stop(),
                #[cfg(any(windows, target_os = "macos"))]
                Capturing::Cpal(c) => c.stop(),
            }
            out.push(s.track.clone());
        }
        out
    }

    pub fn discard(tracks: Vec<Track>) {
        for t in tracks {
            let _ = std::fs::remove_file(&t.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pactl_source_list() {
        let text = "Source #1\n\tState: RUNNING\n\tName: alsa_output.pci.analog-stereo.monitor\n\tDescription: Monitor of Built-in Audio\n\nSource #2\n\tName: alsa_input.pci.analog-stereo\n\tDescription: Built-in Audio Analog Stereo\n";
        let v = parse_pactl_sources(text);
        assert_eq!(v.len(), 2);
        assert_eq!(
            v[1],
            (
                "alsa_input.pci.analog-stereo".to_string(),
                "Built-in Audio Analog Stereo".to_string()
            )
        );
        let mics: Vec<_> = v.iter().filter(|(n, _)| !n.ends_with(".monitor")).collect();
        assert_eq!(mics.len(), 1);
    }

    #[test]
    fn virtual_audio_devices_are_recognised() {
        assert!(is_virtual("BlackHole 2ch") && !is_virtual("MacBook Pro Microphone"));
    }

    #[test]
    fn peak_detects_silence_and_sound() {
        let dir = std::env::temp_dir().join(format!("pinshot_audio_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let silent = dir.join("s.pcm");
        std::fs::write(&silent, vec![0u8; 4000]).unwrap();
        let loud = dir.join("l.pcm");
        let mut data = vec![0u8; 400];
        data[100..102].copy_from_slice(&(-1234i16).to_le_bytes());
        std::fs::write(&loud, data).unwrap();
        assert_eq!(file_peak(&silent), 0);
        assert_eq!(file_peak(&loud), 1234);
        let t = |p: &Path| Track {
            label: "Mic".into(),
            path: p.to_path_buf(),
            channels: 2,
            rate: 48000,
            started_unix: 0.0,
            error: String::new(),
        };
        assert_eq!(
            silence_warnings(&[t(&silent), t(&loud), t(&dir.join("missing.pcm"))]).len(),
            2
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
