//! Single-instance guard and command channel (`pinshot --capture` talks to the running tray app).

use interprocess::local_socket::{prelude::*, GenericNamespaced, ListenerOptions, Stream};
use std::io::{BufRead, BufReader, Write};

fn name() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into());
    let safe: String = user
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("pinshot-{safe}")
}

/// Deliver `cmd` to a running instance. False when none is running.
pub fn send_command(cmd: &str) -> bool {
    let Ok(n) = name().to_ns_name::<GenericNamespaced>() else {
        return false;
    };
    let Ok(mut s) = Stream::connect(n) else {
        return false;
    };
    s.write_all(cmd.as_bytes())
        .and_then(|_| s.write_all(b"\n"))
        .and_then(|_| s.flush())
        .is_ok()
}

/// Listen for commands in a background thread; `on_command` is called for every line received.
pub fn listen(on_command: impl Fn(String) + Send + 'static) -> Result<(), String> {
    let n = name()
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| e.to_string())?;
    let listener = match ListenerOptions::new().name(n.clone()).create_sync() {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // a crashed instance can leave the name behind; if nobody answers, try again after a short pause
            return Err(format!("another instance owns the socket: {e}"));
        }
        Err(e) => return Err(e.to_string()),
    };
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                for line in BufReader::new(conn).lines().map_while(Result::ok) {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        on_command(line);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_second_process_can_talk_to_the_first() {
        // unique name per test run so parallel test processes do not collide with a real instance
        std::env::set_var("USER", format!("t{}", std::process::id()));
        assert!(!send_command("ping"), "nobody listens yet");
        let (tx, rx) = mpsc::channel();
        listen(move |c| {
            let _ = tx.send(c);
        })
        .unwrap();
        assert!(send_command("--capture"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            "--capture"
        );
        assert!(listen(|_| {}).is_err(), "the name is taken");
    }
}
