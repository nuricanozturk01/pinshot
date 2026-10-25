//! The About and colour-picker windows.

use super::*;

impl App {
    // ------------------------------------------------------------------ about / colour
    pub(super) fn open_about(&mut self, el: &ActiveEventLoop) {
        if let Some(w) = self.about_win.as_ref() {
            w.sw.window.focus_window();
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        match AboutWin::create(el, ctx) {
            Ok(w) => {
                self.owners.insert(w.sw.window.id(), Owner::About);
                self.about_win = Some(w);
            }
            Err(e) => log::error!("about window: {e}"),
        }
    }

    pub(super) fn close_about(&mut self) {
        if let Some(w) = self.about_win.take() {
            self.owners.remove(&w.sw.window.id());
        }
    }

    pub(super) fn about_event(&mut self, event: WindowEvent) {
        let Some(w) = self.about_win.as_mut() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => w.draw(),
            WindowEvent::Resized(_) => w.sw.window.request_redraw(),
            WindowEvent::CloseRequested => self.close_about(),
            WindowEvent::CursorMoved { position, .. } => {
                let b = w.button_at(position.x, position.y);
                if w.hover != b {
                    w.hover = b;
                    w.sw.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => match w.hover {
                Some(ABtn::Close) => self.close_about(),
                Some(ABtn::Project) => open_url(crate::aboutwin::REPO),
                Some(ABtn::Logs) => open_path(&pinshot_core::config::log_dir()),
                None => {}
            },
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(event.logical_key, WKey::Named(NamedKey::Escape)) =>
            {
                self.close_about();
            }
            _ => {}
        }
    }

    pub(super) fn open_color(&mut self, el: &ActiveEventLoop, start: Rgba) {
        if self.color_win.is_some() {
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else { return };
        let at = screens::cursor_pos().map(|g| Point::new(g.x, g.y));
        match ColorWin::create(el, ctx, start, at) {
            Ok(w) => {
                self.owners.insert(w.sw.window.id(), Owner::Color);
                self.color_win = Some(w);
            }
            Err(e) => log::error!("colour window: {e}"),
        }
    }

    pub(super) fn close_color(&mut self) {
        if let Some(w) = self.color_win.take() {
            self.owners.remove(&w.sw.window.id());
        }
    }

    pub(super) fn color_event(&mut self, event: WindowEvent) {
        let Some(w) = self.color_win.as_mut() else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => w.draw(),
            WindowEvent::Resized(_) => w.sw.window.request_redraw(),
            WindowEvent::CloseRequested => self.close_color(),
            WindowEvent::CursorMoved { position, .. } => {
                if w.moved(position.x, position.y) {
                    w.sw.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => w.release(),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let hit = w.press();
                w.sw.window.request_redraw();
                match hit {
                    Some(CBtn::Ok) => {
                        let c = w.color();
                        self.close_color();
                        if let Some(ov) = self.overlay.as_mut() {
                            ov.session.editor.set_color(c);
                            self.cfg.color = c.to_hex();
                        }
                    }
                    Some(CBtn::Cancel) => self.close_color(),
                    None => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.logical_key {
                    WKey::Named(NamedKey::Escape) => self.close_color(),
                    WKey::Named(NamedKey::Enter) => {
                        let c = w.color();
                        self.close_color();
                        if let Some(ov) = self.overlay.as_mut() {
                            ov.session.editor.set_color(c);
                            self.cfg.color = c.to_hex();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
