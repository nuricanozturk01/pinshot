//! Capture entry points: grabbing the screens, opening the overlay, one-shot captures and saving.

use super::*;

impl App {
    pub(super) fn schedule_capture(&mut self, lag_ms: u64) {
        self.schedule_capture_mode(lag_ms, false);
    }

    /// `rec`: open the overlay for recording (options panel instead of the toolbar).
    pub(super) fn schedule_capture_mode(&mut self, lag_ms: u64, rec: bool) {
        if self.overlay.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(lag_ms));
            let _ = proxy.send_event(UserEvent::CaptureNow(rec));
        });
    }

    pub(super) fn grab(&mut self, el: &ActiveEventLoop) -> Option<Vec<Capture>> {
        if !screens::screen_access(true) {
            open_url(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
            );
            self.toast(el, tr("macOS: Sistem Ayarları › Gizlilik › Ekran Kaydı'nda PinShot'a izin verin, sonra yeniden başlatın."));
            return None;
        }
        match screens::grab_all() {
            Ok(caps)
                if !caps.is_empty() && !caps.iter().all(|c| screens::looks_blank(&c.pixmap)) =>
            {
                Some(caps)
            }
            Ok(_) => {
                self.toast(
                    el,
                    tr("Ekran yakalanamadı (görüntü boş). İzinleri kontrol edin."),
                );
                None
            }
            Err(e) => {
                log::error!("grab failed: {e}");
                self.toast(el, &format!("{}: {e}", tr("Ekran yakalanamadı")));
                None
            }
        }
    }

    pub(super) fn capture_now(&mut self, el: &ActiveEventLoop, rec: bool) {
        if self.overlay.is_some() {
            return;
        }
        let Some(caps) = self.grab(el) else { return };
        // listed before the overlay exists, so it does not offer itself
        let windows = screens::windows()
            .map_err(|e| log::warn!("window list: {e}"))
            .unwrap_or_default();
        let Some(ctx) = self.ctx.as_ref() else { return };
        let color = Rgba::parse(&self.cfg.color).unwrap_or(Rgba::rgb(0xff, 0x3b, 0x30));
        let session = Session::new(caps, color, self.cfg.pen_width, self.cfg.font_px);
        crate::mem::release_unused(); // the raw grab buffers are gone by now
        let mut session = session;
        session.editor.windows = windows;
        if rec {
            session.rec = Some(self.make_panel(false));
            session.rec_flow = true;
        }
        let mut wins = Vec::new();
        let handles: Vec<_> = el.available_monitors().collect();
        for (i, cap) in session.caps.iter().enumerate() {
            let r = cap.monitor.rect;
            let (_pw, _ph) = (cap.pixmap.width(), cap.pixmap.height());
            let attrs = Window::default_attributes()
                .with_title("PinShot")
                .with_decorations(false)
                .with_resizable(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(crate::units::win_pos(r.x, r.y))
                .with_inner_size(crate::units::size_units(r.w, r.h));
            let mut attrs = crate::gfx::tool_window(attrs);
            if !cfg!(target_os = "macos") {
                let h = handles
                    .iter()
                    .find(|h| h.position().x == r.x as i32 && h.position().y == r.y as i32)
                    .cloned();
                attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(h)));
            }
            match el.create_window(attrs) {
                Ok(w) => {
                    let w = Arc::new(w);
                    let sf = ui_scale(&w);
                    match SoftWindow::new(ctx, w.clone()) {
                        Ok(sw) => {
                            self.owners.insert(w.id(), Owner::Overlay(i));
                            wins.push(OverlayWin {
                                sw,
                                mon: i,
                                sf,
                                prev: None,
                                full: true,
                                hint: true,
                                hover_win: None,
                                hover_ext: None,
                                history: Vec::new(),
                                shown: false,
                                core: None,
                                last_draw: None,
                            });
                        }
                        Err(e) => log::error!("surface: {e}"),
                    }
                }
                Err(e) => log::error!("overlay window: {e}"),
            }
        }
        if wins.is_empty() {
            return;
        }
        self.overlay = Some(Overlay {
            session,
            wins,
            last_click: None,
            hover: None,
            saving: false,
            deferred: Vec::new(),
            cursor_icon: None,
        });
        // the screenshot already contains the pins; as live always-on-top windows they would cover the overlay
        // and swallow the clicks meant for it
        for p in &self.pins {
            p.sw.window.set_visible(false);
        }
        self.redraw_all();
        if cfg!(windows) {
            // the overlays are created hidden and shown after their first frame, but Windows delivers no
            // WM_PAINT to a hidden window: draw the first frame ourselves
            for win in 0..self.overlay.as_ref().map_or(0, |o| o.wins.len()) {
                self.draw_overlay_window(win);
            }
        }
    }

    pub(super) fn capture_fullscreen(&mut self, el: &ActiveEventLoop) {
        let Some(caps) = self.grab(el) else { return };
        let mons: Vec<_> = caps.iter().map(|c| c.monitor.clone()).collect();
        let idx = screens::cursor_pos()
            .and_then(|p| screens::monitor_at(&mons, p))
            .unwrap_or(0);
        let pm = caps.into_iter().nth(idx).map(|c| c.pixmap);
        if let Some(pm) = pm {
            self.finish_image(el, &pm, tr("Tam ekran"));
        }
    }

    /// Capture the window that has the focus, decorations included.
    pub(super) fn capture_active_window(&mut self, el: &ActiveEventLoop) {
        let Some(rect) = screens::active_window() else {
            self.toast(el, tr("Aktif pencere bulunamadı"));
            return;
        };
        let Some(caps) = self.grab(el) else { return };
        let screens: Vec<_> = caps.iter().map(|c| c.screen()).collect();
        match pinshot_core::compose::compose_region(&screens, rect) {
            Some((pm, _)) => self.finish_image(el, &pm, tr("Aktif pencere")),
            None => self.toast(el, tr("Aktif pencere bulunamadı")),
        }
    }

    pub(super) fn capture_last_region(&mut self, el: &ActiveEventLoop) {
        let Some((_, x, y, w, h)) = self.cfg.last_region.clone() else {
            self.toast(el, tr("Önce bir bölge yakalayın"));
            return;
        };
        let Some(caps) = self.grab(el) else { return };
        let screens: Vec<_> = caps.iter().map(|c| c.screen()).collect();
        match pinshot_core::compose::compose_region(&screens, Rect::new(x, y, w, h)) {
            Some((pm, _)) => self.finish_image(el, &pm, tr("Son bölge")),
            None => self.toast(el, tr("Son bölge geçersiz")),
        }
    }

    /// Clipboard + optional auto-save + notification for an image that needs no editing.
    pub(super) fn finish_image(
        &mut self,
        el: &ActiveEventLoop,
        pm: &tiny_skia::Pixmap,
        what: &str,
    ) {
        let mut msg = match self.clip.set_image(pm) {
            Ok(()) => format!(
                "{what} {} ({}×{})",
                tr("panoya kopyalandı"),
                pm.width(),
                pm.height()
            ),
            Err(e) => format!("{what}: {} ({e})", tr("panoya kopyalanamadı")),
        };
        msg.push_str(&self.autosave(pm));
        self.toast(el, &msg);
    }

    pub(super) fn autosave(&self, pm: &tiny_skia::Pixmap) -> String {
        if !self.cfg.auto_save {
            return String::new();
        }
        let path = pinshot_core::output::unique_path(
            &self
                .cfg
                .save_dir_path()
                .join(pinshot_core::output::timestamp_name(&self.cfg.format)),
        );
        match pinshot_core::output::save_image(pm, &path, self.cfg.quality) {
            Ok(()) => format!(
                " · {}: {}",
                tr("kaydedildi"),
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            Err(e) => format!(" · {}: {e}", tr("kaydedilemedi")),
        }
    }
}
