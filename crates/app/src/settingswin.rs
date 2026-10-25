//! Settings window: hosts the form, a Save / Cancel bar and the shortcut capture state.

use crate::form::{Event, Form};
use crate::gfx::{Context, SoftWindow};
use crate::settings;
use crate::ui::{theme, Canvas};
use pinshot_core::config::Config;
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

const W: f32 = 520.0;
const H: f32 = 640.0;
const BAR_H: f32 = 56.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Save,
    Cancel,
}

pub struct SettingsWin {
    pub sw: SoftWindow,
    pub form: Form,
    pub hover: Option<Btn>,
    pub mac: bool,
    pub sf: f32,
    /// Last cursor position in UI units.
    pub last: Point,
}

impl SettingsWin {
    pub fn create(
        el: &ActiveEventLoop,
        ctx: &Context,
        cfg: &Config,
        mac: bool,
    ) -> Result<SettingsWin, String> {
        let attrs = Window::default_attributes()
            .with_title(format!("PinShot – {}", tr("Ayarlar")))
            .with_resizable(false)
            .with_inner_size(winit::dpi::LogicalSize::new(f64::from(W), f64::from(H)));
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sf = window.scale_factor() as f32;
        let sw = SoftWindow::new(ctx, window)?;
        let mut w = SettingsWin {
            sw,
            form: settings::build(cfg, mac, W, H - BAR_H),
            hover: None,
            mac,
            sf,
            last: Point::new(-1.0, -1.0),
        };
        w.draw();
        Ok(w)
    }

    fn buttons() -> [(Btn, Rect); 2] {
        [
            (
                Btn::Save,
                Rect::new(W - 16.0 - 100.0 - 8.0 - 100.0, H - 44.0, 100.0, 32.0),
            ),
            (
                Btn::Cancel,
                Rect::new(W - 16.0 - 100.0, H - 44.0, 100.0, 32.0),
            ),
        ]
    }

    /// Window pixels to UI units.
    pub fn to_ui(&self, x: f64, y: f64) -> Point {
        Point::new(x as f32 / self.sf, y as f32 / self.sf)
    }

    pub fn button_at(&self, p: Point) -> Option<Btn> {
        Self::buttons()
            .into_iter()
            .find(|(_, r)| r.contains(p))
            .map(|(b, _)| b)
    }

    /// Returns true when a redraw is needed.
    pub fn hover_at(&mut self, p: Point) -> bool {
        self.last = p;
        let b = self.button_at(p);
        let in_form = p.y < H - BAR_H;
        let f = self.form.set_hover(in_form.then_some(p));
        let changed = b != self.hover || f;
        self.hover = b;
        changed
    }

    pub fn press(&mut self, p: Point) -> Option<Event> {
        if p.y >= H - BAR_H {
            return None;
        }
        self.form.press(p)
    }

    pub fn scroll_by(&mut self, d: f32) {
        self.form.scroll_by(d);
    }

    pub fn draw(&mut self) {
        self.sw.sync_size();
        let (pw, ph) = self.sw.size();
        let Some(mut pm) = Pixmap::new(pw, ph) else {
            return;
        };
        pm.fill(tiny_skia::Color::from_rgba8(0x2b, 0x2b, 0x2f, 255));
        {
            let mut cv = Canvas::new(&mut pm, self.sf);
            self.form.draw(&mut cv);
            // the form scrolls underneath the bar: repaint the bar on top
            cv.fill_rect(
                Rect::new(0.0, H - BAR_H, W, BAR_H),
                Rgba::rgb(0x2b, 0x2b, 0x2f),
            );
            cv.fill_rect(Rect::new(0.0, H - BAR_H, W, 1.0), theme::SEP);
            for (b, r) in Self::buttons() {
                let (label, accent) = match b {
                    Btn::Save => (tr("Kaydet"), true),
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
            log::warn!("settings window: {e}");
        }
    }
}
