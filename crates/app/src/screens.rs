//! Monitors, screen grabbing and the pointer position.
//!
//! Desktop coordinates are the platform's own: physical pixels on Windows and Linux, points on macOS (where
//! `scale` is 2 on Retina displays). A [`Monitor`]'s image has `rect.size * scale` pixels.

use pinshot_core::compose::Screen;
use pinshot_core::{Point, Rect};
use tiny_skia::Pixmap;

#[derive(Clone, Debug)]
pub struct Monitor {
    pub name: String,
    pub rect: Rect,
    /// Image pixels per desktop unit.
    pub scale: f32,
}

pub struct Capture {
    pub monitor: Monitor,
    pub pixmap: Pixmap,
}

impl Capture {
    pub fn screen(&self) -> Screen<'_> {
        Screen {
            rect: self.monitor.rect,
            scale: self.monitor.scale,
            pixmap: &self.pixmap,
        }
    }
}

/// True when the grabbed picture is one flat colour (permission denied, or a Wayland compositor returning black).
pub fn looks_blank(pm: &Pixmap) -> bool {
    let (w, h) = (pm.width(), pm.height());
    if w == 0 || h == 0 {
        return true;
    }
    let first = pm.pixel(0, 0);
    for i in 1..24u32 {
        for j in 1..24u32 {
            if pm.pixel(w * i / 24 % w, h * j / 24 % h) != first {
                return false;
            }
        }
    }
    true
}

pub fn monitor_at(monitors: &[Monitor], p: Point) -> Option<usize> {
    monitors
        .iter()
        .position(|m| m.rect.contains(p))
        .or(if monitors.is_empty() { None } else { Some(0) })
}

/// Convert a tightly packed BGRA/BGRX buffer (X11, GDI, CoreGraphics) into an opaque pixmap in place: a full-screen grab is tens of
/// megabytes and must not exist twice.
pub fn pixmap_from_bgrx_owned(w: u32, h: u32, mut data: Vec<u8>) -> Option<Pixmap> {
    if data.len() != w as usize * h as usize * 4 {
        return None;
    }
    for px in data.as_chunks_mut::<4>().0 {
        *px = [px[2], px[1], px[0], 255];
    }
    Pixmap::from_vec(data, tiny_skia::IntSize::from_wh(w, h)?)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, ImageFormat, MapState};
    use x11rb::rust_connection::RustConnection;

    fn conn() -> Result<(RustConnection, usize), String> {
        x11rb::connect(None).map_err(|e| e.to_string())
    }

    pub fn monitors() -> Result<Vec<Monitor>, String> {
        let (c, n) = conn()?;
        let root = c.setup().roots[n].root;
        let reply = c
            .randr_get_monitors(root, true)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for m in reply.monitors {
            let name = c
                .get_atom_name(m.name)
                .ok()
                .and_then(|r| r.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.name).into_owned())
                .unwrap_or_else(|| format!("monitor{}", out.len()));
            out.push(Monitor {
                name,
                rect: Rect::new(m.x as f32, m.y as f32, m.width as f32, m.height as f32),
                scale: 1.0,
            });
        }
        if out.is_empty() {
            let s = &c.setup().roots[n];
            out.push(Monitor {
                name: "screen".into(),
                rect: Rect::new(
                    0.0,
                    0.0,
                    s.width_in_pixels as f32,
                    s.height_in_pixels as f32,
                ),
                scale: 1.0,
            });
        }
        out.sort_by(|a, b| {
            (a.rect.x, a.rect.y)
                .partial_cmp(&(b.rect.x, b.rect.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    pub fn grab(m: &Monitor) -> Result<Pixmap, String> {
        grab_part(m, m.rect)
    }

    pub fn grab_part(m: &Monitor, part: Rect) -> Result<Pixmap, String> {
        let (w, h, data) = grab_raw(m, part)?;
        pixmap_from_bgrx_owned(w, h, data).ok_or_else(|| "bad image".to_string())
    }

    /// Grab `part` (desktop coordinates, inside the monitor) as tightly packed BGRX bytes, with no conversion. The
    /// X11 connection is kept per thread, so grabbing frames for a recording is cheap.
    pub fn grab_raw(_m: &Monitor, part: Rect) -> Result<(u32, u32, Vec<u8>), String> {
        POINTER_CONN.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                *slot = Some(conn()?);
            }
            let (c, n) = slot.as_ref().ok_or("no X11 connection")?;
            let root = c.setup().roots[*n].root;
            let (w, h) = (part.w as u16, part.h as u16);
            let img = c
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    root,
                    part.x as i16,
                    part.y as i16,
                    w,
                    h,
                    !0,
                )
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?;
            Ok((w as u32, h as u32, img.data))
        })
    }

    fn atom(c: &RustConnection, name: &str) -> Option<u32> {
        c.intern_atom(false, name.as_bytes())
            .ok()?
            .reply()
            .ok()
            .map(|r| r.atom)
    }

    /// A 32-bit property (window ids, atoms, cardinals) of `win`; empty when it is missing.
    fn prop32(c: &RustConnection, win: u32, name: &str) -> Vec<u32> {
        let Some(prop) = atom(c, name) else {
            return Vec::new();
        };
        c.get_property(false, win, prop, AtomEnum::ANY, 0, 4096)
            .ok()
            .and_then(|r| r.reply().ok())
            .and_then(|r| r.value32().map(Iterator::collect))
            .unwrap_or_default()
    }

    /// The window's outer rectangle on the root window, decorations included when the window manager says how big
    /// they are.
    fn window_rect(c: &RustConnection, root: u32, win: u32) -> Option<Rect> {
        let geo = c.get_geometry(win).ok()?.reply().ok()?;
        let at = c
            .translate_coordinates(win, root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        let (mut x, mut y) = (f32::from(at.dst_x), f32::from(at.dst_y));
        let (mut w, mut h) = (f32::from(geo.width), f32::from(geo.height));
        if let [l, r, t, b] = prop32(c, win, "_NET_FRAME_EXTENTS")[..] {
            x -= l as f32;
            y -= t as f32;
            w += (l + r) as f32;
            h += (t + b) as f32;
        }
        Some(Rect::new(x, y, w, h))
    }

    /// Is `win` something the user would call a window, on the current desktop and not minimised?
    fn is_candidate(c: &RustConnection, win: u32, desktop: Option<u32>) -> bool {
        let viewable = c
            .get_window_attributes(win)
            .ok()
            .and_then(|r| r.reply().ok())
            .is_some_and(|a| a.map_state == MapState::VIEWABLE);
        if !viewable {
            return false;
        }
        let hidden = atom(c, "_NET_WM_STATE_HIDDEN");
        if prop32(c, win, "_NET_WM_STATE")
            .iter()
            .any(|s| Some(*s) == hidden)
        {
            return false;
        }
        let skip = ["_NET_WM_WINDOW_TYPE_DESKTOP", "_NET_WM_WINDOW_TYPE_DOCK"]
            .iter()
            .filter_map(|n| atom(c, n))
            .collect::<Vec<_>>();
        if prop32(c, win, "_NET_WM_WINDOW_TYPE")
            .iter()
            .any(|t| skip.contains(t))
        {
            return false;
        }
        match (prop32(c, win, "_NET_WM_DESKTOP").first(), desktop) {
            (Some(&d), Some(cur)) => d == cur || d == u32::MAX,
            _ => true,
        }
    }

    /// Visible top-level windows, top-most first.
    pub fn windows() -> Result<Vec<Rect>, String> {
        let (c, n) = conn()?;
        let root = c.setup().roots[n].root;
        let desktop = prop32(&c, root, "_NET_CURRENT_DESKTOP").first().copied();
        let mut list = prop32(&c, root, "_NET_CLIENT_LIST_STACKING");
        list.reverse(); // the property is bottom-to-top
        Ok(list
            .into_iter()
            .filter(|w| is_candidate(&c, *w, desktop))
            .filter_map(|w| window_rect(&c, root, w))
            .filter(|r| r.w > 0.0 && r.h > 0.0)
            .collect())
    }

    /// The window that has the keyboard focus.
    pub fn active_window() -> Option<Rect> {
        let (c, n) = conn().ok()?;
        let root = c.setup().roots[n].root;
        let win = *prop32(&c, root, "_NET_ACTIVE_WINDOW").first()?;
        if win == 0 {
            return None;
        }
        window_rect(&c, root, win)
    }

    thread_local! {
        static POINTER_CONN: std::cell::RefCell<Option<(RustConnection, usize)>> = const { std::cell::RefCell::new(None) };
    }

    /// Pointer position. The connection is kept so that polling the pointer is cheap.
    /// Is any mouse button held down?
    pub fn buttons_down() -> bool {
        POINTER_CONN.with(|cell| {
            let slot = cell.borrow();
            let Some((c, n)) = slot.as_ref() else {
                return false;
            };
            let root = c.setup().roots[*n].root;
            c.query_pointer(root)
                .ok()
                .and_then(|r| r.reply().ok())
                .map(|r| u16::from(r.mask) & 0x1f00 != 0)
                .unwrap_or(false)
        })
    }

    pub fn cursor_pos() -> Option<Point> {
        POINTER_CONN.with(|cell| {
            let mut slot = cell.borrow_mut();
            for _ in 0..2 {
                if slot.is_none() {
                    *slot = conn().ok();
                }
                let (c, n) = slot.as_ref()?;
                let root = c.setup().roots[*n].root;
                match c.query_pointer(root).ok().and_then(|r| r.reply().ok()) {
                    Some(r) => return Some(Point::new(r.root_x as f32, r.root_y as f32)),
                    None => *slot = None, // connection died: reconnect once
                }
            }
            None
        })
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe extern "system" fn enum_cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<Monitor>);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(h, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            let r = info.monitorInfo.rcMonitor;
            let name = String::from_utf16_lossy(&info.szDevice)
                .trim_end_matches('\0')
                .to_string();
            list.push(Monitor {
                name,
                rect: Rect::new(
                    r.left as f32,
                    r.top as f32,
                    (r.right - r.left) as f32,
                    (r.bottom - r.top) as f32,
                ),
                scale: 1.0,
            });
        }
        BOOL(1)
    }

    pub fn monitors() -> Result<Vec<Monitor>, String> {
        let mut list: Vec<Monitor> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_cb),
                LPARAM(&mut list as *mut _ as isize),
            );
        }
        if list.is_empty() {
            return Err("no monitors".into());
        }
        list.sort_by(|a, b| {
            (a.rect.x, a.rect.y)
                .partial_cmp(&(b.rect.x, b.rect.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(list)
    }

    pub fn grab(m: &Monitor) -> Result<Pixmap, String> {
        grab_part(m, m.rect)
    }

    pub fn grab_part(m: &Monitor, part: Rect) -> Result<Pixmap, String> {
        let (w, h, data) = grab_raw(m, part)?;
        pixmap_from_bgrx_owned(w, h, data).ok_or_else(|| "bad image".to_string())
    }

    pub fn grab_raw(_m: &Monitor, part: Rect) -> Result<(u32, u32, Vec<u8>), String> {
        let (x, y, w, h) = (part.x as i32, part.y as i32, part.w as i32, part.h as i32);
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
            let mut bi = BITMAPINFO::default();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h; // top-down
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB.0;
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bi,
                DIB_RGB_COLORS,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            if ok.is_err() || lines == 0 {
                return Err("screen capture failed".into());
            }
            Ok((w as u32, h as u32, buf))
        }
    }

    fn window_bounds(h: HWND) -> Option<Rect> {
        use windows::Win32::Graphics::Dwm::{
            DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
        };
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                return None;
            }
            if GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
                return None;
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
            {
                return None; // on another virtual desktop, or a suspended UWP shell
            }
            // the frame without the invisible resize borders; fall back to the plain rectangle
            let mut r = RECT::default();
            if DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut r as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
            {
                GetWindowRect(h, &mut r).ok()?;
            }
            let (w, hh) = ((r.right - r.left) as f32, (r.bottom - r.top) as f32);
            (w > 0.0 && hh > 0.0).then(|| Rect::new(r.left as f32, r.top as f32, w, hh))
        }
    }

    unsafe extern "system" fn window_cb(h: HWND, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<Rect>);
        if let Some(r) = window_bounds(h) {
            list.push(r);
        }
        BOOL(1)
    }

    /// Visible top-level windows, top-most first (EnumWindows walks the z-order from the top).
    pub fn windows() -> Result<Vec<Rect>, String> {
        use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
        let mut list: Vec<Rect> = Vec::new();
        unsafe {
            EnumWindows(Some(window_cb), LPARAM(&mut list as *mut _ as isize))
                .map_err(|e| e.to_string())?;
        }
        Ok(list)
    }

    pub fn active_window() -> Option<Rect> {
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        let h = unsafe { GetForegroundWindow() };
        if h.0.is_null() {
            return None;
        }
        window_bounds(h)
    }

    pub fn buttons_down() -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON,
        };
        unsafe {
            (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16
                | GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16)
                & 0x8000
                != 0
        }
    }

    pub fn cursor_pos() -> Option<Point> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p).ok()? };
        Some(Point::new(p.x as f32, p.y as f32))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_graphics::display::CGDisplay;
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    pub fn monitors() -> Result<Vec<Monitor>, String> {
        let ids = CGDisplay::active_displays().map_err(|e| format!("CGError {e}"))?;
        let mut out: Vec<Monitor> = ids
            .into_iter()
            .map(|id| {
                let d = CGDisplay::new(id);
                let b = d.bounds();
                let scale = if b.size.width > 0.0 {
                    d.pixels_wide() as f32 / b.size.width as f32
                } else {
                    1.0
                };
                Monitor {
                    name: format!("display{id}"),
                    rect: Rect::new(
                        b.origin.x as f32,
                        b.origin.y as f32,
                        b.size.width as f32,
                        b.size.height as f32,
                    ),
                    scale,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            (a.rect.x, a.rect.y)
                .partial_cmp(&(b.rect.x, b.rect.y))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    pub fn grab(m: &Monitor) -> Result<Pixmap, String> {
        grab_part(m, m.rect)
    }

    pub fn grab_part(m: &Monitor, part: Rect) -> Result<Pixmap, String> {
        let (w, h, data) = grab_raw(m, part)?;
        pixmap_from_bgrx_owned(w, h, data).ok_or_else(|| "bad image".to_string())
    }

    pub fn grab_raw(m: &Monitor, part: Rect) -> Result<(u32, u32, Vec<u8>), String> {
        use core_graphics::geometry::{CGPoint, CGRect, CGSize};
        let id: u32 = m
            .name
            .trim_start_matches("display")
            .parse()
            .map_err(|_| "bad display".to_string())?;
        // the rectangle is relative to the display, in points
        let r = CGRect::new(
            &CGPoint::new((part.x - m.rect.x) as f64, (part.y - m.rect.y) as f64),
            &CGSize::new(part.w as f64, part.h as f64),
        );
        let img = CGDisplay::new(id)
            .image_for_rect(r)
            .ok_or("screen recording permission missing or capture failed")?;
        let (w, h, stride) = (img.width() as u32, img.height() as u32, img.bytes_per_row());
        let data = img.data();
        let src = data.bytes();
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for y in 0..h as usize {
            out.extend_from_slice(&src[y * stride..y * stride + w as usize * 4]);
        }
        Ok((w, h, out))
    }

    /// Window picking is not implemented on macOS yet; area selection works as before.
    pub fn windows() -> Result<Vec<Rect>, String> {
        Ok(Vec::new())
    }

    pub fn active_window() -> Option<Rect> {
        None
    }

    pub fn cursor_pos() -> Option<Point> {
        let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
        let p = CGEvent::new(src).ok()?.location();
        Some(Point::new(p.x as f32, p.y as f32))
    }

    pub fn buttons_down() -> bool {
        // `CGMouseButton`: 0 = left, 1 = right
        [0u32, 1].into_iter().any(|button| {
            // SAFETY: plain query of the combined session state; both arguments are valid enum values.
            unsafe {
                CGEventSourceButtonState(CGEventSourceStateID::CombinedSessionState as i32, button)
            }
        })
    }

    extern "C" {
        fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn screen_access(request: bool) -> bool {
        unsafe {
            if CGPreflightScreenCaptureAccess() {
                return true;
            }
            if request {
                CGRequestScreenCaptureAccess();
            }
        }
        false
    }
}

#[cfg(not(target_os = "macos"))]
pub fn screen_access(_request: bool) -> bool {
    true
}

#[cfg(target_os = "macos")]
pub use imp::screen_access;
pub use imp::{
    active_window, buttons_down, cursor_pos, grab, grab_part, grab_raw, monitors, windows,
};

/// Wayland: X11 grabs return black, so the screenshot portal is asked for the whole desktop and cut per monitor.
#[cfg(target_os = "linux")]
mod portal {
    use super::{imp, Capture, Monitor};
    use tiny_skia::Pixmap;

    /// Is this a Wayland session?
    pub fn is_wayland() -> bool {
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
    }

    pub fn decode_uri(uri: &str) -> Option<std::path::PathBuf> {
        let path = uri.strip_prefix("file://")?;
        let b = path.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok()?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        Some(std::path::PathBuf::from(String::from_utf8(out).ok()?))
    }

    /// The whole desktop as one image: the screenshot portal first (GNOME, KDE), `grim` for wlroots compositors
    /// (sway, Hyprland), whose portal backend cannot take screenshots.
    fn desktop_image() -> Result<image::RgbaImage, String> {
        portal_image().or_else(|portal_err| {
            grim_image().map_err(|grim_err| format!("{portal_err}; grim: {grim_err}"))
        })
    }

    fn grim_image() -> Result<image::RgbaImage, String> {
        let out = std::process::Command::new("grim")
            .args(["-t", "png", "-"])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        image::load_from_memory(&out.stdout)
            .map(|i| i.to_rgba8())
            .map_err(|e| e.to_string())
    }

    fn portal_image() -> Result<image::RgbaImage, String> {
        let shot = async_io::block_on(async {
            ashpd::desktop::screenshot::Screenshot::request()
                .interactive(false)
                .send()
                .await
                .and_then(|r| r.response())
        })
        .map_err(|e| format!("screenshot portal: {e}"))?;
        let path = decode_uri(shot.uri().as_str()).ok_or("screenshot portal returned a bad uri")?;
        let img = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
        let _ = std::fs::remove_file(&path);
        Ok(img)
    }

    /// Cut one monitor out of the desktop image. `scale` is image pixels per desktop unit.
    pub fn crop(
        img: &image::RgbaImage,
        m: &Monitor,
        origin: (f32, f32),
        scale: f32,
    ) -> Option<Pixmap> {
        let x = ((m.rect.x - origin.0) * scale).round().max(0.0) as u32;
        let y = ((m.rect.y - origin.1) * scale).round().max(0.0) as u32;
        let w = ((m.rect.w * scale).round() as u32).min(img.width().saturating_sub(x));
        let h = ((m.rect.h * scale).round() as u32).min(img.height().saturating_sub(y));
        if w == 0 || h == 0 {
            return None;
        }
        let mut part = image::imageops::crop_imm(img, x, y, w, h).to_image();
        for p in part.pixels_mut() {
            p.0[3] = 255;
        }
        let size = tiny_skia::IntSize::from_wh(w, h)?;
        Pixmap::from_vec(part.into_raw(), size)
    }

    pub fn grab_all() -> Result<Vec<Capture>, String> {
        let mons = imp::monitors()?;
        let img = desktop_image()?;
        let left = mons.iter().map(|m| m.rect.x).fold(f32::MAX, f32::min);
        let top = mons.iter().map(|m| m.rect.y).fold(f32::MAX, f32::min);
        let right = mons.iter().map(|m| m.rect.right()).fold(f32::MIN, f32::max);
        let scale = img.width() as f32 / (right - left).max(1.0);
        mons.into_iter()
            .map(|m| {
                let pixmap = crop(&img, &m, (left, top), scale)
                    .ok_or_else(|| format!("monitor {} is outside the screenshot", m.name))?;
                Ok(Capture { monitor: m, pixmap })
            })
            .collect()
    }
}

/// Grab every monitor.
pub fn grab_all() -> Result<Vec<Capture>, String> {
    #[cfg(target_os = "linux")]
    if portal::is_wayland() {
        return portal::grab_all();
    }
    let mons = monitors()?;
    mons.into_iter()
        .map(|m| grab(&m).map(|pixmap| Capture { monitor: m, pixmap }))
        .collect()
}

#[cfg(all(test, target_os = "linux"))]
mod portal_tests {
    use super::portal::{crop, decode_uri};
    use super::Monitor;
    use pinshot_core::Rect;

    #[test]
    fn decodes_file_uris() {
        assert_eq!(
            decode_uri("file:///tmp/a%20b.png").unwrap(),
            std::path::PathBuf::from("/tmp/a b.png")
        );
        assert!(decode_uri("https://x/y.png").is_none());
    }

    #[test]
    fn crops_a_monitor_out_of_the_desktop_image() {
        let mut img = image::RgbaImage::new(200, 100);
        for (x, _, p) in img.enumerate_pixels_mut() {
            p.0 = if x < 100 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            };
        }
        let m = Monitor {
            name: "right".into(),
            rect: Rect::new(1000.0, 0.0, 100.0, 100.0),
            scale: 1.0,
        };
        let pm = crop(&img, &m, (900.0, 0.0), 1.0).unwrap();
        assert_eq!((pm.width(), pm.height()), (100, 100));
        assert_eq!(pm.pixel(5, 5).unwrap().blue(), 255);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_detection_and_conversion() {
        let flat = pixmap_from_bgrx_owned(30, 30, vec![10u8; 30 * 30 * 4]).unwrap();
        assert!(looks_blank(&flat));
        let mut data = vec![0u8; 30 * 30 * 4];
        data[(15 * 30 + 15) * 4] = 200;
        let mut busy = pixmap_from_bgrx_owned(30, 30, data).unwrap();
        for (i, p) in busy
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            p[0] = (i % 251) as u8;
        }
        assert!(!looks_blank(&busy));
        let one = pixmap_from_bgrx_owned(1, 1, vec![1, 2, 3, 0]).unwrap();
        let p = one.pixel(0, 0).unwrap();
        assert_eq!(
            (p.red(), p.green(), p.blue(), p.alpha()),
            (3, 2, 1, 255),
            "BGRX becomes RGBA"
        );
    }

    #[test]
    fn owned_conversion_rejects_a_wrong_length() {
        assert!(pixmap_from_bgrx_owned(3, 2, vec![0; 5]).is_none());
    }

    #[test]
    fn monitor_lookup_falls_back_to_the_first() {
        let ms = vec![
            Monitor {
                name: "a".into(),
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                scale: 1.0,
            },
            Monitor {
                name: "b".into(),
                rect: Rect::new(100.0, 0.0, 100.0, 100.0),
                scale: 1.0,
            },
        ];
        assert_eq!(monitor_at(&ms, Point::new(150.0, 5.0)), Some(1));
        assert_eq!(monitor_at(&ms, Point::new(-5.0, 5.0)), Some(0));
        assert_eq!(monitor_at(&[], Point::new(0.0, 0.0)), None);
    }
}

#[cfg(test)]
pub mod tests_support {
    use pinshot_core::text::TextMeasure;

    /// Fixed-size measuring for tests that do not care about real glyph widths.
    pub struct NoMeasure;
    impl TextMeasure for NoMeasure {
        fn measure(&self, text: &str, _px: f32, _b: bool) -> (f32, f32) {
            let lines: Vec<&str> = text.split('\n').collect();
            (
                lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 10.0,
                lines.len() as f32 * 20.0,
            )
        }
    }
}
