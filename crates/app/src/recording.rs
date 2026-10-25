//! Frame source for screen recording: grabs the region (across monitors), hides our own island window and draws the
//! mouse pointer highlight.

use crate::gfx::IRect;
use crate::screens::{self, Monitor};
use pinshot_core::compose::{compose_region, Screen};
use pinshot_core::output::to_rgba8;
use pinshot_core::{Point, Rect, Rgba};
use pinshot_record::{Frame, PixFmt};
use std::sync::{Arc, Mutex};
use tiny_skia::{Paint, PathBuilder, Pixmap, Stroke, Transform};

pub type SharedRect = Arc<Mutex<Option<Rect>>>;

pub struct FrameSource {
    pub monitors: Vec<Monitor>,
    pub region: Rect,
    pub cursor: bool,
    /// Desktop rectangle of the island window, or `None` when it is not shown.
    pub island: SharedRect,
    clean: Option<Pixmap>,
    clean_raw: Option<Vec<u8>>,
}

impl FrameSource {
    pub fn new(
        monitors: Vec<Monitor>,
        region: Rect,
        cursor: bool,
        island: SharedRect,
    ) -> FrameSource {
        FrameSource {
            monitors,
            region,
            cursor,
            island,
            clean: None,
            clean_raw: None,
        }
    }

    pub fn grab(&mut self) -> Option<Frame> {
        // fast path: the area lies on one monitor (the common case): raw BGRX straight from the screen, edited in
        // place, no colour conversion at all
        let hits: Vec<&Monitor> = self
            .monitors
            .iter()
            .filter(|m| !m.rect.intersection(&self.region).is_empty())
            .collect();
        if let [m] = hits[..] {
            let part = m.rect.intersection(&self.region);
            let scale = m.scale;
            let (w, h, mut data) = match screens::grab_raw(m, part) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("frame grab failed: {e}");
                    return None;
                }
            };
            let hidden = self
                .island
                .lock()
                .ok()
                .and_then(|g| *g)
                .and_then(|g| island_in_frame(g, part, scale));
            mask_island_raw(&mut data, w, h, &mut self.clean_raw, hidden);
            if self.cursor {
                if let Some(p) = screens::cursor_pos() {
                    draw_cursor_raw(
                        &mut data,
                        w,
                        h,
                        Point::new((p.x - part.x) * scale, (p.y - part.y) * scale),
                        scale,
                    );
                }
            }
            return Some(Frame {
                width: w,
                height: h,
                format: PixFmt::Bgr0,
                data,
            });
        }
        self.grab_composed()
    }

    /// Several monitors: stitch through pixmaps (handles different scale factors).
    fn grab_composed(&mut self) -> Option<Frame> {
        let mut parts: Vec<(Rect, f32, Pixmap)> = Vec::new();
        for m in &self.monitors {
            let part = m.rect.intersection(&self.region);
            if part.is_empty() {
                continue;
            }
            match screens::grab_part(m, part) {
                Ok(pm) => parts.push((part, m.scale, pm)),
                Err(e) => {
                    log::warn!("frame grab failed: {e}");
                    return None;
                }
            }
        }
        let screens: Vec<Screen<'_>> = parts
            .iter()
            .map(|(rect, scale, pixmap)| Screen {
                rect: *rect,
                scale: *scale,
                pixmap,
            })
            .collect();
        let (mut pm, scale) = compose_region(&screens, self.region)?;
        let hidden = self
            .island
            .lock()
            .ok()
            .and_then(|g| *g)
            .and_then(|g| island_in_frame(g, self.region, scale));
        mask_island(&mut pm, &mut self.clean, hidden);
        if self.cursor {
            if let Some(p) = screens::cursor_pos() {
                draw_cursor(
                    &mut pm,
                    Point::new((p.x - self.region.x) * scale, (p.y - self.region.y) * scale),
                    scale,
                );
            }
        }
        Some(Frame {
            width: pm.width(),
            height: pm.height(),
            format: PixFmt::Rgba,
            data: to_rgba8(&pm),
        })
    }
}

/// Same as [`mask_island`] for raw 4-byte pixels.
pub fn mask_island_raw(
    frame: &mut [u8],
    w: u32,
    h: u32,
    clean: &mut Option<Vec<u8>>,
    hidden: Option<IRect>,
) {
    let full = IRect::new(0, 0, w, h);
    let hidden = hidden.and_then(|r| r.intersect(&full));
    let usable = clean
        .as_ref()
        .map(|c| c.len() == frame.len())
        .unwrap_or(false);
    let (Some(r), true) = (hidden, usable) else {
        match clean {
            Some(c) if c.len() == frame.len() => c.copy_from_slice(frame),
            _ => *clean = Some(frame.to_vec()),
        }
        return;
    };
    let c = clean.as_mut().expect("clean copy");
    let stride = w as usize * 4;
    let (x0, x1) = (r.x as usize * 4, r.right() as usize * 4);
    for y in 0..h as usize {
        let row = y * stride;
        if (y as i32) >= r.y && (y as i32) < r.bottom() {
            // the island row: left and right parts stay live, the middle comes from the clean copy
            c[row..row + x0].copy_from_slice(&frame[row..row + x0]);
            c[row + x1..row + stride].copy_from_slice(&frame[row + x1..row + stride]);
            frame[row + x0..row + x1].copy_from_slice(&c[row + x0..row + x1]);
        } else {
            c[row..row + stride].copy_from_slice(&frame[row..row + stride]);
        }
    }
}

/// Pointer highlight on raw BGRX pixels: translucent red disc with a white ring, anti-aliased.
pub fn draw_cursor_raw(data: &mut [u8], w: u32, h: u32, pos: Point, scale: f32) {
    let r = 9.0 * scale;
    let ring = 2.0 * scale;
    let (x0, x1) = (
        (pos.x - r - ring).floor().max(0.0) as i32,
        ((pos.x + r + ring).ceil() as i32).min(w as i32 - 1),
    );
    let (y0, y1) = (
        (pos.y - r - ring).floor().max(0.0) as i32,
        ((pos.y + r + ring).ceil() as i32).min(h as i32 - 1),
    );
    if pos.x < 0.0 || pos.y < 0.0 || pos.x >= w as f32 || pos.y >= h as f32 {
        return;
    }
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 + 0.5 - pos.x).powi(2) + (y as f32 + 0.5 - pos.y).powi(2)).sqrt();
            // coverage of the disc and of the ring (centred on the edge), with a one pixel soft edge
            let disc = (r + 0.5 - d).clamp(0.0, 1.0);
            let ring_cov = (ring / 2.0 + 0.5 - (d - r).abs()).clamp(0.0, 1.0);
            if disc == 0.0 && ring_cov == 0.0 {
                continue;
            }
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let blend =
                |dst: &mut u8, src: f32, a: f32| *dst = (*dst as f32 * (1.0 - a) + src * a) as u8;
            let a_disc = 120.0 / 255.0 * disc;
            blend(&mut data[i], 48.0, a_disc); // B
            blend(&mut data[i + 1], 59.0, a_disc); // G
            blend(&mut data[i + 2], 255.0, a_disc); // R
            let a_ring = 230.0 / 255.0 * ring_cov;
            for k in 0..3 {
                blend(&mut data[i + k], 255.0, a_ring);
            }
        }
    }
}

/// The island's rectangle in frame pixels (padded a little), or `None` when it does not overlap the region.
pub fn island_in_frame(island: Rect, region: Rect, scale: f32) -> Option<IRect> {
    let local = Rect::new(island.x - region.x, island.y - region.y, island.w, island.h);
    if !local.intersects(&Rect::new(0.0, 0.0, region.w, region.h)) {
        return None;
    }
    let pad = 2.0;
    Some(IRect::around(
        (local.x - pad) * scale,
        (local.y - pad) * scale,
        (local.w + 2.0 * pad) * scale,
        (local.h + 2.0 * pad) * scale,
    ))
}

/// Replace the island's pixels by what was there just before it covered them; everything around it stays live.
pub fn mask_island(frame: &mut Pixmap, clean: &mut Option<Pixmap>, hidden: Option<IRect>) {
    let full = IRect::new(0, 0, frame.width(), frame.height());
    let hidden = hidden.and_then(|h| h.intersect(&full));
    let usable = clean
        .as_ref()
        .map(|c| c.width() == frame.width() && c.height() == frame.height())
        .unwrap_or(false);
    let (Some(h), true) = (hidden, usable) else {
        *clean = Some(frame.clone()); // nothing to hide (or no clean copy yet): remember this frame
        return;
    };
    let c = clean.as_mut().expect("clean copy");
    let w = frame.width() as usize;
    // refresh the clean copy everywhere except under the island, then patch the island area from it
    for y in 0..frame.height() as usize {
        let inside_rows = (y as i32) >= h.y && (y as i32) < h.bottom();
        for x in 0..w {
            let under = inside_rows && (x as i32) >= h.x && (x as i32) < h.right();
            let i = (y * w + x) * 4;
            if under {
                frame.data_mut()[i..i + 4].copy_from_slice(&c.data()[i..i + 4]);
            } else {
                c.data_mut()[i..i + 4].copy_from_slice(&frame.data()[i..i + 4]);
            }
        }
    }
}

/// Highlight ring around the pointer (white ring, translucent red fill), sized with the display scale.
pub fn draw_cursor(pm: &mut Pixmap, pos: Point, scale: f32) {
    if pos.x < 0.0 || pos.y < 0.0 || pos.x >= pm.width() as f32 || pos.y >= pm.height() as f32 {
        return;
    }
    let Some(path) = PathBuilder::from_circle(pos.x, pos.y, 9.0 * scale) else {
        return;
    };
    let mut fill = Paint::default();
    let c = Rgba::new(255, 59, 48, 120);
    fill.set_color_rgba8(c.r, c.g, c.b, c.a);
    fill.anti_alias = true;
    pm.fill_path(
        &path,
        &fill,
        tiny_skia::FillRule::Winding,
        Transform::identity(),
        None,
    );
    let mut ring = Paint::default();
    ring.set_color_rgba8(255, 255, 255, 230);
    ring.anti_alias = true;
    pm.stroke_path(
        &path,
        &ring,
        &Stroke {
            width: 2.0 * scale,
            ..Stroke::default()
        },
        Transform::identity(),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Pixmap {
        let mut pm = Pixmap::new(w, h).unwrap();
        for p in pm.data_mut().as_chunks_mut::<4>().0.iter_mut() {
            p.copy_from_slice(&[v, v, v, 255]);
        }
        pm
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> u8 {
        pm.pixel(x, y).unwrap().red()
    }

    #[test]
    fn island_rect_maps_into_frame_pixels_and_misses_are_none() {
        let region = Rect::new(100.0, 100.0, 400.0, 300.0);
        let r = island_in_frame(Rect::new(150.0, 120.0, 100.0, 40.0), region, 2.0).unwrap();
        assert_eq!((r.x, r.y), (96, 36));
        assert!(r.w >= 208 && r.h >= 88);
        assert!(island_in_frame(Rect::new(600.0, 600.0, 10.0, 10.0), region, 1.0).is_none());
    }

    #[test]
    fn masking_patches_the_island_with_the_previous_clean_pixels() {
        let mut clean = None;
        let mut f1 = solid(40, 20, 50);
        mask_island(&mut f1, &mut clean, None); // remembers frame 1
                                                // frame 2: the screen changed everywhere, and the island (value 255) sits at x 10..20
        let mut f2 = solid(40, 20, 90);
        for y in 0..20 {
            for x in 10..20 {
                let i = ((y * 40 + x) * 4) as usize;
                f2.data_mut()[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        mask_island(&mut f2, &mut clean, Some(IRect::new(10, 0, 10, 20)));
        assert_eq!(px(&f2, 15, 5), 50, "under the island: the old pixels");
        assert_eq!(px(&f2, 30, 5), 90, "elsewhere: live pixels");
        // frame 3: the area under the island keeps showing the last clean pixels, the rest stays live
        let mut f3 = solid(40, 20, 120);
        mask_island(&mut f3, &mut clean, Some(IRect::new(10, 0, 10, 20)));
        assert_eq!((px(&f3, 15, 5), px(&f3, 2, 2)), (50, 120));
    }

    #[test]
    fn cursor_ring_is_drawn_only_inside_the_frame() {
        let mut pm = solid(100, 100, 0);
        draw_cursor(&mut pm, Point::new(50.0, 50.0), 1.0);
        assert!(px(&pm, 50, 50) > 20, "tinted centre");
        assert!(px(&pm, 50 + 9, 50) > 150, "white ring");
        let mut other = solid(100, 100, 0);
        draw_cursor(&mut other, Point::new(-5.0, 50.0), 1.0);
        assert!(other.data().iter().all(|&b| b == 0 || b == 255));
    }

    #[test]
    fn raw_masking_matches_the_pixmap_version_semantics() {
        let (w, h) = (40u32, 20u32);
        let mk = |v: u8| vec![v; (w * h * 4) as usize];
        let mut clean = None;
        let mut f1 = mk(50);
        mask_island_raw(&mut f1, w, h, &mut clean, None);
        let mut f2 = mk(90);
        for y in 0..h as usize {
            for x in 10..20usize {
                let i = (y * w as usize + x) * 4;
                f2[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        mask_island_raw(&mut f2, w, h, &mut clean, Some(IRect::new(10, 0, 10, 20)));
        assert_eq!((f2[(5 * 40 + 15) * 4], f2[(5 * 40 + 30) * 4]), (50, 90));
        let mut f3 = mk(120);
        mask_island_raw(&mut f3, w, h, &mut clean, Some(IRect::new(10, 0, 10, 20)));
        assert_eq!((f3[(5 * 40 + 15) * 4], f3[(5 * 40 + 2) * 4]), (50, 120));
    }

    #[test]
    fn raw_cursor_ring_tints_the_centre_and_whitens_the_ring() {
        let (w, h) = (100u32, 100u32);
        let mut d = vec![0u8; (w * h * 4) as usize];
        draw_cursor_raw(&mut d, w, h, Point::new(50.0, 50.0), 1.0);
        let at = |x: usize, y: usize| &d[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3];
        assert!(
            at(50, 50)[2] > 40 && at(50, 50)[0] < 40,
            "red tint (BGR order)"
        );
        assert!(at(59, 50)[0] > 150, "white ring");
        assert_eq!(at(80, 80), &[0, 0, 0]);
        let mut other = vec![0u8; (w * h * 4) as usize];
        draw_cursor_raw(&mut other, w, h, Point::new(-3.0, 4.0), 1.0);
        assert!(other.iter().all(|&b| b == 0));
    }
}
