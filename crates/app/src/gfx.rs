//! Presenting software-rendered pixmaps in winit windows (softbuffer), with damage rectangles.

use std::num::NonZeroU32;
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

/// Window placement helpers. Desktop coordinates are physical pixels on Windows and Linux but points on macOS, where
/// winit expects logical values; everything that positions a window goes through these.
pub fn win_pos(x: f32, y: f32) -> winit::dpi::Position {
    if cfg!(target_os = "macos") {
        winit::dpi::LogicalPosition::new(x as f64, y as f64).into()
    } else {
        winit::dpi::PhysicalPosition::new(x as i32, y as i32).into()
    }
}

/// Whether fixed-size dialogs must be left resizable. winit's Wayland backend pins a non-resizable window's min/max
/// to its inner size before the client-side title bar exists, so the compositor then squeezes the content area by the
/// title bar's height and clips the dialog's bottom row. Those dialogs lay out at a fixed size anyway.
pub const DIALOG_RESIZABLE: bool = cfg!(target_os = "linux");

/// A size given in desktop units.
pub fn size_units(w: f32, h: f32) -> winit::dpi::Size {
    if cfg!(target_os = "macos") {
        winit::dpi::LogicalSize::new(w.max(1.0) as f64, h.max(1.0) as f64).into()
    } else {
        winit::dpi::PhysicalSize::new(w.max(1.0) as u32, h.max(1.0) as u32).into()
    }
}

/// A size given in physical pixels of a window with scale factor `sf`.
pub fn size_px(w: f32, h: f32, sf: f32) -> winit::dpi::Size {
    if cfg!(target_os = "macos") {
        winit::dpi::LogicalSize::new((w / sf).max(1.0) as f64, (h / sf).max(1.0) as f64).into()
    } else {
        winit::dpi::PhysicalSize::new(w.max(1.0) as u32, h.max(1.0) as u32).into()
    }
}

/// The window's outer rectangle in desktop units.
pub fn outer_rect(w: &Window) -> pinshot_core::Rect {
    let sf = if cfg!(target_os = "macos") {
        w.scale_factor() as f32
    } else {
        1.0
    };
    let p = w.outer_position().unwrap_or_default();
    let s = w.outer_size();
    pinshot_core::Rect::new(
        p.x as f32 / sf,
        p.y as f32 / sf,
        s.width as f32 / sf,
        s.height as f32 / sf,
    )
}

pub type Context = softbuffer::Context<OwnedDisplayHandle>;

/// Integer rectangle in window pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl IRect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        IRect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn union(&self, o: &IRect) -> IRect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        let (r, b) = (self.right().max(o.right()), self.bottom().max(o.bottom()));
        IRect::new(x, y, (r - x) as u32, (b - y) as u32)
    }

    pub fn intersect(&self, o: &IRect) -> Option<IRect> {
        let (x, y) = (self.x.max(o.x), self.y.max(o.y));
        let (r, b) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        (r > x && b > y).then(|| IRect::new(x, y, (r - x) as u32, (b - y) as u32))
    }

    pub fn inflate(&self, d: i32) -> IRect {
        IRect::new(
            self.x - d,
            self.y - d,
            (self.w as i32 + 2 * d).max(0) as u32,
            (self.h as i32 + 2 * d).max(0) as u32,
        )
    }

    /// Smallest integer rectangle containing the float rectangle.
    pub fn around(x: f32, y: f32, w: f32, h: f32) -> IRect {
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (x1, y1) = ((x + w).ceil() as i32, (y + h).ceil() as i32);
        IRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
    }
}

pub struct SoftWindow {
    pub window: Arc<Window>,
    surface: softbuffer::Surface<OwnedDisplayHandle, Arc<Window>>,
    size: (u32, u32),
}

impl SoftWindow {
    pub fn new(ctx: &Context, window: Arc<Window>) -> Result<Self, String> {
        let surface = softbuffer::Surface::new(ctx, window.clone()).map_err(|e| e.to_string())?;
        let mut w = SoftWindow {
            window,
            surface,
            size: (0, 0),
        };
        w.sync_size();
        Ok(w)
    }

    /// Resize the surface to the window's current pixel size. Returns true if it changed.
    pub fn sync_size(&mut self) -> bool {
        let s = self.window.inner_size();
        let (w, h) = (s.width.max(1), s.height.max(1));
        if (w, h) == self.size {
            return false;
        }
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            if self.surface.resize(nw, nh).is_ok() {
                self.size = (w, h);
                return true;
            }
        }
        false
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Ask for a bigger window when the compositor handed out less than the `w` x `h` UI units a dialog lays
    /// out for (some Wayland compositors subtract the title bar from the requested size). Never shrinks.
    pub fn ensure_ui_size(&self, w: f32, h: f32, sf: f32) {
        let s = self.window.inner_size();
        let (want_w, want_h) = ((w * sf).round() as u32, (h * sf).round() as u32);
        if s.width < want_w || s.height < want_h {
            let _ = self
                .window
                .request_inner_size(winit::dpi::PhysicalSize::new(
                    s.width.max(want_w),
                    s.height.max(want_h),
                ));
        }
    }

    /// Draw `pm` (which shows the window region starting at `at`) into the window. When the buffer's previous
    /// contents are unknown and the pixmap does not cover the whole window, returns `false` and draws nothing:
    /// the caller must then redraw everything.
    pub fn present(&mut self, pm: &Pixmap, at: (i32, i32)) -> Result<bool, String> {
        let (ww, wh) = self.size;
        let full = at == (0, 0) && pm.width() >= ww && pm.height() >= wh;
        let mut buf = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        if !full && buf.age() != 1 {
            return Ok(false);
        }
        let x0 = at.0.max(0) as usize;
        let y0 = at.1.max(0) as usize;
        let cw = (pm.width() as usize).min(ww as usize - x0.min(ww as usize));
        let ch = (pm.height() as usize).min(wh as usize - y0.min(wh as usize));
        let src = pm.data();
        for row in 0..ch {
            let s = &src[row * pm.width() as usize * 4..];
            let d = &mut buf[(y0 + row) * ww as usize + x0..][..cw];
            for (px, c) in d.iter_mut().zip(s.as_chunks::<4>().0) {
                *px = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
            }
        }
        let damage = softbuffer::Rect {
            x: x0 as u32,
            y: y0 as u32,
            width: NonZeroU32::new(cw as u32).unwrap_or(NonZeroU32::MIN),
            height: NonZeroU32::new(ch as u32).unwrap_or(NonZeroU32::MIN),
        };
        buf.present_with_damage(&[damage])
            .map_err(|e| e.to_string())?;
        Ok(true)
    }
}

impl SoftWindow {
    /// Present a pixmap whose alpha channel should reach the compositor (transparent windows). Colours are
    /// premultiplied, which is what a compositor expects.
    pub fn present_argb(&mut self, pm: &Pixmap) -> Result<(), String> {
        let (ww, wh) = self.size;
        let mut buf = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        let cw = (pm.width() as usize).min(ww as usize);
        let ch = (pm.height() as usize).min(wh as usize);
        for row in 0..ch {
            let s = &pm.data()[row * pm.width() as usize * 4..];
            let d = &mut buf[row * ww as usize..][..cw];
            for (px, c) in d.iter_mut().zip(s.as_chunks::<4>().0) {
                *px = (c[3] as u32) << 24 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
            }
        }
        buf.present().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irect_algebra() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 10, 10);
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert_eq!(a.intersect(&b), Some(IRect::new(5, 5, 5, 5)));
        assert_eq!(a.intersect(&IRect::new(20, 0, 5, 5)), None);
        assert_eq!(a.inflate(2), IRect::new(-2, -2, 14, 14));
        assert_eq!(IRect::around(1.5, 1.5, 2.0, 2.0), IRect::new(1, 1, 3, 3));
    }
}
