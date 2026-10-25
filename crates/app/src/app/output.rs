//! What the overlay does with the selection: commands, copy and save.

use super::*;

impl App {
    pub(super) fn rec_cancel_redraw(&mut self) {
        self.rec_cancel();
        self.redraw_all();
    }

    pub(super) fn run_cmd(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        match cmd {
            Cmd::None => {}
            Cmd::Close => self.close_overlay(),
            Cmd::Copy => self.overlay_copy(el),
            Cmd::Save => self.overlay_save(el),
            Cmd::Pin => self.overlay_pin(el),
            Cmd::Ocr => self.overlay_ocr(el),
            Cmd::SelectMonitor => {
                let Some(ov) = self.overlay.as_mut() else {
                    return;
                };
                let at = screens::cursor_pos().or(ov.session.cursor);
                if let Some(i) = at.and_then(|p| ov.session.monitor_at(p)) {
                    let r = ov.session.caps[i].monitor.rect;
                    ov.session.editor.select_rect(r);
                    ov.session.sync_toolbar();
                }
            }
        }
    }

    pub(super) fn overlay_copy(&mut self, el: &ActiveEventLoop) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        ov.session.editor.commit_text(Fonts::embedded());
        let Some((pm, _)) = ov.session.render_selection() else {
            return;
        };
        self.close_overlay();
        let mut msg = match self.clip.set_image(&pm) {
            Ok(()) => format!(
                "{} ({}×{})",
                tr("Panoya kopyalandı"),
                pm.width(),
                pm.height()
            ),
            Err(e) => format!("{}: {e}", tr("Panoya kopyalanamadı")),
        };
        msg.push_str(&self.autosave(&pm));
        self.toast(el, &msg);
    }

    pub(super) fn overlay_save(&mut self, _el: &ActiveEventLoop) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        if ov.saving {
            return;
        }
        ov.session.editor.commit_text(Fonts::embedded());
        ov.saving = true;
        // every overlay is always-on-top: hide them all or they would cover the dialog
        for w in &ov.wins {
            w.sw.window.set_visible(false);
        }
        let dir = self.cfg.save_dir_path();
        let name = pinshot_core::output::timestamp_name(&self.cfg.format);
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .set_title(tr("Kaydet"))
                .set_directory(dir)
                .set_file_name(name)
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .add_filter("WebP", &["webp"])
                .save_file();
            let _ = proxy.send_event(UserEvent::SaveChosen(path));
        });
    }

    pub(super) fn save_chosen(&mut self, el: &ActiveEventLoop, path: Option<PathBuf>) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        ov.saving = false;
        let Some(path) = path else {
            for w in &ov.wins {
                w.sw.window.set_visible(true);
                w.sw.window.focus_window();
            }
            self.redraw_all();
            return;
        };
        let Some((pm, _)) = ov.session.render_selection() else {
            return;
        };
        self.close_overlay();
        match pinshot_core::output::save_image(&pm, &path, self.cfg.quality) {
            Ok(()) => {
                let _ = self.clip.set_image(&pm);
                self.toast(el, &format!("{}: {}", tr("Kaydedildi"), path.display()));
            }
            Err(e) => self.toast(el, &format!("{}: {e}", tr("Kaydedilemedi"))),
        }
    }
}
