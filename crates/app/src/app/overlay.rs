//! The capture overlay: drawing, pointer and keyboard input, toolbar events.

use super::*;

impl App {
    // ------------------------------------------------------------------ overlay helpers
    pub(super) fn redraw_all(&self) {
        if let Some(ov) = &self.overlay {
            for w in &ov.wins {
                w.sw.window.request_redraw();
            }
        }
    }

    pub(super) fn close_overlay(&mut self) {
        if let Some(ov) = self.overlay.take() {
            if let Some(sel) = ov.session.editor.sel {
                let name = ov
                    .session
                    .caps
                    .first()
                    .map(|c| c.monitor.name.clone())
                    .unwrap_or_default();
                self.cfg.last_region = Some((name, sel.x, sel.y, sel.w, sel.h));
            }
            for w in &ov.wins {
                self.owners.remove(&w.sw.window.id());
            }
            for p in &self.pins {
                p.sw.window.set_visible(true);
            }
            self.save_cfg();
            drop(ov);
            crate::mem::release_unused();
        }
    }

    pub(super) fn draw_overlay_window(&mut self, win: usize) {
        // a pointer sweep produces far more events than the display can show: draw each window at most once per frame
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let Some(w) = ov.wins.get_mut(win) else {
            return;
        };
        if w.last_draw.is_some_and(|t| t.elapsed() < FRAME) {
            if !ov.deferred.contains(&win) {
                ov.deferred.push(win);
            }
            return;
        }
        w.last_draw = Some(Instant::now());
        w.sw.sync_size();
        let (pw, ph) = w.sw.size();
        let g = Geom {
            mon: w.mon,
            sf: w.sf,
            size: (pw, ph),
        };
        let full = IRect::new(0, 0, pw, ph);
        let cur = ov.session.extent(&g);
        let hint = ov.session.editor.sel.is_none();
        let hover_win = ov.session.editor.hover_win.filter(|_| hint);
        let hover_ext = ov.session.hover_extent(&g).filter(|_| hint);
        let damage = if w.full {
            Some(full)
        } else {
            let moved = match (w.prev, cur) {
                (Some(a), Some(b)) => Some(a.union(&b)),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            };
            // moving the outline to another window: the old and the new one
            let moved = if hover_win == w.hover_win {
                moved
            } else {
                [moved, w.hover_ext, hover_ext]
                    .into_iter()
                    .flatten()
                    .reduce(|a, b| a.union(&b))
            };
            // the hint pill only changes when a selection starts or ends
            if hint == w.hint {
                moved
            } else {
                let strip = ov.session.hint_strip(&g);
                Some(moved.map_or(strip, |m| m.union(&strip)))
            }
        };
        let Some(mut d) = damage.and_then(|d| d.intersect(&full)) else {
            return;
        };
        let age = if d == full { 0 } else { w.sw.buffer_age() };
        if d != full {
            // buffers rotate on some systems (macOS, Wayland): bring an older buffer up to date, or redraw all
            let age = usize::from(age);
            d = match age {
                1 => d,
                2..=4 if w.history.len() >= age - 1 => w.history[w.history.len() - (age - 1)..]
                    .iter()
                    .fold(d, |acc, h| acc.union(h))
                    .intersect(&full)
                    .unwrap_or(full),
                _ => full,
            };
        }
        // a rectangle being dragged out only changes along its edges: skip the unchanged inside
        let core_now = ov.session.draw_core(&g);
        let strips = match (d != full && age == 1, w.core, core_now) {
            (true, Some(a), Some(b)) => a
                .intersect(&b)
                .map(|hole| d.without(&hole))
                .filter(|v| v.len() > 1 || v.first() != Some(&d)),
            _ => None,
        };
        // only the first frame of a window needs it (to pick the window that takes focus)
        let cursor_here = !w.shown
            && screens::cursor_pos()
                .map(|p| ov.session.caps[g.mon].monitor.rect.contains(p))
                .unwrap_or(win == 0);
        let presented = if let Some(strips) = strips {
            let parts: Vec<_> = strips
                .iter()
                .map(|r| (ov.session.compose(&g, *r), (r.x, r.y)))
                .collect();
            w.sw.present_parts(&parts).map(|()| true)
        } else {
            let pm = ov.session.compose(&g, d);
            w.sw.present_covering(&pm, (d.x, d.y)).map(|()| true)
        };
        match presented {
            Ok(true) => {
                if d == full {
                    w.history.clear();
                }
                w.history.push(d);
                if w.history.len() > 4 {
                    w.history.remove(0);
                }
                w.prev = cur;
                w.core = core_now;
                w.hint = hint;
                w.hover_win = hover_win;
                w.hover_ext = hover_ext;
                w.full = false;
                if !w.shown {
                    w.shown = true;
                    w.sw.window.set_visible(true);
                    // only the monitor under the pointer takes focus: several windows grabbing it at once makes
                    // the first clicks get lost
                    if cursor_here {
                        w.sw.window.focus_window();
                    }
                }
            }
            Ok(false) => {
                w.full = true;
                w.sw.window.request_redraw();
            }
            Err(e) => log::error!("present: {e}"),
        }
    }

    /// Desktop coordinates of a pointer position reported to overlay window `win`.
    pub(super) fn desktop_point(&self, win: usize, pos: PhysicalPosition<f64>) -> Point {
        let ov = self.overlay.as_ref().expect("overlay");
        let w = &ov.wins[win];
        let (pw, ph) = w.sw.size();
        let inside = pos.x >= 0.0 && pos.y >= 0.0 && pos.x < pw as f64 && pos.y < ph as f64;
        let mixed = ov
            .session
            .caps
            .iter()
            .any(|c| (c.monitor.scale - ov.session.caps[w.mon].monitor.scale).abs() > 1e-3);
        if !inside && mixed {
            // while a button is held the pointer can leave this window but events keep arriving in this window's
            // scale; with different scale factors those coordinates are wrong, so ask the system instead
            if let Some(p) = screens::cursor_pos() {
                return p;
            }
        }
        ov.session
            .from_px(w.mon, Point::new(pos.x as f32, pos.y as f32))
    }

    pub(super) fn pointer_moved(&mut self, win: usize, pos: PhysicalPosition<f64>) {
        if self.overlay.is_none() {
            return;
        }
        let pt = self.desktop_point(win, pos);
        let shift = self.mods.shift_key();
        let ov = self.overlay.as_mut().expect("overlay");
        ov.session.cursor = Some(pt);
        let w = &ov.wins[win];
        let g = Geom {
            mon: w.mon,
            sf: w.sf,
            size: w.sw.size(),
        };
        let ui = Point::new(pos.x as f32 / w.sf, pos.y as f32 / w.sf);
        let bar = ov.session.toolbar_origin(&g);
        let mut icon = None;
        let panel_hit = match (ov.session.panel_origin(&g), ov.session.rec.as_mut()) {
            (Some(o), Some(p)) => {
                let local = Point::new(ui.x - o.x, ui.y - o.y);
                if p.contains(local) {
                    p.set_hover(Some(local));
                    true
                } else {
                    p.set_hover(None);
                    false
                }
            }
            _ => false,
        };
        if panel_hit {
            icon = Some(CursorIcon::Default);
        } else if ov.session.toolbar.is_dragging() {
            if let Some(o) = bar {
                if let Some(ev) = ov.session.toolbar.drag(Point::new(ui.x - o.x, ui.y - o.y)) {
                    self.apply_toolbar_event(ev, None);
                }
            }
        } else if let Some(o) = bar.filter(|o| {
            ov.session
                .toolbar
                .contains(Point::new(ui.x - o.x, ui.y - o.y))
        }) {
            let local = Point::new(ui.x - o.x, ui.y - o.y);
            ov.session.toolbar.set_hover(Some(local));
            let tip = ov.session.toolbar.tooltip_at(local);
            if ov.session.tooltip.as_ref().map(|t| &t.0) != tip.map(|t| t.0.to_string()).as_ref() {
                ov.session.tooltip = None;
            }
            ov.hover = tip.map(|_| (Instant::now(), win, 0));
            icon = Some(CursorIcon::Default);
        } else {
            ov.session.toolbar.set_hover(None);
            ov.session.tooltip = None;
            ov.hover = None;
            ov.session.editor.moved(pt, shift);
            icon = Some(map_cursor(
                ov.session.editor.cursor_at(pt, Fonts::embedded()),
            ));
        }
        let ov = self.overlay.as_mut().expect("overlay");
        ov.session.sync_toolbar();
        if let Some(i) = icon.filter(|i| ov.cursor_icon != Some(*i)) {
            ov.cursor_icon = Some(i);
            for w in &ov.wins {
                w.sw.window.set_cursor(i);
            }
        }
        self.redraw_all();
    }

    pub(super) fn pointer_button(
        &mut self,
        el: &ActiveEventLoop,
        win: usize,
        button: MouseButton,
        state: ElementState,
    ) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let Some(pos) = ov.session.cursor else { return };
        let w = &ov.wins[win];
        let g = Geom {
            mon: w.mon,
            sf: w.sf,
            size: w.sw.size(),
        };
        let p_px = ov.session.to_px(w.mon, pos);
        let ui = Point::new(p_px.x / w.sf, p_px.y / w.sf);
        let bar = ov.session.toolbar_origin(&g);
        let on_bar = bar.filter(|o| {
            ov.session
                .toolbar
                .contains(Point::new(ui.x - o.x, ui.y - o.y))
        });
        let panel_ev = match (ov.session.panel_origin(&g), ov.session.rec.as_mut()) {
            (Some(o), Some(p)) if button == MouseButton::Left && state == ElementState::Pressed => {
                let local = Point::new(ui.x - o.x, ui.y - o.y);
                if p.contains(local) {
                    Some(p.press(local))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(ev) = panel_ev {
            if let Some(ev) = ev {
                self.rec_panel_event(el, ev);
            }
            self.redraw_all();
            return;
        }
        let m = Fonts::embedded();
        let mut cmd = Cmd::None;
        log::debug!(
            "button {button:?} {state:?} at {pos:?} mode {:?} tool {:?}",
            ov.session.editor.mode,
            ov.session.editor.tool
        );
        match (button, state) {
            (MouseButton::Left, ElementState::Pressed) => {
                if let Some(o) = on_bar {
                    ov.session.editor.commit_text(m);
                    let ev = ov.session.toolbar.press(Point::new(ui.x - o.x, ui.y - o.y));
                    if let Some(ev) = ev {
                        self.apply_toolbar_event(ev, Some(el));
                    }
                } else {
                    if ov.session.editor.mode != pinshot_core::editor::Mode::Idle {
                        ov.session.editor.release(); // a lost button-up must not leave a drag hanging
                    }
                    let now = Instant::now();
                    let dbl = ov
                        .last_click
                        .map(|(t, p)| now.duration_since(t) < DOUBLE_CLICK && p.distance(pos) < 6.0)
                        .unwrap_or(false);
                    ov.last_click = if dbl { None } else { Some((now, pos)) };
                    cmd = if dbl {
                        ov.session.editor.double_click(pos, m)
                    } else {
                        ov.session.editor.press(pos, Button::Left, m)
                    };
                }
            }
            (MouseButton::Right, ElementState::Pressed) => {
                if on_bar.is_none() {
                    cmd = ov.session.editor.press(pos, Button::Right, m);
                }
            }
            (MouseButton::Left, ElementState::Released) => {
                ov.session.toolbar.release();
                ov.session.editor.release();
            }
            _ => {}
        }
        if let Some(ov) = self.overlay.as_mut() {
            ov.session.sync_toolbar();
        }
        self.run_cmd(el, cmd);
        self.redraw_all();
    }

    pub(super) fn apply_toolbar_event(&mut self, ev: TbEvent, el: Option<&ActiveEventLoop>) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let m = Fonts::embedded();
        let ed = &mut ov.session.editor;
        let mut cmd = Cmd::None;
        let mut open_rec = false;
        let mut pick_color = None;
        match ev {
            TbEvent::Tool(t) => ed.set_tool(t, m),
            TbEvent::Color(c) => {
                ed.set_color(c);
                self.cfg.color = c.to_hex();
            }
            TbEvent::CustomColor => {
                pick_color = Some(ed.color);
            }
            TbEvent::Width(w) => {
                ed.set_width(w);
                if ed.selected_item().is_none() || ed.tool != Tool::Select {
                    self.cfg.pen_width = w;
                }
            }
            TbEvent::Fill(v) => ed.set_fill(v),
            TbEvent::Dash(v) => ed.set_dashed(v),
            TbEvent::Font(v) => {
                ed.set_font_px(v);
                self.cfg.font_px = v;
            }
            TbEvent::Act(a) => match a {
                Act::Undo => ed.undo(m),
                Act::Redo => ed.redo(m),
                Act::Delete => ed.delete_selected(),
                Act::Copy => cmd = Cmd::Copy,
                Act::Save => cmd = Cmd::Save,
                Act::Pin => cmd = Cmd::Pin,
                Act::Ocr => cmd = Cmd::Ocr,
                Act::Cancel => cmd = Cmd::Close,
                Act::Record => open_rec = true,
            },
        }
        ov.session.sync_toolbar();
        if let Some(el) = el {
            self.run_cmd(el, cmd);
            if open_rec {
                self.open_rec_panel(el);
            }
            if let Some(c) = pick_color {
                self.open_color(el, c);
            }
        }
    }

    pub(super) fn key_pressed(&mut self, el: &ActiveEventLoop, event: &KeyEvent) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let ctrl = if cfg!(target_os = "macos") {
            self.mods.super_key()
        } else {
            self.mods.control_key()
        };
        let alt = self.mods.alt_key();
        let mods = Mods {
            ctrl: ctrl && !(alt && !cfg!(target_os = "macos") && event.text.is_some()),
            shift: self.mods.shift_key(),
            alt,
        };
        let typing = ov.session.editor.edit.is_some();
        let keys = convert_key(event, typing, mods.ctrl);
        log::debug!(
            "key {:?} -> {keys:?} typing {typing} tool {:?}",
            event.logical_key,
            ov.session.editor.tool
        );
        let m = Fonts::embedded();
        let mut cmd = Cmd::None;
        let recording_setup = ov.session.rec.is_some();
        for k in keys {
            if recording_setup {
                // only Escape, Enter, the arrow keys (move the area) and Ctrl+A mean anything while choosing
                // recording options
                match k {
                    Key::Escape => return self.rec_cancel_redraw(),
                    Key::Enter if ov.session.editor.sel.is_some() => {
                        return self.rec_start_selected(el, false)
                    }
                    Key::Left | Key::Right | Key::Up | Key::Down => {}
                    Key::Char('a') if mods.ctrl => {}
                    _ => continue,
                }
            }
            let c = ov.session.editor.key(k, mods, m);
            if c != Cmd::None {
                cmd = c;
            }
        }
        ov.session.sync_toolbar();
        self.run_cmd(el, cmd);
        self.redraw_all();
    }
}
