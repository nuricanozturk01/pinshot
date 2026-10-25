//! OCR result window: shows the recognised text, copies it, and stays out of the way while the engine works.

use crate::gfx::{Context, SoftWindow};
use crate::ui::{theme, Canvas};
use pinshot_core::i18n::tr;
use pinshot_core::text::Fonts;
use pinshot_core::{Point, Rect, Rgba};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

pub enum State {
    Working,
    Done(String),
    Failed(String),
}

pub struct OcrWin {
    pub id: u64,
    pub sw: SoftWindow,
    pub state: State,
    pub status: String,
    pub scroll: f32,
    pub hover: Option<Btn>,
    sf: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Copy,
    Close,
}

const W: f32 = 520.0;
const H: f32 = 360.0;

pub fn wrap_lines(text: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            out.push(String::new());
            continue;
        }
        let mut cur = String::new();
        for word in raw.split(' ') {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if measure(&cand) <= max_w || cur.is_empty() {
                cur = cand;
            } else {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        out.push(cur);
    }
    out
}

impl OcrWin {
    pub fn create(
        el: &ActiveEventLoop,
        ctx: &Context,
        id: u64,
        at: Point,
        sf: f32,
    ) -> Result<OcrWin, String> {
        let attrs = Window::default_attributes()
            .with_title("PinShot – OCR")
            .with_resizable(crate::gfx::DIALOG_RESIZABLE)
            .with_inner_size(crate::units::size_px(W * sf, H * sf, sf))
            .with_position(crate::units::win_pos(at.x, at.y))
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let sf = window.scale_factor() as f32;
        let sw = SoftWindow::new(ctx, window)?;
        let mut w = OcrWin {
            id,
            sw,
            state: State::Working,
            status: tr("Metin tanınıyor…").to_string(),
            scroll: 0.0,
            hover: None,
            sf,
        };
        w.draw();
        Ok(w)
    }

    fn rects() -> (Rect, Rect, Rect) {
        let text = Rect::new(12.0, 12.0, W - 24.0, H - 64.0);
        let copy = Rect::new(W - 12.0 - 88.0 - 8.0 - 88.0, H - 44.0, 88.0, 32.0);
        let close = Rect::new(W - 12.0 - 88.0, H - 44.0, 88.0, 32.0);
        (text, copy, close)
    }

    pub fn button_at(&self, p: Point) -> Option<Btn> {
        let (_, copy, close) = Self::rects();
        let ui = Point::new(p.x / self.sf, p.y / self.sf);
        if copy.contains(ui) && matches!(self.state, State::Done(_)) {
            Some(Btn::Copy)
        } else if close.contains(ui) {
            Some(Btn::Close)
        } else {
            None
        }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.state {
            State::Done(t) => Some(t),
            _ => None,
        }
    }

    fn lines(&self, max_w: f32) -> Vec<String> {
        let body = match &self.state {
            State::Done(t) | State::Failed(t) => t.as_str(),
            State::Working => "",
        };
        let fonts = Fonts::embedded();
        wrap_lines(body, max_w, |s| fonts.line_width(s, 14.0, false))
    }

    pub fn scroll_by(&mut self, delta: f32) {
        let (text, _, _) = Self::rects();
        let total = self.lines(text.w - 20.0).len() as f32 * 20.0;
        let max = (total - (text.h - 16.0)).max(0.0);
        self.scroll = (self.scroll - delta).clamp(0.0, max);
    }

    pub fn draw(&mut self) {
        self.sw.ensure_ui_size(W, H, self.sf);
        self.sw.sync_size();
        let (pw, ph) = self.sw.size();
        let mut pm = Pixmap::new(pw, ph).expect("size");
        pm.fill(tiny_skia::Color::from_rgba8(0x2b, 0x2b, 0x2f, 255));
        let (text, copy, close) = Self::rects();
        {
            let mut cv = Canvas::new(&mut pm, self.sf);
            cv.fill_rrect(text, 6.0, theme::FIELD_BG);
            let lines = self.lines(text.w - 20.0);
            if matches!(self.state, State::Working) {
                cv.text_centered(text, tr("Metin tanınıyor…"), 14.0, theme::TEXT_DIM, false);
            }
            // clip by only drawing visible lines
            for (i, l) in lines.iter().enumerate() {
                let y = text.y + 8.0 + i as f32 * 20.0 - self.scroll;
                if y + 20.0 < text.y || y > text.bottom() - 4.0 {
                    continue;
                }
                let ink = if matches!(self.state, State::Failed(_)) {
                    theme::DANGER
                } else {
                    Rgba::WHITE
                };
                cv.text(text.x + 10.0, y, l, 14.0, ink, false);
            }
            cv.text(12.0, H - 36.0, &self.status, 12.0, theme::TEXT, false);
            for (b, r, label, accent) in [
                (Btn::Copy, copy, tr("Kopyala"), true),
                (Btn::Close, close, tr("Kapat"), false),
            ] {
                if b == Btn::Copy && !matches!(self.state, State::Done(_)) {
                    cv.fill_rrect(r, 6.0, Rgba::new(255, 255, 255, 20));
                    cv.text_centered(r, label, 13.0, theme::TEXT_DIM, false);
                    continue;
                }
                let bg = if self.hover == Some(b) {
                    theme::BTN_HOVER
                } else {
                    Rgba::rgb(0x3a, 0x3a, 0x40)
                };
                cv.fill_rrect(
                    r,
                    6.0,
                    if accent && self.hover != Some(b) {
                        theme::ACCENT
                    } else {
                        bg
                    },
                );
                cv.text_centered(r, label, 13.0, Rgba::WHITE, false);
            }
        }
        if let Err(e) = self.sw.present(&pm, (0, 0)) {
            log::warn!("ocr window: {e}");
        }
    }

    pub fn finish(&mut self, result: Result<String, String>) {
        match result {
            Ok(t) if !t.trim().is_empty() => {
                self.status = format!(
                    "{} {}",
                    t.chars().count(),
                    tr("karakter – panoya kopyalandı")
                );
                self.state = State::Done(t);
            }
            Ok(_) => {
                self.status = tr("Metin bulunamadı.").to_string();
                self.state = State::Done(String::new());
            }
            Err(e) => {
                self.status = tr("OCR başarısız").to_string();
                self.state = State::Failed(e);
            }
        }
        self.scroll = 0.0;
        self.draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_lines_and_keeps_blank_lines() {
        let m = |s: &str| s.chars().count() as f32 * 10.0;
        let l = wrap_lines("aaa bbb ccc\n\nddd", 70.0, m);
        assert_eq!(l, vec!["aaa bbb", "ccc", "", "ddd"]);
        assert_eq!(wrap_lines("", 70.0, m), Vec::<String>::new());
        assert_eq!(
            wrap_lines("waytoolongword", 50.0, m),
            vec!["waytoolongword"]
        );
    }
}
