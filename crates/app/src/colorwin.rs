//! Custom colour picker: saturation/value square, hue strip, preview and hex value.

use crate::gfx::{Context, SoftWindow};
use crate::ui::{theme, Canvas};
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::dpi::LogicalPosition;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

const W: f32 = 260.0;
const H: f32 = 300.0;
const SV: Rect = Rect::new(16.0, 16.0, 228.0, 160.0);
const HUE: Rect = Rect::new(16.0, 186.0, 228.0, 18.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Ok,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Sv,
    Hue,
}

pub struct ColorWin {
    pub sw: SoftWindow,
    pub hover: Option<Btn>,
    h: f32,
    s: f32,
    v: f32,
    drag: Option<Drag>,
    last: (f64, f64),
    sf: f32,
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgba {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgba::rgb(q(r), q(g), q(b))
}

pub fn rgb_to_hsv(c: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(c.r) / 255.0,
        f32::from(c.g) / 255.0,
        f32::from(c.b) / 255.0,
    );
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

impl ColorWin {
    pub fn create(
        el: &ActiveEventLoop,
        ctx: &Context,
        start: Rgba,
        at: Option<Point>,
    ) -> Result<ColorWin, String> {
        let mut attrs = Window::default_attributes()
            .with_title(format!("PinShot – {}", tr("Renk")))
            .with_resizable(crate::gfx::DIALOG_RESIZABLE)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_inner_size(winit::dpi::LogicalSize::new(f64::from(W), f64::from(H)));
        if let Some(p) = at {
            attrs = attrs.with_position(LogicalPosition::new(f64::from(p.x), f64::from(p.y)));
        }
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sf = window.scale_factor() as f32;
        let (h, s, v) = rgb_to_hsv(start);
        let mut w = ColorWin {
            sw: SoftWindow::new(ctx, window)?,
            hover: None,
            h,
            s,
            v,
            drag: None,
            last: (0.0, 0.0),
            sf,
        };
        w.draw();
        Ok(w)
    }

    pub fn color(&self) -> Rgba {
        hsv_to_rgb(self.h, self.s, self.v)
    }

    fn buttons() -> [(Btn, Rect); 2] {
        [
            (
                Btn::Ok,
                Rect::new(W - 16.0 - 90.0 - 8.0 - 90.0, H - 48.0, 90.0, 32.0),
            ),
            (
                Btn::Cancel,
                Rect::new(W - 16.0 - 90.0, H - 48.0, 90.0, 32.0),
            ),
        ]
    }

    fn ui(&self, x: f64, y: f64) -> Point {
        Point::new(x as f32 / self.sf, y as f32 / self.sf)
    }

    fn set_from(&mut self, d: Drag, p: Point) {
        match d {
            Drag::Sv => {
                self.s = ((p.x - SV.x) / SV.w).clamp(0.0, 1.0);
                self.v = 1.0 - ((p.y - SV.y) / SV.h).clamp(0.0, 1.0);
            }
            Drag::Hue => self.h = ((p.x - HUE.x) / HUE.w).clamp(0.0, 1.0) * 359.9,
        }
    }

    /// Returns true when a redraw is needed.
    pub fn moved(&mut self, x: f64, y: f64) -> bool {
        self.last = (x, y);
        let p = self.ui(x, y);
        if let Some(d) = self.drag {
            self.set_from(d, p);
            return true;
        }
        let b = Self::buttons()
            .into_iter()
            .find(|(_, r)| r.contains(p))
            .map(|(b, _)| b);
        std::mem::replace(&mut self.hover, b) != b
    }

    /// Mouse press at the last cursor position: returns the button hit, if any.
    pub fn press(&mut self) -> Option<Btn> {
        let p = self.ui(self.last.0, self.last.1);
        if let Some((b, _)) = Self::buttons().into_iter().find(|(_, r)| r.contains(p)) {
            return Some(b);
        }
        let d = if SV.contains(p) {
            Some(Drag::Sv)
        } else if HUE.inflated(4.0).contains(p) {
            Some(Drag::Hue)
        } else {
            None
        };
        if let Some(d) = d {
            self.set_from(d, p);
        }
        self.drag = d;
        None
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    pub fn draw(&mut self) {
        self.sw.ensure_ui_size(W, H, self.sf);
        self.sw.sync_size();
        let (pw, ph) = self.sw.size();
        let Some(mut pm) = Pixmap::new(pw, ph) else {
            return;
        };
        pm.fill(tiny_skia::Color::from_rgba8(0x2b, 0x2b, 0x2f, 255));
        // gradients are painted per pixel, in window pixels
        let sf = self.sf;
        let px = |r: Rect| {
            (
                (r.x * sf) as u32,
                (r.y * sf) as u32,
                (r.w * sf) as u32,
                (r.h * sf) as u32,
            )
        };
        let stride = pw;
        let data = pm.pixels_mut();
        let (sx, sy, sw_, sh) = px(SV);
        for y in 0..sh.min(ph.saturating_sub(sy)) {
            for x in 0..sw_.min(pw.saturating_sub(sx)) {
                let c = hsv_to_rgb(self.h, x as f32 / sw_ as f32, 1.0 - y as f32 / sh as f32);
                data[((sy + y) * stride + sx + x) as usize] =
                    tiny_skia::PremultipliedColorU8::from_rgba(c.r, c.g, c.b, 255)
                        .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
            }
        }
        let (hx, hy, hw, hh) = px(HUE);
        for y in 0..hh.min(ph.saturating_sub(hy)) {
            for x in 0..hw.min(pw.saturating_sub(hx)) {
                let c = hsv_to_rgb(x as f32 / hw as f32 * 359.9, 1.0, 1.0);
                data[((hy + y) * stride + hx + x) as usize] =
                    tiny_skia::PremultipliedColorU8::from_rgba(c.r, c.g, c.b, 255)
                        .unwrap_or(tiny_skia::PremultipliedColorU8::TRANSPARENT);
            }
        }
        {
            let mut cv = Canvas::new(&mut pm, sf);
            let cur = self.color();
            cv.stroke_circle(
                SV.x + self.s * SV.w,
                SV.y + (1.0 - self.v) * SV.h,
                6.0,
                2.0,
                Rgba::WHITE,
            );
            let hx = HUE.x + self.h / 359.9 * HUE.w;
            cv.fill_rect(
                Rect::new(hx - 2.0, HUE.y - 3.0, 4.0, HUE.h + 6.0),
                Rgba::WHITE,
            );
            let prev = Rect::new(16.0, 220.0, 40.0, 30.0);
            cv.fill_rrect(prev, 6.0, cur);
            cv.text(68.0, 227.0, &cur.to_hex(), 14.0, Rgba::WHITE, false);
            for (b, r) in Self::buttons() {
                let (label, accent) = match b {
                    Btn::Ok => (tr("Tamam"), true),
                    Btn::Cancel => (tr("İptal"), false),
                };
                let bg = if self.hover == Some(b) {
                    theme::BTN_HOVER
                } else if accent {
                    theme::ACCENT
                } else {
                    Rgba::rgb(0x3a, 0x3a, 0x40)
                };
                cv.fill_rrect(r, 6.0, bg);
                cv.text_centered(r, label, 13.0, Rgba::WHITE, false);
            }
        }
        if let Err(e) = self.sw.present(&pm, (0, 0)) {
            log::warn!("colour window: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trip() {
        for c in [
            Rgba::rgb(255, 59, 48),
            Rgba::rgb(10, 132, 255),
            Rgba::WHITE,
            Rgba::BLACK,
        ] {
            let (h, s, v) = rgb_to_hsv(c);
            let b = hsv_to_rgb(h, s, v);
            assert!(
                (i32::from(b.r) - i32::from(c.r)).abs() <= 1
                    && (i32::from(b.g) - i32::from(c.g)).abs() <= 1
                    && (i32::from(b.b) - i32::from(c.b)).abs() <= 1,
                "{c:?} -> {b:?}"
            );
        }
    }
}
