//! The recognition model must read Turkish letters (they are missing from the Chinese models' dictionary).

use pinshot_core::text::Fonts;
use tiny_skia::Pixmap;

/// White page with black text drawn with the embedded font.
fn page(lines: &[&str], px: f32) -> Pixmap {
    let fonts = Fonts::embedded();
    let w = 900;
    let h = (lines.len() as f32 * px * 2.2) as u32 + 40;
    let mut pm = Pixmap::new(w, h).unwrap();
    pm.fill(tiny_skia::Color::WHITE);
    for (i, line) in lines.iter().enumerate() {
        let base = 30.0 + px + i as f32 * px * 2.2;
        let mut x = 20.0;
        for ch in line.chars() {
            let g = fonts.glyph(ch, px, false);
            for gy in 0..g.height {
                for gx in 0..g.width {
                    let (px, py) = (
                        x as i32 + g.xmin + gx as i32,
                        base as i32 - g.ymin - g.height as i32 + gy as i32,
                    );
                    if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
                        let v = 255 - g.coverage[gy * g.width + gx];
                        let k = ((py as u32 * w + px as u32) * 4) as usize;
                        let d = pm.data_mut();
                        if v < d[k] {
                            d[k..k + 3].fill(v);
                        }
                    }
                }
            }
            x += g.advance;
        }
    }
    pm
}

#[test]
fn reads_turkish_letters() {
    let img = page(
        &[
            "Güneşli bir İstanbul sabahı",
            "Çocuğun öğle yemeği: pilav ve çorba",
            "ISPARTA ŞANLIURFA ÇANKIRI",
        ],
        32.0,
    );
    let text = pinshot_ocr::Engine::new().unwrap().recognize(&img).unwrap();
    println!("{text}");
    for word in [
        "Güneşli",
        "İstanbul",
        "sabahı",
        "Çocuğun",
        "öğle",
        "yemeği",
        "çorba",
    ] {
        assert!(text.contains(word), "missing {word:?} in {text:?}");
    }
}
