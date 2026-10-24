//! Vector icons on a 24×24 grid (same drawings as the Python edition), so there is no emoji/font dependency.

use super::canvas::{paint, Canvas};
use pinshot_core::text::Fonts;
use pinshot_core::{Rect, Rgba};
use tiny_skia::{
    FillRule, LineCap, LineJoin, Path, PathBuilder, Rect as SkRect, Stroke, StrokeDash, Transform,
};

const S: f32 = 24.0;

struct Pen<'a, 'b> {
    cv: &'a mut Canvas<'b>,
    /// logical origin and size of the icon box
    ox: f32,
    oy: f32,
    k: f32,
    color: Rgba,
    flip: bool,
}

impl Pen<'_, '_> {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        let x = if self.flip { S - x } else { x };
        (
            (self.ox + x * self.k) * self.cv.scale - self.cv.off.0,
            (self.oy + y * self.k) * self.cv.scale - self.cv.off.1,
        )
    }

    fn stroke_width(&self) -> f32 {
        1.8 * self.k * self.cv.scale
    }

    fn stroke(&mut self, path: &Path, c: Rgba, dash: bool) {
        let mut st = Stroke {
            width: self.stroke_width(),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        if dash {
            st.dash = StrokeDash::new(
                vec![2.0 * self.k * self.cv.scale, 2.0 * self.k * self.cv.scale],
                0.0,
            );
        }
        self.cv
            .pm
            .stroke_path(path, &paint(c), &st, Transform::identity(), None);
    }

    fn fill(&mut self, path: &Path, c: Rgba) {
        self.cv.pm.fill_path(
            path,
            &paint(c),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn poly(&self, pts: &[f32], close: bool) -> Option<Path> {
        let mut pb = PathBuilder::new();
        for (i, xy) in pts.chunks(2).enumerate() {
            let (x, y) = self.p(xy[0], xy[1]);
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if close {
            pb.close();
        }
        pb.finish()
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
        self.poly(&[x, y, x + w, y, x + w, y + h, x, y + h], true)
    }

    fn ellipse(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
        let (a, b) = (self.p(x, y), self.p(x + w, y + h));
        SkRect::from_ltrb(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)).map(|r| {
            let mut pb = PathBuilder::new();
            pb.push_oval(r);
            pb.finish().expect("oval")
        })
    }

    fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        if let Some(p) = self.poly(&[x1, y1, x2, y2], false) {
            let c = self.color;
            self.stroke(&p, c, false);
        }
    }

    fn outline(&mut self, p: Option<Path>) {
        if let Some(p) = p {
            let c = self.color;
            self.stroke(&p, c, false);
        }
    }

    fn solid(&mut self, p: Option<Path>, c: Rgba) {
        if let Some(p) = p {
            self.fill(&p, c);
        }
    }

    fn glyph(&mut self, s: &str, px: f32, bold: bool) {
        let fonts = Fonts::embedded();
        let ppx = px * self.k * self.cv.scale;
        let w = fonts.line_width(s, ppx, bold);
        let (cx, cy) = self.p(12.0, 12.0);
        let base = cy - fonts.line_height(ppx, bold) / 2.0 + fonts.ascent(ppx, bold);
        let mut pen = cx - w / 2.0;
        let c = self.color;
        for ch in s.chars() {
            let g = fonts.glyph(ch, ppx, bold);
            pinshot_core::render::blit_glyph(
                self.cv.pm,
                &g,
                (pen + g.xmin as f32 + self.cv.off.0).round() - self.cv.off.0,
                (base - g.ymin as f32 - g.height as f32 + self.cv.off.1).round() - self.cv.off.1,
                c,
            );
            pen += g.advance;
        }
    }

    fn undo_shape(&mut self) {
        let mut pb = PathBuilder::new();
        let a = self.p(5.0, 9.0);
        let c1 = self.p(10.0, 4.0);
        let c2 = self.p(19.0, 7.0);
        let e = self.p(19.0, 15.0);
        pb.move_to(a.0, a.1);
        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, e.0, e.1);
        let c = self.color;
        if let Some(p) = pb.finish() {
            self.stroke(&p, c, false);
        }
        let head = self.poly(&[9.0, 5.0, 5.0, 9.0, 9.0, 13.0], false);
        self.outline(head);
    }
}

/// Draw icon `name` fitted into `r` (logical units).
pub fn draw(cv: &mut Canvas, name: &str, r: Rect, color: Rgba) {
    let k = r.w.min(r.h) / S;
    let mut p = Pen {
        cv,
        ox: r.x + (r.w - S * k) / 2.0,
        oy: r.y + (r.h - S * k) / 2.0,
        k,
        color,
        flip: false,
    };
    let c = color;
    match name {
        "select" => {
            let s = p.poly(
                &[
                    6.0, 3.0, 6.0, 18.0, 10.0, 14.0, 13.0, 20.0, 15.0, 19.0, 12.0, 13.0, 17.0, 13.0,
                ],
                true,
            );
            p.solid(s.clone(), c);
            p.outline(s);
        }
        "rect" => {
            let s = p.rect(4.0, 6.0, 16.0, 12.0);
            p.outline(s);
        }
        "ellipse" => {
            let s = p.ellipse(3.5, 5.5, 17.0, 13.0);
            p.outline(s);
        }
        "line" => p.line(5.0, 19.0, 19.0, 5.0),
        "arrow" => {
            p.line(5.0, 19.0, 17.0, 7.0);
            let s = p.poly(&[20.0, 4.0, 12.0, 6.0, 18.0, 12.0], true);
            p.solid(s.clone(), c);
            p.outline(s);
        }
        "pen" => {
            let s = p.poly(
                &[4.0, 20.0, 5.0, 15.0, 16.0, 4.0, 20.0, 8.0, 9.0, 19.0],
                true,
            );
            p.outline(s);
        }
        "marker" => {
            let s = p.poly(
                &[6.0, 17.0, 4.0, 14.0, 14.0, 4.0, 19.0, 9.0, 9.0, 19.0],
                true,
            );
            p.solid(s.clone(), c.with_alpha(110));
            p.outline(s);
            p.line(3.0, 21.0, 21.0, 21.0);
        }
        "text" => p.glyph("T", 20.0, true),
        "number" => {
            let s = p.ellipse(3.5, 3.5, 17.0, 17.0);
            p.outline(s);
            p.glyph("1", 12.0, true);
        }
        "mosaic" => {
            for i in 0..3 {
                for j in 0..3 {
                    if (i + j) % 2 == 0 {
                        let s = p.rect(4.0 + i as f32 * 5.3, 4.0 + j as f32 * 5.3, 5.3, 5.3);
                        p.solid(s, c);
                    }
                }
            }
        }
        "blur" => {
            let s = p.ellipse(4.0, 4.0, 16.0, 16.0);
            p.outline(s);
            let f = p.ellipse(8.0, 8.0, 8.0, 8.0);
            p.solid(f.clone(), c.with_alpha(120));
            p.outline(f);
        }
        "undo" => p.undo_shape(),
        "redo" => {
            p.flip = true;
            p.undo_shape();
        }
        "pin" => {
            let s = p.poly(&[8.0, 3.0, 16.0, 3.0, 15.0, 11.0, 9.0, 11.0], true);
            p.outline(s);
            p.line(6.0, 11.0, 18.0, 11.0);
            p.line(12.0, 11.0, 12.0, 21.0);
        }
        "ocr" => {
            for (x, y, dx, dy) in [
                (4.0, 4.0, 1.0, 1.0),
                (20.0, 4.0, -1.0, 1.0),
                (4.0, 20.0, 1.0, -1.0),
                (20.0, 20.0, -1.0, -1.0),
            ] {
                let s = p.poly(&[x, y + 4.0 * dy, x, y, x + 4.0 * dx, y], false);
                p.outline(s);
            }
            p.glyph("A", 11.0, true);
        }
        "save" => {
            let a = p.rect(4.0, 4.0, 16.0, 16.0);
            p.outline(a);
            let b = p.rect(8.0, 4.0, 8.0, 5.0);
            p.outline(b);
            let d = p.rect(7.0, 13.0, 10.0, 7.0);
            p.outline(d);
        }
        "close" => {
            p.line(6.0, 6.0, 18.0, 18.0);
            p.line(18.0, 6.0, 6.0, 18.0);
        }
        "check" => {
            let s = p.poly(&[5.0, 13.0, 10.0, 18.0, 19.0, 7.0], false);
            p.outline(s);
        }
        "copy" => {
            let a = p.rect(8.0, 8.0, 11.0, 12.0);
            p.outline(a);
            let b = p.poly(&[5.0, 16.0, 5.0, 4.0, 14.0, 4.0], false);
            p.outline(b);
        }
        "trash" => {
            p.line(5.0, 7.0, 19.0, 7.0);
            let a = p.poly(&[7.0, 7.0, 8.0, 20.0, 16.0, 20.0, 17.0, 7.0], true);
            p.outline(a);
            let b = p.poly(&[9.0, 7.0, 9.0, 4.0, 15.0, 4.0, 15.0, 7.0], false);
            p.outline(b);
        }
        "fill" => {
            let s = p.rect(5.0, 5.0, 14.0, 14.0);
            p.solid(s.clone(), c);
            p.outline(s);
        }
        "dash" => {
            if let Some(path) = p.poly(&[3.0, 12.0, 21.0, 12.0], false) {
                p.stroke(&path, c, true);
            }
        }
        "flipx" => {
            p.line(12.0, 3.0, 12.0, 21.0);
            let a = p.poly(&[9.0, 6.0, 9.0, 18.0, 3.0, 18.0], true);
            p.outline(a);
            let b = p.poly(&[15.0, 6.0, 15.0, 18.0, 21.0, 18.0], true);
            p.outline(b);
        }
        "record" => {
            let s = p.ellipse(3.5, 3.5, 17.0, 17.0);
            p.outline(s);
            let d = p.ellipse(8.0, 8.0, 8.0, 8.0);
            p.solid(d, super::theme::DANGER);
        }
        "plus" => {
            p.line(12.0, 5.0, 12.0, 19.0);
            p.line(5.0, 12.0, 19.0, 12.0);
        }
        "minus" => p.line(5.0, 12.0, 19.0, 12.0),
        "settings" => {
            let s = p.ellipse(8.0, 8.0, 8.0, 8.0);
            p.outline(s);
            for a in (0..360).step_by(45) {
                let (sin, cos) = (a as f32).to_radians().sin_cos();
                p.line(
                    12.0 + 7.0 * sin,
                    12.0 - 7.0 * cos,
                    12.0 + 10.0 * sin,
                    12.0 - 10.0 * cos,
                );
            }
        }
        "refresh" => {
            // three quarters of a circle with an arrow head
            let mut pts = Vec::new();
            for i in 0..=18 {
                let a = (40.0 + i as f32 * 15.0).to_radians();
                pts.push(12.0 + 7.0 * a.cos());
                pts.push(12.0 - 7.0 * a.sin());
            }
            let arc = p.poly(&pts, false);
            p.outline(arc);
            let head = p.poly(&[17.0, 3.5, 17.5, 8.5, 12.5, 8.0], false);
            p.outline(head);
        }
        "stop" => {
            let s = p.rect(6.0, 6.0, 12.0, 12.0);
            p.solid(s, c);
        }
        _ => {}
    }
}

/// The application icon (blue rounded square with capture corners and a dot) as a pixmap, for windows and tray.
pub fn app_icon(size: u32, recording: bool) -> tiny_skia::Pixmap {
    let mut pm = tiny_skia::Pixmap::new(size, size).expect("icon size");
    let k = size as f32 / 100.0;
    let mut cv = Canvas::new(&mut pm, k);
    cv.fill_rrect(Rect::new(4.0, 4.0, 92.0, 92.0), 22.0, super::theme::ACCENT);
    for (x, y, dx, dy) in [
        (24.0, 24.0, 1.0, 1.0),
        (76.0, 24.0, -1.0, 1.0),
        (24.0, 76.0, 1.0, -1.0),
        (76.0, 76.0, -1.0, -1.0),
    ] {
        cv.line((x, y + 16.0 * dy), (x, y), 6.0, Rgba::WHITE);
        cv.line((x, y), (x + 16.0 * dx, y), 6.0, Rgba::WHITE);
    }
    cv.fill_circle(50.0, 50.0, 9.0, Rgba::WHITE);
    if recording {
        cv.fill_circle(74.0, 26.0, 19.0, Rgba::WHITE);
        cv.fill_circle(74.0, 26.0, 16.0, Rgba::rgb(0xff, 0x3b, 0x30));
    }
    pm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws_something() {
        let names = [
            "select", "rect", "ellipse", "line", "arrow", "pen", "marker", "text", "number",
            "mosaic", "blur", "undo", "redo", "pin", "ocr", "save", "close", "check", "copy",
            "trash", "fill", "dash", "flipx", "record", "plus", "minus", "settings", "stop",
            "refresh",
        ];
        for n in names {
            let mut pm = tiny_skia::Pixmap::new(48, 48).unwrap();
            let mut cv = Canvas::new(&mut pm, 1.0);
            draw(&mut cv, n, Rect::new(0.0, 0.0, 48.0, 48.0), Rgba::WHITE);
            assert!(
                pm.pixels().iter().any(|p| p.alpha() > 0),
                "icon {n} is empty"
            );
        }
        let mut pm = tiny_skia::Pixmap::new(8, 8).unwrap();
        draw(
            &mut Canvas::new(&mut pm, 1.0),
            "nonexistent",
            Rect::new(0.0, 0.0, 8.0, 8.0),
            Rgba::WHITE,
        );
        assert!(pm.pixels().iter().all(|p| p.alpha() == 0));
    }

    #[test]
    fn redo_is_the_mirror_of_undo() {
        let render = |n: &str| {
            let mut pm = tiny_skia::Pixmap::new(48, 48).unwrap();
            draw(
                &mut Canvas::new(&mut pm, 1.0),
                n,
                Rect::new(0.0, 0.0, 48.0, 48.0),
                Rgba::WHITE,
            );
            pm
        };
        let (u, r) = (render("undo"), render("redo"));
        let mirrored_equal = (0..48).all(|y| {
            (0..48).all(|x| {
                let a = u.pixel(x, y).unwrap().alpha() as i32;
                let b = r.pixel(47 - x, y).unwrap().alpha() as i32;
                (a - b).abs() < 40
            })
        });
        assert!(mirrored_equal);
    }

    #[test]
    fn app_icon_has_the_brand_colour_and_recording_dot() {
        let a = app_icon(64, false);
        let p = a.pixel(10, 32).unwrap();
        assert!(p.blue() > 200 && p.red() < 40);
        let r = app_icon(64, true);
        let dot = r.pixel(47, 17).unwrap();
        assert!(dot.red() > 200 && dot.green() < 90);
    }
}
