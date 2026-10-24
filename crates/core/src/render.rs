//! Software rendering of annotations onto a [`Pixmap`] with tiny-skia.
//!
//! Items are in logical coordinates. `origin` is the logical position of the pixmap's top-left pixel and `scale`
//! converts logical units to pixels, so the same code draws the live preview and the final export.

use crate::annotate::{Item, Kind};
use crate::color::Rgba;
use crate::geom::Point;
use crate::text::Fonts;
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    Rect as SkRect, Stroke, StrokeDash, Transform,
};

pub struct View {
    pub origin: Point,
    pub scale: f32,
}

impl View {
    pub fn new(origin: Point, scale: f32) -> Self {
        View { origin, scale }
    }

    fn pt(&self, p: Point) -> (f32, f32) {
        (
            (p.x - self.origin.x) * self.scale,
            (p.y - self.origin.y) * self.scale,
        )
    }
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c.r, c.g, c.b, c.a));
    p.anti_alias = true;
    p
}

fn stroke(width: f32, dashed: bool) -> Stroke {
    let mut s = Stroke {
        width: width.max(0.5),
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    if dashed {
        s.dash = StrokeDash::new(vec![width * 3.0, width * 2.0], 0.0);
    }
    s
}

fn ellipse_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    SkRect::from_xywh(x, y, w.max(0.01), h.max(0.01)).and_then(|r| {
        let mut pb = PathBuilder::new();
        pb.push_oval(r);
        pb.finish()
    })
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    SkRect::from_xywh(x, y, w.max(0.01), h.max(0.01)).map(PathBuilder::from_rect)
}

/// Draw every item in order.
pub fn render_items(pm: &mut Pixmap, items: &[Item], view: &View) {
    let mut n = 0;
    for it in items {
        if it.kind == Kind::Number {
            n += 1;
        }
        render_item(pm, it, view, n);
    }
}

/// Number badges are numbered by their order among all number items; `number` is that count for this item.
pub fn render_item(pm: &mut Pixmap, it: &Item, view: &View, number: usize) {
    let s = view.scale;
    let id = Transform::identity();
    match it.kind {
        Kind::Rect | Kind::Ellipse => {
            let (a, b) = (view.pt(it.first()), view.pt(it.last()));
            let (x, y, w, h) = (
                a.0.min(b.0),
                a.1.min(b.1),
                (a.0 - b.0).abs(),
                (a.1 - b.1).abs(),
            );
            let path = if it.kind == Kind::Rect {
                rect_path(x, y, w, h)
            } else {
                ellipse_path(x, y, w, h)
            };
            if let Some(path) = path {
                if it.fill {
                    pm.fill_path(&path, &paint(it.color), FillRule::Winding, id, None);
                } else {
                    pm.stroke_path(
                        &path,
                        &paint(it.color),
                        &stroke(it.width as f32 * s, it.dashed),
                        id,
                        None,
                    );
                }
            }
        }
        Kind::Line | Kind::Arrow => draw_line(pm, it, view),
        Kind::Pen | Kind::Marker => draw_freehand(pm, it, view),
        Kind::Text => draw_text(pm, it, view),
        Kind::Number => draw_number(pm, it, view, number),
        Kind::Mosaic | Kind::Blur => {
            let (a, b) = (view.pt(it.first()), view.pt(it.last()));
            let strength = it.width.max(1) as f32 * s;
            let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
            if it.kind == Kind::Mosaic {
                mosaic(pm, x0, y0, x1, y1, (strength * 1.5).max(4.0) as usize);
            } else {
                blur(
                    pm,
                    x0,
                    y0,
                    x1,
                    y1,
                    (strength * 0.8).ceil().max(2.0) as usize,
                );
            }
        }
    }
}

fn draw_line(pm: &mut Pixmap, it: &Item, view: &View) {
    let s = view.scale;
    let (a, b) = (view.pt(it.first()), view.pt(it.last()));
    let width = it.width as f32 * s;
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    if it.kind == Kind::Arrow {
        // stop the shaft at the head base so a thick line does not poke through the tip
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        let head = (10.0 + width * 2.5).min(len.max(1.0));
        let (ux, uy) = if len > 0.0 {
            (dx / len, dy / len)
        } else {
            (1.0, 0.0)
        };
        pb.line_to(b.0 - ux * head * 0.8, b.1 - uy * head * 0.8);
        if let Some(path) = pb.finish() {
            pm.stroke_path(
                &path,
                &paint(it.color),
                &stroke(width, it.dashed),
                Transform::identity(),
                None,
            );
        }
        let (nx, ny) = (-uy, ux);
        let half = head * 0.5;
        let mut hb = PathBuilder::new();
        hb.move_to(b.0, b.1);
        hb.line_to(b.0 - ux * head + nx * half, b.1 - uy * head + ny * half);
        hb.line_to(b.0 - ux * head - nx * half, b.1 - uy * head - ny * half);
        hb.close();
        if let Some(path) = hb.finish() {
            pm.fill_path(
                &path,
                &paint(it.color),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    } else {
        pb.line_to(b.0, b.1);
        if let Some(path) = pb.finish() {
            pm.stroke_path(
                &path,
                &paint(it.color),
                &stroke(width, it.dashed),
                Transform::identity(),
                None,
            );
        }
    }
}

fn draw_freehand(pm: &mut Pixmap, it: &Item, view: &View) {
    let s = view.scale;
    let marker = it.kind == Kind::Marker;
    let width = it.width as f32 * s * if marker { 4.0 } else { 1.0 };
    let mut c = it.color;
    if marker {
        c = c.with_alpha(110);
    }
    let pts: Vec<(f32, f32)> = it.pts.iter().map(|p| view.pt(*p)).collect();
    if pts.len() == 1 {
        if let Some(path) =
            ellipse_path(pts[0].0 - width / 2.0, pts[0].1 - width / 2.0, width, width)
        {
            pm.fill_path(
                &path,
                &paint(c),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        return;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(pts[0].0, pts[0].1);
    // quadratic smoothing through midpoints
    for w in pts.windows(2) {
        let mid = ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
        pb.quad_to(w[0].0, w[0].1, mid.0, mid.1);
    }
    let l = pts[pts.len() - 1];
    pb.line_to(l.0, l.1);
    if let Some(path) = pb.finish() {
        let mut st = stroke(width, false);
        if marker {
            st.line_cap = LineCap::Butt;
        }
        pm.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
    }
}

fn draw_text(pm: &mut Pixmap, it: &Item, view: &View) {
    let fonts = Fonts::embedded();
    let px = it.font_px.max(6) as f32 * view.scale;
    let (ox, oy) = view.pt(it.first());
    let lh = fonts.line_height(px, false);
    let asc = fonts.ascent(px, false);
    for (n, line) in it.text.split('\n').enumerate() {
        let mut pen = ox;
        let base = oy + asc + lh * n as f32;
        for ch in line.chars() {
            let g = fonts.glyph(ch, px, false);
            blit_glyph(
                pm,
                &g,
                pen + g.xmin as f32,
                base - g.ymin as f32 - g.height as f32,
                it.color,
            );
            pen += g.advance;
        }
    }
}

/// Composite an 8-bit coverage bitmap tinted with `c` at (x, y).
pub fn blit_glyph(pm: &mut Pixmap, g: &crate::text::Glyph, x: f32, y: f32, c: Rgba) {
    let (x, y) = (x.round() as i32, y.round() as i32);
    let (pw, ph) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    for gy in 0..g.height as i32 {
        let py = y + gy;
        if py < 0 || py >= ph {
            continue;
        }
        for gx in 0..g.width as i32 {
            let px = x + gx;
            if px < 0 || px >= pw {
                continue;
            }
            let cov = g.coverage[(gy as usize) * g.width + gx as usize] as u32;
            if cov == 0 {
                continue;
            }
            let a = cov * c.a as u32 / 255;
            let i = ((py * pw + px) * 4) as usize;
            let inv = 255 - a;
            // pixmap is premultiplied
            data[i] = ((c.r as u32 * a + data[i] as u32 * inv) / 255) as u8;
            data[i + 1] = ((c.g as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
            data[i + 2] = ((c.b as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
            data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
        }
    }
}

fn draw_number(pm: &mut Pixmap, it: &Item, view: &View, number: usize) {
    let label = number.to_string();
    let s = view.scale;
    let r = it.number_radius() * s;
    let (cx, cy) = view.pt(it.first());
    if let Some(path) = ellipse_path(cx - r, cy - r, 2.0 * r, 2.0 * r) {
        pm.fill_path(
            &path,
            &paint(it.color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let fg = if it.color.luminance() > 160.0 {
        Rgba::BLACK
    } else {
        Rgba::WHITE
    };
    let fonts = Fonts::embedded();
    let px = r * 1.1;
    let w = fonts.line_width(&label, px, true);
    let asc = fonts.ascent(px, true);
    let lh = fonts.line_height(px, true);
    let mut pen = cx - w / 2.0;
    let base = cy - lh / 2.0 + asc;
    for ch in label.chars() {
        let g = fonts.glyph(ch, px, true);
        blit_glyph(
            pm,
            &g,
            pen + g.xmin as f32,
            base - g.ymin as f32 - g.height as f32,
            fg,
        );
        pen += g.advance;
    }
}

fn clamp_region(
    pm: &Pixmap,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
) -> Option<(usize, usize, usize, usize)> {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    let (x0, y0) = (x0.floor().clamp(0.0, w), y0.floor().clamp(0.0, h));
    let (x1, y1) = (x1.ceil().clamp(0.0, w), y1.ceil().clamp(0.0, h));
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        return None;
    }
    Some((x0 as usize, y0 as usize, x1 as usize, y1 as usize))
}

/// Replace each `block`×`block` cell with its average colour. The cell grid starts at the region's top-left corner
/// (even when part of the region lies outside the pixmap), so previews and exports line up exactly.
pub fn mosaic(pm: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, block: usize) {
    let Some((cx0, cy0, cx1, cy1)) = clamp_region(pm, x0, y0, x1, y1) else {
        return;
    };
    let (gx, gy) = (x0.floor() as i64, y0.floor() as i64);
    let w = pm.width() as usize;
    let block = block.max(2) as i64;
    let data = pm.data_mut();
    // first cell row/column that touches the clamped region
    let first = |g: i64, c: i64| g + (c - g).div_euclid(block) * block;
    let mut by = first(gy, cy0 as i64);
    while by < cy1 as i64 {
        let (ya, yb) = (
            by.max(cy0 as i64) as usize,
            (by + block).min(cy1 as i64) as usize,
        );
        let mut bx = first(gx, cx0 as i64);
        while bx < cx1 as i64 {
            let (xa, xb) = (
                bx.max(cx0 as i64) as usize,
                (bx + block).min(cx1 as i64) as usize,
            );
            let mut sum = [0u32; 4];
            for y in ya..yb {
                for x in xa..xb {
                    let i = (y * w + x) * 4;
                    for k in 0..4 {
                        sum[k] += data[i + k] as u32;
                    }
                }
            }
            let n = ((xb - xa) * (yb - ya)) as u32;
            let avg = [
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                (sum[3] / n) as u8,
            ];
            for y in ya..yb {
                for x in xa..xb {
                    let i = (y * w + x) * 4;
                    data[i..i + 4].copy_from_slice(&avg);
                }
            }
            bx += block;
        }
        by += block;
    }
}

/// Box blur (three passes ≈ gaussian) limited to the region; edges clamp to the region.
pub fn blur(pm: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, radius: usize) {
    let Some((x0, y0, x1, y1)) = clamp_region(pm, x0, y0, x1, y1) else {
        return;
    };
    let (rw, rh) = (x1 - x0, y1 - y0);
    let w = pm.width() as usize;
    let data = pm.data_mut();
    let mut buf = vec![0u8; rw * rh * 4];
    for y in 0..rh {
        let src = ((y0 + y) * w + x0) * 4;
        buf[y * rw * 4..(y + 1) * rw * 4].copy_from_slice(&data[src..src + rw * 4]);
    }
    let mut tmp = vec![0u8; buf.len()];
    for _ in 0..3 {
        box_pass(&buf, &mut tmp, rw, rh, radius, true);
        box_pass(&tmp, &mut buf, rw, rh, radius, false);
    }
    for y in 0..rh {
        let dst = ((y0 + y) * w + x0) * 4;
        data[dst..dst + rw * 4].copy_from_slice(&buf[y * rw * 4..(y + 1) * rw * 4]);
    }
}

fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let idx = |line: usize, pos: usize| {
        if horizontal {
            (line * w + pos) * 4
        } else {
            (pos * w + line) * 4
        }
    };
    let r = r.min(len.saturating_sub(1) / 2).max(1).min(len);
    let win = (2 * r + 1) as u32;
    for line in 0..lines {
        for k in 0..4 {
            let at = |p: isize| src[idx(line, p.clamp(0, len as isize - 1) as usize) + k] as u32;
            let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
            for pos in 0..len {
                dst[idx(line, pos) + k] = (sum / win) as u8;
                sum = sum + at(pos as isize + r as isize + 1) - at(pos as isize - r as isize);
            }
        }
    }
}

/// Draw `src` onto `dst` at integer pixel position.
pub fn draw_pixmap(dst: &mut Pixmap, src: &Pixmap, x: i32, y: i32) {
    dst.draw_pixmap(
        x,
        y,
        src.as_ref(),
        &PixmapPaint {
            blend_mode: BlendMode::Source,
            ..PixmapPaint::default()
        },
        Transform::identity(),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        pm.fill(Color::WHITE);
        pm
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let p = pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    fn view() -> View {
        View::new(Point::new(0.0, 0.0), 1.0)
    }

    #[test]
    fn filled_rect_and_ellipse_cover_the_right_pixels() {
        let mut pm = white(100, 100);
        let mut r = Item::new(
            Kind::Rect,
            Rgba::rgb(255, 0, 0),
            3,
            vec![Point::new(10.0, 10.0), Point::new(50.0, 50.0)],
        );
        r.fill = true;
        let mut e = Item::new(
            Kind::Ellipse,
            Rgba::rgb(0, 0, 255),
            3,
            vec![Point::new(60.0, 60.0), Point::new(90.0, 90.0)],
        );
        e.fill = true;
        render_items(&mut pm, &[r, e], &view());
        assert_eq!(px(&pm, 30, 30), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 75, 75), [0, 0, 255, 255]);
        assert_eq!(
            px(&pm, 61, 61),
            [255, 255, 255, 255],
            "ellipse corner stays empty"
        );
        assert_eq!(px(&pm, 5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn view_offset_and_scale_map_logical_to_pixels() {
        let mut pm = white(100, 100);
        let mut r = Item::new(
            Kind::Rect,
            Rgba::rgb(0, 0, 0),
            2,
            vec![Point::new(110.0, 210.0), Point::new(130.0, 230.0)],
        );
        r.fill = true;
        render_item(&mut pm, &r, &View::new(Point::new(100.0, 200.0), 2.0), 0);
        assert_eq!(px(&pm, 30, 30), [0, 0, 0, 255]);
        assert_eq!(px(&pm, 70, 70), [255, 255, 255, 255]);
    }

    #[test]
    fn outline_leaves_the_inside_untouched() {
        let mut pm = white(100, 100);
        let r = Item::new(
            Kind::Rect,
            Rgba::rgb(0, 0, 0),
            4,
            vec![Point::new(10.0, 10.0), Point::new(90.0, 90.0)],
        );
        render_item(&mut pm, &r, &view(), 0);
        assert_eq!(px(&pm, 50, 50), [255, 255, 255, 255]);
        assert_eq!(px(&pm, 10, 50)[0], 0);
    }

    #[test]
    fn arrow_pen_marker_text_and_number_draw_something() {
        let mut pm = white(200, 100);
        let mut t = Item::new(Kind::Text, Rgba::BLACK, 3, vec![Point::new(5.0, 5.0)]);
        t.text = "Aa\nİş".into();
        t.font_px = 24;
        let mut n = Item::new(
            Kind::Number,
            Rgba::rgb(255, 0, 0),
            3,
            vec![Point::new(150.0, 30.0)],
        );
        n.text = "7".into();
        let items = vec![
            Item::new(
                Kind::Arrow,
                Rgba::rgb(0, 128, 0),
                3,
                vec![Point::new(60.0, 10.0), Point::new(120.0, 10.0)],
            ),
            Item::new(
                Kind::Pen,
                Rgba::BLACK,
                3,
                vec![
                    Point::new(60.0, 50.0),
                    Point::new(80.0, 70.0),
                    Point::new(120.0, 50.0),
                ],
            ),
            Item::new(
                Kind::Marker,
                Rgba::rgb(255, 255, 0),
                4,
                vec![Point::new(20.0, 90.0), Point::new(180.0, 90.0)],
            ),
            t,
            n,
        ];
        let before = pm.data().to_vec();
        render_items(&mut pm, &items, &view());
        assert_ne!(pm.data(), &before[..]);
        assert_ne!(px(&pm, 117, 10), [255, 255, 255, 255], "arrow head drawn");
        assert_eq!(
            px(&pm, 150 - 12, 30),
            [255, 0, 0, 255],
            "badge is filled with its colour"
        );
        assert!(
            (140..160).any(|x| (20..40).any(|y| px(&pm, x, y)[1] > 200)),
            "white digit drawn on a red badge"
        );
        assert!(
            pm.data()
                .chunks(4)
                .any(|p| p[0] < 80 && p[1] < 80 && p[2] < 80),
            "text pixels are dark"
        );
    }

    #[test]
    fn mosaic_flattens_cells_and_blur_smooths_edges() {
        let mut pm = white(40, 40);
        for y in 0..40 {
            for x in 0..40 {
                let i = ((y * 40 + x) * 4) as usize;
                let v = if x % 2 == 0 { 0 } else { 255 };
                pm.data_mut()[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let mut m = pm.clone();
        mosaic(&mut m, 0.0, 0.0, 40.0, 40.0, 8);
        assert_eq!(px(&m, 0, 0), px(&m, 7, 7), "one colour per cell");
        let mut b = pm.clone();
        blur(&mut b, 0.0, 0.0, 40.0, 40.0, 3);
        let v = px(&b, 20, 20)[0];
        assert!(v > 60 && v < 200, "stripes become grey, got {v}");
        // outside the region nothing changes
        let mut o = pm.clone();
        blur(&mut o, 10.0, 10.0, 20.0, 20.0, 2);
        assert_eq!(px(&o, 0, 0), px(&pm, 0, 0));
        assert_eq!(px(&o, 30, 30), px(&pm, 30, 30));
    }

    #[test]
    fn degenerate_regions_do_not_panic() {
        let mut pm = white(10, 10);
        mosaic(&mut pm, -50.0, -50.0, -10.0, -10.0, 8);
        blur(&mut pm, 5.0, 5.0, 5.0, 5.0, 4);
        blur(&mut pm, 0.0, 0.0, 1.0, 1.0, 9);
        let tiny = Item::new(
            Kind::Rect,
            Rgba::BLACK,
            3,
            vec![Point::new(5.0, 5.0), Point::new(5.0, 5.0)],
        );
        render_item(&mut pm, &tiny, &view(), 0);
        let one = Item::new(Kind::Pen, Rgba::BLACK, 3, vec![Point::new(5.0, 5.0)]);
        render_item(&mut pm, &one, &view(), 0);
    }
}
