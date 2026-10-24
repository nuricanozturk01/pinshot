mod app;
mod clip;
mod gfx;
mod hotkeys;
mod ipc;
mod logging;
mod ocrwin;
mod pins;
mod popup;
mod screens;
mod session;
mod toast;
mod toolbar;
mod tray;
mod ui;

const COMMANDS: [(&str, &str); 8] = [
    ("--capture", "capture"),
    ("--fullscreen", "fullscreen"),
    ("--repeat", "repeat"),
    ("--pin-clipboard", "pin_clipboard"),
    ("--record", "record"),
    ("--stop-recording", "stop_record"),
    ("--settings", "settings"),
    ("--quit", "quit"),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("PinShot {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    logging::init(std::env::var_os("PINSHOT_LOG_STDERR").is_some() || cfg!(debug_assertions));
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
