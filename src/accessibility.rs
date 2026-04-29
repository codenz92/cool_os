extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::str;
use core::sync::atomic::{AtomicBool, Ordering};

const CONFIG_DIR: &str = "/CONFIG";
const CONFIG_PATH: &str = "/CONFIG/ACCESS.CFG";

#[derive(Clone, Copy)]
pub struct AccessibilitySettings {
    pub keyboard_nav: bool,
    pub focus_rings: bool,
    pub large_text: bool,
    pub reduced_motion: bool,
}

static LOADED: AtomicBool = AtomicBool::new(false);
static KEYBOARD_NAV: AtomicBool = AtomicBool::new(true);
static FOCUS_RINGS: AtomicBool = AtomicBool::new(true);
static LARGE_TEXT: AtomicBool = AtomicBool::new(false);
static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub fn load_from_disk() {
    if LOADED.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(bytes) = crate::fat32::read_file(CONFIG_PATH) else {
        let _ = save_to_disk();
        return;
    };
    let Ok(text) = str::from_utf8(&bytes) else {
        return;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(value) = parse_bool(value.trim()) else {
            continue;
        };
        match key.trim() {
            "keyboard_nav" => KEYBOARD_NAV.store(value, Ordering::Relaxed),
            "focus_rings" => FOCUS_RINGS.store(value, Ordering::Relaxed),
            "large_text" => LARGE_TEXT.store(value, Ordering::Relaxed),
            "reduced_motion" => REDUCED_MOTION.store(value, Ordering::Relaxed),
            _ => {}
        }
    }
}

pub fn snapshot() -> AccessibilitySettings {
    ensure_loaded();
    AccessibilitySettings {
        keyboard_nav: KEYBOARD_NAV.load(Ordering::Relaxed),
        focus_rings: FOCUS_RINGS.load(Ordering::Relaxed),
        large_text: LARGE_TEXT.load(Ordering::Relaxed),
        reduced_motion: REDUCED_MOTION.load(Ordering::Relaxed),
    }
}

pub fn set(key: &str, value: bool) -> bool {
    ensure_loaded();
    match key {
        "keyboard_nav" => KEYBOARD_NAV.store(value, Ordering::Relaxed),
        "focus_rings" => FOCUS_RINGS.store(value, Ordering::Relaxed),
        "large_text" => LARGE_TEXT.store(value, Ordering::Relaxed),
        "reduced_motion" => REDUCED_MOTION.store(value, Ordering::Relaxed),
        _ => return false,
    }
    let _ = save_to_disk();
    crate::wm::request_repaint();
    true
}

pub fn lines() -> Vec<String> {
    let s = snapshot();
    alloc::vec![
        line("keyboard_nav", s.keyboard_nav),
        line("focus_rings", s.focus_rings),
        line("large_text", s.large_text),
        line("reduced_motion", s.reduced_motion),
    ]
}

fn ensure_loaded() {
    if !LOADED.load(Ordering::Acquire) {
        load_from_disk();
    }
}

fn save_to_disk() -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir(CONFIG_DIR);
    let s = AccessibilitySettings {
        keyboard_nav: KEYBOARD_NAV.load(Ordering::Relaxed),
        focus_rings: FOCUS_RINGS.load(Ordering::Relaxed),
        large_text: LARGE_TEXT.load(Ordering::Relaxed),
        reduced_motion: REDUCED_MOTION.load(Ordering::Relaxed),
    };
    let mut out = String::new();
    push_setting(&mut out, "keyboard_nav", s.keyboard_nav);
    push_setting(&mut out, "focus_rings", s.focus_rings);
    push_setting(&mut out, "large_text", s.large_text);
    push_setting(&mut out, "reduced_motion", s.reduced_motion);
    crate::fat32::safe_write_file(CONFIG_PATH, out.as_bytes())
}

fn push_setting(out: &mut String, key: &str, value: bool) {
    out.push_str(key);
    out.push('=');
    out.push(if value { '1' } else { '0' });
    out.push('\n');
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

fn line(key: &str, value: bool) -> String {
    let mut out = String::from(key);
    out.push('=');
    out.push_str(if value { "on" } else { "off" });
    out
}
