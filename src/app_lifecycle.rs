extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

const CONFIG_DIR: &str = "/CONFIG";
const STATE_PATH: &str = "/CONFIG/APPS.CFG";
const MAX_RECENT: usize = 12;
const MAX_PINNED: usize = 10;
const DEFAULT_PINNED: &[&str] = &[
    "Terminal",
    "File Manager",
    "System Monitor",
    "Display Settings",
    "Personalize",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StartMenuPrefs {
    pub width: i32,
    pub height: i32,
    pub compact: bool,
    pub show_recent: bool,
    pub show_widgets: bool,
}

const DEFAULT_START_MENU_PREFS: StartMenuPrefs = StartMenuPrefs {
    width: 620,
    height: 420,
    compact: false,
    show_recent: true,
    show_widgets: true,
};

#[derive(Clone)]
pub struct AppGeometry {
    pub app: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone)]
struct LifecycleState {
    pinned_apps: Vec<String>,
    startup_apps: Vec<String>,
    recent_apps: Vec<String>,
    recent_files: Vec<String>,
    recent_commands: Vec<String>,
    recent_searches: Vec<String>,
    geometries: Vec<AppGeometry>,
    start_menu: StartMenuPrefs,
}

static LOADED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<LifecycleState> = Mutex::new(LifecycleState {
    pinned_apps: Vec::new(),
    startup_apps: Vec::new(),
    recent_apps: Vec::new(),
    recent_files: Vec::new(),
    recent_commands: Vec::new(),
    recent_searches: Vec::new(),
    geometries: Vec::new(),
    start_menu: DEFAULT_START_MENU_PREFS,
});

pub fn init() {
    load_from_disk();
    let _ = crate::fat32::create_dir(CONFIG_DIR);
    let _ = save_to_disk();
}

pub fn load_from_disk() {
    if LOADED.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut next = LifecycleState {
        pinned_apps: DEFAULT_PINNED
            .iter()
            .map(|app| String::from(*app))
            .collect(),
        startup_apps: alloc::vec![String::from("Terminal")],
        recent_apps: Vec::new(),
        recent_files: Vec::new(),
        recent_commands: Vec::new(),
        recent_searches: Vec::new(),
        geometries: Vec::new(),
        start_menu: DEFAULT_START_MENU_PREFS,
    };
    if let Some(bytes) = crate::fat32::read_file(STATE_PATH) {
        if let Ok(text) = core::str::from_utf8(&bytes) {
            for line in text.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key.trim() {
                    "pinned" => next.pinned_apps = parse_list(value),
                    "startup" => next.startup_apps = parse_list(value),
                    "recent_app" => push_unique(&mut next.recent_apps, value.trim()),
                    "recent_file" => push_unique(&mut next.recent_files, value.trim()),
                    "recent_command" => push_unique(&mut next.recent_commands, value.trim()),
                    "recent_search" => push_unique(&mut next.recent_searches, value.trim()),
                    "menu_width" => {
                        if let Ok(width) = value.trim().parse::<i32>() {
                            next.start_menu.width = width.clamp(460, 760);
                        }
                    }
                    "menu_height" => {
                        if let Ok(height) = value.trim().parse::<i32>() {
                            next.start_menu.height = height.clamp(320, 520);
                        }
                    }
                    "menu_compact" => next.start_menu.compact = parse_bool(value),
                    "menu_recent" => next.start_menu.show_recent = parse_bool(value),
                    "menu_widgets" => next.start_menu.show_widgets = parse_bool(value),
                    "geometry" => {
                        if let Some(geometry) = parse_geometry(value) {
                            next.geometries.push(geometry);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    if next.pinned_apps.is_empty() {
        next.pinned_apps = DEFAULT_PINNED
            .iter()
            .map(|app| String::from(*app))
            .collect();
    }
    *STATE.lock() = next;
}

pub fn pinned_apps() -> Vec<String> {
    ensure_loaded();
    STATE.lock().pinned_apps.clone()
}

pub fn startup_apps() -> Vec<String> {
    ensure_loaded();
    STATE.lock().startup_apps.clone()
}

pub fn recent_files() -> Vec<String> {
    ensure_loaded();
    STATE.lock().recent_files.clone()
}

pub fn recent_apps() -> Vec<String> {
    ensure_loaded();
    STATE.lock().recent_apps.clone()
}

pub fn recent_commands() -> Vec<String> {
    ensure_loaded();
    STATE.lock().recent_commands.clone()
}

pub fn recent_searches() -> Vec<String> {
    ensure_loaded();
    STATE.lock().recent_searches.clone()
}

pub fn start_menu_prefs() -> StartMenuPrefs {
    ensure_loaded();
    STATE.lock().start_menu
}

pub fn set_start_menu_compact(compact: bool) {
    ensure_loaded();
    STATE.lock().start_menu.compact = compact;
    let _ = save_to_disk();
}

pub fn record_file(path: &str) {
    if path.is_empty() {
        return;
    }
    ensure_loaded();
    push_recent_locked(|state| &mut state.recent_files, path);
}

pub fn record_app(app: &str) {
    let app = app.trim();
    if app.is_empty() {
        return;
    }
    ensure_loaded();
    push_recent_locked(|state| &mut state.recent_apps, app);
}

pub fn record_command(command: &str) {
    let command = command.trim();
    if command.is_empty() {
        return;
    }
    ensure_loaded();
    push_recent_locked(|state| &mut state.recent_commands, command);
}

pub fn record_search(query: &str) {
    let query = query.trim();
    if query.is_empty() {
        return;
    }
    ensure_loaded();
    push_recent_locked(|state| &mut state.recent_searches, query);
}

pub fn pin_item(item: &str) {
    let item = item.trim();
    if item.is_empty() {
        return;
    }
    ensure_loaded();
    {
        let mut state = STATE.lock();
        push_unique(&mut state.pinned_apps, item);
        if state.pinned_apps.len() > MAX_PINNED {
            state.pinned_apps.truncate(MAX_PINNED);
        }
    }
    let _ = save_to_disk();
}

pub fn unpin_item(item: &str) -> bool {
    ensure_loaded();
    let removed = {
        let mut state = STATE.lock();
        let before = state.pinned_apps.len();
        state
            .pinned_apps
            .retain(|existing| !existing.eq_ignore_ascii_case(item));
        before != state.pinned_apps.len()
    };
    if removed {
        let _ = save_to_disk();
    }
    removed
}

pub fn is_pinned(item: &str) -> bool {
    ensure_loaded();
    STATE
        .lock()
        .pinned_apps
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(item))
}

pub fn remember_geometry(app: &str, x: i32, y: i32, w: i32, h: i32) {
    ensure_loaded();
    let app_key = geometry_key(app);
    {
        let mut state = STATE.lock();
        if let Some(existing) = state
            .geometries
            .iter_mut()
            .find(|geometry| geometry.app.eq_ignore_ascii_case(&app_key))
        {
            existing.x = x;
            existing.y = y;
            existing.w = w;
            existing.h = h;
        } else {
            state.geometries.push(AppGeometry {
                app: app_key,
                x,
                y,
                w,
                h,
            });
        }
    }
    let _ = save_to_disk();
}

pub fn geometry_for(app: &str) -> Option<AppGeometry> {
    ensure_loaded();
    let app_key = geometry_key(app);
    STATE
        .lock()
        .geometries
        .iter()
        .find(|geometry| {
            geometry.app.eq_ignore_ascii_case(&app_key) || geometry.app.eq_ignore_ascii_case(app)
        })
        .cloned()
}

pub fn set_pinned(apps: Vec<String>) {
    ensure_loaded();
    STATE.lock().pinned_apps = apps;
    let _ = save_to_disk();
}

pub fn set_startup(apps: Vec<String>) {
    ensure_loaded();
    STATE.lock().startup_apps = apps;
    let _ = save_to_disk();
}

pub fn lines() -> Vec<String> {
    ensure_loaded();
    let state = STATE.lock();
    let mut lines = Vec::new();
    lines.push(join_label("pinned", &state.pinned_apps));
    lines.push(join_label("startup", &state.startup_apps));
    lines.push(join_label("recent apps", &state.recent_apps));
    lines.push(join_label("recent files", &state.recent_files));
    lines.push(join_label("recent commands", &state.recent_commands));
    lines.push(join_label("recent searches", &state.recent_searches));
    lines.push(format!(
        "start menu: {}x{} compact={} recent={} widgets={}",
        state.start_menu.width,
        state.start_menu.height,
        state.start_menu.compact,
        state.start_menu.show_recent,
        state.start_menu.show_widgets
    ));
    for geometry in state.geometries.iter() {
        lines.push(alloc::format!(
            "geometry {} {} {} {} {}",
            geometry.app,
            geometry.x,
            geometry.y,
            geometry.w,
            geometry.h
        ));
    }
    lines
}

fn ensure_loaded() {
    if !LOADED.load(Ordering::Acquire) {
        load_from_disk();
    }
}

fn push_recent_locked<F>(selector: F, value: &str)
where
    F: FnOnce(&mut LifecycleState) -> &mut Vec<String>,
{
    {
        let mut state = STATE.lock();
        let list = selector(&mut state);
        push_unique(list, value);
        if list.len() > MAX_RECENT {
            list.truncate(MAX_RECENT);
        }
    }
    let _ = save_to_disk();
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    if let Some(pos) = list
        .iter()
        .position(|item| item.eq_ignore_ascii_case(value))
    {
        list.remove(pos);
    }
    list.insert(0, String::from(value));
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_geometry(value: &str) -> Option<AppGeometry> {
    let mut parts = value.split('|');
    Some(AppGeometry {
        app: String::from(parts.next()?.trim()),
        x: parts.next()?.trim().parse().ok()?,
        y: parts.next()?.trim().parse().ok()?,
        w: parts.next()?.trim().parse().ok()?,
        h: parts.next()?.trim().parse().ok()?,
    })
}

fn save_to_disk() -> Result<(), crate::fat32::FsError> {
    let _ = crate::fat32::create_dir(CONFIG_DIR);
    let state = STATE.lock();
    let mut out = String::new();
    out.push_str("pinned=");
    push_joined(&mut out, &state.pinned_apps);
    out.push('\n');
    out.push_str("startup=");
    push_joined(&mut out, &state.startup_apps);
    out.push('\n');
    out.push_str("menu_width=");
    push_i32(&mut out, state.start_menu.width);
    out.push('\n');
    out.push_str("menu_height=");
    push_i32(&mut out, state.start_menu.height);
    out.push('\n');
    out.push_str("menu_compact=");
    out.push_str(if state.start_menu.compact {
        "true"
    } else {
        "false"
    });
    out.push('\n');
    out.push_str("menu_recent=");
    out.push_str(if state.start_menu.show_recent {
        "true"
    } else {
        "false"
    });
    out.push('\n');
    out.push_str("menu_widgets=");
    out.push_str(if state.start_menu.show_widgets {
        "true"
    } else {
        "false"
    });
    out.push('\n');
    for app in state.recent_apps.iter() {
        out.push_str("recent_app=");
        out.push_str(app);
        out.push('\n');
    }
    for file in state.recent_files.iter() {
        out.push_str("recent_file=");
        out.push_str(file);
        out.push('\n');
    }
    for command in state.recent_commands.iter() {
        out.push_str("recent_command=");
        out.push_str(command);
        out.push('\n');
    }
    for search in state.recent_searches.iter() {
        out.push_str("recent_search=");
        out.push_str(search);
        out.push('\n');
    }
    for geometry in state.geometries.iter() {
        out.push_str("geometry=");
        out.push_str(&geometry.app);
        out.push('|');
        push_i32(&mut out, geometry.x);
        out.push('|');
        push_i32(&mut out, geometry.y);
        out.push('|');
        push_i32(&mut out, geometry.w);
        out.push('|');
        push_i32(&mut out, geometry.h);
        out.push('\n');
    }
    crate::fat32::safe_write_file(STATE_PATH, out.as_bytes())
}

fn join_label(label: &str, values: &[String]) -> String {
    let mut out = String::from(label);
    out.push_str(": ");
    if values.is_empty() {
        out.push_str("(none)");
    } else {
        push_joined(&mut out, values);
    }
    out
}

fn push_joined(out: &mut String, values: &[String]) {
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(value);
    }
}

fn push_i32(out: &mut String, value: i32) {
    if value < 0 {
        out.push('-');
        push_u64(out, value.unsigned_abs() as u64);
    } else {
        push_u64(out, value as u64);
    }
}

fn geometry_key(app: &str) -> String {
    let mut key = String::from(app);
    key.push('@');
    push_u64(&mut key, crate::framebuffer::width() as u64);
    key.push('x');
    push_u64(&mut key, crate::framebuffer::height() as u64);
    key
}

fn push_u64(out: &mut String, mut value: u64) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
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
