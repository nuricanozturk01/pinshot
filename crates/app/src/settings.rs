//! Settings: mapping between the configuration and the form rows.

use crate::form::{Control, Form, Row};
use pinshot_core::config::Config;
use pinshot_core::hotkey::{to_display, to_stored};
use pinshot_core::i18n::tr;

const FORMATS: [&str; 3] = ["png", "jpg", "webp"];
const REC_FORMATS: [&str; 2] = ["mp4", "gif"];
const MP4_FPS: [i32; 5] = [10, 15, 24, 30, 60];
const GIF_FPS: [i32; 5] = [10, 15, 24, 30, 50];
const GIF_WIDTHS: [i32; 5] = [640, 960, 1280, 1920, 0];
const VIDEO_WIDTHS: [i32; 4] = [1280, 1920, 2560, 0];
const LANGUAGES: [&str; 3] = ["auto", "en", "tr"];
pub const HOTKEY_ROWS: [(&str, &str); 5] = [
    ("hotkey_capture", "Bölge yakala"),
    ("hotkey_fullscreen", "Tam ekran yakala"),
    ("hotkey_repeat", "Son bölgeyi tekrarla"),
    ("hotkey_pin", "Panodan sabitle"),
    ("hotkey_record", "Ekran kaydını başlat / durdur"),
];

fn cycle(options: Vec<String>, index: usize) -> Control {
    Control::Cycle { options, index }
}

fn pos<T: PartialEq>(list: &[T], v: &T, default: usize) -> usize {
    list.iter().position(|x| x == v).unwrap_or(default)
}

fn nearest(list: &[i32], v: i32) -> usize {
    (0..list.len())
        .min_by_key(|&i| (list[i] - v).abs())
        .unwrap_or(0)
}

fn width_label(v: i32) -> String {
    if v == 0 {
        tr("Orijinal").to_string()
    } else {
        format!("{v} px")
    }
}

pub fn build(cfg: &Config, mac: bool, width: f32, height: f32) -> Form {
    let r = |id: &'static str, label: &str, control: Control| Row {
        id,
        label: label.to_string(),
        control,
    };
    let sec = |id: &'static str, label: &str| Row {
        id,
        label: label.to_string(),
        control: Control::Section,
    };
    let mut rows = vec![
        sec("s_save", tr("Kayıt")),
        r(
            "save_dir",
            tr("Klasör"),
            Control::Folder(if cfg.save_dir.is_empty() {
                cfg.save_dir_path().to_string_lossy().into_owned()
            } else {
                cfg.save_dir.clone()
            }),
        ),
        r(
            "format",
            tr("Biçim"),
            cycle(
                FORMATS.iter().map(|s| s.to_uppercase()).collect(),
                pos(&FORMATS, &cfg.format.as_str(), 0),
            ),
        ),
        r(
            "quality",
            tr("JPEG/WebP kalitesi"),
            Control::Stepper {
                value: cfg.quality as i32,
                min: 1,
                max: 100,
                step: 5,
                suffix: "%".into(),
            },
        ),
        r(
            "auto_save",
            tr("Her yakalamayı otomatik klasöre kaydet"),
            Control::Check(cfg.auto_save),
        ),
        sec("s_rec", tr("Video kaydı")),
        r(
            "record_format",
            tr("Varsayılan biçim"),
            cycle(
                REC_FORMATS.iter().map(|s| s.to_uppercase()).collect(),
                pos(&REC_FORMATS, &cfg.record_format.as_str(), 0),
            ),
        ),
        r(
            "record_fps",
            tr("Kare hızı (MP4)"),
            cycle(
                MP4_FPS.iter().map(|v| format!("{v} fps")).collect(),
                nearest(&MP4_FPS, cfg.record_fps as i32),
            ),
        ),
        r(
            "record_fps_gif",
            tr("Kare hızı (GIF, en fazla 50)"),
            cycle(
                GIF_FPS.iter().map(|v| format!("{v} fps")).collect(),
                nearest(&GIF_FPS, cfg.record_fps_gif as i32),
            ),
        ),
        r(
            "record_gif_width",
            tr("GIF en fazla genişlik"),
            cycle(
                GIF_WIDTHS.iter().map(|v| width_label(*v)).collect(),
                pos(&GIF_WIDTHS, &(cfg.record_gif_width as i32), 3),
            ),
        ),
        r(
            "record_video_width",
            tr("Video en fazla genişlik"),
            cycle(
                VIDEO_WIDTHS.iter().map(|v| width_label(*v)).collect(),
                pos(&VIDEO_WIDTHS, &(cfg.record_video_width as i32), 3),
            ),
        ),
        r(
            "record_max_minutes",
            tr("En uzun kayıt"),
            Control::Stepper {
                value: (cfg.record_max_seconds / 60).clamp(1, 180) as i32,
                min: 1,
                max: 180,
                step: 1,
                suffix: format!(" {}", tr("dk")),
            },
        ),
        r(
            "record_cursor",
            tr("Fare imlecini göster"),
            Control::Check(cfg.record_cursor),
        ),
        r(
            "record_quality_444",
            tr("Maksimum kalite (4:4:4 renk)"),
            Control::Check(cfg.record_quality_444),
        ),
        sec("s_keys", tr("Kısayollar")),
    ];
    for (id, label) in HOTKEY_ROWS {
        let stored = match id {
            "hotkey_capture" => &cfg.hotkey_capture,
            "hotkey_fullscreen" => &cfg.hotkey_fullscreen,
            "hotkey_repeat" => &cfg.hotkey_repeat,
            "hotkey_pin" => &cfg.hotkey_pin,
            _ => &cfg.hotkey_record,
        };
        rows.push(r(id, tr(label), Control::Hotkey(to_display(stored, mac))));
    }
    rows.push(sec("s_general", tr("Genel")));
    rows.push(r(
        "language",
        tr("Dil"),
        cycle(
            vec!["Auto".into(), "English".into(), "Türkçe".into()],
            pos(&LANGUAGES, &cfg.language.as_str(), 0),
        ),
    ));
    rows.push(r(
        "autostart",
        tr("Oturum açıldığında başlat"),
        Control::Check(cfg.autostart),
    ));
    rows.push(r(
        "pin_border",
        tr("Sabitlenen görsellerde kenarlık göster"),
        Control::Check(cfg.pin_border),
    ));
    Form::new(rows, width, height)
}

/// Write the form back into the configuration.
pub fn apply(form: &Form, cfg: &mut Config, mac: bool) {
    if let Some(crate::form::Control::Folder(p)) = form.row("save_dir").map(|r| &r.control) {
        let default = Config::default();
        cfg.save_dir = if *p == default.save_dir_path().to_string_lossy() {
            String::new()
        } else {
            p.clone()
        };
    }
    if let Some((i, _)) = form.cycle("format") {
        cfg.format = FORMATS[i].to_string();
    }
    if let Some(q) = form.number("quality") {
        cfg.quality = q.clamp(1, 100) as u8;
    }
    cfg.auto_save = form.check("auto_save");
    if let Some((i, _)) = form.cycle("record_format") {
        cfg.record_format = REC_FORMATS[i].to_string();
    }
    if let Some((i, _)) = form.cycle("record_fps") {
        cfg.record_fps = MP4_FPS[i] as u32;
    }
    if let Some((i, _)) = form.cycle("record_fps_gif") {
        cfg.record_fps_gif = GIF_FPS[i] as u32;
    }
    if let Some((i, _)) = form.cycle("record_gif_width") {
        cfg.record_gif_width = GIF_WIDTHS[i] as u32;
    }
    if let Some((i, _)) = form.cycle("record_video_width") {
        cfg.record_video_width = VIDEO_WIDTHS[i] as u32;
    }
    if let Some(m) = form.number("record_max_minutes") {
        cfg.record_max_seconds = m.clamp(1, 180) as u32 * 60;
    }
    cfg.record_cursor = form.check("record_cursor");
    cfg.record_quality_444 = form.check("record_quality_444");
    for (id, _) in HOTKEY_ROWS {
        if let Some(crate::form::Control::Hotkey(v)) = form.row(id).map(|r| &r.control) {
            let stored = to_stored(v, mac);
            match id {
                "hotkey_capture" => cfg.hotkey_capture = stored,
                "hotkey_fullscreen" => cfg.hotkey_fullscreen = stored,
                "hotkey_repeat" => cfg.hotkey_repeat = stored,
                "hotkey_pin" => cfg.hotkey_pin = stored,
                _ => cfg.hotkey_record = stored,
            }
        }
    }
    if let Some((i, _)) = form.cycle("language") {
        cfg.language = LANGUAGES[i].to_string();
    }
    cfg.autostart = form.check("autostart");
    cfg.pin_border = form.check("pin_border");
}

/// Combo text such as `Ctrl+Alt+A` for a key press. `key` is a Qt-style key name (`A`, `F3`, `Return`...).
/// Returns `None` when the key alone is not a usable shortcut (a modifier, or a plain letter without modifiers).
pub fn combo_text(ctrl: bool, alt: bool, shift: bool, meta: bool, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let is_function =
        key.len() > 1 && key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit());
    if !(ctrl || alt || meta || is_function) {
        return None; // a bare letter would steal normal typing
    }
    let mut out = Vec::new();
    for (on, n) in [
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
        (meta, "Meta"),
    ] {
        if on {
            out.push(n.to_string());
        }
    }
    out.push(key.to_string());
    Some(out.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::Control;

    #[test]
    fn form_round_trips_the_configuration() {
        let cfg = Config {
            format: "webp".into(),
            quality: 70,
            auto_save: true,
            record_fps: 24,
            record_max_seconds: 300,
            language: "tr".into(),
            pin_border: false,
            ..Config::default()
        };
        let form = build(&cfg, false, 480.0, 600.0);
        let mut back = Config::default();
        apply(&form, &mut back, false);
        assert_eq!(
            (
                back.format.as_str(),
                back.quality,
                back.auto_save,
                back.record_fps
            ),
            ("webp", 70, true, 24)
        );
        assert_eq!(
            (
                back.record_max_seconds,
                back.language.as_str(),
                back.pin_border
            ),
            (300, "tr", false)
        );
        assert_eq!(
            back.hotkey_capture, cfg.hotkey_capture,
            "shortcut text survives display/stored conversion"
        );
        assert_eq!(back.save_dir, "", "the default folder is stored as empty");
    }

    #[test]
    fn edits_land_in_the_config_and_shortcuts_become_stored_form() {
        let mut form = build(&Config::default(), false, 480.0, 600.0);
        form.row_mut("hotkey_capture").unwrap().control = Control::Hotkey("Ctrl+Shift+X".into());
        form.row_mut("autostart").unwrap().control = Control::Check(true);
        form.row_mut("save_dir").unwrap().control = Control::Folder("/data/shots".into());
        let mut cfg = Config::default();
        apply(&form, &mut cfg, false);
        assert_eq!(cfg.hotkey_capture, "<ctrl>+<shift>+x");
        assert!(cfg.autostart);
        assert_eq!(cfg.save_dir, "/data/shots");
    }

    #[test]
    fn combo_text_rejects_bare_letters_and_accepts_modified_keys() {
        assert_eq!(
            combo_text(true, true, false, false, "A"),
            Some("Ctrl+Alt+A".into())
        );
        assert_eq!(
            combo_text(false, false, false, false, "F3"),
            Some("F3".into())
        );
        assert_eq!(
            combo_text(false, false, true, false, "A"),
            None,
            "Shift alone does not make a shortcut"
        );
        assert_eq!(combo_text(false, false, false, false, "A"), None);
        assert_eq!(combo_text(true, false, false, false, ""), None);
    }
}
