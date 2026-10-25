//! About window: version, author and shortcuts to the project page and the log folder.

use crate::gfx::{Context, SoftWindow};
use crate::ui::{theme, Canvas};
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

const W: f32 = 380.0;
const H: f32 = 240.0;
pub const REPO: &str = "https://github.com/nuricanozturk01/pinshot";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Project,
    Logs,
    Close,
}

pub struct AboutWin {
    pub sw: SoftWindow,
    pub hover: Option<Btn>,
    sf: f32,
}

impl AboutWin {
    pub fn create(el: &ActiveEventLoop, ctx: &Context) -> Result<AboutWin, String> {
        let attrs = Window::default_attributes()
            .with_title(format!("PinShot – {}", tr("Hakkında")))
            .with_resizable(crate::gfx::DIALOG_RESIZABLE)
            .with_inner_size(winit::dpi::LogicalSize::new(f64::from(W), f64::from(H)));
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sf = window.scale_factor() as f32;
        let mut w = AboutWin {
            sw: SoftWindow::new(ctx, window)?,
            hover: None,
            sf,
        };
        w.draw();
        Ok(w)
    }

    fn buttons() -> [(Btn, Rect); 3] {
        let y = H - 52.0;
        [
            (Btn::Project, Rect::new(16.0, y, 110.0, 34.0)),
            (Btn::Logs, Rect::new(134.0, y, 130.0, 34.0)),
            (Btn::Close, Rect::new(W - 16.0 - 90.0, y, 90.0, 34.0)),
        ]
    }

    pub fn button_at(&self, x: f64, y: f64) -> Option<Btn> {
        let p = Point::new(x as f32 / self.sf, y as f32 / self.sf);
        Self::buttons()
            .into_iter()
            .find(|(_, r)| r.contains(p))
            .map(|(b, _)| b)
    }

    pub fn draw(&mut self) {
        self.sw.ensure_ui_size(W, H, self.sf);
        self.sw.sync_size();
        let (pw, ph) = self.sw.size();
        let Some(mut pm) = Pixmap::new(pw, ph) else {
            return;
        };
        pm.fill(tiny_skia::Color::from_rgba8(0x2b, 0x2b, 0x2f, 255));
        {
            let mut cv = Canvas::new(&mut pm, self.sf);
            cv.text(24.0, 24.0, "PinShot", 26.0, Rgba::WHITE, true);
            cv.text(
                24.0,
                62.0,
                &format!("{} {}", tr("Sürüm"), env!("CARGO_PKG_VERSION")),
                13.0,
                theme::TEXT,
                false,
            );
            cv.text(
                24.0,
                90.0,
                tr("Yakala, işaretle, sabitle, metni tanı ve ekranı kaydet."),
                12.0,
                theme::TEXT_DIM,
                false,
            );
            cv.text(24.0, 130.0, "Nuri Can OZTURK", 13.0, theme::TEXT, false);
            cv.text(24.0, 152.0, REPO, 12.0, theme::ACCENT, false);
            for (b, r) in Self::buttons() {
                let label = match b {
                    Btn::Project => tr("Proje sayfası"),
                    Btn::Logs => tr("Log klasörü"),
                    Btn::Close => tr("Kapat"),
                };
                let bg = if self.hover == Some(b) {
                    theme::BTN_HOVER
                } else {
                    Rgba::rgb(0x3a, 0x3a, 0x40)
                };
                cv.fill_rrect(r, 6.0, bg);
                cv.text_centered(r, label, 13.0, Rgba::WHITE, false);
            }
        }
        if let Err(e) = self.sw.present(&pm, (0, 0)) {
            log::warn!("about window: {e}");
        }
    }
}
