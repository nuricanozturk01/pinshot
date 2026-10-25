//! Pinned-image geometry and image transforms (no window system involved).

use crate::geom::{Point, Rect};
use tiny_skia::Pixmap;

pub const EDGE: f32 = 8.0;
pub const MIN_SIDE: f32 = 40.0;
pub const MAX_SIDE: f32 = 16000.0;

/// Which edges a point close to the border grabs: any of `l r t b`.
pub fn edge_at(p: Point, w: f32, h: f32) -> String {
    let mut s = String::new();
    if p.y < EDGE {
        s.push('t');
    } else if p.y > h - EDGE {
        s.push('b');
    }
    if p.x < EDGE {
        s.push('l');
    } else if p.x > w - EDGE {
        s.push('r');
    }
    s
}

/// Size after zooming by `factor` about `anchor` (desktop coordinates); returns the new rectangle.
pub fn zoom_about(g: Rect, factor: f32, anchor: Point) -> Rect {
    let w = (g.w * factor).round().clamp(MIN_SIDE, MAX_SIDE);
    let h = (g.h * factor).round().clamp(MIN_SIDE, MAX_SIDE);
    let fx = (anchor.x - g.x) / g.w.max(1.0);
    let fy = (anchor.y - g.y) / g.h.max(1.0);
    Rect::new(
        (anchor.x - fx * w).round(),
        (anchor.y - fy * h).round(),
        w,
        h,
    )
}

/// Rectangle for `pct` percent of the natural size, keeping the centre.
pub fn percent_rect(g: Rect, natural: (f32, f32), pct: f32) -> Rect {
    let c = g.center();
    let (w, h) = (
        (natural.0 * pct / 100.0).round().max(MIN_SIDE),
        (natural.1 * pct / 100.0).round().max(MIN_SIDE),
    );
    Rect::new((c.x - w / 2.0).round(), (c.y - h / 2.0).round(), w, h)
}

/// Resize by dragging edge/corner `edge` by `d` from the original rectangle `g0`. Aspect ratio is kept unless `free`.
pub fn resize_drag(g0: Rect, edge: &str, d: Point, free: bool) -> Rect {
    let (mut l, mut t, mut r, mut b) = (g0.x, g0.y, g0.right(), g0.bottom());
    if edge.contains('l') {
        l = (g0.x + d.x).min(r - MIN_SIDE);
    }
    if edge.contains('r') {
        r = (g0.right() + d.x).max(l + MIN_SIDE);
    }
    if edge.contains('t') {
        t = (g0.y + d.y).min(b - MIN_SIDE);
    }
    if edge.contains('b') {
        b = (g0.bottom() + d.y).max(t + MIN_SIDE);
    }
    let mut rect = Rect::new(l, t, r - l, b - t);
    if !free {
        if edge.len() == 2 {
            let s = (rect.w / g0.w).max(rect.h / g0.h);
            let (w, h) = ((g0.w * s).round(), (g0.h * s).round());
            let x = if edge.contains('l') {
                g0.right() - w
            } else {
                g0.x
            };
            let y = if edge.contains('t') {
                g0.bottom() - h
            } else {
                g0.y
            };
            rect = Rect::new(x, y, w, h);
        } else if edge == "l" || edge == "r" {
            let h = (g0.h * rect.w / g0.w).round();
            rect = Rect::new(rect.x, (g0.center().y - h / 2.0).round(), rect.w, h);
        } else if edge == "t" || edge == "b" {
            let w = (g0.w * rect.h / g0.h).round();
            rect = Rect::new((g0.center().x - w / 2.0).round(), rect.y, w, rect.h);
        }
    }
    rect
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    FlipH,
    FlipV,
    RotateCw,
    RotateCcw,
}

pub fn transform(src: &Pixmap, t: Transform) -> Pixmap {
    let (w, h) = (src.width(), src.height());
    let (nw, nh) = match t {
        Transform::RotateCw | Transform::RotateCcw => (h, w),
        _ => (w, h),
    };
    let mut out = Pixmap::new(nw, nh).expect("same area");
    let s = src.data();
    let d = out.data_mut();
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match t {
                Transform::FlipH => (w - 1 - x, y),
                Transform::FlipV => (x, h - 1 - y),
                Transform::RotateCw => (h - 1 - y, x),
                Transform::RotateCcw => (y, w - 1 - x),
            };
            let si = ((y * w + x) * 4) as usize;
            let di = ((ny * nw + nx) * 4) as usize;
            d[di..di + 4].copy_from_slice(&s[si..si + 4]);
        }
    }
    out
}

/// Render plain text onto a light card (for pinning clipboard text), wrapped at about 560 px.
pub fn render_text(text: &str) -> Pixmap {
    use crate::text::Fonts;
    let fonts = Fonts::embedded();
    let px = 15.0 * 2.0;
    let max_w = 560.0 * 2.0;
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let mut cur = String::new();
        for word in raw.split(' ') {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if fonts.line_width(&cand, px, false) <= max_w || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            }
        }
        lines.push(cur);
    }
    let lh = fonts.line_height(px, false);
    let w = lines
        .iter()
        .map(|l| fonts.line_width(l, px, false))
        .fold(40.0_f32, f32::max)
        + 56.0;
    let h = lh * lines.len() as f32 + 48.0;
    let mut pm = Pixmap::new(w.ceil() as u32, h.ceil() as u32).expect("text card");
    pm.fill(tiny_skia::Color::from_rgba8(0xfa, 0xfa, 0xfa, 255));
    for (i, l) in lines.iter().enumerate() {
        let base = 24.0 + fonts.ascent(px, false) + lh * i as f32;
        let mut pen = 28.0;
        for ch in l.chars() {
            let g = fonts.glyph(ch, px, false);
            crate::render::blit_glyph(
                &mut pm,
                &g,
                (pen + g.xmin as f32).round(),
                (base - g.ymin as f32 - g.height as f32).round(),
                crate::Rgba::rgb(0x1c, 0x1c, 0x1e),
            );
            pen += g.advance;
        }
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_detected_at_borders_and_corners() {
        assert_eq!(edge_at(Point::new(100.0, 100.0), 200.0, 200.0), "");
        assert_eq!(edge_at(Point::new(2.0, 100.0), 200.0, 200.0), "l");
        assert_eq!(edge_at(Point::new(198.0, 100.0), 200.0, 200.0), "r");
        assert_eq!(edge_at(Point::new(100.0, 3.0), 200.0, 200.0), "t");
        assert_eq!(edge_at(Point::new(199.0, 199.0), 200.0, 200.0), "br");
    }

    #[test]
    fn zoom_keeps_the_anchor_point_fixed() {
        let g = Rect::new(100.0, 100.0, 200.0, 100.0);
        let z = zoom_about(g, 2.0, Point::new(200.0, 150.0));
        assert_eq!((z.w, z.h), (400.0, 200.0));
        assert_eq!(
            (z.x, z.y),
            (0.0, 50.0),
            "the centre stays under the pointer"
        );
        let small = zoom_about(g, 0.01, Point::new(100.0, 100.0));
        assert_eq!((small.w, small.h), (MIN_SIDE, MIN_SIDE));
        let big = zoom_about(g, 1000.0, g.center());
        assert_eq!(big.w, MAX_SIDE);
    }

    #[test]
    fn percent_keeps_the_centre() {
        let g = Rect::new(100.0, 100.0, 200.0, 100.0);
        let p = percent_rect(g, (200.0, 100.0), 50.0);
        assert_eq!((p.w, p.h), (100.0, 50.0));
        assert_eq!(p.center(), g.center());
    }

    #[test]
    fn corner_resize_keeps_aspect_and_free_resize_does_not() {
        let g0 = Rect::new(100.0, 100.0, 200.0, 100.0);
        let r = resize_drag(g0, "br", Point::new(100.0, 10.0), false);
        assert_eq!((r.w / r.h * 100.0).round(), 200.0);
        assert_eq!((r.x, r.y), (100.0, 100.0));
        let tl = resize_drag(g0, "tl", Point::new(-50.0, -50.0), false);
        assert_eq!(tl.right(), g0.right());
        assert_eq!(tl.bottom(), g0.bottom());
        let free = resize_drag(g0, "r", Point::new(50.0, 30.0), true);
        assert_eq!((free.w, free.h), (250.0, 100.0));
        let side = resize_drag(g0, "r", Point::new(100.0, 0.0), false);
        assert_eq!((side.w, side.h), (300.0, 150.0));
        assert_eq!(side.center().y, g0.center().y);
        let tiny = resize_drag(g0, "br", Point::new(-5000.0, -5000.0), true);
        assert_eq!((tiny.w, tiny.h), (MIN_SIDE, MIN_SIDE));
    }

    fn two_by_three() -> Pixmap {
        let mut pm = Pixmap::new(2, 3).unwrap();
        for (i, p) in pm.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
            p.copy_from_slice(&[i as u8 * 10, 0, 0, 255]);
        }
        pm
    }

    #[test]
    fn transforms_move_pixels_correctly_and_invert_each_other() {
        let src = two_by_three(); // values: row0: 0 10 / row1: 20 30 / row2: 40 50
        let red = |pm: &Pixmap, x, y| pm.pixel(x, y).unwrap().red();
        let h = transform(&src, Transform::FlipH);
        assert_eq!((red(&h, 0, 0), red(&h, 1, 0)), (10, 0));
        let v = transform(&src, Transform::FlipV);
        assert_eq!((red(&v, 0, 0), red(&v, 0, 2)), (40, 0));
        let cw = transform(&src, Transform::RotateCw);
        assert_eq!((cw.width(), cw.height()), (3, 2));
        assert_eq!(
            (red(&cw, 0, 0), red(&cw, 2, 0), red(&cw, 0, 1)),
            (40, 0, 50)
        );
        let back = transform(&cw, Transform::RotateCcw);
        assert_eq!(back.data(), src.data());
    }

    #[test]
    fn text_card_wraps_and_draws_dark_glyphs() {
        let short = render_text("Merhaba");
        let long = render_text(&"kelime ".repeat(80));
        assert!(long.height() > short.height() * 2);
        assert!(long.width() < 1400, "wrapped near 560 logical px at 2x");
        assert!(short.pixels().iter().any(|p| p.red() < 90));
        assert_eq!(short.pixel(1, 1).unwrap().red(), 0xfa);
    }
}
