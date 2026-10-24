//! Text line recognition (SVTR/CTC): crop → resize to height 48 → logits → greedy CTC decode.

use crate::det::TextBox;
use crate::resize_rgb;
use tract_onnx::prelude::*;

type Plan = std::sync::Arc<TypedSimplePlan>;

const HEIGHT: usize = 48;
/// Widths the model is compiled for (a line is padded up to the next one).
const WIDTHS: [usize; 6] = [320, 640, 960, 1440, 2160, 3200];
const MAX_PLANS: usize = 3;

pub struct Recognizer {
    proto: InferenceModel,
    plans: Vec<(usize, Plan)>,
    /// Index 0 is the CTC blank; the last entry is the space.
    chars: Vec<String>,
}

pub fn parse_keys(keys: &str) -> Vec<String> {
    let mut chars = vec![String::new()];
    chars.extend(keys.lines().map(|l| l.to_string()));
    chars.push(" ".to_string());
    chars
}

/// Greedy CTC: argmax per step, collapse repeats, drop blanks. `logits` is steps × classes.
pub fn ctc_decode(logits: &[f32], classes: usize, chars: &[String]) -> (String, f32) {
    let mut text = String::new();
    let (mut last, mut score, mut n) = (usize::MAX, 0.0_f32, 0usize);
    for step in logits.chunks_exact(classes) {
        let (idx, &best) = step
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .expect("classes > 0");
        if idx != 0 && idx != last {
            if let Some(c) = chars.get(idx) {
                text.push_str(c);
                score += best;
                n += 1;
            }
        }
        last = idx;
    }
    (text, if n == 0 { 0.0 } else { score / n as f32 })
}

impl Recognizer {
    pub fn new(model: &[u8], keys: &str) -> Result<Recognizer, String> {
        let proto = tract_onnx::onnx()
            .model_for_read(&mut &model[..])
            .map_err(|e| format!("{e:#}"))?;
        Ok(Recognizer {
            proto,
            plans: Vec::new(),
            chars: parse_keys(keys),
        })
    }

    fn plan(&mut self, w: usize) -> Result<&Plan, String> {
        if let Some(i) = self.plans.iter().position(|(k, _)| *k == w) {
            let p = self.plans.remove(i);
            self.plans.push(p);
        } else {
            let mut m = self.proto.clone();
            m.set_input_fact(0, f32::fact([1usize, 3, HEIGHT, w]).into())
                .map_err(|e| format!("{e:#}"))?;
            let plan = m
                .into_optimized()
                .and_then(|m| m.into_runnable())
                .map_err(|e| format!("{e:#}"))?;
            if self.plans.len() >= MAX_PLANS {
                self.plans.remove(0);
            }
            self.plans.push((w, plan));
        }
        Ok(&self.plans.last().expect("just pushed").1)
    }

    pub fn recognize(
        &mut self,
        rgb: &[u8],
        iw: usize,
        ih: usize,
        b: &TextBox,
    ) -> Result<(String, f32), String> {
        let (x0, y0) = (
            b.x0.floor().max(0.0) as usize,
            b.y0.floor().max(0.0) as usize,
        );
        let (x1, y1) = (
            (b.x1.ceil() as usize).min(iw),
            (b.y1.ceil() as usize).min(ih),
        );
        if x1 <= x0 + 1 || y1 <= y0 + 1 {
            return Ok((String::new(), 0.0));
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut crop = Vec::with_capacity(cw * ch * 3);
        for y in y0..y1 {
            crop.extend_from_slice(&rgb[(y * iw + x0) * 3..(y * iw + x1) * 3]);
        }
        // tall narrow boxes hold vertical text: rotate to horizontal
        let (crop, cw, ch) = if ch as f32 >= cw as f32 * 1.5 {
            (rotate90(&crop, cw, ch), ch, cw)
        } else {
            (crop, cw, ch)
        };
        let want = ((HEIGHT as f32 * cw as f32 / ch as f32).ceil() as usize).max(8);
        let width = WIDTHS
            .iter()
            .copied()
            .find(|&w| w >= want)
            .unwrap_or(*WIDTHS.last().expect("widths"));
        let rw = want.min(width);
        let resized = resize_rgb(&crop, cw, ch, rw, HEIGHT);
        // BGR, (x/255 - 0.5)/0.5; padding stays 0
        let mut data = vec![0f32; 3 * HEIGHT * width];
        for y in 0..HEIGHT {
            for x in 0..rw {
                let px = &resized[(y * rw + x) * 3..];
                for (c, src) in [2usize, 1, 0].into_iter().enumerate() {
                    data[c * HEIGHT * width + y * width + x] = (px[src] as f32 / 255.0 - 0.5) / 0.5;
                }
            }
        }
        let input =
            Tensor::from_shape(&[1, 3, HEIGHT, width], &data).map_err(|e| format!("{e:#}"))?;
        let classes = self.chars.len();
        let plan = self.plan(width)?;
        let out = plan
            .run(tvec!(input.into()))
            .map_err(|e| format!("{e:#}"))?;
        let logits: Vec<f32> = out[0]
            .to_plain_array_view::<f32>()
            .map_err(|e| format!("{e:#}"))?
            .iter()
            .copied()
            .collect();
        // padded steps decode to blanks; keep only those that cover the real line
        let steps = logits.len() / classes;
        let keep = ((rw as f32 / width as f32) * steps as f32).ceil() as usize + 1;
        Ok(ctc_decode(
            &logits[..keep.min(steps) * classes],
            classes,
            &self.chars,
        ))
    }
}

fn rotate90(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    // clockwise: new width = h, new height = w
    let mut out = vec![0u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = (h - 1 - y, x);
            out[(ny * h + nx) * 3..(ny * h + nx) * 3 + 3]
                .copy_from_slice(&src[(y * w + x) * 3..(y * w + x) * 3 + 3]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_has_blank_first_and_space_last() {
        let c = parse_keys("a\nb\n");
        assert_eq!(c, vec!["", "a", "b", " "]);
    }

    #[test]
    fn ctc_collapses_repeats_and_drops_blanks() {
        let chars = parse_keys("a\nb\n");
        // classes: blank a b space ; steps: a a blank a b b → "aab"
        let rows = [
            [0.0, 0.9, 0.0, 0.0],
            [0.0, 0.8, 0.1, 0.0],
            [0.9, 0.0, 0.0, 0.0],
            [0.0, 0.7, 0.0, 0.0],
            [0.0, 0.0, 0.6, 0.0],
            [0.0, 0.0, 0.5, 0.0],
        ];
        let flat: Vec<f32> = rows.iter().flatten().copied().collect();
        let (t, s) = ctc_decode(&flat, 4, &chars);
        assert_eq!(t, "aab");
        assert!(s > 0.6 && s < 0.8);
        assert_eq!(
            ctc_decode(&[1.0, 0.0, 0.0, 0.0], 4, &chars),
            (String::new(), 0.0)
        );
    }

    #[test]
    fn rotation_swaps_dimensions_and_moves_pixels() {
        // 2x3 image (w=2,h=3), mark top-left pixel
        let mut src = vec![0u8; 2 * 3 * 3];
        src[0] = 255;
        let r = rotate90(&src, 2, 3);
        // after clockwise rotation the top-left pixel ends in the top-right of the 3x2 result
        assert_eq!(r[(0 * 3 + 2) * 3], 255);
    }
}
