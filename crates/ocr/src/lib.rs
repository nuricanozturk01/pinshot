//! Offline OCR: PP-OCRv4 detection + recognition models run with tract (pure Rust, no native libraries).
//!
//! Pipeline: scale → text detection (DB) → boxes → per-line recognition (CTC) → text. Horizontal and vertical
//! lines are handled; arbitrary rotation is not.

mod det;
mod rec;

use tiny_skia::Pixmap;

pub use det::TextBox;

static DET_MODEL: &[u8] = include_bytes!("../../../assets/ocr/ch_PP-OCRv4_det_infer.onnx");
static REC_MODEL: &[u8] = include_bytes!("../../../assets/ocr/ch_PP-OCRv4_rec_infer.onnx");
static REC_KEYS: &str = include_str!("../../../assets/ocr/rec_keys.txt");

/// Lowest recognition confidence that is still reported.
const MIN_SCORE: f32 = 0.5;

pub struct Engine {
    det: det::Detector,
    rec: rec::Recognizer,
}

impl Engine {
    pub fn new() -> Result<Engine, String> {
        Ok(Engine {
            det: det::Detector::new(DET_MODEL)?,
            rec: rec::Recognizer::new(REC_MODEL, REC_KEYS)?,
        })
    }

    /// Recognise all text in `img`, one line per detected text line, top to bottom.
    pub fn recognize(&mut self, img: &Pixmap) -> Result<String, String> {
        let rgb = to_rgb(img);
        let boxes = self
            .det
            .detect(&rgb, img.width() as usize, img.height() as usize)?;
        let mut lines: Vec<(TextBox, String)> = Vec::new();
        for b in det::sort_boxes(boxes) {
            let (text, score) =
                self.rec
                    .recognize(&rgb, img.width() as usize, img.height() as usize, &b)?;
            if score >= MIN_SCORE && !text.trim().is_empty() {
                lines.push((b, text));
            }
        }
        Ok(lines
            .into_iter()
            .map(|(_, t)| t)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Opaque RGB bytes (alpha composited over white).
pub fn to_rgb(img: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(img.width() as usize * img.height() as usize * 3);
    for p in img.pixels() {
        let a = p.alpha() as u32;
        let c = p.demultiply();
        let f = |v: u8| ((v as u32 * a + 255 * (255 - a)) / 255) as u8;
        out.extend_from_slice(&[f(c.red()), f(c.green()), f(c.blue())]);
    }
    out
}

/// Bilinear resize of tightly packed RGB.
pub(crate) fn resize_rgb(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    let mut out = vec![0u8; dw * dh * 3];
    let (xr, yr) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
    for y in 0..dh {
        let fy = ((y as f32 + 0.5) * yr - 0.5).clamp(0.0, (sh - 1) as f32);
        let (y0, ty) = (fy as usize, fy.fract());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..dw {
            let fx = ((x as f32 + 0.5) * xr - 0.5).clamp(0.0, (sw - 1) as f32);
            let (x0, tx) = (fx as usize, fx.fract());
            let x1 = (x0 + 1).min(sw - 1);
            for k in 0..3 {
                let p = |xx: usize, yy: usize| src[(yy * sw + xx) * 3 + k] as f32;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bot = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * dw + x) * 3 + k] = (top * (1.0 - ty) + bot * ty).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinshot_core::pin::render_text;

    #[test]
    fn resize_keeps_flat_colours_and_size() {
        let src = vec![200u8; 4 * 4 * 3];
        let out = resize_rgb(&src, 4, 4, 9, 7);
        assert_eq!(out.len(), 9 * 7 * 3);
        assert!(out.iter().all(|&v| v == 200));
    }

    #[test]
    fn reads_rendered_text_end_to_end() {
        let mut engine = Engine::new().expect("models load");
        let card = render_text("Hello World 12345\nScreenshot tool for everyone");
        let text = engine.recognize(&card).expect("recognition runs");
        let lower = text.to_lowercase();
        assert!(lower.contains("hello"), "got {text:?}");
        assert!(lower.contains("12345"), "got {text:?}");
        assert!(lower.contains("screenshot"), "got {text:?}");
        assert!(
            text.lines().count() >= 2,
            "two lines expected, got {text:?}"
        );
        // a blank page has no text
        let mut blank = Pixmap::new(300, 200).unwrap();
        blank.fill(tiny_skia::Color::WHITE);
        assert_eq!(engine.recognize(&blank).unwrap(), "");
    }
}
