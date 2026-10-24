//! The annotation model: shapes, hit testing, editing handles and shift-constraints.
//!
//! Coordinates are logical desktop coordinates (the same space as the selection).

use crate::color::Rgba;
use crate::geom::{Point, Rect};
use crate::text::TextMeasure;
use std::sync::atomic::{AtomicU64, Ordering};

pub const HIT_TOLERANCE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rect,
    Ellipse,
    Line,
    Arrow,
    Pen,
    Marker,
    Text,
    Number,
    Mosaic,
    Blur,
}

impl Kind {
    pub fn is_two_point(self) -> bool {
        matches!(self, Kind::Rect | Kind::Ellipse | Kind::Mosaic | Kind::Blur)
    }

    pub fn is_line_like(self) -> bool {
        matches!(self, Kind::Line | Kind::Arrow)
    }

    pub fn is_freehand(self) -> bool {
        matches!(self, Kind::Pen | Kind::Marker)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub color: Rgba,
    pub width: u32,
    pub pts: Vec<Point>,
    pub text: String,
    pub fill: bool,
    pub dashed: bool,
    pub font_px: u32,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Item {
    pub fn new(kind: Kind, color: Rgba, width: u32, pts: Vec<Point>) -> Self {
        Item {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            kind,
            color,
            width,
            pts,
            text: String::new(),
            fill: false,
            dashed: false,
            font_px: 28,
        }
    }

    pub fn first(&self) -> Point {
        self.pts[0]
    }

    pub fn last(&self) -> Point {
        *self.pts.last().expect("an item always has points")
    }

    /// Rectangle spanned by the first and last point.
    pub fn rect(&self) -> Rect {
        Rect::from_points(self.first(), self.last())
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for p in &mut self.pts {
            p.x += dx;
            p.y += dy;
        }
    }

    pub fn number_radius(&self) -> f32 {
        9.0 + self.width as f32 * 2.0
    }

    pub fn text_rect(&self, m: &dyn TextMeasure) -> Rect {
        let (w, h) = m.measure(
            if self.text.is_empty() {
                " "
            } else {
                &self.text
            },
            self.font_px.max(6) as f32,
            false,
        );
        Rect::new(self.first().x, self.first().y, w.max(4.0), h.max(4.0))
    }

    /// Bounding box including stroke width and arrow heads (used for the selection outline).
    pub fn bounds(&self, m: &dyn TextMeasure) -> Rect {
        let pad = self.width as f32 / 2.0 + 2.0;
        match self.kind {
            k if k.is_two_point() => self.rect().inflated(pad),
            k if k.is_line_like() => {
                let extra = if self.kind == Kind::Arrow {
                    4.0 + self.width as f32 * 2.0
                } else {
                    0.0
                };
                self.rect().inflated(pad + extra)
            }
            k if k.is_freehand() => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &self.pts {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                let pad =
                    self.width as f32 * if self.kind == Kind::Marker { 2.0 } else { 0.5 } + 2.0;
                Rect::new(x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad)
            }
            Kind::Text => self.text_rect(m).inflated(3.0),
            _ => {
                let r = self.number_radius();
                Rect::new(self.first().x - r, self.first().y - r, 2.0 * r, 2.0 * r)
            }
        }
    }
}

fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    p.distance(Point::new(a.x + t * dx, a.y + t * dy))
}

/// Does a click at `pt` hit the item? Unfilled outlines are hit only near their edge.
pub fn hit_test(it: &Item, pt: Point, tol: f32, m: &dyn TextMeasure) -> bool {
    match it.kind {
        Kind::Rect | Kind::Mosaic | Kind::Blur | Kind::Ellipse => {
            let r = it.rect();
            let outer = r.inflated(tol);
            if matches!(it.kind, Kind::Mosaic | Kind::Blur) || it.fill {
                return outer.contains(pt);
            }
            if !outer.contains(pt) {
                return false;
            }
            if it.kind == Kind::Rect {
                let inner = Rect::new(r.x + tol, r.y + tol, r.w - 2.0 * tol, r.h - 2.0 * tol);
                return !(inner.w > 0.0 && inner.h > 0.0 && inner.contains(pt));
            }
            let c = r.center();
            let (rx, ry) = ((r.w / 2.0).max(1e-6), (r.h / 2.0).max(1e-6));
            let d = (((pt.x - c.x) / rx).powi(2) + ((pt.y - c.y) / ry).powi(2)).sqrt();
            (d - 1.0).abs() * rx.min(ry) <= tol
        }
        Kind::Line | Kind::Arrow => {
            segment_distance(pt, it.first(), it.last()) <= tol + it.width as f32 / 2.0
        }
        Kind::Pen | Kind::Marker => {
            let lim = tol + it.width as f32 * if it.kind == Kind::Marker { 2.0 } else { 0.5 };
            if it.pts.len() == 1 {
                return pt.distance(it.pts[0]) <= lim;
            }
            it.pts
                .windows(2)
                .any(|w| segment_distance(pt, w[0], w[1]) <= lim)
        }
        _ => it.bounds(m).contains(pt),
    }
}

/// Which points of an item a handle moves: (point index owning x, point index owning y).
pub type Handle = (Option<usize>, Option<usize>);

/// Editable handles: 4 corners for rectangle-like items, the two ends for lines.
pub fn handles(it: &Item) -> Vec<(Point, Handle)> {
    let last = it.pts.len() - 1;
    if it.kind.is_two_point() {
        let (a, b) = (it.first(), it.last());
        let (xl, xr) = if a.x <= b.x { (0, last) } else { (last, 0) };
        let (yt, yb) = if a.y <= b.y { (0, last) } else { (last, 0) };
        let r = it.rect();
        vec![
            (Point::new(r.x, r.y), (Some(xl), Some(yt))),
            (Point::new(r.right(), r.y), (Some(xr), Some(yt))),
            (Point::new(r.x, r.bottom()), (Some(xl), Some(yb))),
            (Point::new(r.right(), r.bottom()), (Some(xr), Some(yb))),
        ]
    } else if it.kind.is_line_like() {
        vec![
            (it.first(), (Some(0), Some(0))),
            (it.last(), (Some(last), Some(last))),
        ]
    } else {
        Vec::new()
    }
}

/// Move the handle to `pt`; with `square` the shape stays square (shift key).
pub fn drag_handle(it: &mut Item, spec: Handle, pt: Point, square: bool) {
    let last = it.pts.len() - 1;
    let mut pt = pt;
    if it.kind.is_two_point() && square {
        if let (Some(xi), Some(yi)) = spec {
            let ox = it.pts[if xi == 0 { last } else { 0 }].x;
            let oy = it.pts[if yi == 0 { last } else { 0 }].y;
            let side = (pt.x - ox).abs().max((pt.y - oy).abs());
            let sx = if pt.x - ox >= 0.0 { 1.0 } else { -1.0 };
            let sy = if pt.y - oy >= 0.0 { 1.0 } else { -1.0 };
            pt = Point::new(ox + sx * side, oy + sy * side);
        }
    }
    if let Some(xi) = spec.0 {
        it.pts[xi].x = pt.x;
    }
    if let Some(yi) = spec.1 {
        it.pts[yi].y = pt.y;
    }
}

/// Shift-key behaviour: squares for two-point shapes, 15° steps for lines.
pub fn constrain(kind: Kind, start: Point, pt: Point) -> Point {
    let (dx, dy) = (pt.x - start.x, pt.y - start.y);
    if kind.is_two_point() {
        let side = dx.abs().max(dy.abs());
        let sx = if dx >= 0.0 { 1.0 } else { -1.0 };
        let sy = if dy >= 0.0 { 1.0 } else { -1.0 };
        Point::new(start.x + sx * side, start.y + sy * side)
    } else if kind.is_line_like() {
        let step = std::f32::consts::PI / 12.0;
        let ang = (dy.atan2(dx) / step).round() * step;
        let len = dx.hypot(dy);
        Point::new(start.x + len * ang.cos(), start.y + len * ang.sin())
    } else {
        pt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-size fake: every character is 10 px wide, lines are 20 px tall.
    pub struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, text: &str, _px: f32, _bold: bool) -> (f32, f32) {
            let lines: Vec<&str> = text.split('\n').collect();
            (
                lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * 10.0,
                lines.len() as f32 * 20.0,
            )
        }
    }

    fn rect_item(fill: bool) -> Item {
        let mut it = Item::new(
            Kind::Rect,
            Rgba::rgb(255, 0, 0),
            3,
            vec![Point::new(0.0, 0.0), Point::new(100.0, 100.0)],
        );
        it.fill = fill;
        it
    }

    #[test]
    fn rectangle_is_hit_on_its_edge_only_unless_filled() {
        let mut r = rect_item(false);
        assert!(hit_test(&r, Point::new(0.0, 50.0), HIT_TOLERANCE, &Fake));
        assert!(!hit_test(&r, Point::new(50.0, 50.0), HIT_TOLERANCE, &Fake));
        r.fill = true;
        assert!(hit_test(&r, Point::new(50.0, 50.0), HIT_TOLERANCE, &Fake));
    }

    #[test]
    fn ellipse_and_line_hit_testing() {
        let e = Item::new(
            Kind::Ellipse,
            Rgba::BLACK,
            3,
            vec![Point::new(0.0, 0.0), Point::new(100.0, 100.0)],
        );
        assert!(hit_test(&e, Point::new(50.0, 0.0), HIT_TOLERANCE, &Fake));
        assert!(!hit_test(&e, Point::new(50.0, 50.0), HIT_TOLERANCE, &Fake));
        let l = Item::new(
            Kind::Line,
            Rgba::BLACK,
            3,
            vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
        );
        assert!(hit_test(&l, Point::new(50.0, 4.0), HIT_TOLERANCE, &Fake));
        assert!(!hit_test(&l, Point::new(50.0, 30.0), HIT_TOLERANCE, &Fake));
    }

    #[test]
    fn freehand_and_text_hit_testing() {
        let pen = Item::new(
            Kind::Pen,
            Rgba::BLACK,
            4,
            vec![
                Point::new(0.0, 0.0),
                Point::new(50.0, 50.0),
                Point::new(100.0, 0.0),
            ],
        );
        assert!(hit_test(&pen, Point::new(25.0, 25.0), HIT_TOLERANCE, &Fake));
        assert!(!hit_test(&pen, Point::new(50.0, 0.0), HIT_TOLERANCE, &Fake));
        let mut t = Item::new(Kind::Text, Rgba::BLACK, 3, vec![Point::new(10.0, 10.0)]);
        t.text = "abc".into();
        assert!(hit_test(&t, Point::new(20.0, 20.0), HIT_TOLERANCE, &Fake));
        assert!(!hit_test(&t, Point::new(200.0, 20.0), HIT_TOLERANCE, &Fake));
    }

    #[test]
    fn corner_handles_resize_without_flipping_ownership() {
        let mut r = rect_item(false);
        let hs = handles(&r);
        assert_eq!(hs.len(), 4);
        let (pos, spec) = hs[3]; // bottom-right
        assert_eq!(pos, Point::new(100.0, 100.0));
        drag_handle(&mut r, spec, Point::new(160.0, 130.0), false);
        assert_eq!(r.rect(), Rect::new(0.0, 0.0, 160.0, 130.0));
        let (_, top_left) = handles(&r)[0];
        drag_handle(&mut r, top_left, Point::new(10.0, 20.0), false);
        assert_eq!(r.rect(), Rect::new(10.0, 20.0, 150.0, 110.0));
    }

    #[test]
    fn line_has_two_end_handles_and_square_drag_keeps_aspect() {
        let mut l = Item::new(
            Kind::Line,
            Rgba::BLACK,
            3,
            vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
        );
        assert_eq!(handles(&l).len(), 2);
        let end = handles(&l)[1].1;
        drag_handle(&mut l, end, Point::new(40.0, 5.0), false);
        assert_eq!(l.last(), Point::new(40.0, 5.0));
        let mut r = rect_item(false);
        let (_, br) = handles(&r)[3];
        drag_handle(&mut r, br, Point::new(150.0, 120.0), true);
        assert_eq!(r.rect().w, r.rect().h);
    }

    #[test]
    fn shift_constraints() {
        let p = constrain(Kind::Rect, Point::new(0.0, 0.0), Point::new(30.0, -10.0));
        assert_eq!(p, Point::new(30.0, -30.0));
        let q = constrain(Kind::Line, Point::new(0.0, 0.0), Point::new(100.0, 8.0));
        assert!(q.y.abs() < 1e-3, "nearly horizontal snaps to horizontal");
        assert_eq!(
            constrain(Kind::Pen, Point::new(0.0, 0.0), Point::new(3.0, 7.0)),
            Point::new(3.0, 7.0)
        );
    }

    #[test]
    fn bounds_cover_arrow_heads_and_text() {
        let a = Item::new(
            Kind::Arrow,
            Rgba::BLACK,
            4,
            vec![Point::new(0.0, 0.0), Point::new(50.0, 0.0)],
        );
        assert!(
            a.bounds(&Fake).h
                > Item::new(Kind::Line, Rgba::BLACK, 4, a.pts.clone())
                    .bounds(&Fake)
                    .h
        );
        let mut t = Item::new(Kind::Text, Rgba::BLACK, 3, vec![Point::new(5.0, 5.0)]);
        t.text = "ab\ncd".into();
        let b = t.bounds(&Fake);
        assert_eq!((b.w, b.h), (20.0 + 6.0, 40.0 + 6.0));
    }

    #[test]
    fn ids_are_unique() {
        let a = rect_item(false);
        let b = rect_item(false);
        assert_ne!(a.id, b.id);
    }
}
