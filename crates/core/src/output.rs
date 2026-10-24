//! Saving images and naming files.

use image::{codecs::jpeg::JpegEncoder, ExtendedColorType, ImageEncoder};
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

pub const FORMATS: [&str; 3] = ["png", "jpg", "webp"];

pub fn timestamp_name(fmt: &str) -> String {
    format!(
        "PinShot_{}.{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        fmt
    )
}

/// `a.png` → `a.png` if free, otherwise `a_2.png`, `a_3.png`, …
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    (2..)
        .map(|n| dir.join(format!("{stem}_{n}{ext}")))
        .find(|p| !p.exists())
        .expect("endless range")
}

/// Straight (non-premultiplied) RGBA bytes of a pixmap.
pub fn to_rgba8(pm: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

pub fn from_rgba8(w: u32, h: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    if rgba.len() != (w * h * 4) as usize {
        return None;
    }
    for (dst, src) in pm.pixels_mut().iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = tiny_skia::ColorU8::from_rgba(src[0], src[1], src[2], src[3]).premultiply();
    }
    Some(pm)
}

/// Write `pm` using the extension of `path` (png, jpg/jpeg, webp). `quality` (1–100) applies to JPEG only: WebP is
/// written lossless because the pure-Rust encoder has no lossy mode.
pub fn save_image(pm: &Pixmap, path: &Path, quality: u8) -> Result<(), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "png".into());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let rgba = to_rgba8(pm);
    let (w, h) = (pm.width(), pm.height());
    // write to a temp name first so a failure never leaves a truncated picture behind
    let tmp = path.with_extension(format!("{ext}.part"));
    let result = (|| -> Result<(), String> {
        let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let out = BufWriter::new(file);
        match ext.as_str() {
            "jpg" | "jpeg" => {
                let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(flatten_on_white).collect();
                JpegEncoder::new_with_quality(out, quality.clamp(1, 100))
                    .write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                    .map_err(|e| e.to_string())
            }
            "webp" => image::codecs::webp::WebPEncoder::new_lossless(out)
                .write_image(&rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(|e| e.to_string()),
            _ => image::codecs::png::PngEncoder::new(out)
                .write_image(&rgba, w, h, ExtendedColorType::Rgba8)
                .map_err(|e| e.to_string()),
        }
    })();
    match result {
        Ok(()) => fs::rename(&tmp, path).map_err(|e| e.to_string()),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            log::error!("save failed {}: {e}", path.display());
            Err(e)
        }
    }
}

fn flatten_on_white(p: &[u8]) -> [u8; 3] {
    let a = p[3] as u32;
    let f = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
    [f(p[0]), f(p[1]), f(p[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Pixmap {
        let mut pm = Pixmap::new(16, 8).unwrap();
        for (i, p) in pm.pixels_mut().iter_mut().enumerate() {
            *p = tiny_skia::ColorU8::from_rgba((i * 7) as u8, 100, 200, 255).premultiply();
        }
        pm
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pinshot_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn png_round_trips_exactly_and_creates_folders() {
        let d = tmpdir("png");
        let p = d.join("sub/dir/a.png");
        save_image(&sample(), &p, 92).unwrap();
        let back = image::open(&p).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (16, 8));
        assert_eq!(back.as_raw(), &to_rgba8(&sample()));
        assert!(!p.with_extension("png.part").exists());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn jpg_and_webp_are_written_and_readable() {
        let d = tmpdir("fmt");
        for ext in ["jpg", "jpeg", "webp"] {
            let p = d.join(format!("a.{ext}"));
            save_image(&sample(), &p, 80).unwrap();
            assert_eq!(
                image::open(&p).unwrap().to_rgba8().dimensions(),
                (16, 8),
                "{ext}"
            );
        }
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn unique_path_counts_up() {
        let d = tmpdir("uniq");
        fs::create_dir_all(&d).unwrap();
        let p = d.join("x.png");
        assert_eq!(unique_path(&p), p);
        fs::write(&p, b"1").unwrap();
        assert_eq!(unique_path(&p), d.join("x_2.png"));
        fs::write(d.join("x_2.png"), b"1").unwrap();
        assert_eq!(unique_path(&p), d.join("x_3.png"));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn unwritable_target_reports_an_error() {
        let d = tmpdir("err");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("file"), b"x").unwrap();
        assert!(
            save_image(&sample(), &d.join("file/a.png"), 90).is_err(),
            "parent is a file"
        );
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn names_and_rgba_conversion() {
        let n = timestamp_name("png");
        assert!(
            n.starts_with("PinShot_")
                && n.ends_with(".png")
                && n.len() == "PinShot_20260101_000000.png".len()
        );
        let pm = from_rgba8(2, 1, &[255, 0, 0, 255, 0, 255, 0, 128]).unwrap();
        assert_eq!(pm.pixel(0, 0).unwrap().red(), 255);
        assert_eq!(pm.pixel(1, 0).unwrap().alpha(), 128);
        assert!(from_rgba8(2, 2, &[0; 3]).is_none());
    }
}
