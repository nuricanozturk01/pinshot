//! Text detection with the DB model: probability map → connected regions → boxes.

use crate::resize_rgb;
use std::collections::VecDeque;
use tract_onnx::prelude::*;

type Plan = std::sync::Arc<TypedSimplePlan>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl TextBox {
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
}

const THRESH: f32 = 0.3;
const BOX_THRESH: f32 = 0.5;
const UNCLIP_RATIO: f32 = 1.6;
const MIN_SIDE: usize = 736;
const MAX_SIDE: usize = 1920;
const MAX_PLANS: usize = 3;

pub struct Detector {
    proto: InferenceModel,
    plans: Vec<((usize, usize), Plan)>,
}

impl Detector {
    pub fn new(model: &[u8]) -> Result<Detector, String> {
        let proto = tract_onnx::onnx()
            .model_for_read(&mut &model[..])
            .map_err(|e| format!("{e:#}"))?;
        Ok(Detector {
            proto,
            plans: Vec::new(),
        })
    }

    fn plan(&mut self, h: usize, w: usize) -> Result<&Plan, String> {
        if let Some(i) = self.plans.iter().position(|(k, _)| *k == (h, w)) {
            let p = self.plans.remove(i);
            self.plans.push(p);
        } else {
            let mut m = self.proto.clone();
            m.set_input_fact(0, f32::fact([1usize, 3, h, w]).into())
                .map_err(|e| format!("{e:#}"))?;
            let plan = m
                .into_optimized()
                .and_then(|m| m.into_runnable())
                .map_err(|e| format!("{e:#}"))?;
            if self.plans.len() >= MAX_PLANS {
                self.plans.remove(0);
            }
            self.plans.push(((h, w), plan));
        }
        Ok(&self.plans.last().expect("just pushed").1)
    }

    /// Size the model sees: shorter side at least `MIN_SIDE`, longer side at most `MAX_SIDE`, both multiples of 32.
    pub fn target_size(w: usize, h: usize) -> (usize, usize) {
        let mut ratio = 1.0_f32;
        if w.min(h) < MIN_SIDE {
            ratio = MIN_SIDE as f32 / w.min(h) as f32;
        }
        if (w.max(h) as f32 * ratio) > MAX_SIDE as f32 {
            ratio = MAX_SIDE as f32 / w.max(h) as f32;
        }
        let r32 = |v: f32| (((v * ratio) / 32.0).round() as usize).max(1) * 32;
        (r32(w as f32), r32(h as f32))
    }

    pub fn detect(&mut self, rgb: &[u8], w: usize, h: usize) -> Result<Vec<TextBox>, String> {
        let (tw, th) = Self::target_size(w, h);
        let resized = if (tw, th) == (w, h) {
            rgb.to_vec()
        } else {
            resize_rgb(rgb, w, h, tw, th)
        };
        // BGR order, (x/255 - 0.5)/0.5
        let mut data = vec![0f32; 3 * th * tw];
        for (i, px) in resized.chunks_exact(3).enumerate() {
            for (c, src) in [2usize, 1, 0].into_iter().enumerate() {
                data[c * th * tw + i] = (px[src] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
        let input = Tensor::from_shape(&[1, 3, th, tw], &data).map_err(|e| format!("{e:#}"))?;
        let plan = self.plan(th, tw)?;
        let out = plan
            .run(tvec!(input.into()))
            .map_err(|e| format!("{e:#}"))?;
        let prob = out[0]
            .to_plain_array_view::<f32>()
            .map_err(|e| format!("{e:#}"))?;
        let flat: Vec<f32> = prob.iter().copied().collect();
        Ok(boxes_from_map(
            &flat,
            tw,
            th,
            w as f32 / tw as f32,
            h as f32 / th as f32,
        ))
    }
}

/// Connected regions above the threshold → unclipped boxes in original coordinates.
pub fn boxes_from_map(prob: &[f32], w: usize, h: usize, sx: f32, sy: f32) -> Vec<TextBox> {
    // dilate the binary map slightly (as the reference implementation does) so glyph gaps do not split words
    let mut bin: Vec<bool> = prob.iter().map(|&p| p > THRESH).collect();
    let src = bin.clone();
    for y in 0..h {
        for x in 0..w {
            if src[y * w + x] {
                for (dx, dy) in [(1usize, 0usize), (0, 1), (1, 1)] {
                    if x + dx < w && y + dy < h {
                        bin[(y + dy) * w + x + dx] = true;
                    }
                }
            }
        }
    }
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    for start in 0..w * h {
        if !bin[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let (mut sum, mut n) = (0.0_f32, 0usize);
        let mut q = VecDeque::from([start]);
        seen[start] = true;
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            sum += prob[i];
            n += 1;
            let mut push = |j: usize| {
                if bin[j] && !seen[j] {
                    seen[j] = true;
                    q.push_back(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
        let (bw, bh) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        if n < 12 || bw.min(bh) < 3.0 || sum / n as f32 <= BOX_THRESH {
            continue;
        }
        // DB unclip: grow by area * ratio / perimeter
        let d = bw * bh * UNCLIP_RATIO / (2.0 * (bw + bh));
        let b = TextBox {
            x0: ((x0 as f32 - d) * sx).max(0.0),
            y0: ((y0 as f32 - d) * sy).max(0.0),
            x1: ((x1 as f32 + 1.0 + d) * sx).min(w as f32 * sx),
            y1: ((y1 as f32 + 1.0 + d) * sy).min(h as f32 * sy),
        };
        if b.w() >= 3.0 && b.h() >= 3.0 {
            out.push(b);
        }
    }
    out
}

/// Reading order: top to bottom, left to right; boxes on (nearly) the same row are ordered by x.
pub fn sort_boxes(mut boxes: Vec<TextBox>) -> Vec<TextBox> {
    boxes.sort_by(|a, b| {
        (a.y0, a.x0)
            .partial_cmp(&(b.y0, b.x0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for i in 0..boxes.len().saturating_sub(1) {
        for j in (0..=i).rev() {
            let (a, b) = (boxes[j], boxes[j + 1]);
            let same_row = (b.y0 - a.y0).abs() < (a.h().min(b.h()) * 0.5).max(10.0);
            if same_row && b.x0 < a.x0 {
                boxes.swap(j, j + 1);
            } else {
                break;
            }
        }
    }
    boxes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_size_scales_small_and_large_images_into_range() {
        let (w, h) = Detector::target_size(400, 400);
        assert!(h >= 704 && w % 32 == 0 && h % 32 == 0, "{w}x{h}");
        let (w, h) = Detector::target_size(400, 100);
        assert!(
            w <= 1920 && w % 32 == 0 && h % 32 == 0,
            "wide strips are capped: {w}x{h}"
        );
        let (w, h) = Detector::target_size(1000, 800);
        assert_eq!((w, h), (992, 800));
        let (w, h) = Detector::target_size(5000, 2000);
        assert!(w <= 1920 && h % 32 == 0);
    }

    #[test]
    fn finds_regions_in_a_probability_map_and_ignores_noise() {
        let (w, h) = (100, 60);
        let mut map = vec![0.0f32; w * h];
        for y in 20..32 {
            for x in 10..70 {
                map[y * w + x] = 0.9;
            }
        }
        map[5 * w + 5] = 0.9; // speckle
        let boxes = boxes_from_map(&map, w, h, 1.0, 1.0);
        assert_eq!(boxes.len(), 1);
        let b = boxes[0];
        assert!(
            b.x0 < 10.0 && b.x1 > 70.0 && b.y0 < 20.0 && b.y1 > 32.0,
            "unclipped: {b:?}"
        );
        let scaled = boxes_from_map(&map, w, h, 2.0, 3.0);
        assert!(scaled[0].x1 > 140.0 && scaled[0].y1 > 96.0);
    }

    #[test]
    fn boxes_are_sorted_into_reading_order() {
        let b = |x: f32, y: f32| TextBox {
            x0: x,
            y0: y,
            x1: x + 50.0,
            y1: y + 20.0,
        };
        let sorted = sort_boxes(vec![
            b(200.0, 102.0),
            b(10.0, 100.0),
            b(10.0, 10.0),
            b(120.0, 12.0),
        ]);
        let xs: Vec<(i32, i32)> = sorted.iter().map(|b| (b.x0 as i32, b.y0 as i32)).collect();
        assert_eq!(xs, vec![(10, 10), (120, 12), (10, 100), (200, 102)]);
    }
}
