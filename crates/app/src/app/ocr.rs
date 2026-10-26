//! Text recognition windows.

use super::*;

impl App {
    // ------------------------------------------------------------------ OCR
    pub(super) fn overlay_ocr(&mut self, el: &ActiveEventLoop) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        ov.session.editor.commit_text(Fonts::embedded());
        let Some((pm, _)) = ov.session.render_selection() else {
            return;
        };
        let at = ov
            .session
            .editor
            .sel
            .map(|s| s.top_left())
            .unwrap_or_default();
        self.close_overlay();
        self.ocr_start(el, pm, at);
    }

    pub(super) fn ocr_start(&mut self, el: &ActiveEventLoop, img: tiny_skia::Pixmap, at: Point) {
        let Some(ctx) = self.ctx.as_ref() else { return };
        let id = self.next_pin;
        self.next_pin += 1;
        let (sf, bounds) = self.monitor_info(el, at);
        let at = Point::new(
            at.x.min(bounds.right() - 540.0 * sf).max(bounds.x),
            at.y.min(bounds.bottom() - 400.0 * sf).max(bounds.y),
        );
        match OcrWin::create(el, ctx, id, at, sf) {
            Ok(w) => {
                w.sw.window.focus_window();
                self.owners.insert(w.sw.window.id(), Owner::Ocr(id));
                self.ocr_wins.push(w);
            }
            Err(e) => return log::error!("ocr window: {e}"),
        }
        let engine = self.ocr_engine.clone();
        let proxy = self.proxy.clone();
        // normal priority on purpose: macOS throttles background threads so hard that OCR can take minutes
        std::thread::spawn(move || {
            let result = (|| {
                let mut guard = engine
                    .lock()
                    .map_err(|_| "engine lock poisoned".to_string())?;
                if guard.is_none() {
                    *guard = Some(pinshot_ocr::Engine::new()?);
                }
                guard.as_mut().expect("engine").recognize(&img)
            })();
            // free the picture before the main thread is told, so its memory sweep can return it
            drop(img);
            let _ = proxy.send_event(UserEvent::OcrDone(id, result));
        });
    }

    pub(super) fn ocr_done(&mut self, id: u64, result: Result<String, String>) {
        self.ocr_last_use = Some(Instant::now());
        crate::mem::release_unused(); // the recognition buffers are gone by now
        let Some(i) = self.ocr_wins.iter().position(|w| w.id == id) else {
            return;
        };
        if let Ok(t) = &result {
            if !t.trim().is_empty() {
                let _ = self.clip.set_text(t);
            }
        }
        self.ocr_wins[i].finish(result);
    }

    pub(super) fn close_ocr(&mut self, id: u64) {
        if let Some(i) = self.ocr_wins.iter().position(|w| w.id == id) {
            let w = self.ocr_wins.remove(i);
            self.owners.remove(&w.sw.window.id());
            drop(w);
            crate::mem::release_unused();
        }
    }

    pub(super) fn ocr_event(&mut self, _el: &ActiveEventLoop, id: u64, event: WindowEvent) {
        let Some(i) = self.ocr_wins.iter().position(|w| w.id == id) else {
            return;
        };
        let ctrl = if cfg!(target_os = "macos") {
            self.mods.super_key()
        } else {
            self.mods.control_key()
        };
        match event {
            WindowEvent::RedrawRequested => self.ocr_wins[i].draw(),
            WindowEvent::Resized(_) => self.ocr_wins[i].sw.window.request_redraw(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CloseRequested => self.close_ocr(id),
            WindowEvent::CursorMoved { position, .. } => {
                let b =
                    self.ocr_wins[i].button_at(Point::new(position.x as f32, position.y as f32));
                if self.ocr_wins[i].hover != b {
                    self.ocr_wins[i].hover = b;
                    self.ocr_wins[i].sw.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y * 40.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.ocr_wins[i].scroll_by(d);
                self.ocr_wins[i].sw.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => match self.ocr_wins[i].hover {
                Some(Btn::Close) => self.close_ocr(id),
                Some(Btn::Copy) => self.ocr_copy(i),
                None => {}
            },
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match &event.logical_key {
                    WKey::Named(NamedKey::Escape) => self.close_ocr(id),
                    WKey::Character(c) if ctrl && c.eq_ignore_ascii_case("c") => self.ocr_copy(i),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub(super) fn ocr_copy(&mut self, i: usize) {
        if let Some(t) = self.ocr_wins[i].text().map(|t| t.to_string()) {
            let _ = self.clip.set_text(&t);
            self.ocr_wins[i].status = format!(
                "{} {}",
                t.chars().count(),
                tr("karakter – panoya kopyalandı")
            );
            self.ocr_wins[i].sw.window.request_redraw();
        }
    }
}
