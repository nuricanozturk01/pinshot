//! Short-lived notification shown in a corner of the screen. Same look on every OS and nothing for the user to
//! click away. (On macOS a system banner at the top right would end up in screen recordings; this stays at the
//! bottom.)

use crate::gfx::{Context, SoftWindow};
use crate::ui::{icons, theme, Canvas};
use pinshot_core::{Rect, Rgba};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

const MAX_TEXT_W: f32 = 460.0;

pub struct Toast {
    pub sw: SoftWindow,
    /// The painted notification: shown again whenever the compositor asks for the window's content.
    pm: Pixmap,
    text: String,
    area: Rect,
    /// Scale factor `pm` was painted for.
    sf: f32,
    pub deadline: Instant,
    /// Command to run when the toast is clicked.
    pub click: Option<&'static str>,
}

/// Greedy word wrap using real glyph widths.
pub fn wrap(text: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let cand = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if measure(&cand) <= max_w || cur.is_empty() {
            cur = cand;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

pub fn paint(text: &str, sf: f32) -> Pixmap {
    let probe_w = |s: &str| {
        let mut pm = Pixmap::new(1, 1).expect("1px");
        Canvas::new(&mut pm, sf).text_width(s, 13.0, false)
    };
    let lines = wrap(text, MAX_TEXT_W, probe_w);
    let w = lines.iter().map(|l| probe_w(l)).fold(0.0_f32, f32::max) + 24.0 + 26.0;
    let h = lines.len() as f32 * 18.0 + 22.0;
    let mut pm = Pixmap::new((w * sf).ceil() as u32, (h * sf).ceil() as u32).expect("toast size");
    pm.fill(tiny_skia::Color::from_rgba8(0x1e, 0x1e, 0x21, 255));
    let mut cv = Canvas::new(&mut pm, sf);
    cv.stroke_rect(Rect::new(0.0, 0.0, w, h), 1.0, theme::BAR_BORDER, false);
    cv.fill_rect(Rect::new(0.0, 0.0, 4.0, h), theme::ACCENT);
    let _ = icons::app_icon;
    for (i, l) in lines.iter().enumerate() {
        cv.text(18.0, 11.0 + i as f32 * 18.0, l, 13.0, Rgba::WHITE, false);
    }
    pm
}

/// Bottom-right corner of `area` for a toast of `w`×`h` pixels (desktop units: pixels, points on macOS).
fn place(area: Rect, w: u32, h: u32, sf: f32) -> winit::dpi::Position {
    let u = crate::units::per_px(sf); // desktop units per pixel
    crate::units::win_pos(
        area.right() - (w as f32 + 24.0 * sf) * u,
        area.bottom() - (h as f32 + 64.0 * sf) * u,
    )
}

impl Toast {
    /// Paint the notification again (expose events, the first frame after the window appears).
    pub fn redraw(&mut self) {
        // the monitor lookup can be wrong (mixed scale factors): trust the window's own scale
        let real = self.sw.window.scale_factor() as f32;
        if (real - self.sf).abs() > 0.01 {
            self.sf = real;
            self.pm = paint(&self.text, real);
            let (w, h) = (self.pm.width(), self.pm.height());
            let _ = self
                .sw
                .window
                .request_inner_size(crate::units::size_px(w as f32, h as f32, real));
            self.sw
                .window
                .set_outer_position(place(self.area, w, h, real));
        }
        self.sw.sync_size();
        let (ww, wh) = self.sw.size();
        let result = if (ww, wh) == (self.pm.width(), self.pm.height()) {
            self.sw.present(&self.pm, (0, 0)).map(|_| ())
        } else {
            // the window is not the size of the picture (yet): never show unpainted pixels
            match Pixmap::new(ww, wh) {
                Some(mut full) => {
                    full.fill(tiny_skia::Color::from_rgba8(0x1e, 0x1e, 0x21, 255));
                    full.draw_pixmap(
                        0,
                        0,
                        self.pm.as_ref(),
                        &tiny_skia::PixmapPaint::default(),
                        tiny_skia::Transform::identity(),
                        None,
                    );
                    self.sw.present(&full, (0, 0)).map(|_| ())
                }
                None => Ok(()),
            }
        };
        if let Err(e) = result {
            log::warn!("toast redraw: {e}");
        }
    }

    /// Show `text` near the bottom-right of the area `(x, y, w, h)` (physical pixels of one monitor).
    pub fn show(
        el: &ActiveEventLoop,
        ctx: &Context,
        text: &str,
        area: Rect,
        sf: f32,
        secs: u64,
        click: Option<&'static str>,
    ) -> Result<Toast, String> {
        let pm = paint(text, sf);
        let (w, h) = (pm.width(), pm.height());
        let pos = place(area, w, h, sf);
        let attrs = Window::default_attributes()
            .with_title("PinShot")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(crate::units::size_px(w as f32, h as f32, sf))
            .with_position(pos)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false);
        let attrs = crate::gfx::tool_window(attrs);
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attrs.with_x11_window_type(vec![WindowType::Notification])
        };
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let _ = window.set_cursor_hittest(click.is_some());
        let mut sw = SoftWindow::new(ctx, window)?;
        sw.sync_size();
        sw.present(&pm, (0, 0))?;
        sw.window.request_redraw(); // content drawn before the window is mapped can be lost
        Ok(Toast {
            sw,
            pm,
            text: text.to_string(),
            area,
            sf,
            deadline: Instant::now() + Duration::from_secs(secs),
            click,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_words_and_keeps_long_words_whole() {
        let m = |s: &str| s.chars().count() as f32 * 10.0;
        assert_eq!(wrap("aa bb cc dd", 50.0, m), vec!["aa bb", "cc dd"]);
        assert_eq!(
            wrap("supercalifragilistic x", 50.0, m),
            vec!["supercalifragilistic", "x"]
        );
        assert!(wrap("", 50.0, m).is_empty());
    }

    #[test]
    fn toast_pixmap_grows_with_text() {
        let short = paint("Kopyalandı", 1.0);
        let long = paint(
            &"Panoya kopyalandı (1920×1080) · kaydedildi: PinShot_20260101_120000.png ".repeat(3),
            1.0,
        );
        assert!(long.height() > short.height());
        assert!(long.width() > short.width());
        assert!(short.width() < 300);
    }
}
