//! `pinshot --selftest`: checks the parts that fail on a fresh machine (screen access, OCR models,
//! ffmpeg, config) and prints one line each. Exit code 0 when everything needed works.

use crate::screens;
use pinshot_core::config::Config;

fn report(name: &str, result: Result<String, String>, failed: &mut bool) {
    match result {
        Ok(d) => println!("ok    {name}: {d}"),
        Err(e) => {
            *failed = true;
            println!("FAIL  {name}: {e}");
        }
    }
}

/// Run all checks; returns the process exit code.
pub fn run() -> i32 {
    let mut failed = false;
    println!("PinShot {}", env!("CARGO_PKG_VERSION"));
    let path = Config::default_path();
    report(
        "config",
        Ok(format!(
            "{} (loads with defaults when missing)",
            path.display()
        )),
        &mut failed,
    );
    report(
        "screen capture",
        screens::grab_all().and_then(|caps| {
            if caps.is_empty() {
                Err("no screens found".into())
            } else if caps.iter().all(|c| screens::looks_blank(&c.pixmap)) {
                Err("captured image is blank (missing screen permission?)".into())
            } else {
                Ok(format!("{} screen(s)", caps.len()))
            }
        }),
        &mut failed,
    );
    report(
        "ocr",
        pinshot_ocr::Engine::new().map(|_| "models loaded".to_string()),
        &mut failed,
    );
    report(
        "ffmpeg",
        pinshot_record::ffmpeg::find_ffmpeg()
            .map(|p| p.display().to_string())
            .ok_or_else(|| "not found (video recording unavailable)".into()),
        &mut failed,
    );
    i32::from(failed)
}
