// release builds on Windows are GUI apps: no console window next to the tray icon
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// mimalloc hands freed memory back to the system; the glibc heap kept ~100 MB after a capture
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod aboutwin;
mod app;
mod clip;
mod colorwin;
mod form;
mod gfx;
mod hotkeys;
mod ipc;
mod logging;
mod mem;
mod ocrwin;
mod pins;
mod popup;
mod recording;
mod recpanel;
mod recui;
mod screens;
mod selftest;
mod session;
mod settings;
mod settingswin;
mod toast;
mod toolbar;
mod tray;
mod ui;
mod units;

const COMMANDS: [(&str, &str); 10] = [
    ("--capture", "capture"),
    ("--fullscreen", "fullscreen"),
    ("--repeat", "repeat"),
    ("--window", "window"),
    ("--pin-clipboard", "pin_clipboard"),
    ("--record", "record"),
    ("--stop-recording", "stop_record"),
    ("--settings", "settings"),
    ("--about", "about"),
    ("--quit", "quit"),
];

/// Windows: opt in to per-monitor DPI before any window or screen query, otherwise the system scales
/// our windows (blurry) and reports virtualised monitor sizes (wrong capture regions).
#[cfg(windows)]
fn init_windows() {
    use std::os::windows::io::IntoRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    // SAFETY: plain Win32 calls; failure (already set, no parent console) is harmless.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // a GUI-subsystem exe has no stdout; `--version` / `--selftest` print into the launching terminal
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            for (std_handle, name) in [
                (STD_OUTPUT_HANDLE, "CONOUT$"),
                (STD_ERROR_HANDLE, "CONOUT$"),
            ] {
                let unset = GetStdHandle(std_handle).map_or(true, |h| h.is_invalid());
                // redirected output (a pipe or file) is already inherited; only a bare console needs reopening
                if unset {
                    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(name) {
                        let _ = SetStdHandle(std_handle, HANDLE(f.into_raw_handle()));
                    }
                }
            }
        }
    }
}

fn main() {
    #[cfg(windows)]
    init_windows();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("PinShot {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--selftest-window") {
        std::process::exit(selftest::run_window());
    }
    if args.iter().any(|a| a == "--selftest") {
        std::process::exit(selftest::run());
    }
    logging::init(std::env::var_os("PINSHOT_LOG_STDERR").is_some() || cfg!(debug_assertions));
    // release builds abort on panic; leave the reason in the log first
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    let cmd = COMMANDS
        .iter()
        .find(|(flag, _)| args.iter().any(|a| a == flag))
        .map(|(_, c)| c.to_string());
    let cmd = cmd.or_else(|| {
        args.iter()
            .any(|a| a == "--now")
            .then(|| "capture".to_string())
    });
    // another instance is running: hand the command over and leave
    if ipc::send_command(cmd.as_deref().unwrap_or("ping")) {
        return;
    }
    if cmd.as_deref() == Some("quit") {
        return;
    }
    if let Err(e) = app::run(cmd) {
        log::error!("fatal: {e}");
        eprintln!("PinShot: {e}");
        std::process::exit(1);
    }
}
