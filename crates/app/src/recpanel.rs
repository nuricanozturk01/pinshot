//! The recording options panel (format, frame rate, size, audio) and its start/cancel buttons.
//! Pure layout, drawing and hit testing, like the toolbar.

use crate::ui::{theme, Canvas};
use pinshot_core::config::Config;
use pinshot_core::i18n::tr;
use pinshot_core::{Point, Rect, Rgba};

pub const MP4_FPS: [u32; 5] = [10, 15, 24, 30, 60];
pub const GIF_FPS: [u32; 5] = [10, 15, 24, 30, 50];
pub const MP4_SIZES: [u32; 4] = [1280, 1920, 2560, 0];
pub const GIF_SIZES: [u32; 5] = [640, 960, 1280, 1920, 0];

/// What the user chose.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub gif: bool,
    pub fps: u32,
    pub max_width: u32,
    pub hq444: bool,
    pub audio_system: bool,
    pub audio_mic: bool,
    pub mic_device: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Id {
    Fmt(bool),
    Fps(usize),
    Size(usize),
    Hq,
    Sys,
    Mic,
    MicPrev,
    MicNext,
    Refresh,
    Cancel,
    Full,
    Start,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Start,
    StartFull,
    Cancel,
    RefreshAudio,
    Changed,
}

pub struct RecPanel {
    pub gif: bool,
    pub fps_mp4: u32,
    pub fps_gif: u32,
    pub width_mp4: u32,
    pub width_gif: u32,
    pub hq444: bool,
    pub sys: bool,
    pub mic: bool,
    /// `""` = default microphone.
    pub mic_device: String,
    pub mics: Vec<String>,
    pub sys_ok: (bool, String),
    pub mic_ok: (bool, String),
    /// Whether an area is selected (otherwise "record whole screen" is offered).
    pub has_selection: bool,
    pub hover: Option<usize>,
    elems: Vec<(Id, Rect)>,
    notes: Vec<String>,
    size: (f32, f32),
}

pub const W: f32 = 430.0;

fn nearest(list: &[u32], want: u32) -> u32 {
    *list
        .iter()
        .min_by_key(|v| (**v as i64 - want as i64).abs())
        .unwrap_or(&list[0])
}

impl RecPanel {
    pub fn new(
        cfg: &Config,
        mics: Vec<String>,
        sys_ok: (bool, String),
        mic_ok: (bool, String),
    ) -> RecPanel {
        let mut p = RecPanel {
            gif: cfg.record_format == "gif",
            fps_mp4: nearest(&MP4_FPS, cfg.record_fps),
            fps_gif: nearest(&GIF_FPS, cfg.record_fps_gif),
            width_mp4: if MP4_SIZES.contains(&cfg.record_video_width) {
                cfg.record_video_width
            } else {
                0
            },
            width_gif: if GIF_SIZES.contains(&cfg.record_gif_width) {
                cfg.record_gif_width
            } else {
                0
            },
            hq444: cfg.record_quality_444,
            sys: cfg.record_audio_system,
            mic: cfg.record_audio_mic,
            mic_device: cfg.record_mic_device.clone(),
            mics,
            sys_ok,
            mic_ok,
            has_selection: false,
            hover: None,
            elems: Vec::new(),
            notes: Vec::new(),
            size: (W, 100.0),
        };
        p.relayout();
        p
    }

    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    fn fps_list(&self) -> &'static [u32] {
        if self.gif {
            &GIF_FPS
        } else {
            &MP4_FPS
        }
    }

    fn size_list(&self) -> &'static [u32] {
        if self.gif {
            &GIF_SIZES
        } else {
            &MP4_SIZES
        }
    }

    fn fps(&self) -> u32 {
        if self.gif {
            self.fps_gif
        } else {
            self.fps_mp4
        }
    }

    fn width(&self) -> u32 {
        if self.gif {
            self.width_gif
        } else {
            self.width_mp4
        }
    }

    pub fn choice(&self) -> Choice {
        let mp4 = !self.gif;
        Choice {
            gif: self.gif,
            fps: self.fps(),
            max_width: self.width(),
            hq444: mp4 && self.hq444,
            audio_system: mp4 && self.sys && self.sys_ok.0,
            audio_mic: mp4 && self.mic && self.mic_ok.0,
            mic_device: self.mic_device.clone(),
        }
    }

    /// Copy the choice back into the settings.
    pub fn store(&self, cfg: &mut Config) {
        cfg.record_format = if self.gif { "gif" } else { "mp4" }.into();
        cfg.record_fps = self.fps_mp4;
        cfg.record_fps_gif = self.fps_gif;
        cfg.record_video_width = self.width_mp4;
        cfg.record_gif_width = self.width_gif;
        cfg.record_quality_444 = self.hq444;
        cfg.record_audio_system = self.sys;
        cfg.record_audio_mic = self.mic;
        cfg.record_mic_device = self.mic_device.clone();
    }

    pub fn relayout(&mut self) {
        let mut e: Vec<(Id, Rect)> = Vec::new();
        let mut y = 40.0;
        let x0 = 16.0;
        // format
        e.push((Id::Fmt(false), Rect::new(x0 + 86.0, y, 64.0, 28.0)));
        e.push((Id::Fmt(true), Rect::new(x0 + 86.0 + 66.0, y, 64.0, 28.0)));
        y += 38.0;
        for i in 0..self.fps_list().len() {
            e.push((
                Id::Fps(i),
                Rect::new(x0 + 86.0 + i as f32 * 58.0, y, 56.0, 28.0),
            ));
        }
        y += 38.0;
        for i in 0..self.size_list().len() {
            e.push((
                Id::Size(i),
                Rect::new(x0 + 86.0 + i as f32 * 58.0, y, 56.0, 28.0),
            ));
        }
        y += 38.0;
        let mp4 = !self.gif;
        self.notes.clear();
        if mp4 {
            e.push((Id::Hq, Rect::new(x0, y, W - 32.0, 24.0)));
            y += 30.0;
            e.push((Id::Sys, Rect::new(x0, y, 130.0, 24.0)));
            e.push((Id::Mic, Rect::new(x0 + 138.0, y, 112.0, 24.0)));
            e.push((Id::Refresh, Rect::new(W - 16.0 - 28.0, y - 2.0, 28.0, 28.0)));
            y += 32.0;
            if self.mic && self.mic_ok.0 {
                e.push((Id::MicPrev, Rect::new(x0, y, 26.0, 26.0)));
                e.push((Id::MicNext, Rect::new(W - 16.0 - 26.0, y, 26.0, 26.0)));
                y += 34.0;
            }
            for (ok, why) in [&self.sys_ok, &self.mic_ok] {
                if !*ok && !why.is_empty() {
                    self.notes.push(why.clone());
                }
            }
            for _ in &self.notes {
                y += 36.0;
            }
        }
        y += 6.0;
        let start_x = W - 16.0 - 130.0;
        let full_x = start_x - 8.0 - 130.0;
        let cancel_x = (if self.has_selection { start_x } else { full_x }) - 8.0 - 90.0;
        e.push((Id::Cancel, Rect::new(cancel_x, y, 90.0, 32.0)));
        if !self.has_selection {
            e.push((Id::Full, Rect::new(full_x, y, 130.0, 32.0)));
        }
        e.push((Id::Start, Rect::new(start_x, y, 130.0, 32.0)));
        self.size = (W, y + 32.0 + 14.0);
        self.elems = e;
    }

    fn index_at(&self, p: Point) -> Option<usize> {
        self.elems.iter().position(|(_, r)| r.contains(p))
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x < self.size.0 && p.y < self.size.1
    }

    pub fn set_hover(&mut self, p: Option<Point>) -> bool {
        let n = p.and_then(|p| self.index_at(p));
        let changed = n != self.hover;
        self.hover = n;
        changed
    }

    fn enabled(&self, id: Id) -> bool {
        match id {
            Id::Sys => self.sys_ok.0,
            Id::Mic => self.mic_ok.0,
            _ => true,
        }
    }

    pub fn press(&mut self, p: Point) -> Option<Event> {
        let i = self.index_at(p)?;
        let id = self.elems[i].0;
        if !self.enabled(id) {
            return None;
        }
        let ev = match id {
            Id::Fmt(g) => {
                self.gif = g;
                Event::Changed
            }
            Id::Fps(i) => {
                let v = self.fps_list()[i];
                if self.gif {
                    self.fps_gif = v
                } else {
                    self.fps_mp4 = v
                }
                Event::Changed
            }
            Id::Size(i) => {
                let v = self.size_list()[i];
                if self.gif {
                    self.width_gif = v
                } else {
                    self.width_mp4 = v
                }
                Event::Changed
            }
            Id::Hq => {
                self.hq444 = !self.hq444;
                Event::Changed
            }
            Id::Sys => {
                self.sys = !self.sys;
                Event::Changed
            }
            Id::Mic => {
                self.mic = !self.mic;
                Event::Changed
            }
            Id::MicPrev | Id::MicNext => {
                let n = self.mics.len() as i64 + 1; // index 0 is the default device
                let cur = self
                    .mics
                    .iter()
                    .position(|m| *m == self.mic_device)
                    .map(|i| i as i64 + 1)
                    .unwrap_or(0);
                let next = (cur + if id == Id::MicNext { 1 } else { n - 1 }) % n;
                self.mic_device = if next == 0 {
                    String::new()
                } else {
                    self.mics[next as usize - 1].clone()
                };
                Event::Changed
            }
            Id::Refresh => Event::RefreshAudio,
            Id::Cancel => Event::Cancel,
            Id::Full => Event::StartFull,
            Id::Start => Event::Start,
        };
        self.relayout();
        Some(ev)
    }

    pub fn draw(&self, cv: &mut Canvas, origin: Point) {
        let body = Rect::new(origin.x, origin.y, self.size.0, self.size.1);
        cv.fill_rrect(body, 10.0, theme::BAR_BORDER);
        cv.fill_rrect(body.inflated(-1.0), 9.0, theme::BAR_BG);
        let title = if self.has_selection {
            tr("Seçili alanı kaydet")
        } else {
            tr("Ekranı kaydet")
        };
        cv.text(
            origin.x + 16.0,
            origin.y + 12.0,
            title,
            14.0,
            Rgba::WHITE,
            true,
        );
        let label = |cv: &mut Canvas, id: Id, text: &str| {
            if let Some((_, r)) = self.elems.iter().find(|(i, _)| *i == id) {
                cv.text(
                    origin.x + 16.0,
                    origin.y + r.y + 6.0,
                    text,
                    12.0,
                    theme::TEXT,
                    false,
                );
            }
        };
        label(cv, Id::Fmt(false), tr("Biçim"));
        label(cv, Id::Fps(0), tr("Kare hızı"));
        label(cv, Id::Size(0), tr("Boyut"));
        for (i, (id, r)) in self.elems.iter().enumerate() {
            let r = r.translated(origin.x, origin.y);
            let hov = self.hover == Some(i) && self.enabled(*id);
            let ink = if self.enabled(*id) {
                Rgba::WHITE
            } else {
                theme::TEXT_DIM
            };
            let seg = |cv: &mut Canvas, on: bool, text: &str| {
                let bg = if on {
                    theme::ACCENT
                } else if hov {
                    theme::BTN_HOVER
                } else {
                    Rgba::rgb(0x34, 0x34, 0x3a)
                };
                cv.fill_rrect(r, 6.0, bg);
                cv.text_centered(r, text, 12.0, Rgba::WHITE, false);
            };
            let check = |cv: &mut Canvas, on: bool, text: &str| {
                let b = Rect::new(r.x, r.y + 3.0, 18.0, 18.0);
                cv.fill_rrect(
                    b,
                    4.0,
                    if on && self.enabled(*id) {
                        theme::ACCENT
                    } else {
                        Rgba::rgb(0x34, 0x34, 0x3a)
                    },
                );
                if on {
                    crate::ui::icons::draw(cv, "check", b.inflated(-1.0), ink);
                }
                cv.text(r.x + 26.0, r.y + 5.0, text, 13.0, ink, false);
            };
            match *id {
                Id::Fmt(g) => seg(cv, self.gif == g, if g { "GIF" } else { "MP4" }),
                Id::Fps(k) => seg(
                    cv,
                    self.fps_list()[k] == self.fps(),
                    &self.fps_list()[k].to_string(),
                ),
                Id::Size(k) => {
                    let v = self.size_list()[k];
                    seg(
                        cv,
                        v == self.width(),
                        &if v == 0 {
                            tr("Orijinal").to_string()
                        } else {
                            v.to_string()
                        },
                    );
                }
                Id::Hq => check(cv, self.hq444, tr("Maksimum kalite (4:4:4 renk)")),
                Id::Sys => check(cv, self.sys && self.sys_ok.0, tr("Sistem sesi")),
                Id::Mic => check(cv, self.mic && self.mic_ok.0, tr("Mikrofon")),
                Id::Refresh => {
                    if hov {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    crate::ui::icons::draw(cv, "refresh", r.inflated(-4.0), Rgba::WHITE);
                }
                Id::MicPrev | Id::MicNext => {
                    if hov {
                        cv.fill_rrect(r, 6.0, theme::BTN_HOVER);
                    }
                    cv.text_centered(
                        r,
                        if *id == Id::MicPrev { "‹" } else { "›" },
                        16.0,
                        Rgba::WHITE,
                        true,
                    );
                    if *id == Id::MicPrev {
                        let name = if self.mic_device.is_empty() {
                            tr("Varsayılan mikrofon").to_string()
                        } else {
                            self.mic_device.clone()
                        };
                        let field = Rect::new(r.right() + 4.0, r.y, W - 32.0 - 60.0, 26.0);
                        cv.fill_rrect(field, 5.0, theme::FIELD_BG);
                        cv.text(
                            field.x + 8.0,
                            field.y + 6.0,
                            &ellipsize(cv, &name, field.w - 16.0),
                            12.0,
                            Rgba::WHITE,
                            false,
                        );
                    }
                }
                Id::Cancel => {
                    cv.fill_rrect(
                        r,
                        6.0,
                        if hov {
                            theme::BTN_HOVER
                        } else {
                            Rgba::rgb(0x34, 0x34, 0x3a)
                        },
                    );
                    cv.text_centered(r, tr("İptal"), 13.0, Rgba::WHITE, false);
                }
                Id::Full => {
                    cv.fill_rrect(
                        r,
                        6.0,
                        if hov {
                            theme::BTN_HOVER
                        } else {
                            Rgba::rgb(0x34, 0x34, 0x3a)
                        },
                    );
                    cv.text_centered(r, tr("Tüm ekranı kaydet"), 13.0, Rgba::WHITE, false);
                }
                Id::Start => {
                    cv.fill_rrect(
                        r,
                        6.0,
                        if hov {
                            Rgba::rgb(0xff, 0x6a, 0x60)
                        } else {
                            theme::DANGER
                        },
                    );
                    cv.fill_circle(r.x + 16.0, r.y + r.h / 2.0, 5.0, Rgba::WHITE);
                    let label = Rect::new(r.x + 24.0, r.y, r.w - 24.0, r.h);
                    cv.text_centered(label, tr("Kaydı başlat"), 13.0, Rgba::WHITE, true);
                }
            }
        }
        // notes about unavailable audio sources
        if let Some(note_y) = self.notes_top() {
            for (i, n) in self.notes.iter().enumerate() {
                let lines = crate::toast::wrap(n, W - 32.0, |s| cv.text_width(s, 11.0, false));
                for (j, l) in lines.iter().take(2).enumerate() {
                    cv.text(
                        origin.x + 16.0,
                        origin.y + note_y + i as f32 * 36.0 + j as f32 * 15.0,
                        l,
                        11.0,
                        Rgba::rgb(0xff, 0xb3, 0x40),
                        false,
                    );
                }
            }
        }
    }

    fn notes_top(&self) -> Option<f32> {
        if self.notes.is_empty() {
            return None;
        }
        let y = self
            .elems
            .iter()
            .find(|(i, _)| *i == Id::Cancel)
            .map(|(_, r)| r.y)?;
        Some(y - 6.0 - self.notes.len() as f32 * 36.0 + 2.0)
    }
}

fn ellipsize(cv: &Canvas, text: &str, max_w: f32) -> String {
    if cv.text_width(text, 12.0, false) <= max_w {
        return text.to_string();
    }
    let mut s: String = text.chars().collect();
    while !s.is_empty() && cv.text_width(&format!("{s}…"), 12.0, false) > max_w {
        s.pop();
    }
    format!("{s}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> RecPanel {
        RecPanel::new(
            &Config::default(),
            vec!["USB Mic".into(), "Built-in".into()],
            (true, String::new()),
            (true, String::new()),
        )
    }

    fn centre(p: &RecPanel, id: Id) -> Point {
        p.elems.iter().find(|(i, _)| *i == id).unwrap().1.center()
    }

    #[test]
    fn defaults_come_from_the_settings_and_snap_to_available_values() {
        let cfg = Config {
            record_fps: 28,
            record_fps_gif: 12,
            record_video_width: 1234,
            ..Config::default()
        };
        let p = RecPanel::new(&cfg, vec![], (true, String::new()), (true, String::new()));
        assert_eq!((p.fps_mp4, p.fps_gif, p.width_mp4), (30, 10, 0));
        let c = p.choice();
        assert!(!c.gif && c.fps == 30 && c.max_width == 0);
    }

    #[test]
    fn elements_fit_and_do_not_overlap_for_every_state() {
        for gif in [false, true] {
            for sel in [false, true] {
                for mic in [false, true] {
                    let mut p = panel();
                    p.gif = gif;
                    p.mic = mic;
                    p.has_selection = sel;
                    p.sys_ok = (false, "System sound is unavailable here.".into());
                    p.relayout();
                    let (w, h) = p.size();
                    for (i, (ia, a)) in p.elems.iter().enumerate() {
                        assert!(
                            a.x >= 0.0 && a.right() <= w && a.bottom() <= h,
                            "{ia:?} outside (gif {gif} sel {sel} mic {mic})"
                        );
                        for (ib, b) in p.elems.iter().skip(i + 1) {
                            assert!(!a.intersects(b), "{ia:?} overlaps {ib:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn switching_format_changes_the_choices_and_disables_audio_for_gif() {
        let mut p = panel();
        p.sys = true;
        p.mic = true;
        assert!(p.choice().audio_system && p.choice().audio_mic);
        assert_eq!(p.press(centre(&p, Id::Fmt(true))), Some(Event::Changed));
        let c = p.choice();
        assert!(c.gif && !c.audio_system && !c.audio_mic && !c.hq444);
        assert!(
            p.elems
                .iter()
                .all(|(i, _)| !matches!(i, Id::Sys | Id::Mic | Id::Hq)),
            "audio controls hidden for GIF"
        );
        assert_eq!(p.press(centre(&p, Id::Fps(4))), Some(Event::Changed));
        assert_eq!(p.choice().fps, 50);
        assert_eq!(p.press(centre(&p, Id::Size(0))), Some(Event::Changed));
        assert_eq!(p.choice().max_width, 640);
    }

    #[test]
    fn unavailable_sources_cannot_be_enabled_and_explain_why() {
        let mut p = panel();
        p.sys = false;
        p.sys_ok = (false, "No system sound source found.".into());
        p.relayout();
        assert_eq!(p.press(centre(&p, Id::Sys)), None);
        assert!(!p.choice().audio_system);
        assert_eq!(p.notes.len(), 1);
    }

    #[test]
    fn microphone_selector_cycles_through_default_and_devices() {
        let mut p = panel();
        p.mic = true;
        p.relayout();
        assert_eq!(p.mic_device, "");
        p.press(centre(&p, Id::MicNext));
        assert_eq!(p.mic_device, "USB Mic");
        p.press(centre(&p, Id::MicNext));
        p.press(centre(&p, Id::MicNext));
        assert_eq!(p.mic_device, "", "wraps back to the default");
        p.press(centre(&p, Id::MicPrev));
        assert_eq!(p.mic_device, "Built-in");
    }

    #[test]
    fn buttons_report_events_and_choices_are_stored() {
        let mut p = panel();
        p.has_selection = true;
        p.relayout();
        assert_eq!(p.press(centre(&p, Id::Start)), Some(Event::Start));
        assert_eq!(p.press(centre(&p, Id::Cancel)), Some(Event::Cancel));
        p.has_selection = false;
        p.relayout();
        assert_eq!(p.press(centre(&p, Id::Full)), Some(Event::StartFull));
        p.press(centre(&p, Id::Fps(0)));
        p.press(centre(&p, Id::Hq));
        let mut cfg = Config::default();
        p.store(&mut cfg);
        assert_eq!((cfg.record_fps, cfg.record_quality_444), (10, false));
    }

    #[test]
    fn draws_without_panicking() {
        let mut p = panel();
        p.mic = true;
        p.sys_ok = (
            false,
            "Something long that needs to wrap over two lines to be readable in the panel.".into(),
        );
        p.relayout();
        let (w, h) = p.size();
        let mut pm = tiny_skia::Pixmap::new(w as u32 + 20, h as u32 + 20).unwrap();
        p.draw(&mut Canvas::new(&mut pm, 1.0), Point::new(10.0, 10.0));
        assert!(
            pm.pixels().iter().any(|q| q.red() > 240 && q.green() < 100),
            "red start button"
        );
    }
}
