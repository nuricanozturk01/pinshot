//! The settings window.

use super::*;

impl App {
    // ------------------------------------------------------------------ settings
    pub(super) fn open_settings(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = self.settings_win.as_ref() {
            w.sw.window.focus_window();
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        match SettingsWin::create(el, ctx, &self.cfg, cfg!(target_os = "macos")) {
            Ok(w) => {
                self.owners.insert(w.sw.window.id(), Owner::Settings);
                self.settings_win = Some(w);
            }
            Err(e) => log::error!("settings window: {e}"),
        }
    }

    pub(super) fn close_settings(&mut self) {
        if let Some(w) = self.settings_win.take() {
            self.owners.remove(&w.sw.window.id());
        }
    }

    /// Write the form into the configuration and make it take effect.
    pub(super) fn apply_settings(&mut self, el: &ActiveEventLoop) {
        let Some(w) = self.settings_win.as_ref() else {
            return;
        };
        let was_autostart = self.cfg.autostart;
        settings::apply(&w.form, &mut self.cfg, w.mac);
        self.save_cfg();
        pinshot_core::i18n::apply(&self.cfg.language, sys_locale::get_locale());
        self.reload_hotkeys(el);
        if self.cfg.autostart != was_autostart {
            if let Err(e) = set_autostart(self.cfg.autostart) {
                log::warn!("autostart: {e}");
                self.toast(el, tr("Otomatik başlatma ayarlanamadı"));
            }
        }
        self.close_settings();
    }

    pub(super) fn folder_chosen(&mut self, path: Option<PathBuf>) {
        let (Some(path), Some(w)) = (path, self.settings_win.as_mut()) else {
            return;
        };
        if let Some(r) = w.form.row_mut("save_dir") {
            r.control = crate::form::Control::Folder(path.to_string_lossy().into_owned());
        }
        w.sw.window.request_redraw();
    }

    pub(super) fn settings_event(&mut self, el: &ActiveEventLoop, event: WindowEvent) {
        let Some(w) = self.settings_win.as_mut() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => w.draw(),
            WindowEvent::Resized(_) => w.sw.window.request_redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CloseRequested => self.close_settings(),
            WindowEvent::CursorMoved { position, .. } => {
                let p = w.to_ui(position.x, position.y);
                if w.hover_at(p) {
                    w.sw.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if w.hover_at(Point::new(-1.0, -1.0)) {
                    w.sw.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / w.sf,
                };
                w.scroll_by(d);
                w.sw.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                match w.hover {
                    Some(SBtn::Save) => return self.apply_settings(el),
                    Some(SBtn::Cancel) => return self.close_settings(),
                    None => {}
                }
                let ev = w.press(w.last);
                w.sw.window.request_redraw();
                if let Some(crate::form::Event::Browse(_)) = ev {
                    let proxy = self.proxy.clone();
                    let start = self.cfg.save_dir_path();
                    std::thread::spawn(move || {
                        log::debug!("folder picker opens at {}", start.display());
                        let p = rfd::FileDialog::new().set_directory(start).pick_folder();
                        log::debug!("folder picker returned {p:?}");
                        let _ = proxy.send_event(UserEvent::FolderChosen(p));
                    });
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.settings_key(&event);
            }
            _ => {}
        }
    }

    pub(super) fn settings_key(&mut self, event: &KeyEvent) {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        let Some(w) = self.settings_win.as_mut() else {
            return;
        };
        if w.form.capturing.is_none() {
            if matches!(event.logical_key, WKey::Named(NamedKey::Escape)) {
                self.close_settings();
            }
            return;
        }
        let m = self.mods;
        let name = match event.key_without_modifiers() {
            WKey::Character(c) if c.chars().count() == 1 => Some(c.to_uppercase()),
            WKey::Named(n) => f_key_name(n),
            _ => None,
        };
        let combo = match (&event.logical_key, name) {
            (WKey::Named(NamedKey::Escape), _) => None,
            (WKey::Named(NamedKey::Delete | NamedKey::Backspace), _) => Some(String::new()),
            (_, Some(n)) => settings::combo_text(
                m.control_key(),
                m.alt_key(),
                m.shift_key(),
                m.super_key(),
                &n,
            ),
            _ => return, // a lone modifier: keep waiting
        };
        w.form.set_hotkey(combo);
        w.sw.window.request_redraw();
    }
}
