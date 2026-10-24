//! Drawing surface: a pixmap plus a scale, so widget code is written in logical units.

use pinshot_core::text::Fonts;
use pinshot_core::{Rect, Rgba};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect as SkRect, Stroke,
    StrokeDash, Transform,
};

pub struct Canvas<'a> {
    pub pm: &'a mut Pixmap,
    /// Physical pixels per logical unit.
    pub scale: f32,
    /// Physical pixel position of the logical origin (the pixmap may show only part of a larger surface).
    pub off: (f32, f32),
}

pub fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c.r, c.g, c.b, c.a));
    p.anti_alias = true;
    p
}

pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.552_284_75 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

impl<'a> Canvas<'a> {
    pub fn new(pm: &'a mut Pixmap, scale: f32) -> Self {
        Canvas {
            pm,
            scale,
            off: (0.0, 0.0),
        }
    }

    /// A canvas whose logical (0, 0) lies at physical `off` of the surface; the pixmap covers a window onto it.
    pub fn with_offset(pm: &'a mut Pixmap, scale: f32, off: (f32, f32)) -> Self {
        Canvas { pm, scale, off }
    }

    fn tf(&self) -> Transform {
        Transform::from_translate(-self.off.0, -self.off.1)
    }

    fn s(&self, v: f32) -> f32 {
        v * self.scale
    }

    pub fn fill_rect(&mut self, r: Rect, c: Rgba) {
        if let Some(sk) = SkRect::from_xywh(
            self.s(r.x),
            self.s(r.y),
            self.s(r.w).max(0.01),
            self.s(r.h).max(0.01),
        ) {
            let tf = self.tf();
            self.pm.fill_rect(sk, &paint(c), tf, None);
        }
    }

    pub fn fill_rrect(&mut self, r: Rect, radius: f32, c: Rgba) {
        if let Some(p) = rrect_path(
            self.s(r.x),
            self.s(r.y),
            self.s(r.w),
            self.s(r.h),
            self.s(radius),
        ) {
            let tf = self.tf();
            self.pm
                .fill_path(&p, &paint(c), FillRule::Winding, tf, None);
        }
    }

    pub fn stroke_rrect(&mut self, r: Rect, radius: f32, width: f32, c: Rgba) {
        // inset by half the stroke so the outline stays inside `r`
        let h = width / 2.0;
        if let Some(p) = rrect_path(
            self.s(r.x + h),
            self.s(r.y + h),
            self.s(r.w - width),
            self.s(r.h - width),
            self.s(radius),
        ) {
            let tf = self.tf();
            self.pm.stroke_path(
                &p,
                &paint(c),
                &Stroke {
                    width: self.s(width),
                    ..Stroke::default()
                },
                tf,
                None,
            );
        }
    }

    pub fn stroke_rect(&mut self, r: Rect, width: f32, c: Rgba, dashed: bool) {
        let h = width / 2.0;
        if let Some(sk) = SkRect::from_xywh(
            self.s(r.x + h),
            self.s(r.y + h),
            self.s(r.w - width).max(0.01),
            self.s(r.h - width).max(0.01),
        ) {
            let mut st = Stroke {
                width: self.s(width),
                ..Stroke::default()
            };
            if dashed {
                st.dash = StrokeDash::new(vec![self.s(4.0), self.s(3.0)], 0.0);
            }
            let tf = self.tf();
            self.pm
                .stroke_path(&PathBuilder::from_rect(sk), &paint(c), &st, tf, None);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if let Some(p) = PathBuilder::from_circle(self.s(cx), self.s(cy), self.s(r)) {
            let tf = self.tf();
            self.pm
                .fill_path(&p, &paint(c), FillRule::Winding, tf, None);
        }
    }

    pub fn stroke_circle(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgba) {
        if let Some(p) = PathBuilder::from_circle(self.s(cx), self.s(cy), self.s(r)) {
            let tf = self.tf();
            self.pm.stroke_path(
                &p,
                &paint(c),
                &Stroke {
                    width: self.s(width),
                    ..Stroke::default()
                },
                tf,
                None,
            );
        }
    }

    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), width: f32, c: Rgba) {
        let mut pb = PathBuilder::new();
        pb.move_to(self.s(a.0), self.s(a.1));
        pb.line_to(self.s(b.0), self.s(b.1));
        if let Some(p) = pb.finish() {
            let st = Stroke {
                width: self.s(width),
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            let tf = self.tf();
            self.pm.stroke_path(&p, &paint(c), &st, tf, None);
        }
    }

    pub fn text_width(&self, text: &str, px: f32, bold: bool) -> f32 {
        Fonts::embedded().line_width(text, px * self.scale, bold) / self.scale
    }

    /// Draw one line of text with its top-left at (x, y), in logical units.
    pub fn text(&mut self, x: f32, y: f32, text: &str, px: f32, c: Rgba, bold: bool) {
        let fonts = Fonts::embedded();
        let ppx = px * self.scale;
        let base = self.s(y) + fonts.ascent(ppx, bold);
        let mut pen = self.s(x);
        for ch in text.chars() {
            let g = fonts.glyph(ch, ppx, bold);
            pinshot_core::render::blit_glyph(
                self.pm,
                &g,
                (pen + g.xmin as f32).round() - self.off.0,
                (base - g.ymin as f32 - g.height as f32).round() - self.off.1,
                c,
            );
            pen += g.advance;
        }
    }

    /// Text centred in `r`.
    pub fn text_centered(&mut self, r: Rect, text: &str, px: f32, c: Rgba, bold: bool) {
        let w = self.text_width(text, px, bold);
        let lh = Fonts::embedded().line_height(px * self.scale, bold) / self.scale;
        self.text(
            r.x + (r.w - w) / 2.0,
            r.y + (r.h - lh) / 2.0,
            text,
            px,
            c,
            bold,
        );
    }

    /// Rounded dark label used for hints and readouts. Returns its rectangle.
    pub fn pill(&mut self, x: f32, y: f32, text: &str) -> Rect {
        let w = self.text_width(text, 12.0, false) + 16.0;
        let r = Rect::new(x, y, w, 22.0);
        self.fill_rrect(r, 6.0, super::theme::PILL_BG);
        self.text_centered(r, text, 12.0, Rgba::WHITE, false);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_drawing_hits_physical_pixels() {
        let mut pm = Pixmap::new(100, 100).unwrap();
        let mut c = Canvas::new(&mut pm, 2.0);
        c.fill_rect(Rect::new(10.0, 10.0, 10.0, 10.0), Rgba::rgb(255, 0, 0));
        assert_eq!(pm.pixel(25, 25).unwrap().red(), 255);
        assert_eq!(pm.pixel(15, 15).unwrap().alpha(), 0);
    }

    #[test]
    fn text_and_pill_draw_pixels_and_measure() {
        let mut pm = Pixmap::new(300, 60).unwrap();
        let mut c = Canvas::new(&mut pm, 1.0);
        assert!(c.text_width("Merhaba dünya", 14.0, false) > 60.0);
        let r = c.pill(10.0, 10.0, "1920 × 1080");
        assert!(r.w > 60.0);
        assert!(
            pm.pixels().iter().any(|p| p.red() > 200 && p.alpha() > 200),
            "white text on the pill"
        );
    }
}
