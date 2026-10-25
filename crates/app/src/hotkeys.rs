//! Global shortcuts (global-hotkey): stored combo strings → registered hot keys.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use pinshot_core::hotkey::{parse_stored, Combo};
use std::collections::HashMap;

pub fn key_code(name: &str) -> Option<Code> {
    use Code::*;
    let n = name.trim_start_matches('<').trim_end_matches('>');
    if n.chars().count() == 1 {
        let c = n.chars().next()?.to_ascii_lowercase();
        return Some(match c {
            'a' => KeyA,
            'b' => KeyB,
            'c' => KeyC,
            'd' => KeyD,
            'e' => KeyE,
            'f' => KeyF,
            'g' => KeyG,
            'h' => KeyH,
            'i' => KeyI,
            'j' => KeyJ,
            'k' => KeyK,
            'l' => KeyL,
            'm' => KeyM,
            'n' => KeyN,
            'o' => KeyO,
            'p' => KeyP,
            'q' => KeyQ,
            'r' => KeyR,
            's' => KeyS,
            't' => KeyT,
            'u' => KeyU,
            'v' => KeyV,
            'w' => KeyW,
            'x' => KeyX,
            'y' => KeyY,
            'z' => KeyZ,
            '0' => Digit0,
            '1' => Digit1,
            '2' => Digit2,
            '3' => Digit3,
            '4' => Digit4,
            '5' => Digit5,
            '6' => Digit6,
            '7' => Digit7,
            '8' => Digit8,
            '9' => Digit9,
            '`' => Backquote,
            '-' => Minus,
            '=' => Equal,
            '[' => BracketLeft,
            ']' => BracketRight,
            '\\' => Backslash,
            ';' => Semicolon,
            '\'' => Quote,
            ',' => Comma,
            '.' => Period,
            '/' => Slash,
            _ => return None,
        });
    }
    Some(match n {
        "enter" => Enter,
        "esc" => Escape,
        "space" => Space,
        "tab" => Tab,
        "backspace" => Backspace,
        "delete" => Delete,
        "insert" => Insert,
        "home" => Home,
        "end" => End,
        "page_up" => PageUp,
        "page_down" => PageDown,
        "up" => ArrowUp,
        "down" => ArrowDown,
        "left" => ArrowLeft,
        "right" => ArrowRight,
        "print_screen" => PrintScreen,
        "pause" => Pause,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        "f13" => F13,
        "f14" => F14,
        "f15" => F15,
        "f16" => F16,
        "f17" => F17,
        "f18" => F18,
        "f19" => F19,
        "f20" => F20,
        _ => return None,
    })
}

pub fn to_hotkey(c: &Combo) -> Option<HotKey> {
    let mut m = Modifiers::empty();
    if c.ctrl {
        m |= Modifiers::CONTROL;
    }
    if c.alt {
        m |= Modifiers::ALT;
    }
    if c.shift {
        m |= Modifiers::SHIFT;
    }
    if c.cmd {
        m |= Modifiers::META;
    }
    Some(HotKey::new(
        if m.is_empty() { None } else { Some(m) },
        key_code(&c.key)?,
    ))
}

pub struct Hotkeys {
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
}

impl Hotkeys {
    pub fn new() -> Hotkeys {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| log::warn!("global shortcuts unavailable: {e}"))
            .ok();
        Hotkeys {
            manager,
            registered: Vec::new(),
        }
    }

    /// Replace all shortcuts; `actions` maps a stored combo to a command name. Returns the commands that could
    /// not be registered (taken by another program, unsupported key, no permission).
    pub fn set(
        &mut self,
        actions: &[(String, String)],
        on_press: impl Fn(String) + Send + Sync + 'static,
    ) -> Vec<String> {
        let mut failed = Vec::new();
        let Some(m) = self.manager.as_ref() else {
            return actions.iter().map(|(_, a)| a.clone()).collect();
        };
        let _ = m.unregister_all(&self.registered);
        self.registered.clear();
        let mut by_id: HashMap<u32, String> = HashMap::new();
        for (stored, action) in actions {
            let hk = parse_stored(stored).and_then(|c| to_hotkey(&c));
            match hk {
                Some(h) => match m.register(h) {
                    Ok(()) => {
                        by_id.insert(h.id(), action.clone());
                        self.registered.push(h);
                    }
                    Err(e) => {
                        log::warn!("cannot register {stored} for {action}: {e}");
                        failed.push(action.clone());
                    }
                },
                None => failed.push(action.clone()),
            }
        }
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            log::debug!("hotkey event {e:?}");
            if e.state == HotKeyState::Pressed {
                if let Some(a) = by_id.get(&e.id) {
                    on_press(a.clone());
                }
            }
        }));
        failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_stored_combos_to_hotkeys() {
        let c = parse_stored("<ctrl>+<alt>+a").unwrap();
        let h = to_hotkey(&c).unwrap();
        assert_eq!(
            h,
            HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyA)
        );
        assert_eq!(
            to_hotkey(&parse_stored("<f3>").unwrap()).unwrap(),
            HotKey::new(None, Code::F3)
        );
        assert_eq!(
            to_hotkey(&parse_stored("<cmd>+<shift>+p").unwrap()).unwrap(),
            HotKey::new(Some(Modifiers::META | Modifiers::SHIFT), Code::KeyP)
        );
        assert!(to_hotkey(&Combo {
            key: "<banana>".into(),
            ..Combo::default()
        })
        .is_none());
    }

    #[test]
    fn every_default_shortcut_is_mappable() {
        for mac in [false, true] {
            for (_, v) in pinshot_core::config::default_hotkeys(mac) {
                assert!(parse_stored(v).and_then(|c| to_hotkey(&c)).is_some(), "{v}");
            }
        }
    }
}
