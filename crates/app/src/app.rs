//! The application: event loop, windows and the capture flow.

use crate::clip::Clip;
use crate::gfx::{Context, IRect, SoftWindow};
mod rec;

use crate::ocrwin::{Btn, OcrWin};
use crate::pins::PinWin;
use crate::popup::{Outcome, Popup};
use crate::screens::{self, Capture};
use crate::session::{Geom, Session};
use crate::settings;
use crate::settingswin::{Btn as SBtn, SettingsWin};
use crate::toast::Toast;
use crate::toolbar::{Act, Event as TbEvent};
use crate::ui::menu::Item as MenuItem;
use pinshot_core::config::Config;
use pinshot_core::editor::{Button, Cmd, CursorKind, Key, Mods, Tool};
use pinshot_core::i18n::tr;
use pinshot_core::pin::{self, Transform};
use pinshot_core::text::Fonts;
use pinshot_core::{Point, Rect, Rgba};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowId, WindowLevel};

/// Things that happen outside the event loop's own window events.
pub enum UserEvent {
    /// A command name such as `capture` (from the tray, a hotkey or the command line).
    Command(String),
    /// Start the capture now (after the delay that lets menus disappear).
    CaptureNow(bool),
    SaveChosen(Option<PathBuf>),
    PinSaveChosen(u64, Option<PathBuf>),
    PinFilesChosen(Vec<PathBuf>),
    OcrDone(u64, Result<String, String>),
    RecordNow(Box<rec::RecordRequest>),
    Rec(pinshot_record::Event),
    Toast(String),
    FolderChosen(Option<PathBuf>),
}

const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const TOOLTIP_DELAY: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Overlay(usize),
    Toast,
    Pin(u64),
    Menu,
    Ocr(u64),
    Island,
    Settings,
}

struct OverlayWin {
    sw: SoftWindow,
    mon: usize,
    sf: f32,
    prev: Option<IRect>,
    full: bool,
    shown: bool,
}

struct Overlay {
    session: Session,
    wins: Vec<OverlayWin>,
    last_click: Option<(Instant, Point)>,
    hover: Option<(Instant, usize, usize)>,
    saving: bool,
}

pub struct App {
    proxy: EventLoopProxy<UserEvent>,
    ctx: Option<Context>,
    cfg: Config,
    cfg_path: PathBuf,
    clip: Clip,
    overlay: Option<Overlay>,
    owners: HashMap<WindowId, Owner>,
    toasts: Vec<Toast>,
    pins: Vec<PinWin>,
    next_pin: u64,
    popup: Option<Popup>,
    ocr_wins: Vec<OcrWin>,
    settings_win: Option<SettingsWin>,
    recording: Option<rec::Recording>,
    rec_finalizing: Option<u64>,
    last_output: Option<PathBuf>,
    ocr_engine: Arc<std::sync::Mutex<Option<pinshot_ocr::Engine>>>,
    ocr_last_use: Option<Instant>,
    mods: ModifiersState,
    started: bool,
    tray: Option<crate::tray::Tray>,
    hotkeys: Option<crate::hotkeys::Hotkeys>,
}

pub fn ui_scale(w: &Window) -> f32 {
    w.scale_factor() as f32
}

impl App {
    pub fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        let cfg_path = Config::default_path();
        App {
            proxy,
            ctx: None,
            cfg: Config::load(&cfg_path),
            cfg_path,
            clip: Clip::new(),
            overlay: None,
            owners: HashMap::new(),
            toasts: Vec::new(),
            pins: Vec::new(),
            next_pin: 1,
            popup: None,
            ocr_wins: Vec::new(),
            settings_win: None,
            recording: None,
            rec_finalizing: None,
            last_output: None,
            ocr_engine: Arc::new(std::sync::Mutex::new(None)),
            ocr_last_use: None,
            mods: ModifiersState::default(),
            started: false,
            tray: None,
            hotkeys: None,
        }
    }

    fn save_cfg(&self) {
        if let Err(e) = self.cfg.save(&self.cfg_path) {
            log::error!("cannot save config: {e}");
        }
    }

    // ------------------------------------------------------------------ notifications
    fn toast(&mut self, el: &ActiveEventLoop, text: &str) {
        self.toast_click(el, text, "");
    }

    /// A notification that runs `click` (a command name) when clicked; `""` = not clickable.
    fn toast_click(&mut self, el: &ActiveEventLoop, text: &str, click: &'static str) {
        let Some(ctx) = self.ctx.as_ref() else { return };
        let mons = screens::monitors().unwrap_or_default();
        let at = screens::cursor_pos()
            .and_then(|p| screens::monitor_at(&mons, p))
            .and_then(|i| mons.get(i).cloned());
        let Some(m) = at else { return };
        let sf = el
            .available_monitors()
            .find(|h| h.position().x == m.rect.x as i32 && h.position().y == m.rect.y as i32)
            .map(|h| h.scale_factor() as f32)
            .unwrap_or(1.0);
        let secs = if click.is_empty() { 4 } else { 8 };
        match Toast::show(
            el,
            ctx,
            text,
            m.rect,
            sf,
            secs,
            (!click.is_empty()).then_some(click),
        ) {
            Ok(t) => {
                self.owners.insert(t.sw.window.id(), Owner::Toast);
                self.toasts.push(t);
            }
            Err(e) => log::warn!("toast failed: {e}"),
        }
        log::info!("notify: {text}");
    }

    fn toast_clicked(&mut self, el: &ActiveEventLoop, id: WindowId) {
        let click = self
            .toasts
            .iter()
            .find(|t| t.sw.window.id() == id)
            .and_then(|t| t.click);
        if let Some(c) = click {
            self.owners.remove(&id);
            self.toasts.retain(|t| t.sw.window.id() != id);
            self.command(el, c);
        }
    }

    /// Scale factor and bounds of the monitor holding `p`.
    fn monitor_info(&self, el: &ActiveEventLoop, p: Point) -> (f32, Rect) {
        let mons = screens::monitors().unwrap_or_default();
        let m = screens::monitor_at(&mons, p).and_then(|i| mons.get(i).cloned());
        match m {
            Some(m) => {
                let sf = el
                    .available_monitors()
                    .find(|h| {
                        h.position().x == m.rect.x as i32 && h.position().y == m.rect.y as i32
                    })
                    .map(|h| h.scale_factor() as f32)
                    .unwrap_or(1.0);
                (sf, m.rect)
            }
            None => (1.0, Rect::new(0.0, 0.0, 1920.0, 1080.0)),
        }
    }

    fn expire_toasts(&mut self) {
        let now = Instant::now();
        let owners = &mut self.owners;
        self.toasts.retain(|t| {
            let keep = t.deadline > now;
            if !keep {
                owners.remove(&t.sw.window.id());
            }
            keep
        });
    }

    fn reload_hotkeys(&mut self, el: &ActiveEventLoop) {
        let actions: Vec<(String, String)> = [
            (self.cfg.hotkey_capture.clone(), "capture"),
            (self.cfg.hotkey_fullscreen.clone(), "fullscreen"),
            (self.cfg.hotkey_repeat.clone(), "repeat"),
            (self.cfg.hotkey_pin.clone(), "pin_clipboard"),
            (self.cfg.hotkey_record.clone(), "record"),
        ]
        .into_iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, a)| (k, a.to_string()))
        .collect();
        let p = self.proxy.clone();
        let failed = match self.hotkeys.as_mut() {
            Some(h) => h.set(&actions, move |a| {
                let _ = p.send_event(UserEvent::Command(a));
            }),
            None => actions.iter().map(|(_, a)| a.clone()).collect(),
        };
        if !failed.is_empty() {
            self.toast(el, tr("Global kısayollar başlatılamadı. Tray menüsünü veya 'pinshot --capture' komutunu kullanın."));
        }
    }

    // ------------------------------------------------------------------ commands
    fn command(&mut self, el: &ActiveEventLoop, name: &str) {
        match name {
            "capture" => self.schedule_capture(150),
            "fullscreen" => self.capture_fullscreen(el),
            "repeat" => self.capture_last_region(el),
            c if c.starts_with("delay:") => {
                let secs: u64 = c[6..].parse().unwrap_or(3);
                self.toast(el, &format!("{secs} {}", tr("saniye sonra yakalanacak")));
                self.schedule_capture(secs * 1000);
            }
            "open_folder" => open_path(&self.cfg.save_dir_path()),
            "record" => self.record_command(el),
            "stop_record" => self.stop_recording(el),
            "open_output" => {
                if let Some(p) = self.last_output.as_ref().and_then(|p| p.parent()) {
                    open_path(p);
                }
            }
            "settings" => self.open_settings(el),
            "pin_clipboard" => self.pin_clipboard(el),
            "pin_file" => self.pin_file_dialog(),
            "close_pins" => self.close_all_pins(),
            "quit" => {
                self.cancel_recording();
                self.save_cfg();
                el.exit();
            }
            "ping" => self.toast(el, tr("PinShot zaten çalışıyor")),
            other => log::warn!("unknown command {other}"),
        }
    }

    fn schedule_capture(&mut self, lag_ms: u64) {
        self.schedule_capture_mode(lag_ms, false);
    }

    /// `rec`: open the overlay for recording (options panel instead of the toolbar).
    fn schedule_capture_mode(&mut self, lag_ms: u64, rec: bool) {
        if self.overlay.is_some() {
            return;
        }
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(lag_ms));
            let _ = proxy.send_event(UserEvent::CaptureNow(rec));
        });
    }

    fn grab(&mut self, el: &ActiveEventLoop) -> Option<Vec<Capture>> {
        if !screens::screen_access(true) {
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

    fn capture_now(&mut self, el: &ActiveEventLoop, rec: bool) {
        if self.overlay.is_some() {
            return;
        }
        let Some(caps) = self.grab(el) else { return };
        let Some(ctx) = self.ctx.as_ref() else { return };
        let color = Rgba::parse(&self.cfg.color).unwrap_or(Rgba::rgb(0xff, 0x3b, 0x30));
        let session = Session::new(caps, color, self.cfg.pen_width, self.cfg.font_px);
        let mut session = session;
        if rec {
            session.rec = Some(self.make_panel(false));
            session.rec_flow = true;
        }
        let mut wins = Vec::new();
        let handles: Vec<_> = el.available_monitors().collect();
        for (i, cap) in session.caps.iter().enumerate() {
            let r = cap.monitor.rect;
            let (pw, ph) = (cap.pixmap.width(), cap.pixmap.height());
            let mut attrs = Window::default_attributes()
                .with_title("PinShot")
                .with_decorations(false)
                .with_resizable(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(crate::gfx::win_pos(r.x, r.y))
                .with_inner_size(crate::gfx::size_units(r.w, r.h));
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
                                shown: false,
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
        });
        self.redraw_all();
    }

    fn capture_fullscreen(&mut self, el: &ActiveEventLoop) {
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

    fn capture_last_region(&mut self, el: &ActiveEventLoop) {
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
    fn finish_image(&mut self, el: &ActiveEventLoop, pm: &tiny_skia::Pixmap, what: &str) {
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

    fn autosave(&self, pm: &tiny_skia::Pixmap) -> String {
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

    // ------------------------------------------------------------------ overlay helpers
    fn redraw_all(&self) {
        if let Some(ov) = &self.overlay {
            for w in &ov.wins {
                w.sw.window.request_redraw();
            }
        }
    }

    fn close_overlay(&mut self) {
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
            self.save_cfg();
        }
    }

    fn draw_overlay_window(&mut self, win: usize) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let Some(w) = ov.wins.get_mut(win) else {
            return;
        };
        w.sw.sync_size();
        let (pw, ph) = w.sw.size();
        let g = Geom {
            mon: w.mon,
            sf: w.sf,
            size: (pw, ph),
        };
        let full = IRect::new(0, 0, pw, ph);
        let cur = ov.session.extent(&g);
        let damage = if w.full {
            Some(full)
        } else {
            match (w.prev, cur) {
                (Some(a), Some(b)) => Some(a.union(&b)),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            }
        };
        let Some(d) = damage.and_then(|d| d.intersect(&full)) else {
            return;
        };
        let cursor_here = screens::cursor_pos()
            .map(|p| ov.session.caps[g.mon].monitor.rect.contains(p))
            .unwrap_or(win == 0);
        let pm = ov.session.compose(&g, d);
        match w.sw.present(&pm, (d.x, d.y)) {
            Ok(true) => {
                w.prev = cur;
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
    fn desktop_point(&self, win: usize, pos: PhysicalPosition<f64>) -> Point {
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

    fn pointer_moved(&mut self, win: usize, pos: PhysicalPosition<f64>) {
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
        if let Some(i) = icon {
            for w in &ov.wins {
                w.sw.window.set_cursor(i);
            }
        }
        self.redraw_all();
    }

    fn pointer_button(
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

    fn apply_toolbar_event(&mut self, ev: TbEvent, el: Option<&ActiveEventLoop>) {
        let Some(ov) = self.overlay.as_mut() else {
            return;
        };
        let m = Fonts::embedded();
        let ed = &mut ov.session.editor;
        let mut cmd = Cmd::None;
        let mut open_rec = false;
        match ev {
            TbEvent::Tool(t) => ed.set_tool(t, m),
            TbEvent::Color(c) => {
                ed.set_color(c);
                self.cfg.color = c.to_hex();
            }
            TbEvent::CustomColor => log::info!("custom colour picker not available yet"),
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
        }
    }

    fn key_pressed(&mut self, el: &ActiveEventLoop, event: &KeyEvent) {
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

    fn rec_cancel_redraw(&mut self) {
        self.rec_cancel();
        self.redraw_all();
    }

    fn run_cmd(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        match cmd {
            Cmd::None => {}
            Cmd::Close => self.close_overlay(),
            Cmd::Copy => self.overlay_copy(el),
            Cmd::Save => self.overlay_save(el),
            Cmd::Pin => self.overlay_pin(el),
            Cmd::Ocr => self.overlay_ocr(el),
            Cmd::CopyColor { hex } => {
                let Some(ov) = self.overlay.as_ref() else {
                    return;
                };
                let at = screens::cursor_pos().or(ov.session.cursor);
                if let Some(c) = at.and_then(|p| ov.session.pixel_at(p)) {
                    let text = if hex {
                        c.to_hex().to_uppercase()
                    } else {
                        format!("rgb({}, {}, {})", c.r, c.g, c.b)
                    };
                    let _ = self.clip.set_text(&text);
                    self.close_overlay();
                    self.toast(el, &format!("{}: {text}", tr("Renk kopyalandı")));
                }
            }
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

    fn overlay_copy(&mut self, el: &ActiveEventLoop) {
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

    fn overlay_save(&mut self, _el: &ActiveEventLoop) {
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

    fn save_chosen(&mut self, el: &ActiveEventLoop, path: Option<PathBuf>) {
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

    // ------------------------------------------------------------------ OCR
    fn overlay_ocr(&mut self, el: &ActiveEventLoop) {
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

    fn ocr_start(&mut self, el: &ActiveEventLoop, img: tiny_skia::Pixmap, at: Point) {
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
            let _ = proxy.send_event(UserEvent::OcrDone(id, result));
        });
    }

    fn ocr_done(&mut self, id: u64, result: Result<String, String>) {
        self.ocr_last_use = Some(Instant::now());
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

    fn close_ocr(&mut self, id: u64) {
        if let Some(i) = self.ocr_wins.iter().position(|w| w.id == id) {
            let w = self.ocr_wins.remove(i);
            self.owners.remove(&w.sw.window.id());
        }
    }

    fn ocr_event(&mut self, _el: &ActiveEventLoop, id: u64, event: WindowEvent) {
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

    fn ocr_copy(&mut self, i: usize) {
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

    // ------------------------------------------------------------------ settings
    fn open_settings(&mut self, el: &ActiveEventLoop) {
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

    fn close_settings(&mut self) {
        if let Some(w) = self.settings_win.take() {
            self.owners.remove(&w.sw.window.id());
        }
    }

    /// Write the form into the configuration and make it take effect.
    fn apply_settings(&mut self, el: &ActiveEventLoop) {
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

    fn folder_chosen(&mut self, path: Option<PathBuf>) {
        let (Some(path), Some(w)) = (path, self.settings_win.as_mut()) else {
            return;
        };
        if let Some(r) = w.form.row_mut("save_dir") {
            r.control = crate::form::Control::Folder(path.to_string_lossy().into_owned());
        }
        w.sw.window.request_redraw();
    }

    fn settings_event(&mut self, el: &ActiveEventLoop, event: WindowEvent) {
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
                        let p = rfd::FileDialog::new().set_directory(start).pick_folder();
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

    fn settings_key(&mut self, event: &KeyEvent) {
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

    // ------------------------------------------------------------------ pins
    fn add_pin(&mut self, el: &ActiveEventLoop, pm: tiny_skia::Pixmap, at: Point, dpr: f32) {
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
    fn pin_centered(&mut self, el: &ActiveEventLoop, pm: tiny_skia::Pixmap) {
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

    fn pin_clipboard(&mut self, el: &ActiveEventLoop) {
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

    fn pin_file_dialog(&mut self) {
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

    fn pin_files_chosen(&mut self, el: &ActiveEventLoop, files: Vec<PathBuf>) {
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

    fn close_all_pins(&mut self) {
        for p in self.pins.drain(..) {
            self.owners.remove(&p.sw.window.id());
        }
        self.close_popup();
    }

    fn close_pin(&mut self, id: u64) {
        if let Some(i) = self.pins.iter().position(|p| p.id == id) {
            let p = self.pins.remove(i);
            self.owners.remove(&p.sw.window.id());
        }
        if self.popup.as_ref().map(|p| p.owner) == Some(id) {
            self.close_popup();
        }
    }

    fn overlay_pin(&mut self, el: &ActiveEventLoop) {
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

    fn pin_index(&self, id: u64) -> Option<usize> {
        self.pins.iter().position(|p| p.id == id)
    }

    fn pin_event(&mut self, el: &ActiveEventLoop, id: u64, event: WindowEvent) {
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
                        .set_outer_position(crate::gfx::win_pos(r.x, r.y));
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

    fn pin_menu(&mut self, el: &ActiveEventLoop, id: u64, at: Point) {
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

    fn popup_contains(&self, p: Point) -> bool {
        self.popup
            .as_ref()
            .map(|pop| {
                pop.stack
                    .iter()
                    .any(|w| crate::gfx::outer_rect(&w.sw.window).contains(p))
            })
            .unwrap_or(false)
    }

    fn close_popup(&mut self) {
        if let Some(p) = self.popup.take() {
            for w in &p.stack {
                self.owners.remove(&w.sw.window.id());
            }
        }
    }

    fn menu_event(&mut self, el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
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

    fn last_menu_pos(&self, level: usize) -> Option<PhysicalPosition<f64>> {
        let w = self.popup.as_ref()?.stack.get(level)?;
        let g = screens::cursor_pos()?;
        let o = crate::gfx::outer_rect(&w.sw.window);
        let k = if cfg!(target_os = "macos") {
            w.sw.window.scale_factor()
        } else {
            1.0
        };
        Some(PhysicalPosition::new(
            (g.x as f64 - o.x as f64) * k,
            (g.y as f64 - o.y as f64) * k,
        ))
    }

    fn pin_action(&mut self, el: &ActiveEventLoop, id: u64, action: &str) {
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

    fn pin_save_chosen(&mut self, el: &ActiveEventLoop, id: u64, path: Option<PathBuf>) {
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

fn load_image(path: &std::path::Path) -> Option<tiny_skia::Pixmap> {
    let img = image::open(path).ok()?.to_rgba8();
    pinshot_core::output::from_rgba8(img.width(), img.height(), img.as_raw())
}

/// Open a folder or file with the system's default program.
pub fn open_path(path: &std::path::Path) {
    let (cmd, args): (&str, Vec<&std::ffi::OsStr>) = if cfg!(target_os = "macos") {
        ("open", vec![path.as_os_str()])
    } else if cfg!(windows) {
        ("explorer", vec![path.as_os_str()])
    } else {
        ("xdg-open", vec![path.as_os_str()])
    };
    if let Err(e) = std::process::Command::new(cmd).args(args).spawn() {
        log::warn!("cannot open {}: {e}", path.display());
    }
}

fn map_cursor(k: CursorKind) -> CursorIcon {
    match k {
        CursorKind::Cross => CursorIcon::Crosshair,
        CursorKind::Arrow => CursorIcon::Default,
        CursorKind::Text => CursorIcon::Text,
        CursorKind::Hand => CursorIcon::Grab,
        CursorKind::Move => CursorIcon::Move,
        CursorKind::SizeFDiag => CursorIcon::NwseResize,
        CursorKind::SizeBDiag => CursorIcon::NeswResize,
        CursorKind::SizeVer => CursorIcon::NsResize,
        CursorKind::SizeHor => CursorIcon::EwResize,
    }
}

/// Translate a winit key event into editor keys. While typing, the produced text is used (case, layout, dead keys);
/// otherwise the lower-cased logical key, so shortcuts work on any layout.
fn convert_key(event: &KeyEvent, typing: bool, ctrl: bool) -> Vec<Key> {
    match &event.logical_key {
        WKey::Named(n) => match n {
            NamedKey::Escape => vec![Key::Escape],
            NamedKey::Enter => vec![Key::Enter],
            NamedKey::Backspace => vec![Key::Backspace],
            NamedKey::Delete => vec![Key::Delete],
            NamedKey::ArrowLeft => vec![Key::Left],
            NamedKey::ArrowRight => vec![Key::Right],
            NamedKey::ArrowUp => vec![Key::Up],
            NamedKey::ArrowDown => vec![Key::Down],
            NamedKey::Home => vec![Key::Home],
            NamedKey::End => vec![Key::End],
            NamedKey::F3 => vec![Key::F3],
            NamedKey::Shift if !event.repeat => vec![Key::Shift],
            NamedKey::Space => vec![Key::Char(' ')],
            NamedKey::Tab if typing => vec![Key::Char('\t')],
            _ => vec![],
        },
        WKey::Character(s) => {
            if typing && !ctrl {
                let text = event
                    .text
                    .as_ref()
                    .map(|t| t.as_str())
                    .unwrap_or(s.as_str());
                text.chars()
                    .filter(|c| !c.is_control())
                    .map(Key::Char)
                    .collect()
            } else {
                s.chars()
                    .next()
                    .map(|c| vec![Key::Char(c.to_lowercase().next().unwrap_or(c))])
                    .unwrap_or_default()
            }
        }
        _ => vec![],
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.started {
            return;
        }
        self.started = true;
        pinshot_core::i18n::apply(&self.cfg.language, sys_locale::get_locale());
        self.ctx =
            Some(softbuffer::Context::new(el.owned_display_handle()).expect("display context"));
        el.set_control_flow(ControlFlow::Wait);
        let p = self.proxy.clone();
        self.tray = Some(crate::tray::Tray::new(move |c| {
            let _ = p.send_event(UserEvent::Command(c));
        }));
        self.hotkeys = Some(crate::hotkeys::Hotkeys::new());
        self.reload_hotkeys(el);
        log::info!("PinShot {} started", env!("CARGO_PKG_VERSION"));
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Command(c) => self.command(el, &c),
            UserEvent::CaptureNow(rec) => self.capture_now(el, rec),
            UserEvent::SaveChosen(p) => self.save_chosen(el, p),
            UserEvent::OcrDone(id, r) => self.ocr_done(id, r),
            UserEvent::RecordNow(req) => self.record_now(el, *req),
            UserEvent::Rec(e) => self.rec_event(el, e),
            UserEvent::PinSaveChosen(id, p) => self.pin_save_chosen(el, id, p),
            UserEvent::PinFilesChosen(files) => self.pin_files_chosen(el, files),
            UserEvent::Toast(t) => self.toast(el, &t),
            UserEvent::FolderChosen(p) => self.folder_chosen(p),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(&owner) = self.owners.get(&id) else {
            return;
        };
        let win = match owner {
            Owner::Overlay(i) => i,
            Owner::Pin(pid) => return self.pin_event(el, pid, event),
            Owner::Menu => return self.menu_event(el, id, event),
            Owner::Ocr(oid) => return self.ocr_event(el, oid, event),
            Owner::Island => return self.island_event(el, event),
            Owner::Settings => return self.settings_event(el, event),
            Owner::Toast => {
                if let WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    ..
                } = event
                {
                    self.toast_clicked(el, id);
                }
                return;
            }
        };
        if !matches!(
            event,
            WindowEvent::CursorMoved { .. } | WindowEvent::RedrawRequested
        ) {
            log::debug!("win {win} event {event:?}");
        }
        match event {
            WindowEvent::RedrawRequested => self.draw_overlay_window(win),
            WindowEvent::Resized(_) => {
                if let Some(ov) = self.overlay.as_mut() {
                    if let Some(w) = ov.wins.get_mut(win) {
                        w.sf = ui_scale(&w.sw.window);
                        w.full = true;
                        w.sw.window.request_redraw();
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorMoved { position, .. } => self.pointer_moved(win, position),
            WindowEvent::MouseInput { state, button, .. } => {
                self.pointer_button(el, win, button, state)
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.key_pressed(el, &event)
            }
            WindowEvent::CloseRequested => self.close_overlay(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.expire_toasts();
        if let Some(t) = self.ocr_last_use {
            let due = t + Duration::from_secs(300);
            if Instant::now() >= due {
                if let Ok(mut g) = self.ocr_engine.try_lock() {
                    *g = None; // free the engine's memory after 5 idle minutes
                    self.ocr_last_use = None;
                }
            }
        }
        let mut wake: Option<Instant> = self.toasts.iter().map(|t| t.deadline).min();
        if let Some(ov) = self.overlay.as_mut() {
            if let Some((since, win, _)) = ov.hover {
                let due = since + TOOLTIP_DELAY;
                if Instant::now() >= due {
                    let g = Geom {
                        mon: ov.wins[win].mon,
                        sf: ov.wins[win].sf,
                        size: ov.wins[win].sw.size(),
                    };
                    if let (Some(o), Some(pos)) = (ov.session.toolbar_origin(&g), ov.session.cursor)
                    {
                        let p = ov.session.to_px(g.mon, pos);
                        let local = Point::new(p.x / g.sf - o.x, p.y / g.sf - o.y);
                        ov.session.tooltip = ov
                            .session
                            .toolbar
                            .tooltip_at(local)
                            .map(|(t, r)| (t.to_string(), r));
                    }
                    ov.hover = None;
                    for w in &ov.wins {
                        w.sw.window.request_redraw();
                    }
                } else {
                    wake = Some(wake.map_or(due, |w| w.min(due)));
                }
            }
        }
        if self.popup.is_some() {
            // dismiss when a button goes down outside the menu windows (there is no pointer grab to tell us)
            if screens::buttons_down() {
                let inside = screens::cursor_pos()
                    .map(|p| self.popup_contains(p))
                    .unwrap_or(true);
                if !inside {
                    self.close_popup();
                }
            }
            if self.popup.is_some() {
                let due = Instant::now() + Duration::from_millis(40);
                wake = Some(wake.map_or(due, |w| w.min(due)));
            }
        }
        el.set_control_flow(match wake {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}

pub fn run(first_command: Option<String>) -> Result<(), String> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let el = builder.build().map_err(|e| e.to_string())?;
    let proxy = el.create_proxy();
    let p2 = proxy.clone();
    if let Err(e) = crate::ipc::listen(move |c| {
        let _ = p2.send_event(UserEvent::Command(
            c.trim_start_matches("--").replace('-', "_"),
        ));
    }) {
        log::warn!("ipc: {e}");
    }
    if let Some(c) = first_command {
        let _ = proxy.send_event(UserEvent::Command(c));
    }
    let mut app = App::new(proxy);
    el.run_app(&mut app).map_err(|e| e.to_string())
}

/// `F1`..`F24` for a function key.
fn f_key_name(n: NamedKey) -> Option<String> {
    let i = [
        NamedKey::F1,
        NamedKey::F2,
        NamedKey::F3,
        NamedKey::F4,
        NamedKey::F5,
        NamedKey::F6,
        NamedKey::F7,
        NamedKey::F8,
        NamedKey::F9,
        NamedKey::F10,
        NamedKey::F11,
        NamedKey::F12,
    ]
    .iter()
    .position(|k| *k == n)?;
    Some(format!("F{}", i + 1))
}

/// Register or remove the start-at-login entry for this executable.
fn set_autostart(on: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let launcher = auto_launch::AutoLaunchBuilder::new()
        .set_app_name("PinShot")
        .set_app_path(&exe.to_string_lossy())
        .build()
        .map_err(|e| e.to_string())?;
    if on {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}
