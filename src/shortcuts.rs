extern crate alloc;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

use crate::keyboard::{Key, KeyInput};

const CONFIG_PATH: &str = "/CONFIG/SHORTCUT.CFG";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Launcher,
    Notifications,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShortcutKey {
    Char(char),
    Space,
    Tab,
    Escape,
    F4,
    F5,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

#[derive(Clone, Copy)]
struct Shortcut {
    action: Action,
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: ShortcutKey,
}

static SHORTCUTS: Mutex<Vec<Shortcut>> = Mutex::new(Vec::new());

pub fn load_from_disk() {
    ensure_default_file();
    let mut shortcuts = default_shortcuts();
    if let Some(bytes) = crate::config_store::read(CONFIG_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for line in text.lines() {
                let Some((name, combo)) = line.split_once('=') else {
                    continue;
                };
                let Some(action) = parse_action(name.trim()) else {
                    continue;
                };
                if let Some(shortcut) = parse_shortcut(action, combo.trim()) {
                    if let Some(slot) = shortcuts.iter_mut().find(|slot| slot.action == action) {
                        *slot = shortcut;
                    } else {
                        shortcuts.push(shortcut);
                    }
                }
            }
        }
    }
    *SHORTCUTS.lock() = shortcuts;
}

pub fn matches(action: Action, input: KeyInput) -> bool {
    ensure_loaded();
    SHORTCUTS
        .lock()
        .iter()
        .any(|shortcut| shortcut.action == action && shortcut.matches(input))
}

pub fn summary_lines() -> Vec<String> {
    ensure_loaded();
    SHORTCUTS
        .lock()
        .iter()
        .map(|shortcut| shortcut.summary())
        .collect()
}

fn ensure_loaded() {
    let empty = SHORTCUTS.lock().is_empty();
    if empty {
        load_from_disk();
    }
}

fn ensure_default_file() {
    let _ = crate::config_store::write_default(
        CONFIG_PATH,
        b"launcher=Ctrl+Space\nnotifications=Ctrl+Alt+M\n",
    );
}

fn default_shortcuts() -> Vec<Shortcut> {
    alloc::vec![
        Shortcut {
            action: Action::Launcher,
            ctrl: true,
            alt: false,
            shift: false,
            key: ShortcutKey::Space,
        },
        Shortcut {
            action: Action::Notifications,
            ctrl: true,
            alt: true,
            shift: false,
            key: ShortcutKey::Char('m'),
        },
    ]
}

fn parse_action(name: &str) -> Option<Action> {
    match name {
        "launcher" => Some(Action::Launcher),
        "notifications" => Some(Action::Notifications),
        _ => None,
    }
}

fn parse_shortcut(action: Action, combo: &str) -> Option<Shortcut> {
    let mut shortcut = Shortcut {
        action,
        ctrl: false,
        alt: false,
        shift: false,
        key: ShortcutKey::Space,
    };
    let mut found_key = false;
    for part in combo.split('+') {
        match part.trim() {
            "Ctrl" | "Control" => shortcut.ctrl = true,
            "Alt" => shortcut.alt = true,
            "Shift" => shortcut.shift = true,
            key => {
                shortcut.key = parse_key(key)?;
                found_key = true;
            }
        }
    }
    if found_key {
        Some(shortcut)
    } else {
        None
    }
}

fn parse_key(key: &str) -> Option<ShortcutKey> {
    match key {
        "Space" => Some(ShortcutKey::Space),
        "Tab" => Some(ShortcutKey::Tab),
        "Escape" | "Esc" => Some(ShortcutKey::Escape),
        "F4" => Some(ShortcutKey::F4),
        "F5" => Some(ShortcutKey::F5),
        "Left" => Some(ShortcutKey::ArrowLeft),
        "Right" => Some(ShortcutKey::ArrowRight),
        "Up" => Some(ShortcutKey::ArrowUp),
        "Down" => Some(ShortcutKey::ArrowDown),
        single if single.len() == 1 => single
            .chars()
            .next()
            .map(|c| ShortcutKey::Char(c.to_ascii_lowercase())),
        _ => None,
    }
}

impl Shortcut {
    fn matches(self, input: KeyInput) -> bool {
        let mods = input.modifiers;
        (mods & crate::keyboard::MOD_CTRL != 0) == self.ctrl
            && (mods & crate::keyboard::MOD_ALT != 0) == self.alt
            && (mods & crate::keyboard::MOD_SHIFT != 0) == self.shift
            && key_matches(self.key, input.key)
    }

    fn summary(self) -> String {
        let mut out = String::new();
        out.push_str(match self.action {
            Action::Launcher => "launcher=",
            Action::Notifications => "notifications=",
        });
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        out.push_str(key_label(self.key));
        out
    }
}

fn key_matches(expected: ShortcutKey, key: Key) -> bool {
    match (expected, key) {
        (ShortcutKey::Char(a), Key::Character(b)) => a.eq_ignore_ascii_case(&b),
        (ShortcutKey::Space, Key::Space) => true,
        (ShortcutKey::Space, Key::Character(' ')) => true,
        (ShortcutKey::Tab, Key::Tab) => true,
        (ShortcutKey::Escape, Key::Escape) => true,
        (ShortcutKey::F4, Key::F4) => true,
        (ShortcutKey::F5, Key::F5) => true,
        (ShortcutKey::ArrowLeft, Key::ArrowLeft) => true,
        (ShortcutKey::ArrowRight, Key::ArrowRight) => true,
        (ShortcutKey::ArrowUp, Key::ArrowUp) => true,
        (ShortcutKey::ArrowDown, Key::ArrowDown) => true,
        _ => false,
    }
}

fn key_label(key: ShortcutKey) -> &'static str {
    match key {
        ShortcutKey::Char('m') => "M",
        ShortcutKey::Char(_) => "Key",
        ShortcutKey::Space => "Space",
        ShortcutKey::Tab => "Tab",
        ShortcutKey::Escape => "Escape",
        ShortcutKey::F4 => "F4",
        ShortcutKey::F5 => "F5",
        ShortcutKey::ArrowLeft => "Left",
        ShortcutKey::ArrowRight => "Right",
        ShortcutKey::ArrowUp => "Up",
        ShortcutKey::ArrowDown => "Down",
    }
}
