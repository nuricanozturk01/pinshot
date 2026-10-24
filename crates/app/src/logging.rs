//! Minimal file + stderr logger with a size-capped rotating log (no extra dependency).

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

struct Logger {
    file: Mutex<Option<File>>,
    stderr: bool,
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Info
            || (m.target().starts_with("pinshot") && std::env::var_os("PINSHOT_DEBUG").is_some())
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format!(
            "{} {} {}: {}\n",
            chrono_stamp(),
            r.level(),
            r.target(),
            r.args()
        );
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = f.write_all(line.as_bytes());
            }
        }
        if self.stderr {
            eprint!("{line}");
        }
    }

    fn flush(&self) {}
}

fn chrono_stamp() -> String {
    // seconds since the epoch is enough to order lines; avoids a time-zone dependency in the app crate
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

/// Keep up to three old logs of at most 1 MB each.
fn rotate(path: &PathBuf) {
    if fs::metadata(path)
        .map(|m| m.len() > 1_000_000)
        .unwrap_or(false)
    {
        for i in (1..3).rev() {
            let _ = fs::rename(
                path.with_extension(format!("log.{i}")),
                path.with_extension(format!("log.{}", i + 1)),
            );
        }
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
}

pub fn init(stderr: bool) {
    let dir = pinshot_core::config::log_dir();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("pinshot.log");
    rotate(&path);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();
    let logger = Box::leak(Box::new(Logger {
        file: Mutex::new(file),
        stderr,
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(if std::env::var_os("PINSHOT_DEBUG").is_some() {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        });
    }
}
