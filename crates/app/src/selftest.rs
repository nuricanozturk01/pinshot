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
    #[cfg(target_os = "macos")]
    report(
        "screen recording permission",
        if screens::screen_access(false) {
            Ok("granted".to_string())
        } else {
            Err("not granted: System Settings > Privacy & Security > Screen Recording".into())
        },
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

/// `pinshot --selftest-window`: opens a hidden borderless window the way the capture overlay does, paints the first
/// frame, shows it and waits for the system to ask for another paint. Exit code 0 when that happens in time.
/// Catches platforms where a window never reaches the screen (no paint for hidden windows, no display, no surface).
pub fn run_window() -> i32 {
    use crate::gfx::{Context, SoftWindow};
    use std::time::{Duration, Instant};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::window::{Window, WindowId};

    const TIMEOUT: Duration = Duration::from_secs(10);

    struct Smoke {
        win: Option<SoftWindow>,
        started: Instant,
        result: Option<Result<(), String>>,
    }

    impl Smoke {
        fn open(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
            let attrs = crate::gfx::tool_window(
                Window::default_attributes()
                    .with_title("PinShot selftest")
                    .with_decorations(false)
                    .with_visible(false)
                    .with_inner_size(crate::units::size_units(320.0, 200.0)),
            );
            let window = std::sync::Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
            let ctx: Context =
                softbuffer::Context::new(el.owned_display_handle()).map_err(|e| e.to_string())?;
            let mut sw = SoftWindow::new(&ctx, window)?;
            let mut pm = tiny_skia::Pixmap::new(320, 200).ok_or("pixmap")?;
            pm.fill(tiny_skia::Color::from_rgba8(0x20, 0x40, 0x80, 255));
            sw.sync_size();
            sw.present(&pm, (0, 0))?;
            sw.window.set_visible(true);
            sw.window.request_redraw();
            self.win = Some(sw);
            Ok(())
        }
    }

    impl ApplicationHandler for Smoke {
        fn resumed(&mut self, el: &ActiveEventLoop) {
            if self.win.is_none() {
                if let Err(e) = self.open(el) {
                    self.result = Some(Err(e));
                    el.exit();
                }
            }
        }

        fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            if matches!(event, WindowEvent::RedrawRequested) {
                self.result = Some(Ok(()));
                el.exit();
            }
        }

        fn about_to_wait(&mut self, el: &ActiveEventLoop) {
            if self.started.elapsed() > TIMEOUT {
                self.result = Some(Err("no paint request within 10 s".into()));
                el.exit();
            }
        }
    }

    let mut app = Smoke {
        win: None,
        started: Instant::now(),
        result: None,
    };
    let run = EventLoop::new().map_err(|e| e.to_string()).and_then(|el| {
        el.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_secs(1),
        ));
        el.run_app(&mut app).map_err(|e| e.to_string())
    });
    match (run, app.result) {
        (Ok(()), Some(Ok(()))) => {
            println!("ok    window: opened, painted and received a paint request");
            0
        }
        (Ok(()), Some(Err(e))) | (Err(e), _) => {
            println!("FAIL  window: {e}");
            1
        }
        (Ok(()), None) => {
            println!("FAIL  window: event loop ended without a result");
            1
        }
    }
}
