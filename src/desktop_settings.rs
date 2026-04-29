extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::str;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const SETTINGS_DIR: &str = "/CONFIG";
const SETTINGS_PATH: &str = "/CONFIG/DESK.CFG";
const ICONS_PATH: &str = "/CONFIG/ICONS.CFG";

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DesktopSortMode {
    Default = 0,
    Name = 1,
    Type = 2,
}

impl DesktopSortMode {
    fn from_byte(value: u8) -> Self {
        match value {
            1 => DesktopSortMode::Name,
            2 => DesktopSortMode::Type,
            _ => DesktopSortMode::Default,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DesktopSortMode::Default => "Default",
            DesktopSortMode::Name => "Name",
            DesktopSortMode::Type => "Type",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WallpaperPreset {
    Phosphor = 0,
    Aurora = 1,
    Midnight = 2,
}

impl WallpaperPreset {
    pub const ALL: [WallpaperPreset; 3] = [
        WallpaperPreset::Phosphor,
        WallpaperPreset::Aurora,
        WallpaperPreset::Midnight,
    ];

    fn from_byte(value: u8) -> Self {
        match value {
            1 => WallpaperPreset::Aurora,
            2 => WallpaperPreset::Midnight,
            _ => WallpaperPreset::Phosphor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WallpaperPreset::Phosphor => "Phosphor Blue",
            WallpaperPreset::Aurora => "Aurora Grid",
            WallpaperPreset::Midnight => "Midnight Core",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            WallpaperPreset::Phosphor => "classic coolOS bloom",
            WallpaperPreset::Aurora => "cool cyan-green sweep",
            WallpaperPreset::Midnight => "darker navy-violet shell",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DesktopSettings {
    pub show_icons: bool,
    pub compact_spacing: bool,
    pub sort_mode: DesktopSortMode,
    pub wallpaper: WallpaperPreset,
}

static SHOW_ICONS: AtomicBool = AtomicBool::new(true);
static COMPACT_SPACING: AtomicBool = AtomicBool::new(false);
static SORT_MODE: AtomicU8 = AtomicU8::new(DesktopSortMode::Default as u8);
static WALLPAPER: AtomicU8 = AtomicU8::new(WallpaperPreset::Phosphor as u8);
static LOADED: AtomicBool = AtomicBool::new(false);

pub fn load_from_disk() {
    if LOADED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(bytes) = crate::fat32::read_file(SETTINGS_PATH) else {
        let _ = save_to_disk();
        return;
    };
    let Ok(text) = str::from_utf8(&bytes) else {
        recover_corrupt(&bytes);
        return;
    };

    let mut valid = 0usize;
    let mut invalid = 0usize;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            invalid += 1;
            continue;
        };
        if apply_setting(key.trim(), value.trim()) {
            valid += 1;
        } else {
            invalid += 1;
        }
    }
    if valid == 0 && invalid > 0 {
        recover_corrupt(&bytes);
    }
}

pub fn snapshot() -> DesktopSettings {
    DesktopSettings {
        show_icons: SHOW_ICONS.load(Ordering::Relaxed),
        compact_spacing: COMPACT_SPACING.load(Ordering::Relaxed),
        sort_mode: DesktopSortMode::from_byte(SORT_MODE.load(Ordering::Relaxed)),
        wallpaper: WallpaperPreset::from_byte(WALLPAPER.load(Ordering::Relaxed)),
    }
}

pub fn set_show_icons(value: bool) {
    SHOW_ICONS.store(value, Ordering::Relaxed);
    let _ = save_to_disk();
}

pub fn set_compact_spacing(value: bool) {
    COMPACT_SPACING.store(value, Ordering::Relaxed);
    let _ = save_to_disk();
}

pub fn set_sort_mode(value: DesktopSortMode) {
    SORT_MODE.store(value as u8, Ordering::Relaxed);
    let _ = save_to_disk();
}

pub fn set_wallpaper(value: WallpaperPreset) {
    WALLPAPER.store(value as u8, Ordering::Relaxed);
    let _ = save_to_disk();
}

pub fn save_to_disk() -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir(SETTINGS_DIR);
    match crate::fat32::create_file(SETTINGS_PATH) {
        Ok(()) | Err(crate::fat32::FsError::AlreadyExists) => {}
        Err(err) => return Err(err),
    }

    let settings = snapshot();
    let mut data = String::new();
    data.push_str("show_icons=");
    data.push(if settings.show_icons { '1' } else { '0' });
    data.push('\n');
    data.push_str("compact_spacing=");
    data.push(if settings.compact_spacing { '1' } else { '0' });
    data.push('\n');
    data.push_str("sort_mode=");
    data.push(char::from(b'0' + SORT_MODE.load(Ordering::Relaxed).min(2)));
    data.push('\n');
    data.push_str("wallpaper=");
    data.push(char::from(b'0' + WALLPAPER.load(Ordering::Relaxed).min(2)));
    data.push('\n');

    crate::fat32::write_file(SETTINGS_PATH, data.as_bytes())
}

#[derive(Clone)]
struct IconPosition {
    label: String,
    x: i32,
    y: i32,
}

pub fn icon_position(label: &str) -> Option<(i32, i32)> {
    load_icon_positions()
        .into_iter()
        .find(|entry| entry.label.eq_ignore_ascii_case(label))
        .map(|entry| (entry.x, entry.y))
}

pub fn set_icon_position(label: &str, x: i32, y: i32) -> Result<(), crate::fat32::FsError> {
    let mut entries = load_icon_positions();
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.label.eq_ignore_ascii_case(label))
    {
        entry.x = x;
        entry.y = y;
    } else {
        entries.push(IconPosition {
            label: String::from(label),
            x,
            y,
        });
    }
    save_icon_positions(&entries)
}

pub fn icon_lines() -> Vec<String> {
    let entries = load_icon_positions();
    if entries.is_empty() {
        return alloc::vec![String::from("desktop icon positions: default grid")];
    }
    entries
        .iter()
        .map(|entry| {
            let mut line = String::from(&entry.label);
            line.push('=');
            push_i32(&mut line, entry.x);
            line.push(',');
            push_i32(&mut line, entry.y);
            line
        })
        .collect()
}

fn apply_setting(key: &str, value: &str) -> bool {
    match key {
        "show_icons" => {
            if let Some(value) = parse_bool(value) {
                SHOW_ICONS.store(value, Ordering::Relaxed);
                return true;
            }
        }
        "compact_spacing" => {
            if let Some(value) = parse_bool(value) {
                COMPACT_SPACING.store(value, Ordering::Relaxed);
                return true;
            }
        }
        "sort_mode" => {
            if let Some(value) = parse_byte(value) {
                SORT_MODE.store(DesktopSortMode::from_byte(value) as u8, Ordering::Relaxed);
                return true;
            }
        }
        "wallpaper" => {
            if let Some(value) = parse_byte(value) {
                WALLPAPER.store(WallpaperPreset::from_byte(value) as u8, Ordering::Relaxed);
                return true;
            }
        }
        _ => {}
    }
    false
}

fn recover_corrupt(bytes: &[u8]) {
    let _ = crate::fat32::create_dir(SETTINGS_DIR);
    let _ = crate::fat32::safe_write_file("/CONFIG/DESK.BAD", bytes);
    SHOW_ICONS.store(true, Ordering::Relaxed);
    COMPACT_SPACING.store(false, Ordering::Relaxed);
    SORT_MODE.store(DesktopSortMode::Default as u8, Ordering::Relaxed);
    WALLPAPER.store(WallpaperPreset::Phosphor as u8, Ordering::Relaxed);
    let _ = save_to_disk();
    crate::klog::log("recovered corrupt /CONFIG/DESK.CFG");
}

fn load_icon_positions() -> Vec<IconPosition> {
    let Some(bytes) = crate::fat32::read_file(ICONS_PATH) else {
        return Vec::new();
    };
    let Ok(text) = str::from_utf8(&bytes) else {
        let _ = crate::fat32::safe_write_file("/CONFIG/ICONS.BAD", &bytes);
        return Vec::new();
    };
    let mut entries = Vec::new();
    for line in text.lines() {
        let Some((label, value)) = line.split_once('=') else {
            continue;
        };
        let Some((x, y)) = value.split_once(',') else {
            continue;
        };
        let Some(x) = parse_i32(x.trim()) else {
            continue;
        };
        let Some(y) = parse_i32(y.trim()) else {
            continue;
        };
        entries.push(IconPosition {
            label: String::from(label.trim()),
            x,
            y,
        });
    }
    entries
}

fn save_icon_positions(entries: &[IconPosition]) -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir(SETTINGS_DIR);
    let mut data = String::new();
    for entry in entries {
        data.push_str(&entry.label);
        data.push('=');
        push_i32(&mut data, entry.x);
        data.push(',');
        push_i32(&mut data, entry.y);
        data.push('\n');
    }
    crate::fat32::safe_write_file(ICONS_PATH, data.as_bytes())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

fn parse_byte(value: &str) -> Option<u8> {
    if value.is_empty() {
        return None;
    }
    let mut out = 0u8;
    for b in value.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        out = out.checked_mul(10)?.checked_add(b - b'0')?;
    }
    Some(out)
}

fn parse_i32(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

fn push_i32(out: &mut String, value: i32) {
    if value < 0 {
        out.push('-');
        push_u32(out, value.unsigned_abs());
    } else {
        push_u32(out, value as u32);
    }
}

fn push_u32(out: &mut String, mut value: u32) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut len = 0usize;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        out.push(digits[idx] as char);
    }
}
