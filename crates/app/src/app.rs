//! The application: event loop, windows and the capture flow.

use crate::clip::Clip;
use crate::gfx::{Context, IRect, SoftWindow};
mod capture;
mod dialogs;
mod ocr;
mod output;
mod overlay;
mod pin_menu;
mod pinned;
mod prefs;
mod rec;

use crate::aboutwin::{AboutWin, Btn as ABtn};
use crate::colorwin::{Btn as CBtn, ColorWin};
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
use winit::dpi::PhysicalPosition;
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
    RecorderStarted(Result<Box<pinshot_record::Recorder>, String>),
    FolderChosen(Option<PathBuf>),
}

/// The overlay redraws at most this often (about 60 frames per second).
const FRAME: Duration = Duration::from_millis(16);
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
    About,
    Color,
}

struct OverlayWin {
    sw: SoftWindow,
    mon: usize,
    sf: f32,
    prev: Option<IRect>,
    full: bool,
    /// The hint pill was part of the last frame.
    hint: bool,
    /// The window outline shown in the last frame (its undimmed area changes only when this does).
    hover_win: Option<Rect>,
    /// The area that outline took, so a change of window only redraws the old and the new one.
    hover_ext: Option<IRect>,
    /// Areas drawn by the latest frames (newest last): a swap-chain buffer that is older than the last frame also
    /// needs what those changed.
    history: Vec<IRect>,
    shown: bool,
    /// The unchanging inner part of the rectangle being drawn, as of the last frame.
    core: Option<IRect>,
    /// When this window last drew: its own clock, so windows that have nothing to redraw do not use up its frame.
    last_draw: Option<Instant>,
}

struct Overlay {
    session: Session,
    wins: Vec<OverlayWin>,
    last_click: Option<(Instant, Point)>,
    hover: Option<(Instant, usize, usize)>,
    saving: bool,
    /// The windows whose redraw was held back to keep to [`FRAME`].
    deferred: Vec<usize>,
    /// The cursor icon last set on the overlay windows; setting it again on every pointer move costs a round trip.
    cursor_icon: Option<CursorIcon>,
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
    about_win: Option<AboutWin>,
    color_win: Option<ColorWin>,
    recording: Option<rec::Recording>,
    rec_finalizing: Option<u64>,
    start_abort: Option<rec::Abort>,
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
            about_win: None,
            color_win: None,
            recording: None,
            rec_finalizing: None,
            start_abort: None,
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
        let sf = monitor_scale(el, &m.rect);
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
            Some(m) => (monitor_scale(el, &m.rect), m.rect),
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
            (self.cfg.hotkey_window.clone(), "window"),
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
            "window" => self.capture_active_window(el),
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
            "about" => self.open_about(el),
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
}

fn load_image(path: &std::path::Path) -> Option<tiny_skia::Pixmap> {
    let img = image::open(path).ok()?.to_rgba8();
    pinshot_core::output::from_rgba8(img.width(), img.height(), img.as_raw())
}

/// Open a folder or file with the system's default program.
/// Open a web address in the default browser.
pub fn open_url(url: &str) {
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(windows) {
        ("rundll32", vec!["url.dll,FileProtocolHandler", url]) // no console window, no `&` quoting issues
    } else {
        ("xdg-open", vec![url])
    };
    if let Err(e) = std::process::Command::new(cmd).args(args).spawn() {
        log::warn!("cannot open {url}: {e}");
    }
}

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
            UserEvent::RecorderStarted(r) => self.recorder_started(el, r),
            UserEvent::PinSaveChosen(id, p) => self.pin_save_chosen(el, id, p),
            UserEvent::PinFilesChosen(files) => self.pin_files_chosen(el, files),
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
            Owner::About => return self.about_event(event),
            Owner::Color => return self.color_event(event),
            Owner::Toast => {
                match event {
                    WindowEvent::RedrawRequested | WindowEvent::Resized(_) => {
                        if let Some(t) = self.toasts.iter_mut().find(|t| t.sw.window.id() == id) {
                            t.redraw();
                        }
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        ..
                    } => self.toast_clicked(el, id),
                    _ => {}
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
            if !ov.deferred.is_empty() {
                let now = Instant::now();
                let due_of = |w: usize| {
                    ov.wins
                        .get(w)
                        .and_then(|w| w.last_draw)
                        .map_or(now, |t| t + FRAME)
                };
                let (ready, waiting): (Vec<usize>, Vec<usize>) =
                    ov.deferred.iter().partition(|w| now >= due_of(**w));
                if let Some(due) = waiting.iter().map(|w| due_of(*w)).min() {
                    wake = Some(wake.map_or(due, |w| w.min(due)));
                }
                for w in ready {
                    if let Some(w) = ov.wins.get(w) {
                        w.sw.window.request_redraw();
                    }
                }
                ov.deferred = waiting;
            }
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

/// Scale factor of the winit monitor that sits at `rect` (desktop units: physical pixels, but points on macOS where
/// winit reports physical positions, so those are converted before comparing).
fn monitor_scale(el: &ActiveEventLoop, rect: &Rect) -> f32 {
    el.available_monitors()
        .find(|h| {
            let k = if cfg!(target_os = "macos") {
                h.scale_factor() as f32
            } else {
                1.0
            };
            let p = h.position();
            (p.x as f32 / k - rect.x).abs() < 1.5 && (p.y as f32 / k - rect.y).abs() < 1.5
        })
        .map_or(1.0, |h| h.scale_factor() as f32)
}

/// winit's X11 backend panics when `libxkbcommon-x11` is missing, so only pick it when the library loads.
#[cfg(target_os = "linux")]
fn x11_keyboard_support() -> bool {
    ["libxkbcommon-x11.so.0", "libxkbcommon-x11.so"]
        .iter()
        .any(|n| {
            // SAFETY: loading the library runs its (trivial) initialisers; the handle is dropped right away.
            #[expect(
                unsafe_code,
                reason = "dlopen probe, nothing from the library is called"
            )]
            unsafe {
                libloading::Library::new(n).is_ok()
            }
        })
}

pub fn run(first_command: Option<String>) -> Result<(), String> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    #[cfg(target_os = "linux")]
    {
        // Wayland compositors let a client neither place its windows nor read the global pointer, which pins,
        // menus and the capture overlay rely on; XWayland gives every desktop the same behaviour. Pure Wayland
        // sessions (no DISPLAY) and `PINSHOT_BACKEND=wayland` keep the native backend.
        use winit::platform::x11::EventLoopBuilderExtX11;
        let want_native = std::env::var("PINSHOT_BACKEND").is_ok_and(|v| v == "wayland");
        if !want_native
            && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
            && x11_keyboard_support()
        {
            builder.with_x11();
        }
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
