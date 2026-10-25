//! The pin context menu and the actions it triggers.

use super::*;

impl App {
    pub(super) fn pin_menu(&mut self, el: &ActiveEventLoop, id: u64, at: Point) {
        let Some(i) = self.pin_index(id) else { return };
        let p = &self.pins[i];
        let pct = |ids: [&'static str; 7]| {
            let v = [25, 50, 75, 100, 150, 200, 300];
            ids.iter()
                .zip(v)
                .map(|(id, v)| MenuItem::action(id, &format!("%{v}")))
                .collect::<Vec<_>>()
        };
        let opacity: Vec<MenuItem> = [
            ("op100", 100),
            ("op80", 80),
            ("op60", 60),
            ("op40", 40),
            ("op20", 20),
        ]
        .iter()
        .map(|(id, v)| MenuItem::action(id, &format!("%{v}")))
        .collect();
        let items = vec![
            MenuItem::action("copy", tr("Kopyala")).with_hint("Ctrl+C"),
            MenuItem::action("save", tr("Kaydet…")).with_hint("Ctrl+S"),
            MenuItem::action("ocr", tr("Metni tanı (OCR)")),
            MenuItem::sep(),
            MenuItem::sub(
                tr("Boyut"),
                pct([
                    "size25", "size50", "size75", "size100", "size150", "size200", "size300",
                ]),
            ),
            MenuItem::sub(tr("Opaklık"), opacity),
            MenuItem::sub(
                tr("Dönüştür"),
                vec![
                    MenuItem::action("flipx", tr("Yatay çevir")),
                    MenuItem::action("flipy", tr("Dikey çevir")),
                    MenuItem::action("rotcw", tr("Saat yönünde döndür")),
                    MenuItem::action("rotccw", tr("Saat yönünün tersine döndür")),
                ],
            ),
            MenuItem::check("lock", tr("Konumu kilitle"), p.locked),
            MenuItem::check("border", tr("Kenarlık"), p.border),
            MenuItem::sep(),
            MenuItem::action("close", tr("Kapat")).with_hint("Esc"),
            MenuItem::action("closeall", tr("Tüm pinleri kapat")),
        ];
        self.close_popup();
        let (sf, bounds) = self.monitor_info(el, at);
        let Some(ctx) = self.ctx.as_ref() else { return };
        if let Some(pop) = Popup::open(el, ctx, items, at, sf, bounds, id) {
            for w in &pop.stack {
                self.owners.insert(w.sw.window.id(), Owner::Menu);
            }
            self.popup = Some(pop);
        }
    }

    pub(super) fn popup_contains(&self, p: Point) -> bool {
        self.popup
            .as_ref()
            .map(|pop| {
                pop.stack
                    .iter()
                    .any(|w| crate::units::outer_rect(&w.sw.window).contains(p))
            })
            .unwrap_or(false)
    }

    pub(super) fn close_popup(&mut self) {
        if let Some(p) = self.popup.take() {
            for w in &p.stack {
                self.owners.remove(&w.sw.window.id());
            }
        }
    }

    pub(super) fn menu_event(&mut self, el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let Some(level) = self.popup.as_ref().and_then(|p| p.owns(wid)) else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(p) = self.popup.as_mut() {
                    p.stack[level].paint();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if matches!(event.logical_key, WKey::Named(NamedKey::Escape)) {
                    self.close_popup();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let bounds = screens::cursor_pos()
                    .map(|p| self.monitor_info(el, p).1)
                    .unwrap_or_default();
                if let (Some(pop), Some(ctx)) = (self.popup.as_mut(), self.ctx.as_ref()) {
                    pop.hover(el, ctx, level, position, bounds);
                    let new: Vec<WindowId> = pop.stack.iter().map(|w| w.sw.window.id()).collect();
                    self.owners
                        .retain(|k, v| *v != Owner::Menu || new.contains(k));
                    for id in new {
                        self.owners.insert(id, Owner::Menu);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let Some(pos) = self
                    .popup
                    .as_ref()
                    .and_then(|p| p.stack.get(level))
                    .map(|w| w.sw.window.id())
                    .and(self.last_menu_pos(level))
                else {
                    return;
                };
                let outcome = self
                    .popup
                    .as_mut()
                    .map(|p| p.click(level, pos))
                    .unwrap_or(Outcome::None);
                if let Outcome::Chosen(id) = outcome {
                    let owner = self.popup.as_ref().map(|p| p.owner).unwrap_or(0);
                    self.close_popup();
                    self.pin_action(el, owner, id);
                }
            }
            _ => {}
        }
    }

    pub(super) fn last_menu_pos(&self, level: usize) -> Option<PhysicalPosition<f64>> {
        let w = self.popup.as_ref()?.stack.get(level)?;
        let g = screens::cursor_pos()?;
        let o = crate::units::outer_rect(&w.sw.window);
        let k = f64::from(crate::units::px_per_unit(w.sw.window.scale_factor() as f32));
        Some(PhysicalPosition::new(
            (g.x as f64 - o.x as f64) * k,
            (g.y as f64 - o.y as f64) * k,
        ))
    }

    pub(super) fn pin_action(&mut self, el: &ActiveEventLoop, id: u64, action: &str) {
        if action == "closeall" {
            return self.close_all_pins();
        }
        let Some(i) = self.pin_index(id) else { return };
        match action {
            "copy" => {
                let r = self.clip.set_image(&self.pins[i].image);
                if let Err(e) = r {
                    self.toast(el, &format!("{}: {e}", tr("Panoya kopyalanamadı")));
                }
            }
            "save" => {
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
                    let _ = proxy.send_event(UserEvent::PinSaveChosen(id, path));
                });
            }
            "ocr" => {
                let (img, at) = (
                    self.pins[i].image.clone(),
                    Point::new(self.pins[i].geom.x + 20.0, self.pins[i].geom.y + 20.0),
                );
                self.ocr_start(el, img, at);
            }
            "close" => self.close_pin(id),
            "lock" => self.pins[i].locked = !self.pins[i].locked,
            "border" => {
                self.pins[i].border = !self.pins[i].border;
                self.pins[i].sw.window.request_redraw();
            }
            "flipx" => self.pins[i].transform(Transform::FlipH),
            "flipy" => self.pins[i].transform(Transform::FlipV),
            "rotcw" => self.pins[i].transform(Transform::RotateCw),
            "rotccw" => self.pins[i].transform(Transform::RotateCcw),
            other => {
                let p = &mut self.pins[i];
                if let Some(v) = other
                    .strip_prefix("size")
                    .and_then(|n| n.parse::<f32>().ok())
                {
                    let g = pin::percent_rect(p.geom, p.natural(), v);
                    p.set_geom(g);
                } else if let Some(v) = other.strip_prefix("op").and_then(|n| n.parse::<f32>().ok())
                {
                    p.opacity = v / 100.0;
                    p.sw.window.request_redraw();
                }
            }
        }
    }

    pub(super) fn pin_save_chosen(&mut self, el: &ActiveEventLoop, id: u64, path: Option<PathBuf>) {
        let (Some(path), Some(i)) = (path, self.pin_index(id)) else {
            return;
        };
        if let Err(e) =
            pinshot_core::output::save_image(&self.pins[i].image, &path, self.cfg.quality)
        {
            self.toast(el, &format!("{}: {e}", tr("Kaydedilemedi")));
        }
    }
}
