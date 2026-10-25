//! Pinned images: creating, closing and their window events.

use super::*;

impl App {
    // ------------------------------------------------------------------ pins
    pub(super) fn add_pin(
        &mut self,
        el: &ActiveEventLoop,
        pm: tiny_skia::Pixmap,
        at: Point,
        dpr: f32,
    ) {
        let Some(ctx) = self.ctx.as_ref() else { return };
        let id = self.next_pin;
        self.next_pin += 1;
        match PinWin::create(el, ctx, id, pm, at, dpr, self.cfg.pin_border) {
            Ok(p) => {
                self.owners.insert(p.sw.window.id(), Owner::Pin(id));
                self.pins.push(p);
            }
            Err(e) => log::error!("pin: {e}"),
        }
    }

    /// Pin an image in the middle of the monitor under the pointer.
    pub(super) fn pin_centered(&mut self, el: &ActiveEventLoop, pm: tiny_skia::Pixmap) {
        let cur = screens::cursor_pos().unwrap_or_default();
        let (sf, bounds) = self.monitor_info(el, cur);
        let (w, h) = (pm.width() as f32 / sf, pm.height() as f32 / sf);
        let c = bounds.center();
        self.add_pin(
            el,
            pm,
            Point::new((c.x - w / 2.0).max(bounds.x), (c.y - h / 2.0).max(bounds.y)),
            sf,
        );
    }

    pub(super) fn pin_clipboard(&mut self, el: &ActiveEventLoop) {
        if let Some(img) = self.clip.image() {
            return self.pin_centered(el, img);
        }
        if let Some(text) = self.clip.text() {
            let t = text.trim();
            let path = PathBuf::from(t.strip_prefix("file://").unwrap_or(t));
            if path.is_file() {
                if let Some(pm) = load_image(&path) {
                    return self.pin_centered(el, pm);
                }
            }
            return self.pin_centered(el, pin::render_text(&text));
        }
        self.toast(el, tr("Panoda sabitlenecek görsel veya metin yok"));
    }

    pub(super) fn pin_file_dialog(&mut self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title(tr("Sabitlenecek görseller"))
                .add_filter(
                    tr("Görseller"),
                    &["png", "jpg", "jpeg", "bmp", "gif", "webp", "tif", "tiff"],
                )
                .pick_files()
                .unwrap_or_default();
            let _ = proxy.send_event(UserEvent::PinFilesChosen(files));
        });
    }

    pub(super) fn pin_files_chosen(&mut self, el: &ActiveEventLoop, files: Vec<PathBuf>) {
        for f in files {
            match load_image(&f) {
                Some(pm) => self.pin_centered(el, pm),
                None => {
                    let name = f
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.toast(el, &format!("{}: {name}", tr("Açılamadı")));
                }
            }
        }
    }

    pub(super) fn close_all_pins(&mut self) {
        for p in self.pins.drain(..) {
            self.owners.remove(&p.sw.window.id());
        }
        self.close_popup();
    }

    pub(super) fn close_pin(&mut self, id: u64) {
        if let Some(i) = self.pins.iter().position(|p| p.id == id) {
            let p = self.pins.remove(i);
            self.owners.remove(&p.sw.window.id());
        }
        if self.popup.as_ref().map(|p| p.owner) == Some(id) {
            self.close_popup();
        }
    }

    pub(super) fn overlay_pin(&mut self, el: &ActiveEventLoop) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        ov.session.editor.commit_text(Fonts::embedded());
        let Some((pm, scale)) = ov.session.render_selection() else {
            return;
        };
        let at = ov
            .session
            .editor
            .sel
            .map(|s| s.top_left())
            .unwrap_or_default();
        self.close_overlay();
        self.add_pin(el, pm, at, scale);
    }

    pub(super) fn pin_index(&self, id: u64) -> Option<usize> {
        self.pins.iter().position(|p| p.id == id)
    }

    pub(super) fn pin_event(&mut self, el: &ActiveEventLoop, id: u64, event: WindowEvent) {
        if !matches!(
            event,
            WindowEvent::CursorMoved { .. } | WindowEvent::RedrawRequested
        ) {
            log::debug!("pin {id} event {event:?}");
        }
        let Some(i) = self.pin_index(id) else { return };
        let alt = self.mods.alt_key();
        let ctrl = if cfg!(target_os = "macos") {
            self.mods.super_key()
        } else {
            self.mods.control_key()
        };
        let shift = self.mods.shift_key();
        match event {
            WindowEvent::RedrawRequested => self.pins[i].draw(),
            WindowEvent::Resized(_) => self.pins[i].sw.window.request_redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorEntered { .. } => {
                self.pins[i].hover = true;
                self.pins[i].sw.window.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.pins[i].hover = false;
                self.pins[i].sw.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let global = screens::cursor_pos();
                let p = &mut self.pins[i];
                p.pos = Point::new(position.x as f32, position.y as f32);
                let (w, h) = p.sw.size();
                if let (Some((edge, g0, press)), Some(g)) = (p.resize.clone(), global) {
                    let r = pin::resize_drag(
                        g0,
                        &edge,
                        Point::new(g.x - press.x, g.y - press.y),
                        shift,
                    );
                    p.set_geom(r);
                } else if let (Some(off), Some(g)) = (p.drag, global) {
                    let r = Rect::new(g.x - off.x, g.y - off.y, p.geom.w, p.geom.h);
                    p.geom = r;
                    p.sw.window
                        .set_outer_position(crate::units::win_pos(r.x, r.y));
                } else {
                    let edge = pin::edge_at(p.pos, w as f32, h as f32);
                    p.sw.window.set_cursor(match edge.as_str() {
                        "l" | "r" => CursorIcon::EwResize,
                        "t" | "b" => CursorIcon::NsResize,
                        "tl" | "br" => CursorIcon::NwseResize,
                        "tr" | "bl" => CursorIcon::NeswResize,
                        _ => CursorIcon::Default,
                    });
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => {
                self.pins[i].drag = None;
                self.pins[i].resize = None;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => {
                let global = screens::cursor_pos().unwrap_or_default();
                let p = &mut self.pins[i];
                let (w, h) = p.sw.size();
                match button {
                    MouseButton::Left => {
                        if p.hover && PinWin::close_rect()(w as f32).contains(p.pos) {
                            return self.close_pin(id);
                        }
                        let edge = pin::edge_at(p.pos, w as f32, h as f32);
                        let now = Instant::now();
                        let dbl = p
                            .last_click
                            .map(|t| now.duration_since(t) < DOUBLE_CLICK)
                            .unwrap_or(false);
                        p.last_click = Some(now);
                        if edge.is_empty() && dbl {
                            return self.close_pin(id);
                        }
                        if !edge.is_empty() {
                            p.resize = Some((edge, p.geom, global));
                        } else if !p.locked {
                            p.drag = Some(Point::new(global.x - p.geom.x, global.y - p.geom.y));
                        }
                    }
                    MouseButton::Right => self.pin_menu(el, id, global),
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        if y != 0.0 {
                            y
                        } else {
                            x
                        }
                    }
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                };
                if d == 0.0 {
                    return;
                }
                let p = &mut self.pins[i];
                if alt || (ctrl && shift) {
                    p.opacity = (p.opacity + if d > 0.0 { 0.05 } else { -0.05 }).clamp(0.1, 1.0);
                    p.sw.window.request_redraw();
                } else {
                    let anchor = screens::cursor_pos().unwrap_or(p.geom.center());
                    let g = pin::zoom_about(p.geom, 1.08_f32.powf(d), anchor);
                    p.set_geom(g);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let key = event.logical_key.clone();
                match key {
                    WKey::Named(NamedKey::Escape | NamedKey::Delete) => self.close_pin(id),
                    WKey::Character(c) => {
                        let c = c.to_lowercase();
                        let p = &mut self.pins[i];
                        match c.as_str() {
                            "c" if ctrl => self.pin_action(el, id, "copy"),
                            "s" if ctrl => self.pin_action(el, id, "save"),
                            "+" | "=" => {
                                let g = pin::zoom_about(p.geom, 1.1, p.geom.center());
                                p.set_geom(g);
                            }
                            "-" => {
                                let g = pin::zoom_about(p.geom, 1.0 / 1.1, p.geom.center());
                                p.set_geom(g);
                            }
                            "0" => {
                                let g = pin::percent_rect(p.geom, p.natural(), 100.0);
                                p.set_geom(g);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::CloseRequested => self.close_pin(id),
            _ => {}
        }
    }
}
