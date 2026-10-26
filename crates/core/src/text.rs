//! Text measurement and glyph rasterisation with the embedded Noto Sans fonts (covers Turkish and most Latin scripts).
//!
//! `fontdue` expands every glyph of a font into outlines in memory (about 15 MB for the full 3884 glyphs of one Noto
//! Sans face), so the always-loaded faces are Latin subsets. The full faces are parsed only when a character outside
//! the subset (Cyrillic, Greek) is first used.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular-Latin.ttf");
static BOLD: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold-Latin.ttf");
static REGULAR_FULL: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Regular.ttf");
static BOLD_FULL: &[u8] = include_bytes!("../../../assets/fonts/NotoSans-Bold.ttf");

fn load(data: &[u8]) -> Font {
    // INVARIANT: the bytes are the embedded font files
    Font::from_bytes(
        data,
        FontSettings {
            // glyphs are looked up by character, never by index, so ligature substitutions would only cost memory
            load_substitutions: false,
            ..FontSettings::default()
        },
    )
    .expect("embedded font is valid")
}

/// Anything that can tell how large a piece of text is. Annotation hit-testing only needs this, so it can be tested
/// with a trivial fake.
pub trait TextMeasure {
    /// (width of the widest line, total height) in pixels for text drawn at `px` pixels.
    fn measure(&self, text: &str, px: f32, bold: bool) -> (f32, f32);
}

/// One rasterised glyph: coverage bitmap (0..255) and where to draw it relative to the pen position.
#[derive(Clone)]
pub struct Glyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

pub struct Fonts {
    regular: Font,
    bold: Font,
    full_regular: OnceLock<Font>,
    full_bold: OnceLock<Font>,
    cache: Mutex<HashMap<(char, u32, bool), Glyph>>,
}

impl Fonts {
    pub fn embedded() -> &'static Fonts {
        static FONTS: OnceLock<Fonts> = OnceLock::new();
        FONTS.get_or_init(Fonts::new)
    }

    fn new() -> Fonts {
        Fonts {
            regular: load(REGULAR),
            bold: load(BOLD),
            full_regular: OnceLock::new(),
            full_bold: OnceLock::new(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The Latin subset: line metrics are the same in the full face.
    fn font(&self, bold: bool) -> &Font {
        if bold {
            &self.bold
        } else {
            &self.regular
        }
    }

    /// The face that draws `ch`: the subset, or the full face for characters the subset lacks.
    fn font_for(&self, ch: char, bold: bool) -> &Font {
        let subset = self.font(bold);
        if ch.is_control() || subset.lookup_glyph_index(ch) != 0 {
            return subset;
        }
        if bold {
            self.full_bold.get_or_init(|| load(BOLD_FULL))
        } else {
            self.full_regular.get_or_init(|| load(REGULAR_FULL))
        }
    }

    /// Distance between two baselines.
    pub fn line_height(&self, px: f32, bold: bool) -> f32 {
        self.font(bold)
            .horizontal_line_metrics(px)
            .map(|m| m.new_line_size)
            .unwrap_or(px * 1.3)
    }

    /// Distance from the top of a line box to its baseline.
    pub fn ascent(&self, px: f32, bold: bool) -> f32 {
        self.font(bold)
            .horizontal_line_metrics(px)
            .map(|m| m.ascent)
            .unwrap_or(px)
    }

    pub fn advance(&self, ch: char, px: f32, bold: bool) -> f32 {
        self.font_for(ch, bold).metrics(ch, px).advance_width
    }

    pub fn line_width(&self, line: &str, px: f32, bold: bool) -> f32 {
        line.chars().map(|c| self.advance(c, px, bold)).sum()
    }

    /// Rasterised glyph, cached (sizes are rounded to quarter pixels to keep the cache small).
    pub fn glyph(&self, ch: char, px: f32, bold: bool) -> Glyph {
        let key = (ch, (px * 4.0).round() as u32, bold);
        if let Some(g) = self.cache.lock().unwrap().get(&key) {
            return g.clone();
        }
        let (m, coverage) = self.font_for(ch, bold).rasterize(ch, key.1 as f32 / 4.0);
        let g = Glyph {
            width: m.width,
            height: m.height,
            xmin: m.xmin,
            ymin: m.ymin,
            advance: m.advance_width,
            coverage,
        };
        self.cache.lock().unwrap().insert(key, g.clone());
        g
    }
}

impl TextMeasure for Fonts {
    fn measure(&self, text: &str, px: f32, bold: bool) -> (f32, f32) {
        let lines: Vec<&str> = text.split('\n').collect();
        let w = lines
            .iter()
            .map(|l| self.line_width(l, px, bold))
            .fold(0.0_f32, f32::max);
        (w, self.line_height(px, bold) * lines.len() as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_lines_and_turkish_letters() {
        let f = Fonts::embedded();
        let (w1, h1) = f.measure("Merhaba", 20.0, false);
        let (w2, h2) = f.measure("Merhaba\nİğüşçö", 20.0, false);
        assert!(w1 > 40.0 && h1 > 15.0);
        assert!(h2 > h1 * 1.9, "two lines are about twice as tall");
        assert!(w2 >= w1);
        // Turkish letters must exist in the font (a missing glyph would be a zero-width box)
        for ch in "ğüşıöçĞÜŞİÖÇ".chars() {
            assert!(f.advance(ch, 20.0, false) > 5.0, "no glyph for {ch}");
        }
    }

    #[test]
    fn subset_draws_like_the_full_faces() {
        // a private instance: the shared one may have loaded the full faces for another test
        let f = Fonts::new();
        for bold in [false, true] {
            let full = load(if bold { BOLD_FULL } else { REGULAR_FULL });
            assert_eq!(
                f.line_height(20.0, bold),
                full.horizontal_line_metrics(20.0).unwrap().new_line_size
            );
            assert_eq!(
                f.ascent(20.0, bold),
                full.horizontal_line_metrics(20.0).unwrap().ascent
            );
            for ch in "Merhaba Dünya ğüşıöçĞÜŞİÖÇ 0123 −×·–…“”€₺".chars()
            {
                let (m, cov) = full.rasterize(ch, 20.0);
                let g = f.glyph(ch, 20.0, bold);
                assert_eq!(
                    (g.width, g.height, g.xmin, g.ymin),
                    (m.width, m.height, m.xmin, m.ymin)
                );
                assert_eq!(g.advance, m.advance_width, "advance of {ch}");
                assert_eq!(g.coverage, cov, "bitmap of {ch}");
            }
        }
        assert!(
            f.full_regular.get().is_none(),
            "Latin text must not load the full face"
        );
    }

    #[test]
    fn cyrillic_and_greek_fall_back_to_the_full_face() {
        let f = Fonts::embedded();
        for ch in "Привет Ωμέγα".chars().filter(|c| !c.is_whitespace()) {
            assert!(f.advance(ch, 20.0, false) > 5.0, "no glyph for {ch}");
            assert!(f.glyph(ch, 20.0, true).width > 0, "no bold glyph for {ch}");
        }
        assert!(f.full_regular.get().is_some() && f.full_bold.get().is_some());
    }

    #[test]
    fn bold_is_wider_and_glyphs_have_coverage() {
        let f = Fonts::embedded();
        assert!(f.line_width("Hello", 20.0, true) > f.line_width("Hello", 20.0, false));
        let g = f.glyph('H', 24.0, false);
        assert!(g.width > 5 && g.height > 10 && g.coverage.iter().any(|&c| c > 200));
        assert_eq!(
            f.glyph('H', 24.0, false).coverage,
            g.coverage,
            "cache returns the same bitmap"
        );
    }
}
