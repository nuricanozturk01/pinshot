//! Desktop units. Desktop coordinates are physical pixels on Windows and Linux but points on macOS, where winit
//! expects logical values. Everything that places or sizes a window, or converts between the unit systems, goes
//! through here instead of testing the platform at each call site.

use pinshot_core::Rect;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Position, Size};
use winit::window::Window;

/// Whether desktop coordinates are points (macOS) rather than physical pixels.
pub const POINTS: bool = cfg!(target_os = "macos");

/// Desktop units per logical UI unit of a window with scale factor `sf`.
pub fn per_ui(sf: f32) -> f32 {
    if POINTS {
        1.0
    } else {
        sf
    }
}

/// Desktop units per physical pixel of a window with scale factor `sf`.
pub fn per_px(sf: f32) -> f32 {
    if POINTS {
        1.0 / sf
    } else {
        1.0
    }
}

/// Physical pixels per desktop unit of a window with scale factor `sf`.
pub fn px_per_unit(sf: f32) -> f32 {
    if POINTS {
        sf
    } else {
        1.0
    }
}

/// A window position given in desktop units.
pub fn win_pos(x: f32, y: f32) -> Position {
    if POINTS {
        LogicalPosition::new(f64::from(x), f64::from(y)).into()
    } else {
        PhysicalPosition::new(x as i32, y as i32).into()
    }
}

/// A size given in desktop units.
pub fn size_units(w: f32, h: f32) -> Size {
    if POINTS {
        LogicalSize::new(f64::from(w.max(1.0)), f64::from(h.max(1.0))).into()
    } else {
        PhysicalSize::new(w.max(1.0) as u32, h.max(1.0) as u32).into()
    }
}

/// A size given in physical pixels of a window with scale factor `sf`.
pub fn size_px(w: f32, h: f32, sf: f32) -> Size {
    if POINTS {
        LogicalSize::new(f64::from((w / sf).max(1.0)), f64::from((h / sf).max(1.0))).into()
    } else {
        PhysicalSize::new(w.max(1.0) as u32, h.max(1.0) as u32).into()
    }
}

/// The window's outer rectangle in desktop units.
pub fn outer_rect(w: &Window) -> Rect {
    let k = per_px(w.scale_factor() as f32);
    let p = w.outer_position().unwrap_or_default();
    let s = w.outer_size();
    Rect::new(
        p.x as f32 * k,
        p.y as f32 * k,
        s.width as f32 * k,
        s.height as f32 * k,
    )
}
