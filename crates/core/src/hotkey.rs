//! Shortcut strings. The config stores pynput-style text (`<ctrl>+<alt>+a`) so existing settings keep working; the UI
//! shows and edits the Qt-style form (`Ctrl+Alt+A`).

const MOD_PAIRS: [(&str, &str); 4] = [
    ("Ctrl", "<ctrl>"),
    ("Meta", "<cmd>"),
    ("Alt", "<alt>"),
    ("Shift", "<shift>"),
];
const KEY_PAIRS: [(&str, &str); 17] = [
    ("Return", "<enter>"),
    ("Esc", "<esc>"),
    ("Space", "<space>"),
    ("Tab", "<tab>"),
    ("Backspace", "<backspace>"),
    ("Del", "<delete>"),
    ("Ins", "<insert>"),
    ("Home", "<home>"),
    ("End", "<end>"),
    ("PgUp", "<page_up>"),
    ("PgDown", "<page_down>"),
    ("Up", "<up>"),
    ("Down", "<down>"),
    ("Left", "<left>"),
    ("Right", "<right>"),
    ("Print", "<print_screen>"),
    ("Pause", "<pause>"),
];

fn mod_to_stored(name: &str, mac: bool) -> Option<&'static str> {
    // On macOS Qt's "Ctrl" is Command and "Meta" is the Control key.
    Some(match (name, mac) {
        ("Ctrl", false) | ("Meta", true) => "<ctrl>",
        ("Ctrl", true) | ("Meta", false) => "<cmd>",
        ("Alt", _) => "<alt>",
        ("Shift", _) => "<shift>",
        _ => return None,
    })
}

fn key_to_stored(key: &str) -> Option<String> {
    let alias = match key {
        "Enter" => "Return",
        "Escape" => "Esc",
        "Delete" => "Del",
        "Insert" => "Ins",
        "PageUp" => "PgUp",
        "PageDown" => "PgDown",
        "SysReq" => "Print",
        k => k,
    };
    if let Some((_, v)) = KEY_PAIRS.iter().find(|(k, _)| *k == alias) {
        return Some((*v).to_string());
    }
    let mut chars = key.chars();
    match (chars.next(), key.chars().count()) {
        (Some('F'), n) if n > 1 && key[1..].chars().all(|c| c.is_ascii_digit()) => {
            Some(format!("<{}>", key.to_lowercase()))
        }
        (Some(c), 1) => Some(c.to_lowercase().to_string()),
        _ => None,
    }
}

/// `Ctrl+Alt+A` → `<ctrl>+<alt>+a`. Empty string for an empty or unsupported sequence.
pub fn to_stored(seq: &str, mac: bool) -> String {
    let seq = seq.split(',').next().unwrap_or("").trim();
    if seq.is_empty() {
        return String::new();
    }
    let mut parts: Vec<&str> = seq.split('+').collect();
    if seq.ends_with("++") {
        parts.truncate(parts.len() - 2);
        parts.push("+");
    }
    let Some((key, mods)) = parts.split_last() else {
        return String::new();
    };
    let mut out = Vec::new();
    for m in mods {
        match mod_to_stored(m, mac) {
            Some(s) => out.push(s.to_string()),
            None => return String::new(),
        }
    }
    match key_to_stored(key) {
        Some(k) => out.push(k),
        None => return String::new(),
    }
    out.join("+")
}

/// `<ctrl>+<alt>+a` → `Ctrl+Alt+A`.
pub fn to_display(stored: &str, mac: bool) -> String {
    let mut out = Vec::new();
    for part in stored.split('+') {
        if part.is_empty() {
            continue;
        }
        let shown = MOD_PAIRS
            .iter()
            .find_map(|(q, s)| {
                // invert the macOS swap
                let name = match (*q, mac) {
                    ("Ctrl", true) => "Meta",
                    ("Meta", true) => "Ctrl",
                    (q, _) => q,
                };
                (*s == part).then(|| name.to_string())
            })
            .or_else(|| {
                KEY_PAIRS
                    .iter()
                    .find(|(_, s)| *s == part)
                    .map(|(q, _)| (*q).to_string())
            })
            .or_else(|| {
                part.strip_prefix("<f")
                    .and_then(|r| r.strip_suffix('>'))
                    .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                    .map(|n| format!("F{n}"))
            })
            .unwrap_or_else(|| part.to_uppercase());
        out.push(shown);
    }
    out.join("+")
}

/// Parsed form used by the platform hotkey backends.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Key name as stored: a single lowercase character or `<name>`.
    pub key: String,
}

pub fn parse_stored(stored: &str) -> Option<Combo> {
    let mut c = Combo::default();
    for part in stored.split('+').filter(|p| !p.is_empty()) {
        match part {
            "<ctrl>" => c.ctrl = true,
            "<alt>" => c.alt = true,
            "<shift>" => c.shift = true,
            "<cmd>" => c.cmd = true,
            k if c.key.is_empty() => c.key = k.to_string(),
            _ => return None,
        }
    }
    (!c.key.is_empty()).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_qt_style_to_stored() {
        assert_eq!(to_stored("Ctrl+Alt+A", false), "<ctrl>+<alt>+a");
        assert_eq!(to_stored("F3", false), "<f3>");
        assert_eq!(
            to_stored("Ctrl+Shift+Return", false),
            "<ctrl>+<shift>+<enter>"
        );
        assert_eq!(to_stored("Ctrl+Alt++", false), "<ctrl>+<alt>++");
        assert_eq!(
            to_stored("Ctrl+K, Ctrl+C", false),
            "<ctrl>+k",
            "chords keep only the first part"
        );
        assert_eq!(to_stored("", false), "");
        assert_eq!(to_stored("Foo+A", false), "");
        assert_eq!(to_stored("Ctrl+Banana", false), "");
    }

    #[test]
    fn macos_swaps_ctrl_and_meta() {
        assert_eq!(to_stored("Ctrl+Shift+A", true), "<cmd>+<shift>+a");
        assert_eq!(to_stored("Meta+Shift+A", true), "<ctrl>+<shift>+a");
        assert_eq!(to_display("<ctrl>+<shift>+a", true), "Meta+Shift+A");
        assert_eq!(to_display("<cmd>+<shift>+a", true), "Ctrl+Shift+A");
    }

    #[test]
    fn display_round_trips() {
        for s in [
            "Ctrl+Alt+A",
            "F3",
            "Ctrl+Shift+PgUp",
            "Alt+Space",
            "Ctrl+F12",
        ] {
            assert_eq!(to_display(&to_stored(s, false), false), s);
        }
        assert_eq!(to_display("<ctrl>+<alt>+v", false), "Ctrl+Alt+V");
    }

    #[test]
    fn parse_stored_extracts_modifiers_and_key() {
        let c = parse_stored("<ctrl>+<alt>+a").unwrap();
        assert!(c.ctrl && c.alt && !c.shift && !c.cmd);
        assert_eq!(c.key, "a");
        assert_eq!(parse_stored("<f3>").unwrap().key, "<f3>");
        assert!(parse_stored("<ctrl>").is_none());
        assert!(parse_stored("a+b").is_none());
        assert!(parse_stored("").is_none());
    }
}
