//! JSON settings. The file name, location and keys match the Python edition, so settings carry over.

use crate::hotkey;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 2;

/// macOS: Option turns the key into another character (`Option+A` types "å"), and F3 is Mission Control.
pub fn default_hotkeys(mac: bool) -> [(&'static str, &'static str); 5] {
    if mac {
        [
            ("hotkey_capture", "<ctrl>+<shift>+a"),
            ("hotkey_fullscreen", "<ctrl>+<shift>+f"),
            ("hotkey_repeat", "<ctrl>+<shift>+r"),
            ("hotkey_record", "<ctrl>+<shift>+v"),
            ("hotkey_pin", "<ctrl>+<shift>+p"),
        ]
    } else {
        [
            ("hotkey_capture", "<ctrl>+<alt>+a"),
            ("hotkey_fullscreen", "<ctrl>+<alt>+f"),
            ("hotkey_repeat", "<ctrl>+<alt>+r"),
            ("hotkey_record", "<ctrl>+<alt>+v"),
            ("hotkey_pin", "<f3>"),
        ]
    }
}

fn is_mac() -> bool {
    cfg!(target_os = "macos")
}

fn hk(name: &str) -> String {
    default_hotkeys(is_mac())
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub config_version: u32,
    pub save_dir: String,
    pub format: String,
    pub quality: u8,
    pub auto_save: bool,
    pub hotkey_capture: String,
    pub hotkey_fullscreen: String,
    pub hotkey_repeat: String,
    pub hotkey_record: String,
    pub hotkey_pin: String,
    pub record_format: String,
    pub record_fps: u32,
    pub record_fps_gif: u32,
    pub record_quality_444: bool,
    pub record_audio_system: bool,
    pub record_audio_mic: bool,
    pub record_mic_device: String,
    pub record_max_seconds: u32,
    pub record_cursor: bool,
    pub record_gif_width: u32,
    pub record_video_width: u32,
    pub island_pos: Option<[i32; 2]>,
    pub color: String,
    pub pen_width: u32,
    pub font_px: u32,
    pub pin_border: bool,
    pub autostart: bool,
    pub mac_permission_help_shown: bool,
    pub last_region: Option<(String, f32, f32, f32, f32)>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            config_version: CONFIG_VERSION,
            save_dir: String::new(),
            format: "png".into(),
            quality: 92,
            auto_save: false,
            hotkey_capture: hk("hotkey_capture"),
            hotkey_fullscreen: hk("hotkey_fullscreen"),
            hotkey_repeat: hk("hotkey_repeat"),
            hotkey_record: hk("hotkey_record"),
            hotkey_pin: hk("hotkey_pin"),
            record_format: "mp4".into(),
            record_fps: 30,
            record_fps_gif: 15,
            record_quality_444: true,
            record_audio_system: false,
            record_audio_mic: false,
            record_mic_device: String::new(),
            record_max_seconds: 600,
            record_cursor: true,
            record_gif_width: 1920,
            record_video_width: 0,
            island_pos: None,
            color: "#ff3b30".into(),
            pen_width: 3,
            font_px: 28,
            pin_border: true,
            autostart: false,
            mac_permission_help_shown: false,
            last_region: None,
        }
    }
}

/// Where settings live; matches Qt's GenericConfigLocation used by the Python edition.
pub fn config_dir() -> PathBuf {
    let home = directories::BaseDirs::new();
    let base = if cfg!(target_os = "macos") {
        home.as_ref()
            .map(|b| b.home_dir().join("Library/Preferences"))
    } else if cfg!(windows) {
        home.as_ref().map(|b| b.data_local_dir().to_path_buf())
    } else {
        home.as_ref().map(|b| b.config_dir().to_path_buf())
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("PinShot")
}

pub fn log_dir() -> PathBuf {
    let base = directories::BaseDirs::new().map(|b| {
        if cfg!(target_os = "macos") {
            b.home_dir().join("Library/Application Support")
        } else {
            b.data_local_dir().to_path_buf()
        }
    });
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("PinShot")
        .join("logs")
}

impl Config {
    pub fn default_path() -> PathBuf {
        config_dir().join("config.json")
    }

    /// Load settings; a missing or damaged file gives defaults. Unknown keys are ignored, bad values fall back
    /// to the default for that key only.
    pub fn load(path: &Path) -> Config {
        Self::load_for(path, is_mac())
    }

    pub fn load_for(path: &Path, mac: bool) -> Config {
        let Ok(text) = fs::read_to_string(path) else {
            return Config::default();
        };
        let Ok(serde_json::Value::Object(mut map)) =
            serde_json::from_str::<serde_json::Value>(&text)
        else {
            log::warn!("config unreadable, using defaults: {}", path.display());
            return Config::default();
        };
        let version = map
            .get("config_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        if mac && version < 2 {
            // old macOS builds saved Option-based shortcuts that never worked: drop them so the new defaults apply
            map.retain(|k, _| !k.starts_with("hotkey_"));
        }
        map.remove("config_version");
        // apply key by key so one bad value cannot discard the rest
        let mut cfg = Config::default();
        let mut base = serde_json::to_value(&cfg).expect("config serialises");
        for (k, v) in map {
            if let Some(slot) = base.get_mut(&k) {
                let old = std::mem::replace(slot, v);
                if serde_json::from_value::<Config>(serde_json::Value::Object(
                    base.as_object().cloned().unwrap_or_default(),
                ))
                .is_err()
                {
                    *base.get_mut(&k).expect("key exists") = old;
                }
            }
        }
        if let Ok(c) = serde_json::from_value::<Config>(base) {
            cfg = c;
        }
        cfg.config_version = CONFIG_VERSION;
        cfg
    }

    /// Atomic save: write a temp file then rename, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(
            &tmp,
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?,
        )?;
        fs::rename(&tmp, path)
    }

    pub fn save_dir_path(&self) -> PathBuf {
        let dir = if self.save_dir.is_empty() {
            directories::UserDirs::new()
                .and_then(|u| u.picture_dir().map(|p| p.to_path_buf()))
                .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join("Pictures")))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("PinShot")
        } else {
            PathBuf::from(&self.save_dir)
        };
        if fs::create_dir_all(&dir).is_err() {
            return directories::BaseDirs::new()
                .map(|b| b.home_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
        }
        dir
    }

    /// All shortcuts as (action, stored combo), skipping empty/invalid ones.
    pub fn hotkeys(&self) -> Vec<(&'static str, String)> {
        [
            ("capture", &self.hotkey_capture),
            ("fullscreen", &self.hotkey_fullscreen),
            ("repeat", &self.hotkey_repeat),
            ("record", &self.hotkey_record),
            ("pin", &self.hotkey_pin),
        ]
        .into_iter()
        .filter(|(_, v)| hotkey::parse_stored(v).is_some())
        .map(|(k, v)| (k, v.clone()))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pinshot_cfg_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("config.json")
    }

    #[test]
    fn defaults_for_each_platform() {
        let win = default_hotkeys(false);
        assert_eq!(win[0], ("hotkey_capture", "<ctrl>+<alt>+a"));
        assert_eq!(win[4], ("hotkey_pin", "<f3>"));
        let mac = default_hotkeys(true);
        assert!(mac.iter().all(|(_, v)| v.starts_with("<ctrl>+<shift>+")));
    }

    #[test]
    fn missing_or_broken_files_give_defaults() {
        let p = tmp("missing");
        assert_eq!(Config::load_for(&p, false), Config::default());
        fs::write(&p, "{not json").unwrap();
        assert_eq!(Config::load_for(&p, false), Config::default());
        fs::write(&p, "[1,2]").unwrap();
        assert_eq!(Config::load_for(&p, false), Config::default());
    }

    #[test]
    fn save_and_load_round_trip_and_python_files_are_read() {
        let p = tmp("rt");
        let c = Config {
            format: "jpg".into(),
            island_pos: Some([10, 20]),
            last_region: Some(("HDMI-A-0".into(), 1.0, 2.0, 300.0, 400.0)),
            ..Config::default()
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load_for(&p, false), c);
        // a file written by the Python edition (null island, list region, extra keys)
        fs::write(&p, r##"{"config_version":2,"format":"webp","island_pos":null,"last_region":["DP-1",1,2,3,4],"zzz":1,"quality":50}"##).unwrap();
        let py = Config::load_for(&p, false);
        assert_eq!(
            (py.format.as_str(), py.quality, py.island_pos),
            ("webp", 50, None)
        );
        assert_eq!(py.last_region, Some(("DP-1".into(), 1.0, 2.0, 3.0, 4.0)));
    }

    #[test]
    fn one_bad_value_does_not_discard_the_rest() {
        let p = tmp("bad");
        fs::write(&p, r#"{"quality":"high","format":"jpg","record_fps":45}"#).unwrap();
        let c = Config::load_for(&p, false);
        assert_eq!(c.quality, 92, "bad value falls back");
        assert_eq!((c.format.as_str(), c.record_fps), ("jpg", 45));
    }

    #[test]
    fn old_mac_shortcuts_are_reset_on_upgrade() {
        let p = tmp("mac");
        fs::write(
            &p,
            r#"{"config_version":1,"hotkey_capture":"<alt>+<ctrl>+a","format":"jpg"}"#,
        )
        .unwrap();
        let c = Config::load_for(&p, true);
        assert_eq!(c.format, "jpg");
        assert_ne!(c.hotkey_capture, "<alt>+<ctrl>+a");
        let c2 = Config::load_for(&p, false);
        assert_eq!(
            c2.hotkey_capture, "<alt>+<ctrl>+a",
            "other platforms keep them"
        );
    }

    #[test]
    fn hotkeys_skip_invalid_entries() {
        let c = Config {
            hotkey_record: String::new(),
            hotkey_pin: "<ctrl>".into(),
            ..Config::default()
        };
        let names: Vec<&str> = c.hotkeys().iter().map(|(k, _)| *k).collect();
        assert_eq!(names, ["capture", "fullscreen", "repeat"]);
    }
}
