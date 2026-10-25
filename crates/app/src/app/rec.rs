//! Screen recording flow: options panel in the overlay, the recorder, the island and the frame.

use super::*;
use crate::recording::{FrameSource, SharedRect};
use crate::recpanel::{Choice, Event as PanelEvent, RecPanel};
use crate::recui::{self, Island, RecFrame};
use pinshot_record::ffmpeg::Fmt;
use pinshot_record::{Event as RecEvent, Options as RecOptions, Recorder, Stats};

pub struct RecUi {
    #[expect(dead_code, reason = "owns the frame windows; dropping it closes them")]
    pub frame: RecFrame,
    pub island: Island,
    pub shared: SharedRect,
}

pub struct Recording {
    pub recorder: Recorder,
    pub ui: RecUi,
    pub clip_before: u64,
}

pub struct RecordRequest {
    pub region: Rect,
    pub choice: Choice,
}

/// Scan audio devices with a time limit: a stuck sound server must never freeze the UI.
fn scan_audio() -> (Vec<String>, (bool, String), (bool, String)) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let exe = pinshot_record::ffmpeg::find_ffmpeg();
        let probe = pinshot_record::audio::probe(exe.as_deref());
        let mics = pinshot_record::audio::list_microphones(exe.as_deref());
        let _ = tx.send((mics, probe.system, probe.mic));
    });
    rx.recv_timeout(Duration::from_secs(3)).unwrap_or_else(|_| {
        let why = tr("Ses cihazları taranamadı.").to_string();
        (Vec::new(), (false, why.clone()), (false, why))
    })
}

fn clip_fingerprint(clip: &mut Clip) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    clip.text().hash(&mut h);
    clip.image().map(|p| (p.width(), p.height())).hash(&mut h);
    h.finish()
}

impl App {
    pub(super) fn make_panel(&self, has_selection: bool) -> RecPanel {
        let (mics, sys, mic) = scan_audio();
        let mut p = RecPanel::new(&self.cfg, mics, sys, mic);
        p.has_selection = has_selection;
        p.relayout();
        p
    }

    pub(super) fn recording_active(&self) -> bool {
        self.recording.is_some()
    }

    pub(super) fn record_command(&mut self, el: &ActiveEventLoop) {
        if self.recording_active() {
            return self.stop_recording(el);
        }
        if self.overlay.is_some() {
            return;
        }
        if pinshot_record::ffmpeg::find_ffmpeg().is_none() {
            return self.toast(el, tr("Video kaydı için ffmpeg bulunamadı."));
        }
        self.schedule_capture_mode(150, true);
    }

    /// The red record button of the capture toolbar: swap the toolbar for the recording options.
    pub(super) fn open_rec_panel(&mut self, el: &ActiveEventLoop) {
        if pinshot_record::ffmpeg::find_ffmpeg().is_none() {
            return self.toast(el, tr("Video kaydı için ffmpeg bulunamadı."));
        }
        let sel_ok = self
            .overlay
            .as_ref()
            .and_then(|o| o.session.editor.sel)
            .map(|s| s.w >= 16.0 && s.h >= 16.0)
            .unwrap_or(false);
        if !sel_ok {
            return self.toast(el, tr("Kayıt için daha büyük bir alan seçin"));
        }
        let panel = self.make_panel(true);
        if let Some(ov) = self.overlay.as_mut() {
            ov.session.editor.commit_text(Fonts::embedded());
            ov.session.rec = Some(panel);
            ov.session.rec_flow = false;
        }
        self.redraw_all();
    }

    pub(super) fn rec_panel_event(&mut self, el: &ActiveEventLoop, ev: PanelEvent) {
        match ev {
            PanelEvent::Changed => {
                if let Some(p) = self.overlay.as_ref().and_then(|o| o.session.rec.as_ref()) {
                    p.store(&mut self.cfg);
                    self.save_cfg();
                }
            }
            PanelEvent::RefreshAudio => {
                let (mics, sys, mic) = scan_audio();
                if let Some(p) = self.overlay.as_mut().and_then(|o| o.session.rec.as_mut()) {
                    p.mics = mics;
                    p.sys_ok = sys;
                    p.mic_ok = mic;
                    p.relayout();
                }
            }
            PanelEvent::Cancel => self.rec_cancel(),
            PanelEvent::Start => self.rec_start_selected(el, false),
            PanelEvent::StartFull => self.rec_start_selected(el, true),
        }
        self.redraw_all();
    }

    /// Escape in the recording options: back to editing, or close when the overlay was opened for recording only.
    pub(super) fn rec_cancel(&mut self) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        if ov.session.rec_flow {
            self.close_overlay();
        } else {
            ov.session.rec = None;
        }
    }

    pub(super) fn rec_start_selected(&mut self, el: &ActiveEventLoop, full: bool) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let Some(panel) = ov.session.rec.as_ref() else {
            return;
        };
        let choice = panel.choice();
        panel.store(&mut self.cfg);
        let region = if full {
            let at = screens::cursor_pos().or(ov.session.cursor);
            at.and_then(|p| ov.session.monitor_at(p))
                .map(|i| ov.session.caps[i].monitor.rect)
        } else {
            ov.session.editor.sel
        };
        let Some(region) = region.filter(|r| r.w >= 16.0 && r.h >= 16.0) else {
            return self.toast(el, tr("Kayıt için daha büyük bir alan seçin"));
        };
        // even sizes keep every encoder happy; the region may also span monitors
        let region = Rect::new(
            region.x.round(),
            region.y.round(),
            (region.w / 2.0).floor() * 2.0,
            (region.h / 2.0).floor() * 2.0,
        );
        self.close_overlay();
        self.save_cfg();
        let proxy = self.proxy.clone();
        let req = RecordRequest { region, choice };
        // let the overlay windows disappear before the first frame is grabbed
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350));
            let _ = proxy.send_event(UserEvent::RecordNow(Box::new(req)));
        });
    }

    pub(super) fn record_now(&mut self, el: &ActiveEventLoop, req: RecordRequest) {
        if self.recording_active() {
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        let mons = screens::monitors().unwrap_or_default();
        let c = &req.choice;
        let fmt = if c.gif { Fmt::Gif } else { Fmt::Mp4 };
        let ext = if c.gif { "gif" } else { "mp4" };
        let out = pinshot_core::output::unique_path(
            &self
                .cfg
                .save_dir_path()
                .join(pinshot_core::output::timestamp_name(ext)),
        );
        let shared: SharedRect = Arc::new(std::sync::Mutex::new(None));
        let mut src = FrameSource::new(
            mons.clone(),
            req.region,
            self.cfg.record_cursor,
            shared.clone(),
        );
        let opts = RecOptions {
            fmt,
            fps: if c.gif { c.fps.min(50) } else { c.fps },
            max_width: c.max_width,
            hq444: c.hq444,
            max_seconds: self.cfg.record_max_seconds,
            audio_system: c.audio_system,
            audio_mic: c.audio_mic,
            mic_device: c.mic_device.clone(),
            out: out.clone(),
        };
        let proxy = self.proxy.clone();
        let clip_before = clip_fingerprint(&mut self.clip);
        let started = Recorder::start(
            opts,
            move || src.grab(),
            move |e| {
                let _ = proxy.send_event(UserEvent::Rec(e));
            },
        );
        let recorder = match started {
            Ok(r) => r,
            Err(e) => {
                log::error!("recording failed to start: {e}");
                return self.toast(el, &format!("{}: {e}", tr("Kayıt başlatılamadı")));
            }
        };
        // windows: the frame and the island, on the monitors holding the area
        let involved: Vec<Rect> = mons
            .iter()
            .filter(|m| m.rect.intersects(&req.region))
            .map(|m| m.rect)
            .collect();
        let union = involved
            .iter()
            .copied()
            .reduce(|a, b| a.union(&b))
            .unwrap_or(req.region);
        let frame = RecFrame::show(el, ctx, req.region, union, 3.0);
        let others: Vec<Rect> = mons
            .iter()
            .filter(|m| !m.rect.intersects(&req.region))
            .map(|m| m.rect)
            .collect();
        let all: Vec<Rect> = mons.iter().map(|m| m.rect).collect();
        let (sf, _) = self.monitor_info(el, req.region.center());
        let isize = (
            recui::ISLAND_W * if cfg!(target_os = "macos") { 1.0 } else { sf },
            recui::ISLAND_H * if cfg!(target_os = "macos") { 1.0 } else { sf },
        );
        let saved = self
            .cfg
            .island_pos
            .map(|[x, y]| Point::new(x as f32, y as f32))
            .filter(|p| recui::is_on_screen(*p, isize, &all));
        let at =
            saved.unwrap_or_else(|| recui::default_position(req.region, union, &others, isize));
        let label = format!(
            "{} · {} fps",
            if c.gif { "GIF" } else { "MP4" },
            if c.gif { c.fps.min(50) } else { c.fps }
        );
        let ctx = self.ctx.as_ref().expect("ctx");
        let Ok(island) = Island::show(el, ctx, at, sf, label) else {
            let mut r = recorder;
            r.cancel();
            return self.toast(el, tr("Kayıt penceresi açılamadı"));
        };
        self.owners.insert(island.sw.window.id(), Owner::Island);
        *shared.lock().expect("island rect") = Some(island.rect());
        self.recording = Some(Recording {
            recorder,
            ui: RecUi {
                frame,
                island,
                shared,
            },
            clip_before,
        });
        if let Some(t) = &self.tray {
            t.set_recording(true);
        }
        // no "recording started" notification: it could end up in the video
    }

    pub(super) fn stop_recording(&mut self, el: &ActiveEventLoop) {
        let Some(mut r) = self.recording.take() else {
            return;
        };
        r.recorder.stop(); // first stop capturing, so nothing shown afterwards can appear in the video
        *r.ui.shared.lock().expect("island rect") = None;
        self.owners.remove(&r.ui.island.sw.window.id());
        drop(r.ui);
        self.rec_finalizing = Some(r.clip_before);
        if let Some(t) = &self.tray {
            t.set_recording(false);
        }
        self.toast(el, tr("Kayıt kaydediliyor…"));
    }

    pub(super) fn cancel_recording(&mut self) {
        if let Some(mut r) = self.recording.take() {
            *r.ui.shared.lock().expect("island rect") = None;
            self.owners.remove(&r.ui.island.sw.window.id());
            r.recorder.cancel();
        }
    }

    pub(super) fn rec_event(&mut self, el: &ActiveEventLoop, ev: RecEvent) {
        match ev {
            RecEvent::Tick(secs) => {
                if let Some(r) = self.recording.as_mut() {
                    r.ui.island.secs = secs;
                    r.ui.island.draw();
                }
            }
            RecEvent::Warning(w) => self.toast(el, &w),
            RecEvent::Failed(e) => {
                // a failure while recording also ends the UI
                if self.recording.is_some() {
                    self.stop_recording(el);
                }
                self.rec_finalizing = None;
                self.toast(el, &format!("{}: {e}", tr("Kayıt başarısız")));
            }
            RecEvent::Finished(path, stats) => {
                if self.recording.is_some() {
                    // the time limit ended the recording
                    self.stop_recording(el);
                }
                self.rec_finished(el, path, stats);
            }
        }
    }

    fn rec_finished(&mut self, el: &ActiveEventLoop, path: PathBuf, st: Stats) {
        let before = self.rec_finalizing.take();
        self.last_output = Some(path.clone());
        let untouched = before
            .map(|b| b == clip_fingerprint(&mut self.clip))
            .unwrap_or(true);
        let copied = untouched && self.clip.set_files(std::slice::from_ref(&path)).is_ok();
        let mb = std::fs::metadata(&path)
            .map(|m| m.len() as f64 / 1024.0 / 1024.0)
            .unwrap_or(0.0);
        let mut msg = format!(
            "{} ({mb:.1} MB), {}.",
            tr("Kayıt hazır"),
            if copied {
                tr("dosya panoya kopyalandı")
            } else {
                tr("panonuza dokunulmadı")
            }
        );
        let achieved = st.frames as f64 / st.seconds;
        let target = if st.gif { st.fps.min(50) } else { st.fps } as f64;
        if achieved < target * 0.85 {
            msg.push_str(&format!(
                " {} {target:.0} fps ({} {achieved:.0} fps).",
                tr("Bilgisayar hedef kare hızını yakalayamadı:"),
                tr("ortalama")
            ));
        }
        msg.push(' ');
        msg.push_str(tr("Klasörü açmak için tıkla."));
        self.toast_click(el, &msg, "open_output");
    }

    // ------------------------------------------------------------------ the island
    pub(super) fn island_event(&mut self, el: &ActiveEventLoop, event: WindowEvent) {
        let Some(r) = self.recording.as_mut() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => r.ui.island.draw(),
            WindowEvent::CursorMoved { position, .. } => {
                if let (Some(off), Some(g)) = (r.ui.island.drag, screens::cursor_pos()) {
                    r.ui.island
                        .sw
                        .window
                        .set_outer_position(crate::gfx::win_pos(g.x - off.x, g.y - off.y));
                    *r.ui.shared.lock().expect("island rect") = Some(Rect::new(
                        g.x - off.x,
                        g.y - off.y,
                        recui::ISLAND_W,
                        recui::ISLAND_H,
                    ));
                } else {
                    let over =
                        r.ui.island
                            .hits_stop(Point::new(position.x as f32, position.y as f32));
                    r.ui.island.sw.window.set_cursor(if over {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Grab
                    });
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let (Some(g), pos) = (
                    screens::cursor_pos(),
                    r.ui.island.sw.window.outer_position().unwrap_or_default(),
                ) else {
                    return;
                };
                let rect = crate::gfx::outer_rect(&r.ui.island.sw.window);
                let local = Point::new(
                    (g.x - rect.x)
                        * if cfg!(target_os = "macos") {
                            r.ui.island.sf
                        } else {
                            1.0
                        },
                    (g.y - rect.y)
                        * if cfg!(target_os = "macos") {
                            r.ui.island.sf
                        } else {
                            1.0
                        },
                );
                let _ = pos;
                if r.ui.island.hits_stop(local) {
                    return self.stop_recording(el);
                }
                r.ui.island.drag = Some(Point::new(g.x - rect.x, g.y - rect.y));
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } if r.ui.island.drag.take().is_some() => {
                let rect = r.ui.island.rect();
                self.cfg.island_pos = Some([rect.x as i32, rect.y as i32]);
                self.save_cfg();
            }
            _ => {}
        }
    }
}
