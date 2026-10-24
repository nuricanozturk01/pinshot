//! The floating editor toolbar: a property row (colour, width, fill, dash, font size) above the tool row.
//! Pure layout + drawing + hit testing, so it is tested without a window.

use crate::ui::{icons, theme, Canvas};
use pinshot_core::annotate::Kind;
use pinshot_core::editor::Tool;
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};

pub const TOOLS: [(Tool, &str, &str); 11] = [
    (Tool::Select, "select", "Seç / Taşı (V)"),
    (Tool::Draw(Kind::Rect), "rect", "Dikdörtgen (R)"),
    (Tool::Draw(Kind::Ellipse), "ellipse", "Elips (E)"),
    (Tool::Draw(Kind::Line), "line", "Çizgi (L)"),
    (Tool::Draw(Kind::Arrow), "arrow", "Ok (A)"),
    (Tool::Draw(Kind::Pen), "pen", "Kalem (P)"),
    (Tool::Draw(Kind::Marker), "marker", "Fosforlu kalem (H)"),
    (Tool::Draw(Kind::Text), "text", "Metin (T)"),
    (Tool::Draw(Kind::Number), "number", "Numara (N)"),
    (Tool::Draw(Kind::Mosaic), "mosaic", "Mozaik (M)"),
    (Tool::Draw(Kind::Blur), "blur", "Bulanıklaştır (B)"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Undo,
    Redo,
    Delete,
    Pin,
    Ocr,
    Record,
    Save,
    Copy,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Tool(Tool),
    Color(Rgba),
    CustomColor,
    Width(u32),
    Fill(bool),
    Dash(bool),
    Font(u32),
    Act(Act),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Id {
    Tool(Tool),
    Swatch(usize),
    More,
    Slider,
    Fill,
    Dash,
    FontMinus,
    FontPlus,
    Act(Act),
}

struct Elem {
    id: Id,
    rect: Rect,
}

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const BTN: f32 = 34.0;
const GAP: f32 = 2.0;
const ROW_GAP: f32 = 6.0;
const PROPS_H: f32 = 30.0;
const SLIDER_W: f32 = 90.0;
pub const MAX_WIDTH: u32 = 20;

pub struct Toolbar {
    pub tool: Tool,
    /// Which controls the property row shows (`None`: row hidden).
    pub prop_kind: Option<Kind>,
    pub color: Rgba,
    pub width: u32,
    pub fill: bool,
    pub dashed: bool,
    pub font_px: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_delete: bool,
    pub hover: Option<usize>,
    dragging_slider: bool,
    elems: Vec<Elem>,
    size: (f32, f32),
}

fn shows(kind: Option<Kind>) -> (bool, bool, bool, bool, bool) {
    // (color, width, fill, dash, font)
    match kind {
        Some(Kind::Rect | Kind::Ellipse) => (true, true, true, true, false),
        Some(Kind::Line | Kind::Arrow) => (true, true, false, true, false),
        Some(Kind::Pen | Kind::Marker | Kind::Number) => (true, true, false, false, false),
        Some(Kind::Text) => (true, false, false, false, true),
        Some(Kind::Mosaic | Kind::Blur) => (false, true, false, false, false),
        None => (false, false, false, false, false),
    }
}

pub fn width_label(kind: Option<Kind>) -> &'static str {
    match kind {
        Some(Kind::Mosaic | Kind::Blur) => tr("Güç"),
        Some(Kind::Number) => tr("Boyut"),
        _ => tr("Kalınlık"),
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        let mut t = Toolbar {
            tool: Tool::Select,
            prop_kind: None,
            color: Rgba::rgb(0xff, 0x3b, 0x30),
            width: 3,
            fill: false,
            dashed: false,
            font_px: 28,
            can_undo: false,
            can_redo: false,
            can_delete: false,
            hover: None,
            dragging_slider: false,
            elems: Vec::new(),
            size: (0.0, 0.0),
        };
        t.relayout();
        t
    }
}

impl Toolbar {
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Recompute element rectangles; call after changing `prop_kind`.
    pub fn relayout(&mut self) {
        let mut e = Vec::new();
        let (color, width, fill, dash, font) = shows(self.prop_kind);
        let any_prop = color || width || fill || dash || font;
        let mut y = PAD_Y;
        let mut max_x = 0.0_f32;
        if any_prop {
            let mut x = PAD_X + 2.0;
            let cy = y;
            if color {
                for i in 0..theme::SWATCHES.len() {
                    e.push(Elem {
                        id: Id::Swatch(i),
                        rect: Rect::new(x, cy + 5.0, 20.0, 20.0),
                    });
                    x += 25.0;
                }
                e.push(Elem {
                    id: Id::More,
                    rect: Rect::new(x, cy + 1.0, 28.0, 28.0),
                });
                x += 28.0 + 10.0;
            }
            if width {
                x += 58.0; // label
                e.push(Elem {
                    id: Id::Slider,
                    rect: Rect::new(x, cy + 5.0, SLIDER_W, 20.0),
                });
                x += SLIDER_W + 34.0; // value text
            }
            if fill {
                e.push(Elem {
                    id: Id::Fill,
                    rect: Rect::new(x, cy, 30.0, PROPS_H),
                });
                x += 32.0;
            }
            if dash {
                e.push(Elem {
                    id: Id::Dash,
                    rect: Rect::new(x, cy, 30.0, PROPS_H),
                });
                x += 32.0;
            }
            if font {
                x += 36.0; // label
                e.push(Elem {
                    id: Id::FontMinus,
                    rect: Rect::new(x, cy + 1.0, 28.0, 28.0),
                });
                x += 28.0 + 50.0;
                e.push(Elem {
                    id: Id::FontPlus,
                    rect: Rect::new(x, cy + 1.0, 28.0, 28.0),
                });
                x += 28.0;
            }
            max_x = max_x.max(x);
            y += PROPS_H + ROW_GAP;
        }
        let mut x = PAD_X;
        for (tool, _, _) in TOOLS {
            e.push(Elem {
                id: Id::Tool(tool),
                rect: Rect::new(x, y, BTN, BTN),
            });
            x += BTN + GAP;
        }
        x += 6.0;
        for a in [Act::Undo, Act::Redo, Act::Delete] {
            e.push(Elem {
                id: Id::Act(a),
                rect: Rect::new(x, y, BTN, BTN),
            });
            x += BTN + GAP;
        }
        x += 6.0;
        for a in [Act::Pin, Act::Ocr, Act::Record, Act::Save] {
            e.push(Elem {
                id: Id::Act(a),
                rect: Rect::new(x, y, BTN, BTN),
            });
            x += BTN + GAP;
        }
        x += 6.0;
        for a in [Act::Cancel, Act::Copy] {
            e.push(Elem {
                id: Id::Act(a),
                rect: Rect::new(x, y, BTN, BTN),
            });
            x += BTN + GAP;
        }
        max_x = max_x.max(x - GAP);
        self.size = (max_x.max(x - GAP) + PAD_X, y + BTN + PAD_Y);
        self.elems = e;
    }

    fn index_at(&self, p: Point) -> Option<usize> {
        self.elems.iter().position(|e| e.rect.contains(p))
    }

    fn enabled(&self, id: Id) -> bool {
        match id {
            Id::Act(Act::Undo) => self.can_undo,
            Id::Act(Act::Redo) => self.can_redo,
            Id::Act(Act::Delete) => self.can_delete,
            _ => true,
        }
    }

    pub fn tooltip_at(&self, p: Point) -> Option<(&'static str, Rect)> {
        let i = self.index_at(p)?;
        let e = &self.elems[i];
        let text = match e.id {
            Id::Tool(t) => tr(TOOLS.iter().find(|(x, _, _)| *x == t).map(|x| x.2)?),
            Id::More => tr("Özel renk…"),
            Id::Fill => tr("Dolgu"),
            Id::Dash => tr("Kesik çizgi"),
            Id::Act(Act::Undo) => tr("Geri al (Ctrl+Z)"),
            Id::Act(Act::Redo) => tr("Yinele (Ctrl+Y)"),
            Id::Act(Act::Delete) => tr("Seçiliyi sil (Del)"),
            Id::Act(Act::Pin) => tr("Ekrana sabitle (F3 / Ctrl+P)"),
            Id::Act(Act::Ocr) => tr("Metni tanı – OCR (O)"),
            Id::Act(Act::Record) => tr("Bu alanı video olarak kaydet (MP4 / GIF)"),
            Id::Act(Act::Save) => tr("Kaydet… (Ctrl+S)"),
            Id::Act(Act::Cancel) => tr("İptal (Esc)"),
            Id::Act(Act::Copy) => tr("Panoya kopyala (Enter)"),
            _ => return None,
        };
        Some((text, e.rect))
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x < self.size.0 && p.y < self.size.1
    }

    pub fn set_hover(&mut self, p: Option<Point>) -> bool {
        let new = p.and_then(|p| self.index_at(p));
        let changed = new != self.hover;
        self.hover = new;
        changed
    }

    fn slider_value(&self, x: f32) -> u32 {
        let r = self
            .elems
            .iter()
            .find(|e| e.id == Id::Slider)
            .map(|e| e.rect)
            .unwrap_or_default();
        let t = ((x - r.x - 6.0) / (r.w - 12.0)).clamp(0.0, 1.0);
        1 + (t * (MAX_WIDTH - 1) as f32).round() as u32
    }

    pub fn press(&mut self, p: Point) -> Option<Event> {
        let i = self.index_at(p)?;
        let id = self.elems[i].id;
        if !self.enabled(id) {
            return None;
        }
        Some(match id {
            Id::Tool(t) => Event::Tool(t),
            Id::Swatch(i) => Event::Color(Rgba::parse(theme::SWATCHES[i]).expect("swatch colour")),
            Id::More => Event::CustomColor,
            Id::Slider => {
                self.dragging_slider = true;
                let v = self.slider_value(p.x);
                self.width = v;
                Event::Width(v)
            }
            Id::Fill => Event::Fill(!self.fill),
            Id::Dash => Event::Dash(!self.dashed),
            Id::FontMinus => Event::Font(self.font_px.saturating_sub(2).max(8)),
            Id::FontPlus => Event::Font((self.font_px + 2).min(200)),
            Id::Act(a) => Event::Act(a),
        })
    }

    pub fn drag(&mut self, p: Point) -> Option<Event> {
        if !self.dragging_slider {
            return None;
        }
        let v = self.slider_value(p.x);
        if v == self.width {
            return None;
        }
        self.width = v;
        Some(Event::Width(v))
    }

    pub fn release(&mut self) {
        self.dragging_slider = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging_slider
    }

    pub fn draw(&self, cv: &mut Canvas, origin: Point) {
        let body = Rect::new(origin.x, origin.y, self.size.0, self.size.1);
        cv.fill_rrect(body, 10.0, theme::BAR_BORDER);
        cv.fill_rrect(body.inflated(-1.0), 9.0, theme::BAR_BG);
        for (i, e) in self.elems.iter().enumerate() {
            let r = e.rect.translated(origin.x, origin.y);
            let hovered = self.hover == Some(i);
            let enabled = self.enabled(e.id);
            let ink = if enabled {
                Rgba::WHITE
            } else {
                theme::TEXT_DIM
            };
            match e.id {
                Id::Tool(t) => {
                    let name = TOOLS
                        .iter()
                        .find(|(x, _, _)| *x == t)
                        .map(|x| x.1)
                        .unwrap_or("select");
                    if self.tool == t {
                        cv.fill_rrect(r, 6.0, theme::ACCENT);
                    } else if hovered {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    icons::draw(cv, name, r.inflated(-6.0), ink);
                }
                Id::Act(a) => {
                    let (name, bg) = match a {
                        Act::Undo => ("undo", None),
                        Act::Redo => ("redo", None),
                        Act::Delete => ("trash", None),
                        Act::Pin => ("pin", None),
                        Act::Ocr => ("ocr", None),
                        Act::Record => ("record", None),
                        Act::Save => ("save", None),
                        Act::Cancel => ("close", None),
                        Act::Copy => (
                            "check",
                            Some(if hovered {
                                theme::OK_GREEN_HOVER
                            } else {
                                theme::OK_GREEN
                            }),
                        ),
                    };
                    if let Some(bg) = bg {
                        cv.fill_rrect(r, 6.0, bg);
                    } else if hovered && enabled {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    icons::draw(cv, name, r.inflated(-6.0), ink);
                }
                Id::Swatch(i) => {
                    let c = Rgba::parse(theme::SWATCHES[i]).expect("swatch");
                    let on = c == self.color;
                    let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                    cv.fill_circle(
                        cx,
                        cy,
                        10.0,
                        if on {
                            Rgba::WHITE
                        } else {
                            Rgba::rgb(0x55, 0x55, 0x5b)
                        },
                    );
                    cv.fill_circle(cx, cy, if on { 7.0 } else { 9.0 }, c);
                }
                Id::More => {
                    if hovered {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    icons::draw(cv, "plus", r.inflated(-5.0), Rgba::WHITE);
                }
                Id::Slider => {
                    let cy = r.y + r.h / 2.0;
                    cv.line(
                        (r.x + 6.0, cy),
                        (r.right() - 6.0, cy),
                        4.0,
                        Rgba::rgb(0x4a, 0x4a, 0x50),
                    );
                    let t = (self.width.clamp(1, MAX_WIDTH) - 1) as f32 / (MAX_WIDTH - 1) as f32;
                    let hx = r.x + 6.0 + t * (r.w - 12.0);
                    cv.fill_circle(hx, cy, 6.0, Rgba::WHITE);
                    cv.text(
                        r.x - 58.0,
                        r.y + 2.0,
                        width_label(self.prop_kind),
                        12.0,
                        theme::TEXT,
                        false,
                    );
                    cv.text(
                        r.right() + 8.0,
                        r.y + 2.0,
                        &self.width.to_string(),
                        12.0,
                        theme::TEXT,
                        false,
                    );
                }
                Id::Fill | Id::Dash => {
                    let on = if e.id == Id::Fill {
                        self.fill
                    } else {
                        self.dashed
                    };
                    let r = Rect::new(r.x, r.y + 1.0, r.w, r.h - 2.0);
                    if on {
                        cv.fill_rrect(r, 6.0, theme::ACCENT);
                    } else if hovered {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    icons::draw(
                        cv,
                        if e.id == Id::Fill { "fill" } else { "dash" },
                        r.inflated(-5.0),
                        Rgba::WHITE,
                    );
                }
                Id::FontMinus | Id::FontPlus => {
                    if hovered {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    icons::draw(
                        cv,
                        if e.id == Id::FontMinus {
                            "minus"
                        } else {
                            "plus"
                        },
                        r.inflated(-5.0),
                        Rgba::WHITE,
                    );
                    if e.id == Id::FontMinus {
                        let box_ = Rect::new(r.right() + 4.0, r.y, 42.0, r.h);
                        cv.fill_rrect(box_, 4.0, theme::FIELD_BG);
                        cv.text_centered(
                            box_,
                            &format!("{}", self.font_px),
                            12.0,
                            Rgba::WHITE,
                            false,
                        );
                        cv.text(r.x - 36.0, r.y + 6.0, tr("Yazı"), 12.0, theme::TEXT, false);
                    }
                }
            }
        }
        // separators in the tool row: after the tools, after delete, after save
        let row: Vec<&Elem> = self
            .elems
            .iter()
            .filter(|e| e.rect.y == tool_row_y(&self.elems))
            .collect();
        for idx in [10, 13, 17] {
            if let Some(prev) = row.get(idx) {
                cv.fill_rect(
                    Rect::new(
                        origin.x + prev.rect.right() + 3.0,
                        origin.y + prev.rect.y + 6.0,
                        1.0,
                        22.0,
                    ),
                    theme::SEP,
                );
            }
        }
    }
}

fn tool_row_y(elems: &[Elem]) -> f32 {
    elems
        .iter()
        .find(|e| matches!(e.id, Id::Tool(_)))
        .map(|e| e.rect.y)
        .unwrap_or(0.0)
}

/// Where to put the toolbar for a selection: below its bottom-right, above if there is no room, inside as a last
/// resort. `area` is the window; everything is in the same coordinates.
pub fn place(sel: Rect, size: (f32, f32), area: Rect) -> Point {
    let (w, h) = size;
    let x = (sel.right() - w)
        .min(area.right() - w - 8.0)
        .max(area.x + 8.0);
    let mut y = sel.bottom() + 10.0;
    if y + h > area.bottom() - 8.0 {
        y = sel.y - h - 10.0;
    }
    if y < area.y + 8.0 {
        y = (sel.bottom() - h - 10.0).max(area.y + 8.0);
    }
    Point::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(t: &Toolbar, id: Id) -> Point {
        t.elems.iter().find(|e| e.id == id).unwrap().rect.center()
    }

    #[test]
    fn elements_do_not_overlap_and_fit_inside() {
        for kind in [
            None,
            Some(Kind::Rect),
            Some(Kind::Line),
            Some(Kind::Pen),
            Some(Kind::Text),
            Some(Kind::Blur),
            Some(Kind::Number),
        ] {
            let mut t = Toolbar::default();
            t.prop_kind = kind;
            t.relayout();
            let (w, h) = t.size();
            for (i, a) in t.elems.iter().enumerate() {
                assert!(
                    a.rect.x >= 0.0 && a.rect.right() <= w && a.rect.bottom() <= h,
                    "{kind:?} elem {i} outside"
                );
                for b in t.elems.iter().skip(i + 1) {
                    assert!(
                        !a.rect.intersects(&b.rect),
                        "{kind:?}: {:?} overlaps {:?}",
                        a.id,
                        b.id
                    );
                }
            }
        }
    }

    #[test]
    fn property_row_only_appears_when_needed() {
        let mut t = Toolbar::default();
        let h0 = t.size().1;
        t.prop_kind = Some(Kind::Rect);
        t.relayout();
        assert!(t.size().1 > h0 + 20.0);
        assert!(t.elems.iter().any(|e| e.id == Id::Fill));
        t.prop_kind = Some(Kind::Mosaic);
        t.relayout();
        assert!(t.elems.iter().any(|e| e.id == Id::Slider));
        assert!(!t
            .elems
            .iter()
            .any(|e| matches!(e.id, Id::Swatch(_) | Id::Fill)));
    }

    #[test]
    fn clicks_map_to_events() {
        let mut t = Toolbar::default();
        t.prop_kind = Some(Kind::Text);
        t.relayout();
        t.font_px = 28;
        let c = centre(&t, Id::Tool(Tool::Draw(Kind::Arrow)));
        assert_eq!(t.press(c), Some(Event::Tool(Tool::Draw(Kind::Arrow))));
        assert_eq!(t.press(centre(&t, Id::FontPlus)), Some(Event::Font(30)));
        assert_eq!(t.press(centre(&t, Id::FontMinus)), Some(Event::Font(26)));
        assert_eq!(
            t.press(centre(&t, Id::Swatch(4))),
            Some(Event::Color(Rgba::rgb(0x0a, 0x84, 0xff)))
        );
        assert_eq!(
            t.press(centre(&t, Id::Act(Act::Copy))),
            Some(Event::Act(Act::Copy))
        );
        assert_eq!(t.press(Point::new(-5.0, -5.0)), None);
    }

    #[test]
    fn disabled_buttons_ignore_clicks_until_enabled() {
        let mut t = Toolbar::default();
        let u = centre(&t, Id::Act(Act::Undo));
        assert_eq!(t.press(u), None);
        t.can_undo = true;
        assert_eq!(t.press(u), Some(Event::Act(Act::Undo)));
    }

    #[test]
    fn slider_maps_position_to_value_and_drags() {
        let mut t = Toolbar::default();
        t.prop_kind = Some(Kind::Pen);
        t.relayout();
        let r = t.elems.iter().find(|e| e.id == Id::Slider).unwrap().rect;
        assert_eq!(
            t.press(Point::new(r.x + 6.0, r.center().y)),
            Some(Event::Width(1))
        );
        assert!(t.is_dragging());
        assert_eq!(
            t.drag(Point::new(r.right() + 50.0, r.center().y)),
            Some(Event::Width(MAX_WIDTH))
        );
        assert_eq!(
            t.drag(Point::new(r.right() + 80.0, r.center().y)),
            None,
            "no change, no event"
        );
        t.release();
        assert_eq!(t.drag(Point::new(r.x, r.y)), None);
    }

    #[test]
    fn toggles_flip_current_state_and_font_is_clamped() {
        let mut t = Toolbar::default();
        t.prop_kind = Some(Kind::Rect);
        t.relayout();
        t.fill = true;
        assert_eq!(t.press(centre(&t, Id::Fill)), Some(Event::Fill(false)));
        assert_eq!(t.press(centre(&t, Id::Dash)), Some(Event::Dash(true)));
        t.prop_kind = Some(Kind::Text);
        t.relayout();
        t.font_px = 8;
        assert_eq!(t.press(centre(&t, Id::FontMinus)), Some(Event::Font(8)));
        t.font_px = 200;
        assert_eq!(t.press(centre(&t, Id::FontPlus)), Some(Event::Font(200)));
    }

    #[test]
    fn placement_prefers_below_then_above_then_inside() {
        let area = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let size = (700.0, 80.0);
        let below = place(Rect::new(100.0, 100.0, 300.0, 200.0), size, area);
        assert_eq!(below.y, 310.0);
        assert!(below.x >= 8.0);
        let above = place(Rect::new(100.0, 500.0, 300.0, 280.0), size, area);
        assert!(above.y + 80.0 <= 500.0);
        let inside = place(Rect::new(0.0, 0.0, 1000.0, 800.0), size, area);
        assert!(inside.y >= 8.0 && inside.y + 80.0 <= 800.0);
        let right_edge = place(Rect::new(800.0, 100.0, 190.0, 100.0), size, area);
        assert!(right_edge.x + 700.0 <= 992.0);
    }

    #[test]
    fn drawing_does_not_panic_and_paints_the_active_tool() {
        let mut t = Toolbar::default();
        t.prop_kind = Some(Kind::Rect);
        t.relayout();
        t.tool = Tool::Draw(Kind::Rect);
        t.hover = Some(0);
        let mut pm = tiny_skia::Pixmap::new(800, 120).unwrap();
        t.draw(&mut Canvas::new(&mut pm, 1.0), Point::new(10.0, 10.0));
        let blue = pm
            .pixels()
            .iter()
            .filter(|p| p.blue() > 200 && p.red() < 40)
            .count();
        assert!(blue > 300, "active tool highlighted");
    }
}
