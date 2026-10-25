//! A small settings form: sections, labelled rows with check boxes, value cycles, steppers, shortcut capture and
//! a folder button. Pure layout/draw/hit testing with scrolling.

use crate::ui::{theme, Canvas};
use pinshot_core::{Point, Rect, Rgba};

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Section,
    Check(bool),
    /// Click cycles through the options.
    Cycle {
        options: Vec<String>,
        index: usize,
    },
    Stepper {
        value: i32,
        min: i32,
        max: i32,
        step: i32,
        suffix: String,
    },
    Hotkey(String),
    Folder(String),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: &'static str,
    pub label: String,
    pub control: Control,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Changed(&'static str),
    Browse(&'static str),
    /// Start capturing a shortcut for this row.
    Capture(&'static str),
}

pub struct Form {
    pub rows: Vec<Row>,
    pub hover: Option<usize>,
    pub capturing: Option<usize>,
    pub scroll: f32,
    pub width: f32,
    pub height: f32,
}

const ROW_H: f32 = 34.0;
const SECTION_H: f32 = 38.0;
const PAD: f32 = 16.0;

impl Form {
    pub fn new(rows: Vec<Row>, width: f32, height: f32) -> Form {
        Form {
            rows,
            hover: None,
            capturing: None,
            scroll: 0.0,
            width,
            height,
        }
    }

    fn row_rects(&self) -> Vec<Rect> {
        let mut y = PAD - self.scroll;
        self.rows
            .iter()
            .map(|r| {
                let h = if r.control == Control::Section {
                    SECTION_H
                } else {
                    ROW_H
                };
                let rect = Rect::new(PAD, y, self.width - 2.0 * PAD, h);
                y += h;
                rect
            })
            .collect()
    }

    pub fn content_height(&self) -> f32 {
        self.rows
            .iter()
            .map(|r| {
                if r.control == Control::Section {
                    SECTION_H
                } else {
                    ROW_H
                }
            })
            .sum::<f32>()
            + 2.0 * PAD
    }

    pub fn scroll_by(&mut self, d: f32) {
        let max = (self.content_height() - self.height).max(0.0);
        self.scroll = (self.scroll - d).clamp(0.0, max);
    }

    /// The control's rectangle inside its row.
    fn control_rect(&self, row: &Rect, c: &Control) -> Rect {
        match c {
            Control::Section => *row,
            Control::Check(_) => Rect::new(row.right() - 34.0, row.y + 6.0, 34.0, 22.0),
            Control::Folder(_) => Rect::new(row.x + 150.0, row.y + 3.0, row.w - 150.0, 28.0),
            _ => Rect::new(row.x + 190.0, row.y + 3.0, row.w - 190.0, 28.0),
        }
    }

    fn index_at(&self, p: Point) -> Option<usize> {
        let rects = self.row_rects();
        rects.iter().enumerate().position(|(i, r)| {
            let c = &self.rows[i].control;
            *c != Control::Section && self.control_rect(r, c).contains(p)
        })
    }

    pub fn set_hover(&mut self, p: Option<Point>) -> bool {
        let n = p.and_then(|p| self.index_at(p));
        let changed = n != self.hover;
        self.hover = n;
        changed
    }

    pub fn press(&mut self, p: Point) -> Option<Event> {
        let i = self.index_at(p)?;
        let rect = self.row_rects()[i];
        let id = self.rows[i].id;
        let cr = self.control_rect(&rect, &self.rows[i].control);
        match &mut self.rows[i].control {
            Control::Check(v) => {
                *v = !*v;
                Some(Event::Changed(id))
            }
            Control::Cycle { options, index } => {
                if options.is_empty() {
                    return None;
                }
                *index = (*index + 1) % options.len();
                Some(Event::Changed(id))
            }
            Control::Stepper {
                value,
                min,
                max,
                step,
                ..
            } => {
                let minus = p.x < cr.x + 34.0;
                let plus = p.x > cr.right() - 34.0;
                if minus {
                    *value = (*value - *step).max(*min);
                } else if plus {
                    *value = (*value + *step).min(*max);
                } else {
                    return None;
                }
                Some(Event::Changed(id))
            }
            Control::Hotkey(_) => {
                self.capturing = Some(i);
                Some(Event::Capture(id))
            }
            Control::Folder(_) => Some(Event::Browse(id)),
            Control::Section => None,
        }
    }

    /// Store a captured shortcut (or clear it with `None`) and leave capture mode.
    pub fn set_hotkey(&mut self, combo: Option<String>) -> Option<&'static str> {
        let i = self.capturing.take()?;
        if let Control::Hotkey(v) = &mut self.rows[i].control {
            if let Some(c) = combo {
                *v = c;
            }
            return Some(self.rows[i].id);
        }
        None
    }

    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn row_mut(&mut self, id: &str) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| r.id == id)
    }

    pub fn check(&self, id: &str) -> bool {
        matches!(self.row(id).map(|r| &r.control), Some(Control::Check(true)))
    }

    pub fn cycle(&self, id: &str) -> Option<(usize, &str)> {
        match self.row(id).map(|r| &r.control) {
            Some(Control::Cycle { options, index }) => Some((*index, options[*index].as_str())),
            _ => None,
        }
    }

    pub fn number(&self, id: &str) -> Option<i32> {
        match self.row(id).map(|r| &r.control) {
            Some(Control::Stepper { value, .. }) => Some(*value),
            _ => None,
        }
    }

    pub fn draw(&self, cv: &mut Canvas) {
        let rects = self.row_rects();
        for (i, (row, r)) in self.rows.iter().zip(&rects).enumerate() {
            if r.bottom() < 0.0 || r.y > self.height {
                continue;
            }
            if row.control == Control::Section {
                cv.text(r.x, r.y + 14.0, &row.label, 13.0, theme::ACCENT, true);
                cv.fill_rect(Rect::new(r.x, r.bottom() - 4.0, r.w, 1.0), theme::SEP);
                continue;
            }
            cv.text(r.x, r.y + 9.0, &row.label, 13.0, theme::TEXT, false);
            let c = self.control_rect(r, &row.control);
            let hov = self.hover == Some(i);
            let bg = if hov {
                theme::BTN_HOVER
            } else {
                Rgba::rgb(0x34, 0x34, 0x3a)
            };
            match &row.control {
                Control::Check(on) => {
                    cv.fill_rrect(
                        c,
                        11.0,
                        if *on {
                            theme::ACCENT
                        } else {
                            Rgba::rgb(0x55, 0x55, 0x5b)
                        },
                    );
                    let kx = if *on { c.right() - 11.0 } else { c.x + 11.0 };
                    cv.fill_circle(kx, c.y + 11.0, 8.5, Rgba::WHITE);
                }
                Control::Cycle { options, index } => {
                    cv.fill_rrect(c, 6.0, bg);
                    cv.text_centered(c, &options[*index], 12.0, Rgba::WHITE, false);
                }
                Control::Stepper { value, suffix, .. } => {
                    cv.fill_rrect(c, 6.0, Rgba::rgb(0x34, 0x34, 0x3a));
                    cv.text_centered(c, &format!("{value}{suffix}"), 12.0, Rgba::WHITE, false);
                    cv.text_centered(Rect::new(c.x, c.y, 34.0, c.h), "−", 16.0, Rgba::WHITE, true);
                    cv.text_centered(
                        Rect::new(c.right() - 34.0, c.y, 34.0, c.h),
                        "+",
                        16.0,
                        Rgba::WHITE,
                        true,
                    );
                }
                Control::Hotkey(v) => {
                    let capturing = self.capturing == Some(i);
                    cv.fill_rrect(c, 6.0, if capturing { theme::ACCENT } else { bg });
                    let t = if capturing {
                        pinshot_core::i18n::tr("Tuşlara basın… (Esc: iptal, Del: temizle)")
                    } else if v.is_empty() {
                        "—"
                    } else {
                        v.as_str()
                    };
                    cv.text_centered(c, t, 12.0, Rgba::WHITE, false);
                }
                Control::Folder(path) => {
                    cv.fill_rrect(c, 6.0, theme::FIELD_BG);
                    let max = c.w - 16.0;
                    let mut s = path.clone();
                    while s.chars().count() > 3 && cv.text_width(&s, 12.0, false) > max {
                        s.remove(0);
                    }
                    let shown = if s.len() < path.len() {
                        format!("…{s}")
                    } else {
                        s
                    };
                    cv.text(c.x + 8.0, c.y + 7.0, &shown, 12.0, Rgba::WHITE, false);
                }
                Control::Section => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form::new(
            vec![
                Row {
                    id: "sec",
                    label: "Kayıt".into(),
                    control: Control::Section,
                },
                Row {
                    id: "auto",
                    label: "Auto".into(),
                    control: Control::Check(false),
                },
                Row {
                    id: "fmt",
                    label: "Format".into(),
                    control: Control::Cycle {
                        options: vec!["png".into(), "jpg".into()],
                        index: 0,
                    },
                },
                Row {
                    id: "q",
                    label: "Quality".into(),
                    control: Control::Stepper {
                        value: 90,
                        min: 1,
                        max: 100,
                        step: 5,
                        suffix: "%".into(),
                    },
                },
                Row {
                    id: "hk",
                    label: "Capture".into(),
                    control: Control::Hotkey("Ctrl+Alt+A".into()),
                },
                Row {
                    id: "dir",
                    label: "Folder".into(),
                    control: Control::Folder("/home/u/Pictures".into()),
                },
            ],
            480.0,
            400.0,
        )
    }

    fn centre(f: &Form, id: &str) -> Point {
        let i = f.rows.iter().position(|r| r.id == id).unwrap();
        f.control_rect(&f.row_rects()[i], &f.rows[i].control)
            .center()
    }

    #[test]
    fn controls_react_to_clicks() {
        let mut f = form();
        assert_eq!(f.press(centre(&f, "auto")), Some(Event::Changed("auto")));
        assert!(f.check("auto"));
        assert_eq!(f.press(centre(&f, "fmt")), Some(Event::Changed("fmt")));
        assert_eq!(f.cycle("fmt"), Some((1, "jpg")));
        f.press(centre(&f, "fmt"));
        assert_eq!(f.cycle("fmt"), Some((0, "png")), "wraps around");
        let c = centre(&f, "q");
        assert_eq!(
            f.press(Point::new(c.x + 110.0, c.y)),
            Some(Event::Changed("q"))
        );
        assert_eq!(f.number("q"), Some(95));
        assert_eq!(
            f.press(Point::new(c.x - 110.0, c.y)),
            Some(Event::Changed("q"))
        );
        assert_eq!(
            f.press(Point::new(c.x - 110.0, c.y)),
            Some(Event::Changed("q"))
        );
        assert_eq!(f.number("q"), Some(85));
        assert_eq!(f.press(c), None, "the middle of a stepper does nothing");
        assert_eq!(f.press(centre(&f, "dir")), Some(Event::Browse("dir")));
        assert_eq!(f.press(Point::new(2.0, 2.0)), None);
    }

    #[test]
    fn shortcut_capture_stores_or_keeps_the_value() {
        let mut f = form();
        assert_eq!(f.press(centre(&f, "hk")), Some(Event::Capture("hk")));
        assert_eq!(f.set_hotkey(Some("Ctrl+Shift+X".into())), Some("hk"));
        assert!(matches!(&f.row("hk").unwrap().control, Control::Hotkey(v) if v == "Ctrl+Shift+X"));
        f.press(centre(&f, "hk"));
        assert_eq!(
            f.set_hotkey(None),
            Some("hk"),
            "cancelling keeps the old value"
        );
        assert!(matches!(&f.row("hk").unwrap().control, Control::Hotkey(v) if v == "Ctrl+Shift+X"));
        assert_eq!(
            f.set_hotkey(Some("F3".into())),
            None,
            "not capturing any more"
        );
    }

    #[test]
    fn scrolling_is_limited_to_the_content() {
        let mut f = Form::new(
            (0..30)
                .map(|_| Row {
                    id: "x",
                    label: "L".into(),
                    control: Control::Check(false),
                })
                .collect(),
            480.0,
            300.0,
        );
        f.scroll_by(-100.0);
        assert_eq!(f.scroll, 100.0);
        f.scroll_by(-1_000_000.0);
        assert_eq!(f.scroll, f.content_height() - 300.0);
        f.scroll_by(1_000_000.0);
        assert_eq!(f.scroll, 0.0);
        let mut small = form();
        small.scroll_by(-50.0);
        assert_eq!(small.scroll, 0.0, "nothing to scroll");
    }

    #[test]
    fn draws_every_control_type() {
        let f = form();
        let mut pm = tiny_skia::Pixmap::new(480, 400).unwrap();
        pm.fill(tiny_skia::Color::from_rgba8(0x2b, 0x2b, 0x2f, 255));
        f.draw(&mut Canvas::new(&mut pm, 1.0));
        assert!(
            pm.pixels()
                .iter()
                .filter(|p| p.alpha() == 255 && p.blue() > 200 && p.red() < 40)
                .count()
                > 20,
            "accent painted"
        );
    }
}
