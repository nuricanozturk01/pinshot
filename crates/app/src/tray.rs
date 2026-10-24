//! System tray icon and menu. Linux: StatusNotifierItem over D-Bus (ksni, no GTK). Windows/macOS: tray-icon.

use crate::ui::icons;
use pinshot_core::i18n::tr;

/// One menu entry: (command, Turkish label key). Commands starting with `delay:` come from the delay submenu.
pub enum Entry {
    Item(&'static str, &'static str),
    Sub(&'static str, Vec<(&'static str, &'static str)>),
    Sep,
}

pub fn entries() -> Vec<Entry> {
    vec![
        Entry::Item("capture", "Bölge yakala"),
        Entry::Sub(
            "Gecikmeli yakala",
            vec![
                ("delay:3", "3 saniye"),
                ("delay:5", "5 saniye"),
                ("delay:10", "10 saniye"),
            ],
        ),
        Entry::Item("fullscreen", "Tam ekran yakala"),
        Entry::Item("repeat", "Son bölgeyi tekrarla"),
        Entry::Sep,
        Entry::Item("record", "Ekranı video olarak kaydet (GIF / MP4)…"),
        Entry::Sep,
        Entry::Item("pin_clipboard", "Panodan sabitle"),
        Entry::Item("pin_file", "Dosyadan sabitle…"),
        Entry::Item("close_pins", "Tüm sabitlenenleri kapat"),
        Entry::Sep,
        Entry::Item("open_folder", "Kayıt klasörünü aç"),
        Entry::Item("settings", "Ayarlar…"),
        Entry::Item("about", "Hakkında"),
        Entry::Sep,
        Entry::Item("quit", "Çıkış"),
    ]
}

fn label(cmd: &str, recording: bool, key: &'static str) -> String {
    if cmd == "record" && recording {
        tr("Kaydı durdur").to_string()
    } else {
        tr(key).to_string()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::{StandardItem, SubMenu};
    use ksni::{Icon, MenuItem};

    pub struct Tray {
        handle: Option<Handle<Model>>,
    }

    struct Model {
        send: Box<dyn Fn(String) + Send + Sync>,
        recording: bool,
    }

    fn argb(pm: &tiny_skia::Pixmap) -> Icon {
        let mut data = Vec::with_capacity(pm.data().len());
        for p in pm.pixels() {
            let c = p.demultiply();
            data.extend_from_slice(&[c.alpha(), c.red(), c.green(), c.blue()]);
        }
        Icon {
            width: pm.width() as i32,
            height: pm.height() as i32,
            data,
        }
    }

    impl ksni::Tray for Model {
        fn id(&self) -> String {
            "pinshot".into()
        }

        fn title(&self) -> String {
            "PinShot".into()
        }

        fn icon_pixmap(&self) -> Vec<Icon> {
            vec![
                argb(&icons::app_icon(64, self.recording)),
                argb(&icons::app_icon(32, self.recording)),
            ]
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: if self.recording {
                    tr("PinShot – kayıt sürüyor").into()
                } else {
                    "PinShot".into()
                },
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            (self.send)("capture".into());
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let item = |cmd: &'static str, key: &'static str, rec: bool| {
                MenuItem::from(StandardItem {
                    label: label(cmd, rec, key),
                    activate: Box::new(move |t: &mut Model| (t.send)(cmd.to_string())),
                    ..Default::default()
                })
            };
            entries()
                .into_iter()
                .map(|e| match e {
                    Entry::Item(c, k) => item(c, k, self.recording),
                    Entry::Sep => MenuItem::Separator,
                    Entry::Sub(k, subs) => SubMenu {
                        label: tr(k).to_string(),
                        submenu: subs.into_iter().map(|(c, k)| item(c, k, false)).collect(),
                        ..Default::default()
                    }
                    .into(),
                })
                .collect()
        }
    }

    impl Tray {
        pub fn new(send: impl Fn(String) + Send + Sync + 'static) -> Tray {
            let model = Model {
                send: Box::new(send),
                recording: false,
            };
            let handle = model
                .spawn()
                .map_err(|e| log::warn!("no system tray: {e}"))
                .ok();
            Tray { handle }
        }

        pub fn set_recording(&self, on: bool) {
            if let Some(h) = &self.handle {
                let _ = h.update(|m| m.recording = on);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    pub struct Tray {
        icon: Option<TrayIcon>,
        record_item: Option<MenuItem>,
    }

    fn icon(rec: bool) -> Option<Icon> {
        let pm = icons::app_icon(64, rec);
        let rgba = pinshot_core::output::to_rgba8(&pm);
        Icon::from_rgba(rgba, pm.width(), pm.height()).ok()
    }

    impl Tray {
        pub fn new(send: impl Fn(String) + Send + Sync + 'static) -> Tray {
            let menu = Menu::new();
            let mut record_item = None;
            let mut ids: Vec<(tray_icon::menu::MenuId, String)> = Vec::new();
            for e in entries() {
                match e {
                    Entry::Sep => {
                        let _ = menu.append(&PredefinedMenuItem::separator());
                    }
                    Entry::Item(c, k) => {
                        let it = MenuItem::new(tr(k), true, None);
                        ids.push((it.id().clone(), c.to_string()));
                        if c == "record" {
                            record_item = Some(it.clone());
                        }
                        let _ = menu.append(&it);
                    }
                    Entry::Sub(k, subs) => {
                        let sm = Submenu::new(tr(k), true);
                        for (c, k) in subs {
                            let it = MenuItem::new(tr(k), true, None);
                            ids.push((it.id().clone(), c.to_string()));
                            let _ = sm.append(&it);
                        }
                        let _ = menu.append(&sm);
                    }
                }
            }
            let send = std::sync::Arc::new(send);
            let s2 = send.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                if let Some((_, c)) = ids.iter().find(|(id, _)| *id == e.id) {
                    s2(c.clone());
                }
            }));
            if !cfg!(target_os = "macos") {
                let s3 = send.clone();
                TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        s3("capture".into());
                    }
                }));
            }
            let built = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("PinShot")
                .with_icon(icon(false).expect("icon"))
                .build();
            let icon = built.map_err(|e| log::warn!("no tray icon: {e}")).ok();
            Tray { icon, record_item }
        }

        pub fn set_recording(&self, on: bool) {
            if let Some(i) = &self.icon {
                let _ = i.set_icon(icon(on));
                let _ = i.set_tooltip(Some(if on {
                    tr("PinShot – kayıt sürüyor")
                } else {
                    "PinShot"
                }));
            }
            if let Some(r) = &self.record_item {
                r.set_text(label(
                    "record",
                    on,
                    "Ekranı video olarak kaydet (GIF / MP4)…",
                ));
            }
        }
    }
}

pub use imp::Tray;
