//! Pinned image windows.

use crate::gfx::{Context, SoftWindow};
use pinshot_core::pin::{self, Transform};
use pinshot_core::{Point, Rect};
use std::sync::Arc;
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

pub struct PinWin {
    pub id: u64,
    pub sw: SoftWindow,
    pub image: Pixmap,
    /// Pixels per desktop unit of the image's source (to show it at its natural size).
    pub dpr: f32,
    pub geom: Rect,
    pub opacity: f32,
    pub locked: bool,
    pub border: bool,
    pub hover: bool,
    pub drag: Option<Point>,
    pub resize: Option<(String, Rect, Point)>,
    /// Last pointer position inside the window (physical pixels).
    pub pos: Point,
    pub last_click: Option<std::time::Instant>,
    cache: Option<Pixmap>,
}

impl PinWin {
    pub fn create(
        el: &ActiveEventLoop,
        ctx: &Context,
        id: u64,
        image: Pixmap,
        at: Point,
        dpr: f32,
        border: bool,
    ) -> Result<PinWin, String> {
        let dpr = dpr.max(1.0);
        let natural = (image.width() as f32 / dpr, image.height() as f32 / dpr);
        let (w, h) = (
            natural.0.round().max(pin::MIN_SIDE),
            natural.1.round().max(pin::MIN_SIDE),
        );
        let attrs = Window::default_attributes()
            .with_title("PinShot")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_inner_size(crate::gfx::size_units(w, h))
            .with_position(crate::gfx::win_pos(at.x, at.y))
            .with_window_level(WindowLevel::AlwaysOnTop);
        let attrs = crate::gfx::tool_window(attrs);
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attrs.with_x11_window_type(vec![WindowType::Utility])
        };
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sw = SoftWindow::new(ctx, window)?;
        let mut p = PinWin {
            id,
            sw,
            image,
            dpr,
            geom: Rect::new(at.x, at.y, w, h),
            opacity: 1.0,
            locked: false,
            border,
            hover: false,
            drag: None,
            resize: None,
            pos: Point::default(),
            last_click: None,
            cache: None,
        };
        p.draw();
        Ok(p)
    }

    pub fn natural(&self) -> (f32, f32) {
        (
            self.image.width() as f32 / self.dpr,
            self.image.height() as f32 / self.dpr,
        )
    }

    pub fn set_geom(&mut self, g: Rect) {
        self.geom = g;
        let w = &self.sw.window;
        let _ = w.request_inner_size(crate::gfx::size_units(g.w, g.h));
        w.set_outer_position(crate::gfx::win_pos(g.x, g.y));
        self.cache = None;
        w.request_redraw();
    }

    pub fn transform(&mut self, t: Transform) {
        self.image = pin::transform(&self.image, t);
        if matches!(t, Transform::RotateCw | Transform::RotateCcw) {
            let c = self.geom.center();
            let g = Rect::new(
                (c.x - self.geom.h / 2.0).round(),
                (c.y - self.geom.w / 2.0).round(),
                self.geom.h,
                self.geom.w,
            );
            self.set_geom(g);
        }
        self.cache = None;
        self.sw.window.request_redraw();
    }

    pub fn close_rect() -> impl Fn(f32) -> Rect {
        |w| Rect::new(w - 24.0, 6.0, 18.0, 18.0)
    }

    pub fn draw(&mut self) {
        self.sw.sync_size();
        let (w, h) = self.sw.size();
        let scaled = match &self.cache {
            Some(c) if c.width() == w && c.height() == h => c,
            _ => {
                self.cache = Some(scale_to(&self.image, w, h));
                self.cache.as_ref().expect("just set")
            }
        };
        let mut out = Pixmap::new(w, h).expect("window size");
        let paint = PixmapPaint {
            opacity: self.opacity,
            blend_mode: tiny_skia::BlendMode::Source,
            quality: FilterQuality::Nearest,
        };
        out.draw_pixmap(
            0,
            0,
            scaled.as_ref(),
            &paint,
            tiny_skia::Transform::identity(),
            None,
        );
        {
            let mut cv = crate::ui::Canvas::new(&mut out, 1.0);
            if self.border {
                cv.stroke_rect(
                    Rect::new(0.0, 0.0, w as f32, h as f32),
                    1.0,
                    pinshot_core::Rgba::new(10, 132, 255, 210),
                    false,
                );
            }
            if self.hover {
                let r = Self::close_rect()(w as f32);
                cv.fill_circle(
                    r.x + 9.0,
                    r.y + 9.0,
                    9.0,
                    pinshot_core::Rgba::new(0, 0, 0, 170),
                );
                cv.line(
                    (r.x + 5.0, r.y + 5.0),
                    (r.x + 13.0, r.y + 13.0),
                    1.6,
                    pinshot_core::Rgba::WHITE,
                );
                cv.line(
                    (r.x + 13.0, r.y + 5.0),
                    (r.x + 5.0, r.y + 13.0),
                    1.6,
                    pinshot_core::Rgba::WHITE,
                );
            }
        }
        // the surface expects premultiplied-over-black opaque or ARGB; softbuffer treats the alpha byte as is
        if let Err(e) = self.sw.present_argb(&out) {
            log::warn!("pin present: {e}");
        }
    }
}

/// Resample to `w`×`h`: repeated halving for strong reductions, then bilinear.
pub fn scale_to(src: &Pixmap, w: u32, h: u32) -> Pixmap {
    let mut cur = src.clone();
    while cur.width() / 2 >= w.max(1) && cur.height() / 2 >= h.max(1) {
        cur = half(&cur);
    }
    if cur.width() == w && cur.height() == h {
        return cur;
    }
    let mut out = Pixmap::new(w.max(1), h.max(1)).expect("size");
    let sx = w as f32 / cur.width() as f32;
    let sy = h as f32 / cur.height() as f32;
    let paint = PixmapPaint {
        quality: FilterQuality::Bilinear,
        blend_mode: tiny_skia::BlendMode::Source,
        ..PixmapPaint::default()
    };
    out.draw_pixmap(
        0,
        0,
        cur.as_ref(),
        &paint,
        tiny_skia::Transform::from_scale(sx, sy),
        None,
    );
    out
}

fn half(src: &Pixmap) -> Pixmap {
    let (w, h) = (src.width() / 2, src.height() / 2);
    let mut out = Pixmap::new(w, h).expect("half");
    let sw = src.width() as usize;
    let s = src.data();
    let d = out.data_mut();
    for y in 0..h as usize {
        for x in 0..w as usize {
            for k in 0..4 {
                let i = |dx: usize, dy: usize| s[((2 * y + dy) * sw + 2 * x + dx) * 4 + k] as u32;
                d[(y * w as usize + x) * 4 + k] =
                    ((i(0, 0) + i(1, 0) + i(0, 1) + i(1, 1) + 2) / 4) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halving_averages_and_scale_hits_the_requested_size() {
        let mut pm = Pixmap::new(4, 4).unwrap();
        for (i, p) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = if (i % 4 + i / 4) % 2 == 0 { 0 } else { 200 };
            p.copy_from_slice(&[v, v, v, 255]);
        }
        let h = half(&pm);
        assert_eq!((h.width(), h.height()), (2, 2));
        assert_eq!(h.pixel(0, 0).unwrap().red(), 100);
        let s = scale_to(&pm, 10, 7);
        assert_eq!((s.width(), s.height()), (10, 7));
        let big = Pixmap::new(1000, 600).unwrap();
        assert_eq!(scale_to(&big, 130, 80).width(), 130);
    }
}
