//! Popup menu model: layout, drawing and hit testing. The window that shows it lives in the app module.

use super::{canvas::Canvas, theme};
use pinshot_core::{Point, Rect, Rgba};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Action,
    Check(bool),
    Separator,
    Sub(Vec<Item>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: &'static str,
    pub label: String,
    pub hint: String,
    pub kind: Kind,
    pub enabled: bool,
}

impl Item {
    pub fn action(id: &'static str, label: &str) -> Item {
        Item {
            id,
            label: label.to_string(),
            hint: String::new(),
            kind: Kind::Action,
            enabled: true,
        }
    }

    pub fn with_hint(mut self, hint: &str) -> Item {
        self.hint = hint.to_string();
        self
    }

    pub fn check(id: &'static str, label: &str, on: bool) -> Item {
        Item {
            id,
            label: label.to_string(),
            hint: String::new(),
            kind: Kind::Check(on),
            enabled: true,
        }
    }

    pub fn sep() -> Item {
        Item {
            id: "",
            label: String::new(),
            hint: String::new(),
            kind: Kind::Separator,
            enabled: false,
        }
    }

    pub fn sub(label: &str, items: Vec<Item>) -> Item {
        Item {
            id: "",
            label: label.to_string(),
            hint: String::new(),
            kind: Kind::Sub(items),
            enabled: true,
        }
    }
}

const ROW_H: f32 = 28.0;
const SEP_H: f32 = 9.0;
const PAD: f32 = 5.0;

pub struct Menu {
    pub items: Vec<Item>,
    pub hover: Option<usize>,
}

impl Menu {
    pub fn new(items: Vec<Item>) -> Menu {
        Menu { items, hover: None }
    }

    /// Row rectangles in logical units, and the overall size.
    pub fn layout(&self, text_w: impl Fn(&str) -> f32) -> (Vec<Rect>, (f32, f32)) {
        let mut w = 0.0_f32;
        for it in &self.items {
            let hint = if it.hint.is_empty() {
                0.0
            } else {
                text_w(&it.hint) + 28.0
            };
            w = w.max(text_w(&it.label) + hint + 56.0);
        }
        let mut y = PAD;
        let mut rows = Vec::new();
        for it in &self.items {
            let h = if it.kind == Kind::Separator {
                SEP_H
            } else {
                ROW_H
            };
            rows.push(Rect::new(PAD, y, w, h));
            y += h;
        }
        (rows, (w + 2.0 * PAD, y + PAD))
    }

    pub fn item_at(&self, rows: &[Rect], p: Point) -> Option<usize> {
        rows.iter()
            .position(|r| r.contains(p))
            .filter(|&i| self.items[i].kind != Kind::Separator)
    }

    pub fn draw(&self, cv: &mut Canvas, rows: &[Rect], size: (f32, f32)) {
        let body = Rect::new(0.0, 0.0, size.0, size.1);
        cv.fill_rect(body, theme::BAR_BORDER);
        cv.fill_rect(body.inflated(-1.0), theme::BAR_BG);
        for (i, (it, r)) in self.items.iter().zip(rows).enumerate() {
            if it.kind == Kind::Separator {
                cv.fill_rect(
                    Rect::new(r.x + 8.0, r.y + r.h / 2.0, r.w - 16.0, 1.0),
                    theme::SEP,
                );
                continue;
            }
            let ink = if it.enabled {
                Rgba::WHITE
            } else {
                theme::TEXT_DIM
            };
            if self.hover == Some(i) && it.enabled {
                cv.fill_rrect(*r, 5.0, theme::ACCENT);
            }
            if let Kind::Check(true) = it.kind {
                crate::ui::icons::draw(
                    cv,
                    "check",
                    Rect::new(r.x + 6.0, r.y + 5.0, 18.0, 18.0),
                    ink,
                );
            }
            cv.text(r.x + 30.0, r.y + 7.0, &it.label, 13.0, ink, false);
            if !it.hint.is_empty() {
                let w = cv.text_width(&it.hint, 12.0, false);
                cv.text(
                    r.right() - w - 12.0,
                    r.y + 8.0,
                    &it.hint,
                    12.0,
                    if self.hover == Some(i) {
                        Rgba::WHITE
                    } else {
                        theme::TEXT_DIM
                    },
                    false,
                );
            }
            if let Kind::Sub(_) = it.kind {
                let (cx, cy) = (r.right() - 14.0, r.y + r.h / 2.0);
                cv.line((cx - 2.0, cy - 4.0), (cx + 2.0, cy), 1.6, ink);
                cv.line((cx + 2.0, cy), (cx - 2.0, cy + 4.0), 1.6, ink);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu::new(vec![
            Item::action("copy", "Kopyala").with_hint("Ctrl+C"),
            Item::sep(),
            Item::check("lock", "Kilitle", true),
            Item::sub("Boyut", vec![Item::action("p50", "%50")]),
        ])
    }

    #[test]
    fn layout_stacks_rows_and_separators_are_not_selectable() {
        let m = menu();
        let (rows, size) = m.layout(|s| s.chars().count() as f32 * 7.0);
        assert_eq!(rows.len(), 4);
        assert!(rows[1].h < rows[0].h);
        assert!(rows[2].y >= rows[1].bottom());
        assert!(size.1 > rows[3].bottom());
        assert_eq!(m.item_at(&rows, Point::new(20.0, rows[0].y + 3.0)), Some(0));
        assert_eq!(m.item_at(&rows, Point::new(20.0, rows[1].y + 3.0)), None);
        assert_eq!(m.item_at(&rows, Point::new(-5.0, 5.0)), None);
    }

    #[test]
    fn draws_highlight_and_check() {
        let mut m = menu();
        m.hover = Some(0);
        let (rows, size) = m.layout(|s| s.chars().count() as f32 * 7.0);
        let mut pm = tiny_skia::Pixmap::new(size.0.ceil() as u32, size.1.ceil() as u32).unwrap();
        m.draw(&mut Canvas::new(&mut pm, 1.0), &rows, size);
        let blue = pm
            .pixels()
            .iter()
            .filter(|p| p.blue() > 200 && p.red() < 40)
            .count();
        assert!(blue > 500, "hovered row painted with the accent colour");
    }
}
