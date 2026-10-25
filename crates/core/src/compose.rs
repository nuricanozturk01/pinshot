//! Stitching several monitor captures into one image of a logical region (scale-factor safe).
//!
//! Each [`Screen`] covers a logical rectangle and holds pixels at its own scale. The result has a single scale
//! (the largest of the covered screens, so nothing is blurred by down-sampling); parts of the region that no
//! screen covers stay transparent black.

use crate::geom::Rect;
use tiny_skia::Pixmap;

pub struct Screen<'a> {
    /// Logical rectangle in desktop coordinates.
    pub rect: Rect,
    /// Pixels per logical unit.
    pub scale: f32,
    pub pixmap: &'a Pixmap,
}

/// Compose `region` (logical) out of `screens`. Returns `None` if the region is empty or no screen overlaps it.
pub fn compose_region(screens: &[Screen<'_>], region: Rect) -> Option<(Pixmap, f32)> {
    if region.is_empty() {
        return None;
    }
    let hit: Vec<&Screen<'_>> = screens
        .iter()
        .filter(|s| s.rect.intersects(&region))
        .collect();
    let scale = hit.iter().map(|s| s.scale).fold(0.0_f32, f32::max);
    if scale <= 0.0 {
        return None;
    }
    let (w, h) = (
        (region.w * scale).round().max(1.0) as u32,
        (region.h * scale).round().max(1.0) as u32,
    );
    let mut out = Pixmap::new(w, h)?;
    for s in hit {
        let part = s.rect.intersection(&region);
        // source pixels (always whole pixels of the screen's own grid)
        let sx = ((part.x - s.rect.x) * s.scale).round() as i32;
        let sy = ((part.y - s.rect.y) * s.scale).round() as i32;
        let sw = ((part.w * s.scale).round() as i32)
            .min(s.pixmap.width() as i32 - sx)
            .max(0);
        let sh = ((part.h * s.scale).round() as i32)
            .min(s.pixmap.height() as i32 - sy)
            .max(0);
        if sw == 0 || sh == 0 {
            continue;
        }
        let dx = ((part.x - region.x) * scale).round() as i32;
        let dy = ((part.y - region.y) * scale).round() as i32;
        if (s.scale - scale).abs() < 1e-4 {
            copy_rows(s.pixmap, &mut out, sx, sy, sw, sh, dx, dy);
        } else {
            let (tw, th) = (
                (part.w * scale).round() as i32,
                (part.h * scale).round() as i32,
            );
            scale_rows(s.pixmap, &mut out, (sx, sy, sw, sh), (dx, dy, tw, th));
        }
    }
    Some((out, scale))
}

/// Exact pixel copy (no resampling) when both sides use the same scale.
#[allow(clippy::too_many_arguments)]
fn copy_rows(src: &Pixmap, dst: &mut Pixmap, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32) {
    let (sw, dw) = (src.width() as i32, dst.width() as i32);
    let dh = dst.height() as i32;
    let sdata = src.data();
    let ddata = dst.data_mut();
    for row in 0..h {
        let y = dy + row;
        if y < 0 || y >= dh {
            continue;
        }
        let cw = w.min(dw - dx).max(0);
        if cw == 0 || dx < 0 {
            continue;
        }
        let s0 = (((sy + row) * sw + sx) * 4) as usize;
        let d0 = ((y * dw + dx) * 4) as usize;
        ddata[d0..d0 + cw as usize * 4].copy_from_slice(&sdata[s0..s0 + cw as usize * 4]);
    }
}

/// Bilinear resample of a source rectangle into a destination rectangle (used across mixed-scale monitors).
fn scale_rows(src: &Pixmap, dst: &mut Pixmap, s: (i32, i32, i32, i32), d: (i32, i32, i32, i32)) {
    let (sx, sy, sw, sh) = s;
    let (dx, dy, tw, th) = d;
    let (spw, dpw, dph) = (src.width() as i32, dst.width() as i32, dst.height() as i32);
    let sdata = src.data();
    let ddata = dst.data_mut();
    for y in 0..th {
        let oy = dy + y;
        if oy < 0 || oy >= dph {
            continue;
        }
        let fy = ((y as f32 + 0.5) * sh as f32 / th as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy.floor() as i32, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..tw {
            let ox = dx + x;
            if ox < 0 || ox >= dpw {
                continue;
            }
            let fx = ((x as f32 + 0.5) * sw as f32 / tw as f32 - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx.floor() as i32, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            let at = |xx: i32, yy: i32, k: usize| {
                sdata[(((sy + yy) * spw + sx + xx) * 4) as usize + k] as f32
            };
            for k in 0..4 {
                let top = at(x0, y0, k) * (1.0 - tx) + at(x1, y0, k) * tx;
                let bot = at(x0, y1, k) * (1.0 - tx) + at(x1, y1, k) * tx;
                ddata[((oy * dpw + ox) * 4) as usize + k] =
                    (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::Color;

    struct Shot(Rect, f32, Pixmap);

    impl Shot {
        fn screen(&self) -> Screen<'_> {
            Screen {
                rect: self.0,
                scale: self.1,
                pixmap: &self.2,
            }
        }
    }

    fn shot(rect: Rect, scale: f32, c: Color) -> Shot {
        let mut pm = Pixmap::new((rect.w * scale) as u32, (rect.h * scale) as u32).unwrap();
        pm.fill(c);
        Shot(rect, scale, pm)
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let p = pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    #[test]
    fn region_inside_one_screen_is_an_exact_crop() {
        let mut s = shot(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            1.0,
            Color::from_rgba8(10, 20, 30, 255),
        );
        s.2.data_mut()[(50 * 100 + 20) * 4..(50 * 100 + 20) * 4 + 4]
            .copy_from_slice(&[1, 2, 3, 255]);
        let (out, scale) =
            compose_region(&[s.screen()], Rect::new(10.0, 40.0, 30.0, 20.0)).unwrap();
        assert_eq!((out.width(), out.height(), scale), (30, 20, 1.0));
        assert_eq!(px(&out, 10, 10), [1, 2, 3, 255]);
        assert_eq!(px(&out, 0, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn region_across_two_screens_joins_them_at_the_seam() {
        let a = shot(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            1.0,
            Color::from_rgba8(255, 0, 0, 255),
        );
        let b = shot(
            Rect::new(100.0, 0.0, 100.0, 100.0),
            1.0,
            Color::from_rgba8(0, 0, 255, 255),
        );
        let (out, _) =
            compose_region(&[a.screen(), b.screen()], Rect::new(90.0, 10.0, 20.0, 10.0)).unwrap();
        assert_eq!(px(&out, 9, 5), [255, 0, 0, 255]);
        assert_eq!(px(&out, 10, 5), [0, 0, 255, 255]);
    }

    #[test]
    fn mixed_scales_use_the_larger_one_and_leave_no_seam() {
        let a = shot(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            1.0,
            Color::from_rgba8(255, 0, 0, 255),
        );
        let b = shot(
            Rect::new(100.0, 0.0, 100.0, 100.0),
            2.0,
            Color::from_rgba8(0, 0, 255, 255),
        );
        let (out, scale) =
            compose_region(&[a.screen(), b.screen()], Rect::new(90.0, 0.0, 20.0, 10.0)).unwrap();
        assert_eq!((out.width(), out.height(), scale), (40, 20, 2.0));
        assert_eq!(px(&out, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&out, 19, 19), [255, 0, 0, 255]);
        assert_eq!(px(&out, 20, 0), [0, 0, 255, 255]);
        assert_eq!(px(&out, 39, 19), [0, 0, 255, 255]);
        for x in 0..40 {
            assert_eq!(px(&out, x, 5)[3], 255, "no transparent gap at x={x}");
        }
    }

    #[test]
    fn uncovered_parts_are_transparent_and_empty_regions_fail() {
        let a = shot(Rect::new(0.0, 0.0, 50.0, 50.0), 1.0, Color::WHITE);
        let (out, _) = compose_region(&[a.screen()], Rect::new(40.0, 0.0, 20.0, 10.0)).unwrap();
        assert_eq!(px(&out, 5, 5)[3], 255);
        assert_eq!(px(&out, 15, 5)[3], 0);
        assert!(compose_region(&[], Rect::new(0.0, 0.0, 10.0, 10.0)).is_none());
        let b = shot(Rect::new(0.0, 0.0, 10.0, 10.0), 1.0, Color::WHITE);
        assert!(compose_region(&[b.screen()], Rect::new(0.0, 0.0, 0.0, 5.0)).is_none());
    }
}
