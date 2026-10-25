//! Windows shown while recording: a frame around the area and the island with the timer and stop button.

use crate::gfx::{Context, SoftWindow};
use crate::ui::{theme, Canvas};
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

fn plain_window(
    el: &ActiveEventLoop,
    pos: Point,
    size: winit::dpi::Size,
    notification: bool,
) -> Result<Arc<Window>, String> {
    let attrs = Window::default_attributes()
        .with_title("PinShot")
        .with_decorations(false)
        .with_resizable(false)
        .with_inner_size(size)
        .with_position(crate::gfx::win_pos(pos.x, pos.y))
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_active(false);
    #[cfg(target_os = "linux")]
    let attrs = {
        use winit::platform::x11::{WindowAttributesExtX11, WindowType};
        attrs.with_x11_window_type(vec![if notification {
            WindowType::Notification
        } else {
            WindowType::Utility
        }])
    };
    #[cfg(not(target_os = "linux"))]
    let _ = notification;
    el.create_window(attrs)
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

/// Four thin strips around the recorded area (outside it, so they never appear in the video).
pub struct RecFrame {
    #[expect(dead_code, reason = "owns the strip windows; dropping it closes them")]
    pub strips: Vec<SoftWindow>,
}

pub fn strip_rects(region: Rect, bounds: Rect, margin: f32) -> Vec<Rect> {
    let m = margin;
    [
        Rect::new(region.x - m, region.y - m, region.w + 2.0 * m, m),
        Rect::new(region.x - m, region.bottom(), region.w + 2.0 * m, m),
        Rect::new(region.x - m, region.y, m, region.h),
        Rect::new(region.right(), region.y, m, region.h),
    ]
    .into_iter()
    .map(|r| r.intersection(&bounds))
    .filter(|r| r.w >= 1.0 && r.h >= 1.0)
    .collect()
}

impl RecFrame {
    pub fn show(
        el: &ActiveEventLoop,
        ctx: &Context,
        region: Rect,
        bounds: Rect,
        margin: f32,
    ) -> RecFrame {
        let mut strips = Vec::new();
        for r in strip_rects(region, bounds, margin) {
            let Ok(win) = plain_window(
                el,
                Point::new(r.x, r.y),
                crate::gfx::size_units(r.w, r.h),
                true,
            ) else {
                continue;
            };
            let _ = win.set_cursor_hittest(false);
            let Ok(mut sw) = SoftWindow::new(ctx, win) else {
                continue;
            };
            sw.sync_size();
            let (pw, ph) = sw.size();
            let mut pm = Pixmap::new(pw.max(1), ph.max(1)).expect("strip");
            pm.fill(tiny_skia::Color::from_rgba8(0xff, 0x3b, 0x30, 255));
            let _ = sw.present(&pm, (0, 0));
            strips.push(sw);
        }
        RecFrame { strips }
    }
}

pub const ISLAND_W: f32 = 236.0;
pub const ISLAND_H: f32 = 44.0;

pub struct Island {
    pub sw: SoftWindow,
    pub secs: u32,
    pub label: String,
    pub sf: f32,
    pub drag: Option<Point>,
}

pub fn format_time(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Default spot: below the area if there is room, else above, preferring a monitor that is not being recorded.
pub fn default_position(region: Rect, bounds: Rect, others: &[Rect], size: (f32, f32)) -> Point {
    if let Some(o) = others.first() {
        return Point::new(o.x + (o.w - size.0) / 2.0, o.bottom() - size.1 - 64.0);
    }
    let x = (region.x + (region.w - size.0) / 2.0).clamp(
        bounds.x + 8.0,
        (bounds.right() - size.0 - 8.0).max(bounds.x),
    );
    let below = region.bottom() + 12.0;
    let y = if below + size.1 + 8.0 <= bounds.bottom() {
        below
    } else {
        (region.y - size.1 - 12.0).max(bounds.y + 8.0)
    };
    Point::new(x, y)
}

pub fn is_on_screen(pos: Point, size: (f32, f32), screens: &[Rect]) -> bool {
    let r = Rect::new(pos.x, pos.y, size.0, size.1);
    screens
        .iter()
        .any(|s| s.intersection(&r).w >= size.0 * 0.5 && s.intersection(&r).h >= size.1 * 0.5)
}

impl Island {
    pub fn show(
        el: &ActiveEventLoop,
        ctx: &Context,
        at: Point,
        sf: f32,
        label: String,
    ) -> Result<Island, String> {
        let win = plain_window(
            el,
            at,
            crate::gfx::size_px(ISLAND_W * sf, ISLAND_H * sf, sf),
            false,
        )?;
        let sf = win.scale_factor() as f32;
        let mut sw = SoftWindow::new(ctx, win)?;
        sw.sync_size();
        let mut i = Island {
            sw,
            secs: 0,
            label,
            sf,
            drag: None,
        };
        i.draw();
        Ok(i)
    }

    pub fn stop_rect() -> Rect {
        Rect::new(ISLAND_W - 40.0, 8.0, 28.0, 28.0)
    }

    pub fn hits_stop(&self, p: Point) -> bool {
        Self::stop_rect().contains(Point::new(p.x / self.sf, p.y / self.sf))
    }

    pub fn rect(&self) -> Rect {
        crate::gfx::outer_rect(&self.sw.window)
    }

    pub fn draw(&mut self) {
        self.sw.sync_size();
        let (w, h) = self.sw.size();
        let mut pm = Pixmap::new(w, h).expect("island size");
        pm.fill(tiny_skia::Color::from_rgba8(0x1e, 0x1e, 0x21, 255));
        {
            let mut cv = Canvas::new(&mut pm, self.sf);
            cv.stroke_rect(
                Rect::new(0.0, 0.0, ISLAND_W, ISLAND_H),
                1.0,
                theme::BAR_BORDER,
                false,
            );
            let blink = self.secs.is_multiple_of(2);
            cv.fill_circle(
                20.0,
                ISLAND_H / 2.0,
                6.0,
                if blink {
                    theme::DANGER
                } else {
                    Rgba::rgb(0x8a, 0x2a, 0x26)
                },
            );
            cv.text(34.0, 6.0, &format_time(self.secs), 15.0, Rgba::WHITE, true);
            cv.text(34.0, 25.0, &self.label, 10.0, theme::TEXT_DIM, false);
            let r = Self::stop_rect();
            cv.fill_rrect(r, 6.0, theme::DANGER);
            cv.fill_rrect(
                Rect::new(r.x + 9.0, r.y + 9.0, 10.0, 10.0),
                2.0,
                Rgba::WHITE,
            );
            let hint = tr("Durdur");
            let hw = cv.text_width(hint, 11.0, false);
            cv.text(r.x - hw - 10.0, 15.0, hint, 11.0, theme::TEXT, false);
        }
        let _ = self.sw.present(&pm, (0, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_surround_the_region_without_touching_it() {
        let region = Rect::new(100.0, 100.0, 400.0, 300.0);
        let strips = strip_rects(region, Rect::new(0.0, 0.0, 1920.0, 1080.0), 3.0);
        assert_eq!(strips.len(), 4);
        for s in &strips {
            assert!(!s.intersects(&region), "{s:?} overlaps the recorded area");
        }
        let at_edge = strip_rects(
            Rect::new(0.0, 0.0, 400.0, 300.0),
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            3.0,
        );
        assert_eq!(at_edge.len(), 2, "strips outside the screen are dropped");
    }

    #[test]
    fn time_format_and_placement() {
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(75), "01:15");
        let bounds = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let below = default_position(
            Rect::new(500.0, 200.0, 400.0, 300.0),
            bounds,
            &[],
            (236.0, 44.0),
        );
        assert_eq!(below.y, 512.0);
        let above = default_position(
            Rect::new(500.0, 700.0, 400.0, 370.0),
            bounds,
            &[],
            (236.0, 44.0),
        );
        assert!(above.y + 44.0 <= 700.0);
        let other = default_position(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            bounds,
            &[Rect::new(1920.0, 0.0, 1000.0, 800.0)],
            (236.0, 44.0),
        );
        assert!(other.x >= 1920.0);
        assert!(is_on_screen(
            Point::new(100.0, 100.0),
            (236.0, 44.0),
            &[bounds]
        ));
        assert!(!is_on_screen(
            Point::new(5000.0, 100.0),
            (236.0, 44.0),
            &[bounds]
        ));
    }
}
