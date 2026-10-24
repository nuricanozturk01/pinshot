//! Popup menus shown in their own borderless windows (a root menu plus at most one open submenu).

use crate::gfx::{Context, SoftWindow};
use crate::ui::menu::{Item, Kind, Menu};
use crate::ui::Canvas;
use pinshot_core::{Point, Rect};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

pub struct MenuWin {
    pub sw: SoftWindow,
    pub menu: Menu,
    rows: Vec<Rect>,
    size: (f32, f32),
    sf: f32,
    /// Row whose submenu is open.
    pub open_sub: Option<usize>,
}

pub struct Popup {
    pub stack: Vec<MenuWin>,
    /// Who the menu belongs to (e.g. a pin id).
    pub owner: u64,
}

fn text_w(sf: f32) -> impl Fn(&str) -> f32 {
    move |s: &str| {
        let mut pm = Pixmap::new(1, 1).expect("1px");
        Canvas::new(&mut pm, sf).text_width(s, 13.0, false)
    }
}

impl MenuWin {
    fn create(
        el: &ActiveEventLoop,
        ctx: &Context,
        items: Vec<Item>,
        at: Point,
        sf: f32,
        bounds: Rect,
    ) -> Result<MenuWin, String> {
        let menu = Menu::new(items);
        let (rows, size) = menu.layout(text_w(sf));
        let (pw, ph) = ((size.0 * sf).ceil(), (size.1 * sf).ceil());
        // keep the whole menu on screen
        let x = at.x.min(bounds.right() - pw).max(bounds.x);
        let y = at.y.min(bounds.bottom() - ph).max(bounds.y);
        let attrs = Window::default_attributes()
            .with_title("PinShot")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(pw as u32, ph as u32))
            .with_position(PhysicalPosition::new(x as i32, y as i32))
            .with_window_level(WindowLevel::AlwaysOnTop);
        #[cfg(target_os = "linux")]
        let attrs = {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attrs.with_x11_window_type(vec![WindowType::PopupMenu])
        };
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sw = SoftWindow::new(ctx, window)?;
        let mut w = MenuWin {
            sw,
            menu,
            rows,
            size,
            sf,
            open_sub: None,
        };
        w.paint();
        Ok(w)
    }

    pub fn paint(&mut self) {
        let (pw, ph) = (
            (self.size.0 * self.sf).ceil() as u32,
            (self.size.1 * self.sf).ceil() as u32,
        );
        let mut pm = Pixmap::new(pw.max(1), ph.max(1)).expect("menu size");
        self.menu
            .draw(&mut Canvas::new(&mut pm, self.sf), &self.rows, self.size);
        self.sw.sync_size();
        if let Err(e) = self.sw.present(&pm, (0, 0)) {
            log::warn!("menu present: {e}");
        }
    }

    fn row_at(&self, pos: PhysicalPosition<f64>) -> Option<usize> {
        self.menu.item_at(
            &self.rows,
            Point::new(pos.x as f32 / self.sf, pos.y as f32 / self.sf),
        )
    }
}

pub enum Outcome {
    None,
    /// An item was chosen; the menu should close.
    Chosen(&'static str),
    Close,
}

impl Popup {
    pub fn open(
        el: &ActiveEventLoop,
        ctx: &Context,
        items: Vec<Item>,
        at: Point,
        sf: f32,
        bounds: Rect,
        owner: u64,
    ) -> Option<Popup> {
        match MenuWin::create(el, ctx, items, at, sf, bounds) {
            Ok(w) => Some(Popup {
                stack: vec![w],
                owner,
            }),
            Err(e) => {
                log::warn!("menu: {e}");
                None
            }
        }
    }

    pub fn owns(&self, id: WindowId) -> Option<usize> {
        self.stack.iter().position(|w| w.sw.window.id() == id)
    }

    pub fn hover(
        &mut self,
        el: &ActiveEventLoop,
        ctx: &Context,
        level: usize,
        pos: PhysicalPosition<f64>,
        bounds: Rect,
    ) {
        let Some(w) = self.stack.get_mut(level) else {
            return;
        };
        let row = w.row_at(pos);
        if w.menu.hover != row {
            w.menu.hover = row;
            w.paint();
        }
        let sub = row.and_then(|r| match &w.menu.items[r].kind {
            Kind::Sub(items) if w.menu.items[r].enabled => Some((r, items.clone())),
            _ => None,
        });
        let open = w.open_sub;
        match sub {
            Some((r, items)) if open != Some(r) => {
                self.stack.truncate(level + 1);
                let w = &mut self.stack[level];
                w.open_sub = Some(r);
                let base =
                    w.sw.window
                        .outer_position()
                        .map(|p| Point::new(p.x as f32, p.y as f32))
                        .unwrap_or_default();
                let rect = w.rows[r];
                let at = Point::new(
                    base.x + (w.size.0 - 4.0) * w.sf,
                    base.y + rect.y * w.sf - 5.0,
                );
                let sf = w.sf;
                if let Ok(child) = MenuWin::create(el, ctx, items, at, sf, bounds) {
                    self.stack.push(child);
                }
            }
            None if open.is_some() && row.is_some() => {
                self.stack.truncate(level + 1);
                self.stack[level].open_sub = None;
            }
            _ => {}
        }
    }

    pub fn click(&mut self, level: usize, pos: PhysicalPosition<f64>) -> Outcome {
        let Some(w) = self.stack.get(level) else {
            return Outcome::None;
        };
        match w.row_at(pos) {
            Some(r) if w.menu.items[r].enabled => match &w.menu.items[r].kind {
                Kind::Sub(_) => Outcome::None,
                _ => Outcome::Chosen(w.menu.items[r].id),
            },
            _ => Outcome::None,
        }
    }
}
