/// Window compositor — desktop, windows, taskbar, cursor, context menu.
/// All rendering targets a `Vec<u32>` shadow buffer; one blit per frame.
///
/// Visual theme: Retro-Futuristic CRT Phosphor Blue
extern crate alloc;
use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::apps::{
    ColorPickerApp, DisplaySettingsApp, FileManagerApp, FileManagerOpenRequest, PersonalizeApp,
    SysMonApp, TerminalApp, TextViewerApp,
};
use crate::desktop_settings::{self, DesktopSortMode, WallpaperPreset};
use crate::framebuffer::{BLACK, WHITE};
use crate::keyboard::{Key, KeyInput};
use crate::wm::window::{Window, TITLE_H};

// ── Layout constants ──────────────────────────────────────────────────────────

const TASKBAR_H: i32 = 40; // Win11: 40px tall taskbar
const START_BTN_W: i32 = 40; // square start icon button + left gutter
const TASKBAR_CLOCK_W: i32 = 176; // tray + time readout + brand
const TASKBAR_TRAY_W: i32 = 70;
const SHOW_DESKTOP_W: i32 = 18;
const BUTTON_W: i32 = 160;
const WIN_BTN_W: i32 = crate::wm::window::WIN_BTN_W;
const SCROLLBAR_W: i32 = crate::wm::window::SCROLLBAR_W;
const RESIZE_HANDLE: i32 = crate::wm::window::RESIZE_HANDLE;
const EVENT_PACKET_SIZE: usize = 8;
const EVENT_KIND_KEY_CHAR: u8 = 1;
const EVENT_KIND_MOUSE_DOWN: u8 = 2;
const SNAP_EDGE_PX: i32 = 18;
const TASK_SWITCHER_MS: u64 = 1200;
const SESSION_PATH: &str = "/CONFIG/SESSION.CFG";
const SESSION_SAVE_MS: u64 = 1200;
const MAX_SESSION_WINDOWS: usize = 8;
const WORKSPACE_COUNT: usize = 4;
const TASKBAR_MENU_W: i32 = 152;
const TASKBAR_MENU_ROW_H: i32 = 24;
const TASKBAR_MENU_H: i32 = TASKBAR_MENU_ROW_H * 3 + 10;
const START_MENU_SECTION_H: i32 = 11;

static COMPOSITOR_FPS: AtomicU64 = AtomicU64::new(0);
static COMPOSITOR_FRAME_TICKS_LAST: AtomicU64 = AtomicU64::new(0);
static COMPOSITOR_FRAME_TICKS_PEAK: AtomicU64 = AtomicU64::new(0);
static COMPOSITOR_DAMAGE_ROWS: AtomicU64 = AtomicU64::new(0);
static COMPOSITOR_DAMAGE_PIXELS: AtomicU64 = AtomicU64::new(0);
static COMPOSITOR_FRAMES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct CompositorStats {
    pub fps: u64,
    pub frame_ticks_last: u64,
    pub frame_ticks_peak: u64,
    pub damage_rows: u64,
    pub damage_pixels: u64,
    pub frames: u64,
}

pub fn compositor_stats() -> CompositorStats {
    CompositorStats {
        fps: COMPOSITOR_FPS.load(Ordering::Relaxed),
        frame_ticks_last: COMPOSITOR_FRAME_TICKS_LAST.load(Ordering::Relaxed),
        frame_ticks_peak: COMPOSITOR_FRAME_TICKS_PEAK.load(Ordering::Relaxed),
        damage_rows: COMPOSITOR_DAMAGE_ROWS.load(Ordering::Relaxed),
        damage_pixels: COMPOSITOR_DAMAGE_PIXELS.load(Ordering::Relaxed),
        frames: COMPOSITOR_FRAMES.load(Ordering::Relaxed),
    }
}

pub fn compositor_lines() -> Vec<String> {
    let stats = compositor_stats();
    alloc::vec![
        format!("fps={} frames={}", stats.fps, stats.frames),
        format!(
            "frame_ticks last={} peak={}",
            stats.frame_ticks_last, stats.frame_ticks_peak
        ),
        format!(
            "damage rows={} pixels={}",
            stats.damage_rows, stats.damage_pixels
        ),
    ]
}

// ── Colors — Retro-Futuristic CRT Phosphor Blue ───────────────────────────────

// Taskbar / shell
// Accent (CRT phosphor blue)
const ACCENT: u32 = 0x00_00_BB_FF; // #00BBFF  bright phosphor blue (boosted)
const ACCENT_HOV: u32 = 0x00_44_CC_FF; // #44CCFF  lit hover (richer)

// Window chrome (CRT dark mode)
const WIN_BAR_F: u32 = 0x00_00_12_2E; // #00122E  focused title bar — richer navy
const WIN_BAR_U: u32 = 0x00_00_07_14; // #000714  unfocused — near-black
const WIN_CONTENT: u32 = 0x00_00_09_1C; // #00091C  window body
const WIN_BDR_F: u32 = 0x00_00_BB_FF; // #00BBFF  focused border — full phosphor glow
const WIN_BDR_U: u32 = 0x00_00_33_66; // #003366  unfocused — dim blue

// Window caption buttons
const CAP_NORMAL: u32 = 0x00_00_12_2E; // same as title bar
const CAP_HOV: u32 = 0x00_00_26_4E; // slightly lighter navy
const CLOSE_REST: u32 = 0x00_00_12_2E; // close resting
const CLOSE_HOV: u32 = 0x00_CC_11_11; // #CC1111  red-CRT close (deeper red)

/// Sentinel stored in window content buffers to mean "render the window background here".
/// Apps that genuinely need to paint pure black should write `0x00_00_00_01` instead
/// (visually identical, but not intercepted by the compositor blit).
const WIN_TRANSPARENT: u32 = 0x00_00_00_00;

// Desktop wallpaper — deep space phosphor
const DESK_TL: u32 = 0x00_00_02_08; // top-left  pitch black with blue ghost
const DESK_TR: u32 = 0x00_00_03_0C; // top-right
const DESK_BL: u32 = 0x00_00_01_06; // bottom-left
const DESK_BR: u32 = 0x00_00_02_0A; // bottom-right
                                    // CRT phosphor glow toward screen centre
const BLOOM_1: u32 = 0x00_00_55_CC; // primary phosphor blue bloom (richer)

// Desktop icons — CRT phosphor colour set
const ICON_TERM_BG: u32 = 0x00_00_16_08; // terminal — dark green-black
const ICON_TERM_ACC: u32 = 0x00_00_FF_88; // #00FF88  phosphor green
const ICON_MON_BG: u32 = 0x00_00_0E_1E; // monitor — deep blue-black
const ICON_MON_ACC: u32 = 0x00_00_EE_FF; // #00EEFF  cyan phosphor
const ICON_TXT_BG: u32 = 0x00_00_0A_22; // text — navy
const ICON_TXT_ACC: u32 = 0x00_00_99_FF; // #0099FF  blue phosphor
const ICON_COL_BG: u32 = 0x00_10_00_1E; // colour — dark purple-black
const ICON_COL_ACC: u32 = 0x00_AA_44_FF; // #AA44FF  violet phosphor

// ── Cursor ────────────────────────────────────────────────────────────────────

const CURSOR_H: usize = 12;
// Standard Windows arrow cursor (taller, more precise)
const CURSOR_SHAPE: [u16; CURSOR_H] = [
    0b1000000000000000,
    0b1100000000000000,
    0b1110000000000000,
    0b1111000000000000,
    0b1111100000000000,
    0b1111110000000000,
    0b1111111000000000,
    0b1111100000000000,
    0b1101100000000000,
    0b1000110000000000,
    0b0000110000000000,
    0b0000011000000000,
];

// Black outline mask (1-pixel rim)
const CURSOR_OUTLINE: [u16; CURSOR_H] = [
    0b1100000000000000,
    0b1110000000000000,
    0b1111000000000000,
    0b1111100000000000,
    0b1111110000000000,
    0b1111111000000000,
    0b1111111100000000,
    0b1111111000000000,
    0b1111110000000000,
    0b1100111000000000,
    0b0000111000000000,
    0b0000111100000000,
];

const CURSOR_RESIZE_SHAPE: [u16; CURSOR_H] = [
    0b0000000000000000,
    0b0000000000110000,
    0b0000000001111000,
    0b0000000011111100,
    0b0000000110110100,
    0b0000001100110000,
    0b0000011001100000,
    0b0000110011000000,
    0b0001101101100000,
    0b0011111101111000,
    0b0001111000110000,
    0b0000110000000000,
];

const CURSOR_RESIZE_OUTLINE: [u16; CURSOR_H] = [
    0b0000000000110000,
    0b0000000001111000,
    0b0000000011111100,
    0b0000000111111110,
    0b0000001111111110,
    0b0000011111111100,
    0b0000111111111000,
    0b0001111111110000,
    0b0011111111111000,
    0b0111111111111100,
    0b0011111111111000,
    0b0001111000110000,
];

// ── Context menu ──────────────────────────────────────────────────────────────

const CTX_W: i32 = 212;
const CTX_SUB_W: i32 = 196;
const CTX_ITEM_H: i32 = 26;
const CTX_SEP_H: i32 = 8;
const CTX_HEADER_H: i32 = 0;
const CTX_PAD: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopContextCommand {
    ToggleDesktopIcons,
    ToggleCompactSpacing,
    SortByName,
    SortByType,
    Refresh,
    CreateFolder,
    CreateTextDocument,
    DisplaySettings,
    Personalize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopContextSubmenu {
    View,
    SortBy,
    New,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContextEntryKind {
    Action(DesktopContextCommand),
    Submenu(DesktopContextSubmenu),
    Separator,
}

#[derive(Clone, Copy)]
struct ContextEntryDef {
    label: &'static str,
    kind: ContextEntryKind,
    enabled: bool,
}

const DESKTOP_CONTEXT_MENU: &[ContextEntryDef] = &[
    ContextEntryDef {
        label: "View",
        kind: ContextEntryKind::Submenu(DesktopContextSubmenu::View),
        enabled: true,
    },
    ContextEntryDef {
        label: "Sort by",
        kind: ContextEntryKind::Submenu(DesktopContextSubmenu::SortBy),
        enabled: true,
    },
    ContextEntryDef {
        label: "Refresh",
        kind: ContextEntryKind::Action(DesktopContextCommand::Refresh),
        enabled: true,
    },
    ContextEntryDef {
        label: "",
        kind: ContextEntryKind::Separator,
        enabled: false,
    },
    ContextEntryDef {
        label: "Paste",
        kind: ContextEntryKind::Action(DesktopContextCommand::Refresh),
        enabled: false,
    },
    ContextEntryDef {
        label: "Paste shortcut",
        kind: ContextEntryKind::Action(DesktopContextCommand::Refresh),
        enabled: false,
    },
    ContextEntryDef {
        label: "",
        kind: ContextEntryKind::Separator,
        enabled: false,
    },
    ContextEntryDef {
        label: "New",
        kind: ContextEntryKind::Submenu(DesktopContextSubmenu::New),
        enabled: true,
    },
    ContextEntryDef {
        label: "",
        kind: ContextEntryKind::Separator,
        enabled: false,
    },
    ContextEntryDef {
        label: "Display settings",
        kind: ContextEntryKind::Action(DesktopContextCommand::DisplaySettings),
        enabled: true,
    },
    ContextEntryDef {
        label: "Personalize",
        kind: ContextEntryKind::Action(DesktopContextCommand::Personalize),
        enabled: true,
    },
];

const CTX_VIEW_MENU: &[ContextEntryDef] = &[
    ContextEntryDef {
        label: "Show desktop icons",
        kind: ContextEntryKind::Action(DesktopContextCommand::ToggleDesktopIcons),
        enabled: true,
    },
    ContextEntryDef {
        label: "Compact icon spacing",
        kind: ContextEntryKind::Action(DesktopContextCommand::ToggleCompactSpacing),
        enabled: true,
    },
];

const CTX_SORT_MENU: &[ContextEntryDef] = &[
    ContextEntryDef {
        label: "Name",
        kind: ContextEntryKind::Action(DesktopContextCommand::SortByName),
        enabled: true,
    },
    ContextEntryDef {
        label: "Type",
        kind: ContextEntryKind::Action(DesktopContextCommand::SortByType),
        enabled: true,
    },
];

const CTX_NEW_MENU: &[ContextEntryDef] = &[
    ContextEntryDef {
        label: "Folder",
        kind: ContextEntryKind::Action(DesktopContextCommand::CreateFolder),
        enabled: true,
    },
    ContextEntryDef {
        label: "Text Document",
        kind: ContextEntryKind::Action(DesktopContextCommand::CreateTextDocument),
        enabled: true,
    },
];

struct ContextMenu {
    x: i32,
    y: i32,
    submenu: Option<DesktopContextSubmenu>,
}

// ── Desktop icons ──────────────────────────────────────────────────────────────

const ICON_SIZE: i32 = 52;
const ICON_LABEL_H: i32 = 14;

#[derive(Clone, Copy)]
struct DesktopIconSpec {
    label: &'static str,
    app: &'static str,
    type_rank: u8,
}

const DESKTOP_ICON_SPECS: [DesktopIconSpec; 5] = [
    DesktopIconSpec {
        label: "Terminal",
        app: "Terminal",
        type_rank: 2,
    },
    DesktopIconSpec {
        label: "Monitor",
        app: "System Mon",
        type_rank: 1,
    },
    DesktopIconSpec {
        label: "Files",
        app: "File Manager",
        type_rank: 0,
    },
    DesktopIconSpec {
        label: "Viewer",
        app: "Text Viewer",
        type_rank: 3,
    },
    DesktopIconSpec {
        label: "Colors",
        app: "Color Pick",
        type_rank: 4,
    },
];

struct DesktopIcon {
    x: i32,
    y: i32,
    label: &'static str,
    app: &'static str,
}

impl DesktopIcon {
    fn hit(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + ICON_SIZE
            && py >= self.y
            && py < self.y + ICON_SIZE + ICON_LABEL_H
    }
}

fn canonical_app_title(name: &str) -> &str {
    match name {
        "Terminal" => "Terminal",
        "System Mon" | "System Monitor" => "System Monitor",
        "Diag" | "Diagnostics" => "Diagnostics",
        "Text View" | "Text Viewer" => "Text Viewer",
        "Color Pick" | "Color Picker" => "Color Picker",
        "Display Settings" => "Display Settings",
        "Personalize" => "Personalize",
        "Crash Viewer" => "Crash Viewer",
        "Log Viewer" => "Log Viewer",
        "Boot Profiler" => "Boot Profiler",
        "Welcome" => "Welcome",
        "File Mgr" | "File Manager" => "File Manager",
        _ => name,
    }
}

fn window_accent(title: &str) -> u32 {
    match canonical_app_title(title) {
        "Terminal" => ICON_TERM_ACC,
        "System Monitor" => ICON_MON_ACC,
        "Diagnostics" => 0x00_55_FF_CC,
        "Text Viewer" => ICON_TXT_ACC,
        "Color Picker" => ICON_COL_ACC,
        "Display Settings" => 0x00_66_CC_FF,
        "Personalize" => 0x00_CC_66_FF,
        "Crash Viewer" => 0x00_FF_66_66,
        "Log Viewer" => 0x00_55_FF_BB,
        "Boot Profiler" => 0x00_FF_DD_55,
        "Welcome" => 0x00_88_CC_FF,
        "File Manager" => 0x00_55_DD_FF,
        _ => ACCENT,
    }
}

fn window_glyph(title: &str) -> &'static str {
    match canonical_app_title(title) {
        "Terminal" => "T>",
        "System Monitor" => "M#",
        "Diagnostics" => "D!",
        "Text Viewer" => "Tx",
        "Color Picker" => "CP",
        "Display Settings" => "DS",
        "Personalize" => "P*",
        "Crash Viewer" => "CV",
        "Log Viewer" => "LV",
        "Boot Profiler" => "BP",
        "Welcome" => "W?",
        "File Manager" => "FM",
        _ => "[]",
    }
}

fn month_abbrev(month: u8) -> &'static str {
    match month {
        1 => "JAN",
        2 => "FEB",
        3 => "MAR",
        4 => "APR",
        5 => "MAY",
        6 => "JUN",
        7 => "JUL",
        8 => "AUG",
        9 => "SEP",
        10 => "OCT",
        11 => "NOV",
        12 => "DEC",
        _ => "---",
    }
}

fn push_two_digits(out: &mut String, value: u8) {
    out.push((b'0' + (value / 10)) as char);
    out.push((b'0' + (value % 10)) as char);
}

fn push_u16(out: &mut String, value: u16) {
    for &div in &[1000u16, 100, 10, 1] {
        out.push((b'0' + ((value / div) % 10) as u8) as char);
    }
}

fn push_usize_bytes(out: &mut Vec<u8>, mut value: usize) {
    if value == 0 {
        out.push(b'0');
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
        out.push(digits[idx]);
    }
}

fn merge_spans(a: (usize, usize), b: (usize, usize)) -> (usize, usize) {
    match (a.0 < a.1, b.0 < b.1) {
        (false, false) => (0, 0),
        (true, false) => a,
        (false, true) => b,
        (true, true) => (a.0.min(b.0), a.1.max(b.1)),
    }
}

fn should_show_welcome() -> bool {
    crate::config_store::read("/CONFIG/WELCOME.SEEN").is_none()
}

fn mark_welcome_seen() {
    let _ = crate::config_store::safe_write("/CONFIG/WELCOME.SEEN", b"1\n");
}

fn start_menu_banner_clock(uptime_ticks: u64) -> (String, String) {
    if let Some(datetime) = crate::rtc::read_datetime() {
        let mut time = String::with_capacity(5);
        push_two_digits(&mut time, datetime.hour);
        time.push(':');
        push_two_digits(&mut time, datetime.minute);

        let mut date = String::with_capacity(11);
        push_two_digits(&mut date, datetime.day);
        date.push(' ');
        date.push_str(month_abbrev(datetime.month));
        date.push(' ');
        push_u16(&mut date, datetime.year);
        return (time, date);
    }

    let secs = uptime_ticks / crate::interrupts::TIMER_HZ as u64;
    let h = ((secs / 3600) % 24) as u8;
    let m = ((secs / 60) % 60) as u8;

    let mut time = String::with_capacity(5);
    push_two_digits(&mut time, h);
    time.push(':');
    push_two_digits(&mut time, m);

    (time, String::from("RTC offline"))
}

fn draw_snowflake_logo(
    s: &mut [u32],
    sw: usize,
    x: i32,
    y: i32,
    scale: i32,
    primary: u32,
    secondary: u32,
) {
    for rect in crate::branding::SNOWFLAKE_LOGO_RECTS.iter() {
        let color = if rect.highlight { secondary } else { primary };
        s_fill(
            s,
            sw,
            x + rect.x * scale,
            y + rect.y * scale,
            rect.w * scale,
            rect.h * scale,
            color,
        );
    }
}

// ── Drag state ────────────────────────────────────────────────────────────────

struct DragState {
    window: usize,
    off_x: i32,
    off_y: i32,
}

struct ResizeState {
    window: usize,
    start_w: i32,
    start_h: i32,
    start_mx: i32,
    start_my: i32,
}

struct ScrollDragState {
    window: usize,
    start_offset: i32,
    start_my: i32,
    content_h: i32,
    view_h: i32,
    track_h: i32,
}

struct FileDragState {
    source_window: usize,
    paths: Vec<String>,
    cut: bool,
}

struct DesktopIconDragState {
    icon: usize,
    start_mx: i32,
    start_my: i32,
    start_x: i32,
    start_y: i32,
    cur_x: i32,
    cur_y: i32,
    moved: bool,
}

#[derive(Clone, Copy)]
enum SnapTarget {
    Left,
    Right,
    Bottom,
    Maximize,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

struct LauncherState {
    query: String,
    selected: usize,
}

#[derive(Clone)]
enum LauncherMatchKind {
    App(String),
    Path(String),
    Command(String),
    Inline(String),
}

#[derive(Clone, Copy)]
enum LauncherAction {
    Open,
    RunAsAdmin,
    OpenLocation,
    Pin,
    Uninstall,
    Copy,
}

#[derive(Clone)]
struct LauncherMatch {
    label: String,
    detail: String,
    kind: LauncherMatchKind,
    score: usize,
}

#[derive(Clone)]
struct StartMenuEntry {
    section: &'static str,
    label: String,
    detail: String,
    kind: LauncherMatchKind,
}

struct TaskbarMenu {
    window: usize,
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellDialogKind {
    Error,
    Crash,
}

#[derive(Clone)]
struct ShellDialog {
    title: String,
    body: String,
    kind: ShellDialogKind,
    restart_target: Option<String>,
}

// ── AppWindow ─────────────────────────────────────────────────────────────────

pub enum AppWindow {
    Terminal(TerminalApp),
    SysMon(SysMonApp),
    TextViewer(TextViewerApp),
    ColorPicker(ColorPickerApp),
    DisplaySettings(DisplaySettingsApp),
    Personalize(PersonalizeApp),
    FileManager(FileManagerApp),
}

impl AppWindow {
    pub fn window(&self) -> &Window {
        match self {
            AppWindow::Terminal(t) => &t.window,
            AppWindow::SysMon(s) => &s.window,
            AppWindow::TextViewer(v) => &v.window,
            AppWindow::ColorPicker(c) => &c.window,
            AppWindow::DisplaySettings(d) => &d.window,
            AppWindow::Personalize(p) => &p.window,
            AppWindow::FileManager(f) => &f.window,
        }
    }
    pub fn window_mut(&mut self) -> &mut Window {
        match self {
            AppWindow::Terminal(t) => &mut t.window,
            AppWindow::SysMon(s) => &mut s.window,
            AppWindow::TextViewer(v) => &mut v.window,
            AppWindow::ColorPicker(c) => &mut c.window,
            AppWindow::DisplaySettings(d) => &mut d.window,
            AppWindow::Personalize(p) => &mut p.window,
            AppWindow::FileManager(f) => &mut f.window,
        }
    }
    pub fn handle_key(&mut self, c: char) {
        match self {
            AppWindow::Terminal(t) => t.handle_key(c),
            AppWindow::TextViewer(v) => v.handle_key(c),
            AppWindow::FileManager(f) => f.handle_key(c),
            _ => {}
        }
        self.window_mut().mark_dirty_all();
    }
    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        match self {
            AppWindow::ColorPicker(cp) => cp.handle_click(lx, ly),
            AppWindow::DisplaySettings(ds) => ds.handle_click(lx, ly),
            AppWindow::FileManager(fm) => fm.handle_click(lx, ly),
            AppWindow::Personalize(p) => p.handle_click(lx, ly),
            _ => {}
        }
        self.window_mut().mark_dirty_all();
    }
    pub fn handle_secondary_click(&mut self, lx: i32, ly: i32) {
        if let AppWindow::FileManager(fm) = self {
            fm.handle_secondary_click(lx, ly);
        }
        self.window_mut().mark_dirty_all();
    }
    pub fn handle_dbl_click(&mut self, lx: i32, ly: i32) {
        if let AppWindow::FileManager(fm) = self {
            fm.handle_dbl_click(lx, ly);
        }
        self.window_mut().mark_dirty_all();
    }
    pub fn begin_file_drag(&mut self, lx: i32, ly: i32) -> Option<Vec<String>> {
        match self {
            AppWindow::FileManager(fm) => fm.drag_paths_at(lx, ly),
            _ => None,
        }
    }
    pub fn drop_file_paths(&mut self, paths: Vec<String>, cut: bool) -> bool {
        match self {
            AppWindow::FileManager(fm) => fm.drop_paths(paths, cut),
            _ => false,
        }
    }
    pub fn take_open_request(&mut self) -> Option<FileManagerOpenRequest> {
        match self {
            AppWindow::FileManager(fm) => fm.take_open_request(),
            _ => None,
        }
    }
    pub fn handle_scroll(&mut self, delta: i32) {
        match self {
            AppWindow::TextViewer(v) => v.handle_scroll(delta),
            AppWindow::FileManager(f) => f.handle_scroll(delta),
            _ => {}
        }
        self.window_mut().mark_dirty_all();
    }
    pub fn update(&mut self) {
        match self {
            AppWindow::Terminal(t) => t.update(),
            AppWindow::SysMon(s) => s.update(),
            AppWindow::TextViewer(v) => v.update(),
            AppWindow::ColorPicker(c) => c.update(),
            AppWindow::DisplaySettings(d) => d.update(),
            AppWindow::FileManager(f) => f.update(),
            AppWindow::Personalize(p) => p.update(),
        }
    }
    pub fn is_minimized(&self) -> bool {
        self.window().minimized
    }
}

// ── Window manager ────────────────────────────────────────────────────────────

pub struct WindowManager {
    pub windows: Vec<AppWindow>,
    window_workspaces: Vec<usize>,
    z_order: Vec<usize>,
    focused: Option<usize>,
    current_workspace: usize,
    key_sink_fd: Option<usize>,
    key_sink_window: Option<usize>,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    scroll_drag: Option<ScrollDragState>,
    file_drag: Option<FileDragState>,
    prev_left: bool,
    prev_right: bool,
    context_menu: Option<ContextMenu>,
    desktop_show_icons: bool,
    desktop_compact_spacing: bool,
    desktop_sort: DesktopSortMode,
    icon_selected: Option<usize>,
    desktop_multi_selected: Vec<usize>,
    pressed_icon: Option<usize>,
    desktop_icon_drag: Option<DesktopIconDragState>,
    desktop_select_drag: Option<(i32, i32)>,
    start_menu_open: bool,
    start_menu_pinned: Vec<String>,
    start_menu_entries: Vec<StartMenuEntry>,
    start_menu_widget_line: String,
    notification_center_open: bool,
    launcher: Option<LauncherState>,
    taskbar_menu: Option<TaskbarMenu>,
    dialog: Option<ShellDialog>,
    session_ready: bool,
    session_dirty: bool,
    last_session_save_tick: u64,
    last_click_tick: u64,
    last_click_window: Option<usize>,
    last_click_x: i32,
    last_click_y: i32,
    task_switcher_until_tick: u64,
    task_switcher_query: String,
    fps_window_start_tick: u64,
    fps_window_frames: u64,
    frame_ticks_peak: u64,
    /// Shadow buffer — screen_width × screen_height u32 pixels.
    shadow: Vec<u32>,
    prev_shadow: Vec<u32>,
    damage_spans: Vec<(usize, usize)>,
    reported_damage_spans: Vec<(usize, usize)>,
    damage_rows_last: usize,
    damage_pixels_last: usize,
    damage_frames: u64,
    full_damage_next: bool,
    shadow_width: usize,
    shadow_height: usize,
    blit_scratch: Vec<u8>,
    /// Pre-baked wallpaper pixels — computed once in new(), blitted each frame.
    wallpaper: Vec<u32>,
    wallpaper_preset: WallpaperPreset,
}

impl WindowManager {
    pub fn new() -> Self {
        desktop_settings::load_from_disk();
        let settings = desktop_settings::snapshot();
        let w = crate::framebuffer::width();
        let h = crate::framebuffer::height();
        let taskbar_y = h - TASKBAR_H as usize;
        let wallpaper = build_wallpaper(w, taskbar_y, settings.wallpaper, true);
        crate::boot_splash::show(
            "allocating render buffer",
            21,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
        let shadow = alloc::vec![0u32; w * h];
        crate::boot_splash::show(
            "finalizing shell",
            22,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
        let prev_shadow = alloc::vec![0u32; w * h];
        let damage_spans = alloc::vec![(0usize, w); h];
        let reported_damage_spans = alloc::vec![(0usize, 0usize); h];

        let mut wm = WindowManager {
            windows: Vec::new(),
            window_workspaces: Vec::new(),
            z_order: Vec::new(),
            focused: None,
            current_workspace: 0,
            key_sink_fd: None,
            key_sink_window: None,
            drag: None,
            resize: None,
            scroll_drag: None,
            file_drag: None,
            prev_left: false,
            prev_right: false,
            context_menu: None,
            desktop_show_icons: settings.show_icons,
            desktop_compact_spacing: settings.compact_spacing,
            desktop_sort: settings.sort_mode,
            icon_selected: None,
            desktop_multi_selected: Vec::new(),
            pressed_icon: None,
            desktop_icon_drag: None,
            desktop_select_drag: None,
            start_menu_open: false,
            start_menu_pinned: Vec::new(),
            start_menu_entries: Vec::new(),
            start_menu_widget_line: String::new(),
            notification_center_open: false,
            launcher: None,
            taskbar_menu: None,
            dialog: None,
            session_ready: false,
            session_dirty: false,
            last_session_save_tick: 0,
            last_click_tick: 0,
            last_click_window: None,
            last_click_x: 0,
            last_click_y: 0,
            task_switcher_until_tick: 0,
            task_switcher_query: String::new(),
            fps_window_start_tick: 0,
            fps_window_frames: 0,
            frame_ticks_peak: 0,
            shadow,
            prev_shadow,
            damage_spans,
            reported_damage_spans,
            damage_rows_last: 0,
            damage_pixels_last: 0,
            damage_frames: 0,
            full_damage_next: true,
            shadow_width: w,
            shadow_height: h,
            blit_scratch: alloc::vec![0u8; w * 3],
            wallpaper,
            wallpaper_preset: settings.wallpaper,
        };
        wm.restore_session();
        if wm.windows.is_empty() {
            wm.launch_startup_apps();
        }
        if should_show_welcome() {
            wm.launch_app("Welcome", 72, 72);
            mark_welcome_seen();
        }
        if wm.windows.is_empty() {
            wm.add_window(AppWindow::Terminal(TerminalApp::new(24, 24)));
        }
        wm.session_ready = true;
        wm
    }

    pub fn add_window(&mut self, w: AppWindow) {
        let idx = self.windows.len();
        self.windows.push(w);
        self.window_workspaces.push(self.current_workspace);
        self.z_order.push(idx);
        self.focused = Some(idx);
        self.notify_session_changed();
    }

    fn notify_session_changed(&mut self) {
        if self.session_ready {
            self.session_dirty = true;
        }
    }

    fn maybe_save_session(&mut self, ticks: u64) {
        if !self.session_ready || !self.session_dirty {
            return;
        }
        let interval = crate::interrupts::ticks_for_millis(SESSION_SAVE_MS);
        if self.last_session_save_tick != 0
            && ticks.wrapping_sub(self.last_session_save_tick) < interval
        {
            return;
        }
        self.save_session();
        self.session_dirty = false;
        self.last_session_save_tick = ticks;
    }

    fn save_session(&self) {
        let mut data = String::new();
        for (idx, window) in self.windows.iter().enumerate().take(MAX_SESSION_WINDOWS) {
            let win = window.window();
            crate::app_lifecycle::remember_geometry(win.title, win.x, win.y, win.width, win.height);
            data.push_str(win.title);
            data.push('|');
            push_i32_decimal(&mut data, win.x);
            data.push('|');
            push_i32_decimal(&mut data, win.y);
            data.push('|');
            push_i32_decimal(&mut data, win.width);
            data.push('|');
            push_i32_decimal(&mut data, win.height);
            data.push('|');
            if let AppWindow::FileManager(fm) = window {
                data.push_str(fm.current_path());
            }
            data.push('|');
            push_decimal(&mut data, self.window_workspace(idx) as u64);
            data.push('\n');
        }
        let _ = crate::config_store::safe_write(SESSION_PATH, data.as_bytes());
    }

    fn restore_session(&mut self) {
        let Some(bytes) = crate::config_store::read(SESSION_PATH) else {
            return;
        };
        let Ok(text) = core::str::from_utf8(&bytes) else {
            return;
        };

        let mut restored = 0usize;
        for line in text.lines() {
            if restored >= MAX_SESSION_WINDOWS {
                break;
            }
            let mut parts = line.split('|');
            let title = parts.next().unwrap_or("");
            let Some(x) = parts.next().and_then(parse_i32_field) else {
                continue;
            };
            let Some(y) = parts.next().and_then(parse_i32_field) else {
                continue;
            };
            let Some(width) = parts.next().and_then(parse_i32_field) else {
                continue;
            };
            let Some(height) = parts.next().and_then(parse_i32_field) else {
                continue;
            };
            let extra = parts.next().unwrap_or("");
            let workspace = parts
                .next()
                .and_then(parse_usize_field)
                .unwrap_or(0)
                .min(WORKSPACE_COUNT - 1);
            let before = self.windows.len();
            let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
            let screen_w = self.shadow_width as i32;
            let x = x.clamp(-screen_w + 80, screen_w - 80);
            let y = y.clamp(0, taskbar_y.saturating_sub(40));
            let width = width.clamp(180, self.shadow_width as i32);
            let height = height.clamp(TITLE_H + 80, taskbar_y.max(TITLE_H + 80));

            match canonical_app_title(title) {
                "File Manager" => {
                    let dir = if extra.is_empty() { "/" } else { extra };
                    self.launch_file_manager_at(dir, x, y);
                }
                "Terminal" | "System Monitor" | "Diagnostics" | "Text Viewer" | "Color Picker"
                | "Display Settings" | "Personalize" => self.launch_app(title, x, y),
                _ => {}
            }

            if self.windows.len() > before {
                let idx = self.windows.len() - 1;
                self.windows[idx]
                    .window_mut()
                    .set_bounds(x, y, width, height);
                if let Some(slot) = self.window_workspaces.get_mut(idx) {
                    *slot = workspace;
                }
                restored += 1;
            }
        }
        if restored > 0 {
            self.current_workspace = 0;
            self.focused = self.top_visible_window();
            crate::klog::log_owned(format!("desktop restored {} window(s)", restored));
        }
    }

    fn sync_desktop_settings(&mut self) {
        let settings = desktop_settings::snapshot();
        self.desktop_show_icons = settings.show_icons;
        self.desktop_compact_spacing = settings.compact_spacing;
        self.desktop_sort = settings.sort_mode;
        if !self.desktop_show_icons {
            self.icon_selected = None;
            self.desktop_multi_selected.clear();
            self.pressed_icon = None;
            self.desktop_icon_drag = None;
            self.desktop_select_drag = None;
        }
        if self.wallpaper_preset != settings.wallpaper {
            let taskbar_y = self.shadow_height.saturating_sub(TASKBAR_H as usize);
            self.wallpaper =
                build_wallpaper(self.shadow_width, taskbar_y, settings.wallpaper, false);
            self.wallpaper_preset = settings.wallpaper;
            self.full_damage_next = true;
        }
    }

    fn refresh_desktop_state(&mut self) {
        self.sync_desktop_settings();
        let taskbar_y = self.shadow_height.saturating_sub(TASKBAR_H as usize);
        self.wallpaper =
            build_wallpaper(self.shadow_width, taskbar_y, self.wallpaper_preset, false);
        self.full_damage_next = true;
        self.icon_selected = None;
        self.desktop_multi_selected.clear();
        self.pressed_icon = None;
        self.desktop_icon_drag = None;
        self.desktop_select_drag = None;
        for window in self.windows.iter_mut() {
            if let AppWindow::FileManager(fm) = window {
                fm.refresh_current_dir();
            }
        }
    }

    fn launch_app(&mut self, name: &str, wx: i32, wy: i32) {
        let before = self.windows.len();
        if let Some(permission) = crate::security::app_permission_for(canonical_app_title(name)) {
            crate::notifications::push(
                "App permissions",
                &format!("{} requests {}", canonical_app_title(name), permission),
            );
        }
        match canonical_app_title(name) {
            "Terminal" => self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy))),
            "System Monitor" => self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy))),
            "Diagnostics" => self.add_window(AppWindow::TextViewer(
                TextViewerApp::diagnostics_viewer(wx, wy),
            )),
            "Text Viewer" => self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy))),
            "Color Picker" => self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy))),
            "Display Settings" => {
                self.add_window(AppWindow::DisplaySettings(DisplaySettingsApp::new(wx, wy)))
            }
            "File Manager" => self.launch_file_manager_at("/", wx, wy),
            "Personalize" => self.add_window(AppWindow::Personalize(PersonalizeApp::new(wx, wy))),
            "Crash Viewer" => {
                self.add_window(AppWindow::TextViewer(TextViewerApp::crash_viewer(wx, wy)))
            }
            "Log Viewer" => {
                self.add_window(AppWindow::TextViewer(TextViewerApp::log_viewer(wx, wy)))
            }
            "Boot Profiler" => self.add_window(AppWindow::TextViewer(
                TextViewerApp::profiler_viewer(wx, wy),
            )),
            "Welcome" => self.add_window(AppWindow::TextViewer(TextViewerApp::welcome(wx, wy))),
            _ => {}
        }
        if self.windows.len() > before {
            crate::app_lifecycle::record_app(canonical_app_title(name));
            self.apply_remembered_geometry(self.windows.len() - 1);
        }
    }

    fn launch_startup_apps(&mut self) {
        let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
        for app in crate::app_lifecycle::startup_apps().iter() {
            let off = self.windows.len() as i32 * 16;
            let wx = (24 + off).min(self.shadow_width as i32 - 220);
            let wy = (24 + off).min(taskbar_y - 120);
            self.launch_app(app, wx, wy);
        }
    }

    fn apply_remembered_geometry(&mut self, win_idx: usize) {
        if win_idx >= self.windows.len() {
            return;
        }
        let title = self.windows[win_idx].window().title;
        let Some(geometry) = crate::app_lifecycle::geometry_for(title) else {
            return;
        };
        self.windows[win_idx]
            .window_mut()
            .set_bounds(geometry.x, geometry.y, geometry.w, geometry.h);
    }

    fn window_workspace(&self, win_idx: usize) -> usize {
        self.window_workspaces
            .get(win_idx)
            .copied()
            .unwrap_or(0)
            .min(WORKSPACE_COUNT - 1)
    }

    fn is_window_on_current_workspace(&self, win_idx: usize) -> bool {
        self.window_workspace(win_idx) == self.current_workspace
    }

    fn top_visible_window(&self) -> Option<usize> {
        self.z_order.iter().rev().copied().find(|&idx| {
            idx < self.windows.len()
                && self.is_window_on_current_workspace(idx)
                && !self.windows[idx].is_minimized()
        })
    }

    fn switch_workspace(&mut self, workspace: usize) -> bool {
        let workspace = workspace.min(WORKSPACE_COUNT - 1);
        if self.current_workspace == workspace {
            return false;
        }
        self.current_workspace = workspace;
        self.focused = self.top_visible_window();
        self.drag = None;
        self.resize = None;
        self.scroll_drag = None;
        self.file_drag = None;
        self.desktop_icon_drag = None;
        self.desktop_select_drag = None;
        self.context_menu = None;
        self.start_menu_open = false;
        self.taskbar_menu = None;
        self.notification_center_open = false;
        true
    }

    fn launch_file_manager_at(&mut self, dir: &str, wx: i32, wy: i32) {
        crate::app_lifecycle::record_app("File Manager");
        let app = if dir == "/" {
            FileManagerApp::new(wx, wy)
        } else {
            FileManagerApp::new_at_path(wx, wy, dir)
        };
        self.add_window(AppWindow::FileManager(app));
    }

    fn focus_window(&mut self, win_idx: usize) {
        if win_idx >= self.windows.len() {
            return;
        }
        self.current_workspace = self.window_workspace(win_idx);
        if self.windows[win_idx].is_minimized() {
            self.windows[win_idx].window_mut().restore();
        }
        if let Some(z_pos) = self.z_order.iter().position(|&i| i == win_idx) {
            self.z_order.remove(z_pos);
        }
        self.z_order.push(win_idx);
        self.focused = Some(win_idx);
    }

    fn toggle_launcher(&mut self) {
        if self.launcher.is_some() {
            self.launcher = None;
        } else {
            self.launcher = Some(LauncherState {
                query: String::new(),
                selected: 0,
            });
            self.start_menu_open = false;
            self.context_menu = None;
            self.taskbar_menu = None;
        }
    }

    fn toggle_start_menu(&mut self) {
        if self.start_menu_open {
            self.start_menu_open = false;
        } else {
            self.refresh_start_menu_cache();
            self.start_menu_open = true;
        }
        self.context_menu = None;
        self.launcher = None;
        self.taskbar_menu = None;
        self.notification_center_open = false;
    }

    fn refresh_start_menu_cache(&mut self) {
        self.start_menu_pinned = crate::app_lifecycle::pinned_apps();
        self.start_menu_entries = build_start_menu_entries();
        self.start_menu_widget_line = start_menu_widget_status_line();
    }

    fn handle_launcher_input(&mut self, input: KeyInput) -> bool {
        if self.launcher.is_none() {
            return false;
        }

        let mut action: Option<LauncherAction> = None;
        let mut close = false;
        if let Some(state) = self.launcher.as_mut() {
            match input.key {
                Key::Escape => close = true,
                Key::Backspace => {
                    state.query.pop();
                    state.selected = 0;
                }
                Key::ArrowUp => {
                    state.selected = state.selected.saturating_sub(1);
                }
                Key::ArrowDown => {
                    let count = launcher_matches(&state.query).len();
                    if count > 0 {
                        state.selected = (state.selected + 1).min(count - 1);
                    }
                }
                Key::Tab if !input.has_ctrl() && !input.has_alt() => {
                    let count = launcher_matches(&state.query).len();
                    if count > 0 {
                        state.selected = (state.selected + 4).min(count - 1);
                    }
                }
                Key::Enter if input.has_ctrl() => action = Some(LauncherAction::RunAsAdmin),
                Key::Enter => action = Some(LauncherAction::Open),
                Key::Character('p') | Key::Character('P') if input.has_ctrl() => {
                    action = Some(LauncherAction::Pin)
                }
                Key::Character('l') | Key::Character('L') if input.has_ctrl() => {
                    action = Some(LauncherAction::OpenLocation)
                }
                Key::Character('u') | Key::Character('U') if input.has_ctrl() => {
                    action = Some(LauncherAction::Uninstall)
                }
                Key::Character('c') | Key::Character('C') if input.has_ctrl() => {
                    action = Some(LauncherAction::Copy)
                }
                Key::Space if !input.has_ctrl() && !input.has_alt() => {
                    state.query.push(' ');
                    state.selected = 0;
                }
                Key::Character(c) if !input.has_ctrl() && !input.has_alt() => {
                    if c >= ' ' && c != '\u{7f}' {
                        state.query.push(c);
                        state.selected = 0;
                    }
                }
                _ => {}
            }
        }

        if close {
            self.launcher = None;
            return true;
        }
        if let Some(action) = action {
            self.activate_launcher_selection(action);
        }
        true
    }

    fn activate_launcher_selection(&mut self, action: LauncherAction) {
        let Some(state) = self.launcher.as_ref() else {
            return;
        };
        let matches = launcher_matches(&state.query);
        if matches.is_empty() {
            return;
        }
        let entry = matches[state.selected.min(matches.len() - 1)].clone();
        let query = state.query.clone();
        self.launcher = None;
        if !query.trim().is_empty() {
            crate::app_lifecycle::record_search(&query);
        }
        let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
        let off = self.windows.len() as i32 * 16;
        let wx = (16 + off).min(self.shadow_width as i32 - 220);
        let wy = (16 + off).min(taskbar_y - 120);

        self.activate_launcher_match(&entry, action, wx, wy);
    }

    fn activate_launcher_match(
        &mut self,
        entry: &LauncherMatch,
        action: LauncherAction,
        wx: i32,
        wy: i32,
    ) {
        match action {
            LauncherAction::Open => self.activate_launcher_kind(&entry.kind, wx, wy),
            LauncherAction::RunAsAdmin => {
                crate::notifications::push("Launcher", "run-as-admin requested");
                self.activate_launcher_kind(&entry.kind, wx, wy);
            }
            LauncherAction::OpenLocation => self.open_launcher_location(entry, wx, wy),
            LauncherAction::Pin => {
                let pin = launcher_pin_label(entry);
                if crate::app_lifecycle::is_pinned(&pin) {
                    crate::notifications::push("Start menu", "item is already pinned");
                } else {
                    crate::app_lifecycle::pin_item(&pin);
                    crate::notifications::push("Start menu", "item pinned");
                }
            }
            LauncherAction::Uninstall => {
                if let LauncherMatchKind::App(app) = &entry.kind {
                    match crate::packages::uninstall(app) {
                        Ok(()) => crate::notifications::push("Packages", "app uninstalled"),
                        Err(err) => self.show_error_dialog("Uninstall failed", err),
                    }
                } else {
                    self.show_error_dialog("Uninstall failed", "selected item is not an app");
                }
            }
            LauncherAction::Copy => {
                crate::clipboard::set_text(&launcher_copy_text(entry));
                crate::notifications::push("Clipboard", "launcher item copied");
            }
        }
    }

    fn activate_launcher_kind(&mut self, kind: &LauncherMatchKind, wx: i32, wy: i32) {
        match kind {
            LauncherMatchKind::App(app) => self.launch_app(app, wx, wy),
            LauncherMatchKind::Path(path) => {
                self.open_associated_path(path, wx, wy);
            }
            LauncherMatchKind::Command(command) => {
                self.run_terminal_command(command);
            }
            LauncherMatchKind::Inline(action) => self.run_inline_launcher_action(action, wx, wy),
        }
    }

    fn open_launcher_location(&mut self, entry: &LauncherMatch, wx: i32, wy: i32) {
        match &entry.kind {
            LauncherMatchKind::Path(path) => self.launch_file_manager_at(parent_path(path), wx, wy),
            LauncherMatchKind::App(app) => {
                if let Some(meta) = crate::app_metadata::app_by_name(app) {
                    let mut path = String::from("/APPS/");
                    path.push_str(meta.command);
                    self.launch_file_manager_at(&path, wx, wy);
                } else {
                    self.launch_file_manager_at("/APPS", wx, wy);
                }
            }
            LauncherMatchKind::Command(_) => self.launch_app("Terminal", wx, wy),
            LauncherMatchKind::Inline(_) => self.launch_file_manager_at("/CONFIG", wx, wy),
        }
    }

    fn run_inline_launcher_action(&mut self, action: &str, wx: i32, wy: i32) {
        if action == "refresh-index" {
            crate::search_index::refresh();
            crate::notifications::push("Search", "desktop index refreshed");
        } else if action == "restore-session" {
            self.restore_session();
            crate::notifications::push("Session", "restore requested");
        } else if action == "restart-desktop" {
            self.refresh_desktop_state();
            crate::notifications::push("Desktop", "shell state refreshed");
        } else if action == "test-crash-dialog" {
            self.show_crash_dialog(
                "App launch failed",
                "diagnostic crash dialog preview",
                Some("Diagnostics"),
            );
        } else if action == "lock" {
            crate::notifications::push("Session", "lock screen placeholder active");
        } else if action == "logout" {
            self.minimize_all_windows();
            crate::notifications::push("Session", "apps minimized for logout placeholder");
        } else if action == "sleep" {
            match crate::acpi::sleep() {
                Ok(()) => crate::notifications::push("Power", "sleep requested"),
                Err(err) => self.show_error_dialog("Sleep unavailable", err),
            }
        } else if action == "shutdown" {
            match crate::acpi::shutdown() {
                Ok(()) => crate::notifications::push("Power", "shutdown requested"),
                Err(err) => self.show_error_dialog("Shutdown unavailable", err),
            }
        } else if action == "reboot" {
            crate::notifications::push("Power", "reboot requested");
            crate::acpi::reboot();
        } else if let Some(page) = action.strip_prefix("settings:") {
            self.add_window(AppWindow::DisplaySettings(DisplaySettingsApp::with_page(
                wx, wy, page,
            )));
            crate::app_lifecycle::record_app("Display Settings");
        } else if let Some(category) = action.strip_prefix("category:") {
            self.launcher = Some(LauncherState {
                query: {
                    let mut query = String::from("@");
                    query.push_str(category);
                    query
                },
                selected: 0,
            });
        } else if let Some(query) = action.strip_prefix("search:") {
            self.launcher = Some(LauncherState {
                query: String::from(query),
                selected: 0,
            });
        }
    }

    fn activate_start_item(&mut self, item: &str, wx: i32, wy: i32) {
        let kind = start_item_kind(item);
        self.activate_launcher_kind(&kind, wx, wy);
    }

    fn quick_launch_pinned(&mut self, slot: usize) -> bool {
        let pinned = crate::app_lifecycle::pinned_apps();
        let Some(item) = pinned.get(slot) else {
            return false;
        };
        let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
        let off = self.windows.len() as i32 * 16;
        let wx = (16 + off).min(self.shadow_width as i32 - 220);
        let wy = (16 + off).min(taskbar_y - 120);
        self.activate_start_item(item, wx, wy);
        true
    }

    fn open_associated_path(&mut self, path: &str, wx: i32, wy: i32) {
        let info = crate::vfs::inspect_path(path);
        if info.kind == crate::vfs::PathKind::Directory {
            self.launch_file_manager_at(path, wx, wy);
            return;
        }
        crate::app_lifecycle::record_file(path);
        match crate::app_metadata::association_for(path, false) {
            crate::app_metadata::Association::Executable => {
                if let Err(err) = crate::elf::spawn_elf_process(path) {
                    let body = err.as_str();
                    self.print_to_terminal("exec failed: ");
                    self.print_to_terminal(body);
                    self.print_to_terminal("\n");
                    self.show_crash_dialog("App launch failed", body, Some(path));
                }
            }
            crate::app_metadata::Association::AppShortcut(app) => self.launch_app(app, wx, wy),
            crate::app_metadata::Association::Text | crate::app_metadata::Association::Unknown => {
                match TextViewerApp::open_file(wx, wy, path) {
                    Ok(viewer) => self.add_window(AppWindow::TextViewer(viewer)),
                    Err(err) => {
                        self.print_to_terminal("open failed: ");
                        self.print_to_terminal(err);
                        self.print_to_terminal("\n");
                        self.show_error_dialog("Open failed", err);
                    }
                }
            }
            crate::app_metadata::Association::Directory => {
                self.launch_file_manager_at(path, wx, wy)
            }
        }
    }

    fn print_to_terminal(&mut self, msg: &str) {
        if let Some(term) = self.windows.iter_mut().find_map(|w| match w {
            AppWindow::Terminal(t) => Some(t),
            _ => None,
        }) {
            term.print_str(msg);
        }
    }

    fn show_error_dialog(&mut self, title: &str, body: &str) {
        self.dialog = Some(ShellDialog {
            title: String::from(title),
            body: String::from(body),
            kind: ShellDialogKind::Error,
            restart_target: None,
        });
        crate::notifications::push(title, body);
        self.launcher = None;
        self.context_menu = None;
        self.taskbar_menu = None;
    }

    fn show_crash_dialog(&mut self, title: &str, body: &str, restart_target: Option<&str>) {
        self.dialog = Some(ShellDialog {
            title: String::from(title),
            body: String::from(body),
            kind: ShellDialogKind::Crash,
            restart_target: restart_target.map(String::from),
        });
        crate::notifications::push(title, body);
        self.launcher = None;
        self.context_menu = None;
        self.taskbar_menu = None;
    }

    fn run_terminal_command(&mut self, command: &str) {
        let mut idx = self
            .windows
            .iter()
            .position(|w| matches!(w, AppWindow::Terminal(_)));
        if idx.is_none() {
            let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
            let off = self.windows.len() as i32 * 16;
            let wx = (16 + off).min(self.shadow_width as i32 - 220);
            let wy = (16 + off).min(taskbar_y - 120);
            self.launch_app("Terminal", wx, wy);
            idx = self.windows.len().checked_sub(1);
        }
        if let Some(win_idx) = idx {
            self.focus_window(win_idx);
            for c in command.chars() {
                self.windows[win_idx].handle_key(c);
            }
            self.windows[win_idx].handle_key('\n');
        }
    }

    fn toggle_notification_center(&mut self) {
        self.notification_center_open = !self.notification_center_open;
        if self.notification_center_open {
            crate::notifications::mark_all_read();
            self.launcher = None;
            self.start_menu_open = false;
            self.context_menu = None;
            self.taskbar_menu = None;
        }
    }

    fn consume_window_open_request(&mut self, win_idx: usize, sw: usize, taskbar_y: i32) {
        let Some(request) = self
            .windows
            .get_mut(win_idx)
            .and_then(AppWindow::take_open_request)
        else {
            return;
        };

        match request {
            FileManagerOpenRequest::File(path) => {
                crate::app_lifecycle::record_file(&path);
                let off = self.windows.len() as i32 * 16;
                let wx = (20 + off).min(sw as i32 - 220);
                let wy = (20 + off).min(taskbar_y - 120);
                match TextViewerApp::open_file(wx, wy, &path) {
                    Ok(viewer) => self.add_window(AppWindow::TextViewer(viewer)),
                    Err(err) => {
                        if let Some(term) = self.windows.iter_mut().find_map(|w| match w {
                            AppWindow::Terminal(t) => Some(t),
                            _ => None,
                        }) {
                            term.print_str("open failed: ");
                            term.print_str(err);
                            term.print_char('\n');
                        }
                        self.show_error_dialog("Open failed", err);
                    }
                }
            }
            FileManagerOpenRequest::Exec(path) => {
                crate::app_lifecycle::record_file(&path);
                if let Err(err) = crate::elf::spawn_elf_process(&path) {
                    let body = err.as_str();
                    if let Some(term) = self.windows.iter_mut().find_map(|w| match w {
                        AppWindow::Terminal(t) => Some(t),
                        _ => None,
                    }) {
                        term.print_str("exec failed: ");
                        term.print_str(body);
                        term.print_char('\n');
                    }
                    self.show_crash_dialog("App launch failed", body, Some(&path));
                }
            }
            FileManagerOpenRequest::App(app) => {
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(sw as i32 - 220);
                let wy = (10 + off).min(taskbar_y - 120);
                self.launch_app(app, wx, wy);
            }
        }
        crate::wm::request_repaint();
    }

    fn desktop_icons(&self) -> Vec<DesktopIcon> {
        if !self.desktop_show_icons {
            return Vec::new();
        }

        let mut specs = DESKTOP_ICON_SPECS.to_vec();
        match self.desktop_sort {
            DesktopSortMode::Default => {}
            DesktopSortMode::Name => {
                specs.sort_by(|a, b| a.label.cmp(b.label));
            }
            DesktopSortMode::Type => {
                specs.sort_by(|a, b| {
                    a.type_rank
                        .cmp(&b.type_rank)
                        .then_with(|| a.label.cmp(b.label))
                });
            }
        }

        let step_x = if self.desktop_compact_spacing {
            140
        } else {
            168
        };
        let step_y = if self.desktop_compact_spacing { 88 } else { 98 };
        let cols = 3i32;

        specs
            .into_iter()
            .enumerate()
            .map(|(i, spec)| DesktopIcon {
                x: self
                    .desktop_icon_drag
                    .as_ref()
                    .filter(|drag| drag.icon == i)
                    .map(|drag| drag.cur_x)
                    .or_else(|| desktop_settings::icon_position(spec.label).map(|pos| pos.0))
                    .unwrap_or(20 + (i as i32 % cols) * step_x),
                y: self
                    .desktop_icon_drag
                    .as_ref()
                    .filter(|drag| drag.icon == i)
                    .map(|drag| drag.cur_y)
                    .or_else(|| desktop_settings::icon_position(spec.label).map(|pos| pos.1))
                    .unwrap_or(20 + (i as i32 / cols) * step_y),
                label: spec.label,
                app: spec.app,
            })
            .collect()
    }

    fn desktop_icon_hit(&self, px: i32, py: i32) -> Option<usize> {
        self.desktop_icons()
            .iter()
            .position(|icon| icon.hit(px, py))
    }

    fn taskbar_button_hit(&self, px: i32, py: i32, sw: i32, taskbar_y: i32) -> Option<usize> {
        let show_desktop_x = sw - TASKBAR_CLOCK_W - SHOW_DESKTOP_W - 8;
        let taskbar_btn_x0 = START_BTN_W + 8;
        if px < taskbar_btn_x0 || px >= show_desktop_x - 6 {
            return None;
        }
        if py < taskbar_y + 2 || py >= taskbar_y + TASKBAR_H {
            return None;
        }
        let slot = ((px - taskbar_btn_x0) / (BUTTON_W + 6)) as usize;
        let bx = taskbar_btn_x0 + slot as i32 * (BUTTON_W + 6);
        if px >= bx + BUTTON_W {
            return None;
        }
        let mut current_slot = 0usize;
        for win_idx in 0..self.windows.len() {
            if !self.is_window_on_current_workspace(win_idx) {
                continue;
            }
            if current_slot == slot {
                return Some(win_idx);
            }
            current_slot += 1;
        }
        None
    }

    fn open_taskbar_menu(&mut self, win_idx: usize, mx: i32, taskbar_y: i32, sw: i32) {
        if win_idx >= self.windows.len() {
            return;
        }
        let x = mx.min(sw - TASKBAR_MENU_W - 4).max(4);
        let y = (taskbar_y - TASKBAR_MENU_H - 4).max(0);
        self.taskbar_menu = Some(TaskbarMenu {
            window: win_idx,
            x,
            y,
        });
        self.start_menu_open = false;
        self.context_menu = None;
        self.launcher = None;
    }

    fn handle_taskbar_menu_click(&mut self, mx: i32, my: i32) -> bool {
        let Some(menu) = self.taskbar_menu.take() else {
            return false;
        };
        if mx < menu.x
            || mx >= menu.x + TASKBAR_MENU_W
            || my < menu.y
            || my >= menu.y + TASKBAR_MENU_H
        {
            return false;
        }
        if menu.window >= self.windows.len() {
            return true;
        }
        let row_y = menu.y + 5;
        let row = ((my - row_y) / TASKBAR_MENU_ROW_H).clamp(0, 2);
        match row {
            0 => {
                if self.windows[menu.window].is_minimized() {
                    self.windows[menu.window].window_mut().restore();
                    self.focus_window(menu.window);
                } else {
                    self.windows[menu.window].window_mut().minimize();
                    if self.focused == Some(menu.window) {
                        self.focused = self.top_visible_window();
                    }
                }
            }
            1 => {
                self.snap_window(menu.window, SnapTarget::Maximize);
            }
            _ => {
                self.close_window(menu.window);
            }
        }
        true
    }

    fn open_context_menu(&mut self, mx: i32, my: i32, sw: i32, taskbar_y: i32) {
        let menu_h = ctx_menu_height(DESKTOP_CONTEXT_MENU);
        let cx = mx.min(sw - CTX_W).max(0);
        let cy = my.min(taskbar_y - menu_h).max(0);
        self.context_menu = Some(ContextMenu {
            x: cx,
            y: cy,
            submenu: None,
        });
    }

    fn update_context_menu_hover(&mut self, mx: i32, my: i32, sw: i32, taskbar_y: i32) {
        let Some(cm) = self.context_menu.as_mut() else {
            return;
        };

        if let Some(idx) = ctx_menu_hit_index(DESKTOP_CONTEXT_MENU, cm.x, cm.y, CTX_W, mx, my) {
            if let ContextEntryKind::Submenu(submenu) = DESKTOP_CONTEXT_MENU[idx].kind {
                cm.submenu = Some(submenu);
            } else {
                cm.submenu = None;
            }
            return;
        }

        if let Some(submenu) = cm.submenu {
            let (sub_x, sub_y, sub_w, sub_h) = ctx_submenu_rect(cm.x, cm.y, submenu, sw, taskbar_y);
            if mx < sub_x || mx >= sub_x + sub_w || my < sub_y || my >= sub_y + sub_h {
                cm.submenu = None;
            }
        }
    }

    fn handle_context_menu_click(&mut self, mx: i32, my: i32, sw: i32, taskbar_y: i32) -> bool {
        let Some(ref cm) = self.context_menu else {
            return false;
        };
        let cm_x = cm.x;
        let cm_y = cm.y;
        let cm_submenu = cm.submenu;

        if let Some(submenu) = cm_submenu {
            let (sub_x, sub_y, sub_w, sub_h) = ctx_submenu_rect(cm_x, cm_y, submenu, sw, taskbar_y);
            let entries = ctx_submenu_entries(submenu);
            if let Some(idx) = ctx_menu_hit_index(entries, sub_x, sub_y, sub_w, mx, my) {
                let entry = entries[idx];
                if entry.enabled {
                    if let ContextEntryKind::Action(cmd) = entry.kind {
                        self.context_menu = None;
                        self.run_context_command(cmd, sw, taskbar_y);
                    }
                }
                return true;
            }
            if mx >= sub_x && mx < sub_x + sub_w && my >= sub_y && my < sub_y + sub_h {
                return true;
            }
        }

        if let Some(idx) = ctx_menu_hit_index(DESKTOP_CONTEXT_MENU, cm_x, cm_y, CTX_W, mx, my) {
            let entry = DESKTOP_CONTEXT_MENU[idx];
            match entry.kind {
                ContextEntryKind::Action(cmd) if entry.enabled => {
                    self.context_menu = None;
                    self.run_context_command(cmd, sw, taskbar_y);
                }
                ContextEntryKind::Submenu(submenu) if entry.enabled => {
                    if let Some(cm_mut) = self.context_menu.as_mut() {
                        cm_mut.submenu = Some(submenu);
                    }
                }
                _ => {}
            }
            return true;
        }

        let main_h = ctx_menu_height(DESKTOP_CONTEXT_MENU);
        if mx >= cm_x && mx < cm_x + CTX_W && my >= cm_y && my < cm_y + main_h {
            return true;
        }

        self.context_menu = None;
        false
    }

    fn run_context_command(&mut self, cmd: DesktopContextCommand, sw: i32, taskbar_y: i32) {
        match cmd {
            DesktopContextCommand::ToggleDesktopIcons => {
                desktop_settings::set_show_icons(!self.desktop_show_icons);
                self.sync_desktop_settings();
            }
            DesktopContextCommand::ToggleCompactSpacing => {
                desktop_settings::set_compact_spacing(!self.desktop_compact_spacing);
                self.sync_desktop_settings();
            }
            DesktopContextCommand::SortByName => {
                desktop_settings::set_sort_mode(DesktopSortMode::Name);
                self.sync_desktop_settings();
            }
            DesktopContextCommand::SortByType => {
                desktop_settings::set_sort_mode(DesktopSortMode::Type);
                self.sync_desktop_settings();
            }
            DesktopContextCommand::Refresh => self.refresh_desktop_state(),
            DesktopContextCommand::CreateFolder => {
                if let Ok(path) = create_root_item("DIR", None, true) {
                    let off = self.windows.len() as i32 * 16;
                    let wx = (10 + off).min(sw - 640);
                    let wy = (10 + off).min(taskbar_y - 120);
                    self.launch_app("File Manager", wx, wy);
                    if let Some(AppWindow::FileManager(fm)) = self.windows.last_mut() {
                        fm.load_dir("/");
                    }
                    if let Some(term) = self.windows.iter_mut().find_map(|w| match w {
                        AppWindow::Terminal(t) => Some(t),
                        _ => None,
                    }) {
                        term.print_str("created ");
                        term.print_str(&path);
                        term.print_char('\n');
                    }
                }
            }
            DesktopContextCommand::CreateTextDocument => {
                if let Ok(path) = create_root_item("FILE", Some("TXT"), false) {
                    let off = self.windows.len() as i32 * 16;
                    let wx = (10 + off).min(sw - 640);
                    let wy = (10 + off).min(taskbar_y - 120);
                    self.launch_app("File Manager", wx, wy);
                    if let Some(AppWindow::FileManager(fm)) = self.windows.last_mut() {
                        fm.load_dir("/");
                    }
                    if let Some(term) = self.windows.iter_mut().find_map(|w| match w {
                        AppWindow::Terminal(t) => Some(t),
                        _ => None,
                    }) {
                        term.print_str("created ");
                        term.print_str(&path);
                        term.print_char('\n');
                    }
                }
            }
            DesktopContextCommand::DisplaySettings => {
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(sw - crate::apps::displaysettings::DISPLAY_SETTINGS_W);
                let wy =
                    (10 + off).min(taskbar_y - crate::apps::displaysettings::DISPLAY_SETTINGS_H);
                self.launch_app("Display Settings", wx, wy);
            }
            DesktopContextCommand::Personalize => {
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(sw - crate::apps::personalize::PERSONALIZE_W);
                let wy = (10 + off).min(taskbar_y - crate::apps::personalize::PERSONALIZE_H);
                self.launch_app("Personalize", wx, wy);
            }
        }
        crate::wm::request_repaint();
    }

    fn toggle_show_desktop(&mut self) {
        let any_visible = self
            .windows
            .iter()
            .enumerate()
            .any(|(idx, w)| self.is_window_on_current_workspace(idx) && !w.window().minimized);
        if any_visible {
            let current_workspace = self.current_workspace;
            for (idx, w) in self.windows.iter_mut().enumerate() {
                if self
                    .window_workspaces
                    .get(idx)
                    .copied()
                    .unwrap_or(0)
                    .min(WORKSPACE_COUNT - 1)
                    == current_workspace
                {
                    w.window_mut().minimize();
                }
            }
            self.focused = None;
        } else {
            let current_workspace = self.current_workspace;
            for (idx, w) in self.windows.iter_mut().enumerate() {
                if self
                    .window_workspaces
                    .get(idx)
                    .copied()
                    .unwrap_or(0)
                    .min(WORKSPACE_COUNT - 1)
                    == current_workspace
                {
                    w.window_mut().restore();
                }
            }
            self.focused = self.top_visible_window();
        }
    }

    fn minimize_all_windows(&mut self) {
        let current_workspace = self.current_workspace;
        for (idx, w) in self.windows.iter_mut().enumerate() {
            if self
                .window_workspaces
                .get(idx)
                .copied()
                .unwrap_or(0)
                .min(WORKSPACE_COUNT - 1)
                == current_workspace
            {
                w.window_mut().minimize();
            }
        }
        self.focused = None;
    }

    fn show_task_switcher(&mut self) {
        self.task_switcher_until_tick =
            crate::interrupts::ticks() + crate::interrupts::ticks_for_millis(TASK_SWITCHER_MS);
        self.task_switcher_query.clear();
    }

    fn snap_focused_window(&mut self, target: SnapTarget) -> bool {
        let Some(idx) = self.focused else {
            return false;
        };
        self.snap_window(idx, target)
    }

    fn snap_window(&mut self, win_idx: usize, target: SnapTarget) -> bool {
        if win_idx >= self.windows.len() {
            return false;
        }

        let sw = self.shadow_width as i32;
        let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
        let half_w = (sw / 2).max(160);
        let half_h = (taskbar_y / 2).max(TITLE_H + 80);
        let (x, y, w, h) = match target {
            SnapTarget::Left => (0, 0, half_w, taskbar_y),
            SnapTarget::Right => (sw - half_w, 0, half_w, taskbar_y),
            SnapTarget::Maximize => (0, 0, sw, taskbar_y),
            SnapTarget::Bottom => (0, taskbar_y - half_h, sw, half_h),
            SnapTarget::TopLeft => (0, 0, half_w, half_h),
            SnapTarget::TopRight => (sw - half_w, 0, half_w, half_h),
            SnapTarget::BottomLeft => (0, taskbar_y - half_h, half_w, half_h),
            SnapTarget::BottomRight => (sw - half_w, taskbar_y - half_h, half_w, half_h),
        };

        self.windows[win_idx].window_mut().set_bounds(x, y, w, h);
        if let Some(z_pos) = self.z_order.iter().position(|&i| i == win_idx) {
            self.z_order.remove(z_pos);
            self.z_order.push(win_idx);
        }
        self.focused = Some(win_idx);
        self.context_menu = None;
        self.start_menu_open = false;
        self.notify_session_changed();
        true
    }

    fn snap_dragged_window_on_release(&mut self, win_idx: usize) -> bool {
        if win_idx >= self.windows.len() {
            return false;
        }
        let w = self.windows[win_idx].window();
        let sw = self.shadow_width as i32;
        let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
        let target = if w.y <= SNAP_EDGE_PX {
            Some(SnapTarget::Maximize)
        } else if w.x <= SNAP_EDGE_PX {
            Some(SnapTarget::Left)
        } else if w.x + w.width >= sw - SNAP_EDGE_PX {
            Some(SnapTarget::Right)
        } else if w.y + w.height >= taskbar_y - SNAP_EDGE_PX {
            Some(SnapTarget::Bottom)
        } else {
            None
        };
        if let Some(target) = target {
            self.snap_window(win_idx, target)
        } else {
            false
        }
    }

    pub fn stop_key_sink(&mut self) {
        if let Some(fd) = self.key_sink_fd.take() {
            crate::vfs::vfs_close(fd);
        }
        self.key_sink_window = None;
    }

    pub fn handle_key(&mut self, c: char) {
        if let (Some(fd), Some(target)) = (self.key_sink_fd, self.key_sink_window) {
            if self.focused != Some(target) {
                if let Some(idx) = self.focused {
                    if idx < self.windows.len() {
                        self.windows[idx].handle_key(c);
                        crate::wm::request_repaint();
                    }
                }
                return;
            }

            if c == '~' {
                self.stop_key_sink();
                if target < self.windows.len() {
                    if let AppWindow::Terminal(ref mut t) = self.windows[target] {
                        t.print_str("\n[keydemo closed]\n> ");
                    }
                }
                crate::wm::request_repaint();
                return;
            }

            let packet = key_event_packet(c);
            let n = crate::vfs::vfs_write(fd, &packet);
            if n != EVENT_PACKET_SIZE {
                self.stop_key_sink();
                if target < self.windows.len() {
                    if let AppWindow::Terminal(ref mut t) = self.windows[target] {
                        t.print_str("\n[keydemo pipe error]\n> ");
                    }
                }
                crate::wm::request_repaint();
            }
            return;
        }

        if let Some(idx) = self.focused {
            if idx < self.windows.len() {
                self.windows[idx].handle_key(c);
                crate::wm::request_repaint();
            }
        }
    }

    pub fn handle_key_input(&mut self, input: KeyInput) {
        if self.launcher.is_some()
            && !crate::shortcuts::matches(crate::shortcuts::Action::Launcher, input)
        {
            self.handle_launcher_input(input);
            crate::wm::request_repaint();
            return;
        }
        if self.task_switcher_until_tick > crate::interrupts::ticks()
            && self.handle_task_switcher_input(input)
        {
            crate::wm::request_repaint();
            return;
        }
        if self.handle_global_shortcut(input) {
            crate::wm::request_repaint();
            return;
        }
        if let Some(c) = input.legacy_char() {
            self.handle_key(c);
        }
    }

    fn handle_task_switcher_input(&mut self, input: KeyInput) -> bool {
        match input.key {
            Key::Escape => {
                self.task_switcher_until_tick = 0;
                self.task_switcher_query.clear();
                true
            }
            Key::Enter => {
                self.task_switcher_until_tick = 0;
                true
            }
            Key::Backspace if !input.has_alt() && !input.has_ctrl() => {
                self.task_switcher_query.pop();
                self.focus_first_switcher_match();
                true
            }
            Key::Character(c) if !input.has_alt() && !input.has_ctrl() => {
                if c >= ' ' && c != '\u{7f}' {
                    self.task_switcher_query.push(c);
                    self.focus_first_switcher_match();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn focus_first_switcher_match(&mut self) {
        if self.task_switcher_query.is_empty() {
            return;
        }
        let query = self.task_switcher_query.to_ascii_lowercase();
        if let Some(idx) = self.z_order.iter().rev().copied().find(|&idx| {
            idx < self.windows.len()
                && self.is_window_on_current_workspace(idx)
                && !self.windows[idx].is_minimized()
                && self.windows[idx]
                    .window()
                    .title
                    .to_ascii_lowercase()
                    .contains(&query)
        }) {
            self.focus_window(idx);
        }
    }

    fn handle_global_shortcut(&mut self, input: KeyInput) -> bool {
        if crate::shortcuts::matches(crate::shortcuts::Action::Launcher, input) {
            self.toggle_launcher();
            return true;
        }
        if crate::shortcuts::matches(crate::shortcuts::Action::Notifications, input) {
            self.toggle_notification_center();
            return true;
        }
        match input.key {
            Key::Tab if input.has_alt() => {
                self.focus_previous_window();
                self.show_task_switcher();
                true
            }
            Key::PageUp if input.has_ctrl() && input.has_alt() => {
                let next = if self.current_workspace == 0 {
                    WORKSPACE_COUNT - 1
                } else {
                    self.current_workspace - 1
                };
                self.switch_workspace(next);
                true
            }
            Key::PageDown if input.has_ctrl() && input.has_alt() => {
                self.switch_workspace((self.current_workspace + 1) % WORKSPACE_COUNT);
                true
            }
            Key::ArrowLeft if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::Left)
            }
            Key::ArrowRight if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::Right)
            }
            Key::ArrowUp if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::Maximize)
            }
            Key::ArrowDown if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::Bottom)
            }
            Key::Character('1') if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::TopLeft)
            }
            Key::Character('2') if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::TopRight)
            }
            Key::Character('3') if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::BottomLeft)
            }
            Key::Character('4') if input.has_ctrl() && input.has_alt() => {
                self.snap_focused_window(SnapTarget::BottomRight)
            }
            Key::F4 if input.has_alt() => {
                if let Some(idx) = self.focused {
                    self.close_window(idx);
                    true
                } else {
                    false
                }
            }
            Key::F5 => {
                self.refresh_desktop_state();
                true
            }
            Key::F2
                if !input.has_ctrl()
                    && !input.has_alt()
                    && self.focused.is_none()
                    && self.icon_selected.is_some() =>
            {
                self.show_error_dialog(
                    "Rename shortcut",
                    "Desktop shortcut names are loaded from /APPS package metadata.",
                );
                true
            }
            Key::Escape if input.has_ctrl() => {
                self.toggle_start_menu();
                true
            }
            Key::Character('w') | Key::Character('W') if input.has_ctrl() => {
                if let Some(idx) = self.focused {
                    self.close_window(idx);
                    true
                } else {
                    false
                }
            }
            Key::Character('r') | Key::Character('R') if input.has_ctrl() => {
                self.refresh_desktop_state();
                true
            }
            Key::Character('f') | Key::Character('F') if input.has_ctrl() => {
                let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(self.shadow_width as i32 - 220);
                let wy = (10 + off).min(taskbar_y - 120);
                self.launch_file_manager_at("/", wx, wy);
                self.start_menu_open = false;
                true
            }
            Key::Character('n') | Key::Character('N') if input.has_ctrl() => {
                let taskbar_y = self.shadow_height as i32 - TASKBAR_H;
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(self.shadow_width as i32 - 220);
                let wy = (10 + off).min(taskbar_y - 120);
                self.launch_app("Terminal", wx, wy);
                self.start_menu_open = false;
                true
            }
            Key::Character(c) if input.has_ctrl() && !input.has_alt() => {
                if let Some(slot) = ctrl_number_slot(c) {
                    self.quick_launch_pinned(slot)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn focus_previous_window(&mut self) {
        if self.z_order.is_empty() {
            self.focused = None;
            return;
        }
        let start = self
            .focused
            .and_then(|focused| self.z_order.iter().position(|&idx| idx == focused))
            .unwrap_or_else(|| self.z_order.len().saturating_sub(1));

        let mut pos = start;
        for _ in 0..self.z_order.len() {
            pos = if pos == 0 {
                self.z_order.len() - 1
            } else {
                pos - 1
            };
            let candidate = self.z_order[pos];
            if candidate < self.windows.len()
                && self.is_window_on_current_workspace(candidate)
                && !self.windows[candidate].window().minimized
            {
                self.z_order.remove(pos);
                self.z_order.push(candidate);
                self.focused = Some(candidate);
                self.context_menu = None;
                self.start_menu_open = false;
                return;
            }
        }
        self.focused = None;
    }

    fn close_window(&mut self, win_idx: usize) {
        if win_idx >= self.windows.len() {
            return;
        }
        if self.key_sink_window == Some(win_idx) {
            self.stop_key_sink();
        } else if let Some(target) = self.key_sink_window {
            if target > win_idx {
                self.key_sink_window = Some(target - 1);
            }
        }
        self.windows.remove(win_idx);
        if win_idx < self.window_workspaces.len() {
            self.window_workspaces.remove(win_idx);
        }
        self.z_order.retain(|&i| i != win_idx);
        for z in self.z_order.iter_mut() {
            if *z > win_idx {
                *z -= 1;
            }
        }
        self.focused = self.top_visible_window();
        self.drag = None;
        self.resize = None;
        self.scroll_drag = None;
        self.file_drag = None;
        self.context_menu = None;
        self.taskbar_menu = None;
        self.notify_session_changed();
    }

    /// Full composite frame into shadow, then blit to hardware framebuffer.
    pub fn compose(&mut self) {
        let frame_start_tick = crate::interrupts::ticks();
        // Drain buffered keystrokes.
        while let Some(input) = crate::keyboard::pop_input() {
            self.handle_key_input(input);
        }

        // Drain syscall write() output into the first terminal window.
        while let Some(b) = crate::syscall::pop_output_byte() {
            for w in self.windows.iter_mut() {
                if let AppWindow::Terminal(ref mut t) = w {
                    t.print_char(b as char);
                    break;
                }
            }
        }

        // Consume deferred terminal requests to install a compositor-owned key sink.
        for (idx, w) in self.windows.iter_mut().enumerate() {
            if let AppWindow::Terminal(t) = w {
                if let Some(fd) = t.take_pending_key_sink() {
                    if self.key_sink_fd.is_none() {
                        self.key_sink_fd = Some(fd);
                        self.key_sink_window = Some(idx);
                    } else {
                        crate::vfs::vfs_close(fd);
                        t.print_str("keydemo unavailable: input sink busy\n> ");
                    }
                }
            }
        }

        if let Some(path) = crate::wm::take_screenshot_request() {
            match self.save_focused_screenshot(&path) {
                Ok(()) => crate::notifications::push("Screenshot saved", &path),
                Err(err) => crate::notifications::push("Screenshot failed", err),
            }
        }

        let sw = self.shadow_width;
        let sh = self.shadow_height;
        let taskbar_y = sh as i32 - TASKBAR_H;

        // Snapshot the real boot tick count for time-based UI.
        let uptime_ticks = crate::interrupts::ticks();

        let (mx, my) = crate::mouse::pos();
        let (left, right) = crate::mouse::buttons();
        let mx_i = mx as i32;
        let my_i = my as i32;

        let left_pressed = left && !self.prev_left;
        let left_released = !left && self.prev_left;
        let right_pressed = right && !self.prev_right;
        let mut left_press_consumed = false;

        // ── Input ─────────────────────────────────────────────────────────────

        if left_pressed && self.dialog.is_some() {
            self.handle_dialog_click(mx_i, my_i, sw as i32, taskbar_y);
            left_press_consumed = true;
            crate::wm::request_repaint();
        }

        // Start button click — flush left, full height
        let taskbar_click = left_pressed && my_i >= taskbar_y && mx_i < START_BTN_W;
        if taskbar_click {
            self.toggle_start_menu();
            left_press_consumed = true;
            crate::wm::request_repaint();
        }

        if left_pressed {
            let had_taskbar_menu = self.taskbar_menu.is_some();
            if self.handle_taskbar_menu_click(mx_i, my_i) || had_taskbar_menu {
                left_press_consumed = true;
                crate::wm::request_repaint();
            }
        }

        if right_pressed && my_i >= taskbar_y {
            if let Some(btn_idx) = self.taskbar_button_hit(mx_i, my_i, sw as i32, taskbar_y) {
                self.open_taskbar_menu(btn_idx, mx_i, taskbar_y, sw as i32);
                crate::wm::request_repaint();
            }
        } else if right_pressed && my_i < taskbar_y {
            if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                let win_idx = self.z_order[z_pos];
                self.z_order.remove(z_pos);
                self.z_order.push(win_idx);
                self.focused = Some(win_idx);
                self.context_menu = None;
                let lx = mx_i - self.windows[win_idx].window().x;
                let ly = my_i - (self.windows[win_idx].window().y + TITLE_H);
                self.windows[win_idx].handle_secondary_click(lx, ly);
                self.consume_window_open_request(win_idx, sw, taskbar_y);
                crate::wm::request_repaint();
            } else {
                self.open_context_menu(mx_i, my_i, sw as i32, taskbar_y);
                crate::wm::request_repaint();
            }
        }

        if self.context_menu.is_some() {
            self.update_context_menu_hover(mx_i, my_i, sw as i32, taskbar_y);
        }

        if left_pressed {
            if left_press_consumed {
                // Already handled by shell chrome such as Start or a taskbar jump list.
            } else if self.context_menu.is_some() {
                left_press_consumed = true;
                let _ = self.handle_context_menu_click(mx_i, my_i, sw as i32, taskbar_y);
            } else {
                if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                    left_press_consumed = true;
                    let win_idx = self.z_order[z_pos];
                    self.z_order.remove(z_pos);
                    self.z_order.push(win_idx);
                    self.focused = Some(win_idx);

                    let hit_close = self.windows[win_idx].window().hit_close(mx_i, my_i);
                    let hit_minimize = self.windows[win_idx].window().hit_minimize(mx_i, my_i);
                    let hit_maximize = self.windows[win_idx].window().hit_maximize(mx_i, my_i);
                    let hit_title = self.windows[win_idx].window().hit_title(mx_i, my_i);
                    let hit_resize = self.windows[win_idx].window().hit_resize(mx_i, my_i);
                    let hit_scrollbar = self.windows[win_idx].window().hit_scrollbar(mx_i, my_i);

                    if hit_close {
                        self.close_window(win_idx);
                        crate::wm::request_repaint();
                    } else if hit_minimize {
                        self.windows[win_idx].window_mut().minimize();
                        crate::wm::request_repaint();
                    } else if hit_maximize {
                        let sw = self.shadow_width as i32;
                        let sh = self.shadow_height as i32;
                        self.windows[win_idx].window_mut().maximize(sw, sh);
                        self.notify_session_changed();
                        crate::wm::request_repaint();
                    } else if hit_title {
                        self.drag = Some(DragState {
                            window: win_idx,
                            off_x: mx_i - self.windows[win_idx].window().x,
                            off_y: my_i - self.windows[win_idx].window().y,
                        });
                    } else if hit_resize {
                        let w = self.windows[win_idx].window();
                        self.resize = Some(ResizeState {
                            window: win_idx,
                            start_w: w.width,
                            start_h: w.height,
                            start_mx: mx_i,
                            start_my: my_i,
                        });
                    } else if hit_scrollbar {
                        let w = self.windows[win_idx].window();
                        let view_h = (w.height - TITLE_H).max(0);
                        self.scroll_drag = Some(ScrollDragState {
                            window: win_idx,
                            start_offset: w.scroll.offset,
                            start_my: my_i,
                            content_h: w.scroll.content_h,
                            view_h,
                            track_h: view_h,
                        });
                    } else {
                        let lx = mx_i - self.windows[win_idx].window().x;
                        let ly = my_i - (self.windows[win_idx].window().y + TITLE_H);
                        if self.key_sink_fd.is_some() && self.key_sink_window == Some(win_idx) {
                            let fd = self.key_sink_fd.unwrap();
                            let packet = mouse_event_packet(1, lx, ly);
                            if crate::vfs::vfs_write(fd, &packet) != EVENT_PACKET_SIZE {
                                self.stop_key_sink();
                                if win_idx < self.windows.len() {
                                    if let AppWindow::Terminal(ref mut t) = self.windows[win_idx] {
                                        t.print_str("\n[keydemo pipe error]\n> ");
                                    }
                                }
                            }
                        }
                        let file_drag_paths = self.windows[win_idx].begin_file_drag(lx, ly);
                        self.windows[win_idx].handle_click(lx, ly);
                        if let Some(paths) = file_drag_paths {
                            self.file_drag = Some(FileDragState {
                                source_window: win_idx,
                                paths,
                                cut: false,
                            });
                        }
                        self.consume_window_open_request(win_idx, sw, taskbar_y);
                        let is_double_click = self.last_click_window == Some(win_idx)
                            && uptime_ticks.wrapping_sub(self.last_click_tick)
                                <= crate::interrupts::ticks_for_millis(500)
                            && (self.last_click_x - lx).abs() <= 6
                            && (self.last_click_y - ly).abs() <= 6;
                        if is_double_click {
                            self.windows[win_idx].handle_dbl_click(lx, ly);
                            self.consume_window_open_request(win_idx, sw, taskbar_y);
                        }
                        self.last_click_tick = uptime_ticks;
                        self.last_click_window = Some(win_idx);
                        self.last_click_x = lx;
                        self.last_click_y = ly;
                    }
                }

                if my_i >= taskbar_y {
                    left_press_consumed = true;
                    // Tray icon click → open System Monitor
                    let tray_clk_x = sw as i32 - TASKBAR_CLOCK_W;
                    let clock_box_x = tray_clk_x + TASKBAR_TRAY_W;
                    let t_gap = 20i32;
                    let t_start = tray_clk_x + (TASKBAR_TRAY_W - (t_gap * 2 + 13)) / 2;
                    if mx_i >= clock_box_x {
                        self.toggle_notification_center();
                        crate::wm::request_repaint();
                    } else if mx_i >= t_start && mx_i < t_start + t_gap * 2 + 14 {
                        let off = self.windows.len() as i32 * 16;
                        let wx = (10 + off).min(sw as i32 - 540);
                        let wy = (10 + off).min(taskbar_y - 310);
                        self.launch_app("System Monitor", wx, wy);
                        crate::wm::request_repaint();
                    }
                    let show_desktop_x = sw as i32 - TASKBAR_CLOCK_W - SHOW_DESKTOP_W - 8;
                    if mx_i >= show_desktop_x && mx_i < show_desktop_x + SHOW_DESKTOP_W {
                        self.toggle_show_desktop();
                        crate::wm::request_repaint();
                    } else {
                        if let Some(btn_idx) =
                            self.taskbar_button_hit(mx_i, my_i, sw as i32, taskbar_y)
                        {
                            self.focus_window(btn_idx);
                            crate::wm::request_repaint();
                        }
                    }
                }
            }
        }

        if left_released {
            let session_changed = self.drag.is_some() || self.resize.is_some();
            let drag_window = self.drag.as_ref().map(|d| d.window);
            if let Some(win_idx) = drag_window {
                if self.snap_dragged_window_on_release(win_idx) {
                    crate::wm::request_repaint();
                }
            }
            self.drag = None;
            self.resize = None;
            self.scroll_drag = None;
            if let Some(file_drag) = self.file_drag.take() {
                if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                    let target = self.z_order[z_pos];
                    if target != file_drag.source_window && target < self.windows.len() {
                        let count = file_drag.paths.len();
                        let cut = file_drag.cut;
                        let paths = file_drag.paths;
                        if self.windows[target].drop_file_paths(paths, cut) {
                            crate::notifications::push(
                                "File drop",
                                if count == 1 {
                                    "copied 1 item"
                                } else {
                                    "copied selected items"
                                },
                            );
                            crate::wm::request_repaint();
                        }
                    }
                }
            }
            if session_changed {
                self.notify_session_changed();
            }
        }

        if left {
            if let Some(ref d) = self.drag {
                let wi = d.window;
                if wi < self.windows.len() {
                    let w = self.windows[wi].window_mut();
                    w.x = mx_i - d.off_x;
                    w.y = my_i - d.off_y;
                }
            }
            if let Some(ref rs) = self.resize {
                let wi = rs.window;
                if wi < self.windows.len() {
                    let new_w = rs.start_w + mx_i - rs.start_mx;
                    let new_h = rs.start_h + my_i - rs.start_my;
                    self.windows[wi].window_mut().resize_to(new_w, new_h);
                    crate::wm::request_repaint();
                }
            }
            if let Some(ref sd) = self.scroll_drag {
                let wi = sd.window;
                if wi < self.windows.len() {
                    let delta = my_i - sd.start_my;
                    let max_off = (sd.content_h - sd.view_h).max(1);
                    let track_h = sd.track_h.max(1);
                    let new_off = sd.start_offset + delta * max_off / track_h;
                    self.windows[wi].window_mut().scroll.offset = new_off.clamp(0, max_off);
                    crate::wm::request_repaint();
                }
            }
        }

        self.prev_left = left;
        self.prev_right = right;

        // Start menu item click.
        if left_pressed && self.start_menu_open {
            let prefs = crate::app_lifecycle::start_menu_prefs();
            let menu_w = prefs.width.clamp(460, sw as i32 - 8);
            let menu_h = prefs.height.clamp(320, taskbar_y - 4);
            let left_w = (if prefs.compact { 230i32 } else { 280i32 }).min(menu_w - 220);
            let bottom_h = 36i32;
            let left_hdr_h = 32i32;
            let menu_x = 0i32;
            let menu_y = taskbar_y - menu_h;
            let bar_y = menu_y + menu_h - bottom_h;
            let rc_x = menu_x + left_w + 1;
            let right_w = menu_w - left_w;
            let rc_w = right_w - 2;
            if mx_i >= menu_x && mx_i < menu_x + menu_w && my_i >= menu_y && my_i < taskbar_y {
                left_press_consumed = true;
                // Left column — app list rows
                if mx_i < menu_x + left_w {
                    let pinned_apps = &self.start_menu_pinned;
                    let item_h = if prefs.compact { 32i32 } else { 40i32 };
                    let items_y = menu_y + left_hdr_h + 8;
                    let srch_x = menu_x + 8;
                    let srch_y = bar_y + 7;
                    let srch_w = left_w - 16;
                    let srch_h = 20i32;
                    let limit = pinned_apps.len().min(start_menu_pinned_limit(
                        menu_h, bottom_h, left_hdr_h, item_h,
                    ));
                    let all_y = bar_y - item_h;
                    if mx_i >= srch_x
                        && mx_i < srch_x + srch_w
                        && my_i >= srch_y
                        && my_i < srch_y + srch_h
                    {
                        self.launcher = Some(LauncherState {
                            query: String::new(),
                            selected: 0,
                        });
                        self.start_menu_open = false;
                        crate::wm::request_repaint();
                    } else if my_i >= items_y && my_i < items_y + item_h * limit as i32 {
                        let item_idx = ((my_i - items_y) / item_h) as usize;
                        if item_idx < limit {
                            let item = pinned_apps[item_idx].clone();
                            let off = self.windows.len() as i32 * 16;
                            let wx = (10 + off).min(sw as i32 - 200);
                            let wy = (10 + off).min(bar_y - 80);
                            self.activate_start_item(&item, wx, wy);
                            self.start_menu_open = false;
                            crate::wm::request_repaint();
                        }
                    } else if my_i >= all_y && my_i < all_y + item_h {
                        self.launcher = Some(LauncherState {
                            query: String::new(),
                            selected: 0,
                        });
                        self.start_menu_open = false;
                        crate::wm::request_repaint();
                    }
                } else {
                    let banner_x = rc_x + 6;
                    let banner_y = menu_y + 10;
                    let banner_h = 84i32;
                    let settings_w = 104i32;
                    let settings_h = 20i32;
                    let settings_x = banner_x + 56;
                    let settings_y = banner_y + 38;
                    let link_h = 22i32;
                    let links_y = banner_y + banner_h + 8;
                    let sd_w = 96i32;
                    let sd_x = menu_x + left_w + (right_w - sd_w) / 2;
                    let sd_y = bar_y + 8;
                    let sd_h = 20i32;
                    if mx_i >= settings_x
                        && mx_i < settings_x + settings_w
                        && my_i >= settings_y
                        && my_i < settings_y + settings_h
                    {
                        let off = self.windows.len() as i32 * 16;
                        let wx = (10 + off).min(sw as i32 - 200);
                        let wy = (10 + off).min(bar_y - 80);
                        self.launch_app("Display Settings", wx, wy);
                        self.start_menu_open = false;
                        crate::wm::request_repaint();
                    } else if mx_i >= sd_x
                        && mx_i < sd_x + sd_w
                        && my_i >= sd_y
                        && my_i < sd_y + sd_h
                    {
                        let off = self.windows.len() as i32 * 16;
                        let wx = (10 + off).min(sw as i32 - 200);
                        let wy = (10 + off).min(bar_y - 80);
                        self.run_inline_launcher_action("shutdown", wx, wy);
                        self.start_menu_open = false;
                        crate::wm::request_repaint();
                    } else if mx_i >= rc_x && mx_i < rc_x + rc_w && my_i >= links_y {
                        let entries = &self.start_menu_entries;
                        let max_h = bar_y - links_y - 8;
                        if let Some(entry_idx) =
                            start_menu_entry_at(entries, my_i - links_y, link_h, max_h)
                        {
                            let kind = entries[entry_idx].kind.clone();
                            let off = self.windows.len() as i32 * 16;
                            let wx = (10 + off).min(sw as i32 - 200);
                            let wy = (10 + off).min(bar_y - 80);
                            self.activate_launcher_kind(&kind, wx, wy);
                            self.start_menu_open = false;
                            crate::wm::request_repaint();
                        }
                    }
                }
            }
        }

        // Desktop icon click.
        if left_pressed {
            let icon_hit = self.desktop_icon_hit(mx_i, my_i);
            let desktop_hit = !left_press_consumed
                && my_i < taskbar_y
                && self.context_menu.is_none()
                && !self.start_menu_open;
            self.pressed_icon = if desktop_hit { icon_hit } else { None };
            if let Some(i) = self.pressed_icon {
                self.focused = None;
                let icons = self.desktop_icons();
                if let Some(icon) = icons.get(i) {
                    self.desktop_icon_drag = Some(DesktopIconDragState {
                        icon: i,
                        start_mx: mx_i,
                        start_my: my_i,
                        start_x: icon.x,
                        start_y: icon.y,
                        cur_x: icon.x,
                        cur_y: icon.y,
                        moved: false,
                    });
                }
                if crate::keyboard::current_modifiers() & crate::keyboard::MOD_CTRL != 0 {
                    if let Some(pos) = self.desktop_multi_selected.iter().position(|&idx| idx == i)
                    {
                        self.desktop_multi_selected.remove(pos);
                    } else {
                        self.desktop_multi_selected.push(i);
                    }
                    self.icon_selected = Some(i);
                } else {
                    self.desktop_multi_selected.clear();
                    self.desktop_multi_selected.push(i);
                    self.icon_selected = Some(i);
                }
                self.context_menu = None;
                crate::wm::request_repaint();
            } else if desktop_hit {
                self.focused = None;
                self.desktop_select_drag = Some((mx_i, my_i));
                self.icon_selected = None;
                self.desktop_multi_selected.clear();
                self.context_menu = None;
                crate::wm::request_repaint();
            }
        }

        if left && self.desktop_select_drag.is_some() {
            crate::wm::request_repaint();
        }

        if left {
            if let Some(drag) = self.desktop_icon_drag.as_mut() {
                let dx = mx_i - drag.start_mx;
                let dy = my_i - drag.start_my;
                if dx.abs() > 4 || dy.abs() > 4 {
                    drag.moved = true;
                }
                if drag.moved {
                    drag.cur_x = (drag.start_x + dx).clamp(4, sw as i32 - ICON_SIZE - 4);
                    drag.cur_y =
                        (drag.start_y + dy).clamp(4, taskbar_y - ICON_SIZE - ICON_LABEL_H - 4);
                    crate::wm::request_repaint();
                }
            }
        }

        if left_released {
            let dragged_icon = self.desktop_icon_drag.take();
            if let Some(drag) = dragged_icon.as_ref() {
                if drag.moved {
                    if let Some(icon) = self.desktop_icons().get(drag.icon) {
                        let _ =
                            desktop_settings::set_icon_position(icon.label, drag.cur_x, drag.cur_y);
                    }
                    crate::wm::request_repaint();
                }
            }
            if let Some((sx, sy)) = self.desktop_select_drag.take() {
                let dx = (mx_i - sx).abs();
                let dy = (my_i - sy).abs();
                self.desktop_multi_selected.clear();
                if dx > 4 || dy > 4 {
                    let x0 = sx.min(mx_i);
                    let x1 = sx.max(mx_i);
                    let y0 = sy.min(my_i);
                    let y1 = sy.max(my_i);
                    for (idx, icon) in self.desktop_icons().iter().enumerate() {
                        let cx = icon.x + ICON_SIZE / 2;
                        let cy = icon.y + ICON_SIZE / 2;
                        if cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1 {
                            self.desktop_multi_selected.push(idx);
                        }
                    }
                    self.icon_selected = self.desktop_multi_selected.last().copied();
                } else {
                    self.icon_selected = None;
                }
                crate::wm::request_repaint();
            }
            if let Some(icon_idx) = self.pressed_icon.take() {
                let icon_was_dragged = dragged_icon
                    .as_ref()
                    .map(|drag| drag.moved && drag.icon == icon_idx)
                    .unwrap_or(false);
                if self.desktop_icon_hit(mx_i, my_i) == Some(icon_idx)
                    && my_i < taskbar_y
                    && self.front_to_back_hit(mx_i, my_i).is_none()
                    && self.context_menu.is_none()
                    && !self.start_menu_open
                    && self.desktop_multi_selected.len() <= 1
                    && !icon_was_dragged
                {
                    let icons = self.desktop_icons();
                    let icon = &icons[icon_idx];
                    let off = self.windows.len() as i32 * 16;
                    let wx = (10 + off).min(sw as i32 - 200);
                    let wy = (10 + off).min(taskbar_y - 80);
                    self.launch_app(icon.app, wx, wy);
                    crate::wm::request_repaint();
                }
            }
        }

        // ── Scroll wheel ─────────────────────────────────────────────────────
        let wheel_delta = crate::mouse::scroll_delta();
        if wheel_delta != 0 {
            if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                let win_idx = self.z_order[z_pos];
                self.windows[win_idx].handle_scroll(wheel_delta);
                crate::wm::request_repaint();
            }
        }

        self.sync_desktop_settings();
        self.maybe_save_session(uptime_ticks);

        // ── Render ────────────────────────────────────────────────────────────
        for w in self.windows.iter_mut() {
            w.update();
        }
        self.collect_app_dirty_spans(sw, sh);

        // Blit wallpaper before taking the exclusive &mut shadow borrow,
        // so the compiler sees two separate borrows of self.shadow / self.wallpaper.
        {
            let desk_pixels = taskbar_y as usize * sw;
            self.shadow[..desk_pixels].copy_from_slice(&self.wallpaper[..desk_pixels]);
        }
        let resize_hover = self.resize.is_some()
            || self
                .front_to_back_hit(mx_i, my_i)
                .map(|z_pos| {
                    let wi = self.z_order[z_pos];
                    wi < self.windows.len() && self.windows[wi].window().hit_resize(mx_i, my_i)
                })
                .unwrap_or(false);
        let desktop_icons = self.desktop_icons();
        let hovered_taskbar_window = self.taskbar_button_hit(mx_i, my_i, sw as i32, taskbar_y);
        let current_workspace = self.current_workspace;
        {
            let s: &mut [u32] = self.shadow.as_mut_slice();

            // ── Desktop icons — drawn BEFORE windows so windows can cover them ────
            let icon_data: [(u32, u32); 5] = [
                (ICON_TERM_BG, ICON_TERM_ACC),  // Terminal
                (ICON_MON_BG, ICON_MON_ACC),    // Monitor
                (0x00_00_0E_20, 0x00_55_DD_FF), // Files (File Manager)
                (ICON_TXT_BG, ICON_TXT_ACC),    // Viewer (Text Viewer)
                (ICON_COL_BG, ICON_COL_ACC),    // Colors (Color Picker)
            ];
            for (i, icon) in desktop_icons.iter().enumerate() {
                let selected =
                    self.icon_selected == Some(i) || self.desktop_multi_selected.contains(&i);
                let hot = mx_i >= icon.x
                    && mx_i < icon.x + ICON_SIZE
                    && my_i >= icon.y
                    && my_i < icon.y + ICON_SIZE;
                let app_open = self
                    .windows
                    .iter()
                    .any(|w| w.window().title == canonical_app_title(icon.app));

                let (icon_bg, icon_acc) = icon_data[i];

                // Drop shadow
                s_fill(
                    s,
                    sw,
                    icon.x + 3,
                    icon.y + 3,
                    ICON_SIZE,
                    ICON_SIZE,
                    if selected || hot {
                        0x00_00_22_44
                    } else {
                        0x00_00_00_18
                    },
                );
                if selected || hot {
                    s_fill(
                        s,
                        sw,
                        icon.x + 6,
                        icon.y + ICON_SIZE + 2,
                        ICON_SIZE - 12,
                        2,
                        blend_color(icon_acc, 0x00_00_06_12, 90),
                    );
                }

                // Tile background — vertical gradient from lighter top to darker bottom
                let tile_top = if selected || hot {
                    blend_color(icon_bg, 0x00_FF_FF_FF, 40)
                } else {
                    blend_color(icon_bg, 0x00_FF_FF_FF, 10)
                };
                let tile_bot = if selected || hot {
                    blend_color(icon_bg, BLACK, 60)
                } else {
                    blend_color(icon_bg, BLACK, 80)
                };
                for gy in 0..ICON_SIZE {
                    let t = (gy * 255 / ICON_SIZE.max(1)) as u32;
                    let row_col = blend_color(tile_top, tile_bot, t);
                    s_fill(s, sw, icon.x, icon.y + gy, ICON_SIZE, 1, row_col);
                }
                draw_rect_border(
                    s,
                    sw,
                    icon.x,
                    icon.y,
                    ICON_SIZE,
                    ICON_SIZE,
                    blend_color(icon_acc, 0x00_DD_FF_FF, if selected { 80 } else { 132 }),
                );
                draw_rect_border(
                    s,
                    sw,
                    icon.x + 1,
                    icon.y + 1,
                    ICON_SIZE - 2,
                    ICON_SIZE - 2,
                    blend_color(tile_top, icon_acc, 100),
                );

                // Accent top band + bottom edge line
                s_fill(s, sw, icon.x, icon.y, ICON_SIZE, 4, icon_acc);
                s_fill(
                    s,
                    sw,
                    icon.x + 6,
                    icon.y + ICON_SIZE - 5,
                    ICON_SIZE - 12,
                    2,
                    blend_color(icon_acc, tile_bot, 80),
                );

                // ── Per-app pixel-art icon ────────────────────────────────────────
                match i {
                    0 => {
                        // Terminal — ">" prompt + underscore cursor
                        let bx = icon.x + 8;
                        let by = icon.y + 14;
                        s_fill(s, sw, bx, by, 6, 3, icon_acc);
                        s_fill(s, sw, bx + 6, by + 3, 6, 3, icon_acc);
                        s_fill(s, sw, bx + 12, by + 6, 6, 3, icon_acc);
                        s_fill(s, sw, bx + 6, by + 9, 6, 3, icon_acc);
                        s_fill(s, sw, bx, by + 12, 6, 3, icon_acc);
                        s_fill(s, sw, icon.x + 8, icon.y + 38, 22, 2, icon_acc);
                    }
                    1 => {
                        // Monitor — bar chart
                        let base_y = icon.y + 44;
                        let bar_w = 8i32;
                        s_fill(s, sw, icon.x + 6, base_y - 10, bar_w, 10, icon_acc);
                        s_fill(s, sw, icon.x + 18, base_y - 26, bar_w, 26, icon_acc);
                        s_fill(s, sw, icon.x + 30, base_y - 18, bar_w, 18, icon_acc);
                        s_fill(s, sw, icon.x + 4, base_y, 36, 2, icon_acc);
                    }
                    2 => {
                        // Files — folder with content lines
                        s_fill(s, sw, icon.x + 4, icon.y + 10, 36, 30, icon_acc);
                        s_fill(s, sw, icon.x + 2, icon.y + 18, 40, 24, icon_acc);
                        s_fill(s, sw, icon.x + 2, icon.y + 10, 14, 8, icon_acc);
                        s_fill(s, sw, icon.x + 8, icon.y + 26, 28, 2, 0x00_00_0B_20);
                        s_fill(s, sw, icon.x + 8, icon.y + 32, 28, 2, 0x00_00_0B_20);
                    }
                    3 => {
                        // Viewer — document page with text lines
                        draw_rect_border(s, sw, icon.x + 8, icon.y + 6, 36, 40, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 10, 28, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 16, 22, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 22, 28, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 28, 16, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 34, 22, 2, icon_acc);
                    }
                    4 => {
                        // Colors — four colour quadrants
                        s_fill(s, sw, icon.x + 6, icon.y + 8, 16, 16, 0x00_FF_50_50);
                        s_fill(s, sw, icon.x + 26, icon.y + 8, 16, 16, 0x00_50_FF_50);
                        s_fill(s, sw, icon.x + 6, icon.y + 28, 16, 16, 0x00_50_50_FF);
                        s_fill(s, sw, icon.x + 26, icon.y + 28, 16, 16, 0x00_FF_FF_50);
                        s_fill(s, sw, icon.x + 20, icon.y + 20, 10, 10, icon_acc);
                    }
                    _ => unreachable!(),
                }

                // Selection / hover ring
                if selected || hot {
                    let ring = if selected { ACCENT } else { 0x00_00_44_88 };
                    s_fill(s, sw, icon.x - 1, icon.y - 1, ICON_SIZE + 2, 1, ring);
                    s_fill(
                        s,
                        sw,
                        icon.x - 1,
                        icon.y + ICON_SIZE,
                        ICON_SIZE + 2,
                        1,
                        ring,
                    );
                    s_fill(s, sw, icon.x - 1, icon.y, 1, ICON_SIZE, ring);
                    s_fill(s, sw, icon.x + ICON_SIZE, icon.y, 1, ICON_SIZE, ring);
                }

                // Label below icon — plain centred text, no box
                let label_y = icon.y + ICON_SIZE + 8;
                let label_w = icon.label.len() as i32 * 8;
                let label_x = (icon.x + (ICON_SIZE - label_w) / 2).max(1);
                let label_fg = if selected {
                    0x00_EE_FF_FF
                } else if app_open {
                    blend_color(icon_acc, 0x00_DD_FF_FF, 90)
                } else if hot {
                    0x00_AA_DD_FF
                } else {
                    0x00_77_BB_EE
                };
                s_draw_str_small(
                    s,
                    sw,
                    label_x,
                    label_y,
                    icon.label,
                    label_fg,
                    0, // transparent bg — draws over wallpaper
                    label_x + label_w + 4,
                );
            }

            if let Some((sx, sy)) = self.desktop_select_drag {
                let x0 = sx.min(mx_i);
                let x1 = sx.max(mx_i);
                let y0 = sy.min(my_i);
                let y1 = sy.max(my_i);
                if x1 - x0 > 3 && y1 - y0 > 3 {
                    s_fill_alpha(s, sw, x0, y0, x1 - x0, y1 - y0, 0x28_00_BB_FF);
                    draw_rect_border(s, sw, x0, y0, x1 - x0, y1 - y0, ACCENT);
                }
            }

            // ── Windows — drawn AFTER icons so they appear in front ───────────────
            let z: Vec<usize> = self.z_order.clone();
            for &wi in &z {
                if wi < self.windows.len()
                    && self
                        .window_workspaces
                        .get(wi)
                        .copied()
                        .unwrap_or(0)
                        .min(WORKSPACE_COUNT - 1)
                        == current_workspace
                {
                    let win = self.windows[wi].window();
                    if !win.minimized {
                        let focused = self.focused == Some(wi);
                        Self::draw_window(s, sw, win, focused, mx_i, my_i);
                    }
                }
            }

            // ── Taskbar — frosted glass panel ────────────────────────────────────
            // Step 1: Darken + strong blue-tint the wallpaper underneath to fake frosted glass.
            {
                let t0 = taskbar_y as usize;
                let t1 = (t0 + TASKBAR_H as usize).min(s.len() / sw);
                for row in t0..t1 {
                    for col in 0..sw {
                        let p = s[row * sw + col];
                        // Crush red/green heavily, preserve and boost blue
                        let r = ((p >> 16) & 0xFF) * 10 / 100;
                        let g = ((p >> 8) & 0xFF) * 12 / 100;
                        let b = ((p & 0xFF) * 35 / 100).saturating_add(28).min(255);
                        s[row * sw + col] = (r << 16) | (g << 8) | b;
                    }
                }
            }
            // Step 2: Bright 2-px top accent border — phosphor blue glow line.
            s_fill(s, sw, 0, taskbar_y, sw as i32, 2, 0x00_00_BB_EE);
            s_fill(s, sw, 0, taskbar_y + 2, sw as i32, 1, 0x00_00_55_88);

            // ── Start button — square launcher icon control ─────────────────────
            let start_hot = mx_i >= 0
                && mx_i < START_BTN_W
                && my_i >= taskbar_y
                && my_i < taskbar_y + TASKBAR_H;
            let start_pressed = left && start_hot;
            let start_active = self.start_menu_open || start_pressed;

            let btn_x = 10i32;
            let btn_y = taskbar_y + 6;
            let btn_w = TASKBAR_H - 12;
            let btn_h = TASKBAR_H - 12;
            let start_bg = if start_active {
                Some(0x00_00_1A_36)
            } else if start_hot {
                Some(0x00_00_12_28)
            } else {
                None
            };
            // Outer shadow/glow when active
            if start_active {
                s_fill(
                    s,
                    sw,
                    btn_x - 2,
                    btn_y - 2,
                    btn_w + 4,
                    btn_h + 4,
                    blend_color(ACCENT, BLACK, 160),
                );
            }
            if let Some(bg) = start_bg {
                s_fill(s, sw, btn_x, btn_y, btn_w, btn_h, bg);
            }

            let tile_x = btn_x + (btn_w - 18) / 2;
            let tile_y = btn_y + (btn_h - 18) / 2;
            let icon_primary = if start_active { WHITE } else { ACCENT_HOV };
            let icon_secondary = if start_active {
                blend_color(WHITE, ACCENT_HOV, 84)
            } else {
                blend_color(ACCENT_HOV, WHITE, 72)
            };
            // coolOS snowflake mark.
            draw_snowflake_logo(s, sw, tile_x, tile_y, 1, icon_primary, icon_secondary);

            // ── Start menu — shell hub with pinned and quick-launch areas ─────────
            if self.start_menu_open {
                let prefs = crate::app_lifecycle::start_menu_prefs();
                let menu_w = prefs.width.clamp(460, sw as i32 - 8);
                let menu_h = prefs.height.clamp(320, taskbar_y - 4);
                let left_w = (if prefs.compact { 230i32 } else { 280i32 }).min(menu_w - 220);
                let right_w = menu_w - left_w;
                let bottom_h = 36i32;
                let left_hdr_h = 32i32;
                let menu_x = 0i32;
                let menu_y = taskbar_y - menu_h;
                let bar_y = menu_y + menu_h - bottom_h;
                let rc_x = menu_x + left_w + 1;
                let rc_w = right_w - 2;
                let (banner_time, banner_date) = start_menu_banner_clock(uptime_ticks);

                s_fill(s, sw, menu_x, menu_y + 4, menu_w + 4, menu_h, 0x00_00_00_18);
                s_fill(s, sw, menu_x, menu_y, left_w, menu_h, 0x00_00_07_18);
                s_fill(
                    s,
                    sw,
                    menu_x + left_w,
                    menu_y,
                    right_w,
                    menu_h,
                    0x00_00_05_12,
                );
                s_fill(s, sw, menu_x, menu_y, menu_w, 2, ACCENT);
                s_fill(
                    s,
                    sw,
                    menu_x,
                    menu_y + 2,
                    menu_w,
                    2,
                    blend_color(ACCENT, 0x00_00_07_18, 160),
                );
                draw_rect_border(s, sw, menu_x, menu_y, menu_w, menu_h, 0x00_00_66_BB);
                draw_rect_border(
                    s,
                    sw,
                    menu_x + 1,
                    menu_y + 1,
                    menu_w - 2,
                    menu_h - 2,
                    0x00_00_22_44,
                );
                s_fill(
                    s,
                    sw,
                    menu_x + left_w,
                    menu_y + 4,
                    1,
                    menu_h - bottom_h - 4,
                    0x00_00_22_44,
                );
                s_fill(s, sw, menu_x + 1, bar_y, menu_w - 2, 1, 0x00_00_22_44);
                let bar_bg = 0x00_00_04_10;
                s_fill(
                    s,
                    sw,
                    menu_x + 1,
                    bar_y + 1,
                    menu_w - 2,
                    bottom_h - 2,
                    bar_bg,
                );

                let left_hdr_y = menu_y + 4;
                let left_hdr_bg = 0x00_00_0D_24;
                s_fill(
                    s,
                    sw,
                    menu_x + 1,
                    left_hdr_y,
                    left_w - 1,
                    left_hdr_h,
                    left_hdr_bg,
                );
                s_fill(
                    s,
                    sw,
                    menu_x + 1,
                    left_hdr_y + left_hdr_h - 1,
                    left_w - 1,
                    1,
                    0x00_00_22_44,
                );
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 10,
                    left_hdr_y + 8,
                    "PINNED + FAVORITES",
                    0x00_00_EE_FF,
                    left_hdr_bg,
                    menu_x + left_w - 12,
                );
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 10,
                    left_hdr_y + 20,
                    "Ctrl+1..9 quick launch",
                    0x00_33_66_88,
                    left_hdr_bg,
                    menu_x + left_w - 12,
                );

                let srch_x = menu_x + 8;
                let srch_y = bar_y + 7;
                let srch_w = left_w - 16;
                let srch_h = 20i32;
                let srch_bg = 0x00_00_03_0C;
                s_fill(s, sw, srch_x, srch_y, srch_w, srch_h, srch_bg);
                draw_rect_border(s, sw, srch_x, srch_y, srch_w, srch_h, 0x00_00_44_88);
                let sg_x = srch_x + 5;
                let sg_y = srch_y + 6;
                let sg_col = 0x00_00_44_77;
                s_fill(s, sw, sg_x + 1, sg_y, 3, 1, sg_col);
                s_fill(s, sw, sg_x, sg_y + 1, 1, 3, sg_col);
                s_fill(s, sw, sg_x + 4, sg_y + 1, 1, 3, sg_col);
                s_fill(s, sw, sg_x + 1, sg_y + 4, 3, 1, sg_col);
                s_fill(s, sw, sg_x + 4, sg_y + 4, 1, 1, sg_col);
                s_fill(s, sw, sg_x + 5, sg_y + 5, 1, 1, sg_col);
                s_fill(s, sw, sg_x + 6, sg_y + 6, 1, 1, sg_col);
                s_draw_str_small(
                    s,
                    sw,
                    sg_x + 10,
                    srch_y + 6,
                    "Search apps, files, commands",
                    sg_col,
                    srch_bg,
                    srch_x + srch_w - 4,
                );

                let sd_w = 96i32;
                let sd_x = menu_x + left_w + (right_w - sd_w) / 2;
                let sd_y = bar_y + 8;
                let sd_h = 20i32;
                let sd_hot =
                    mx_i >= sd_x && mx_i < sd_x + sd_w && my_i >= sd_y && my_i < sd_y + sd_h;
                let sd_bg = if sd_hot { 0x00_00_22_44 } else { 0x00_00_10_28 };
                s_fill(s, sw, sd_x, sd_y, sd_w, sd_h, sd_bg);
                draw_rect_border(s, sw, sd_x, sd_y, sd_w, sd_h, 0x00_00_44_88);
                let pw_x = sd_x + 7;
                let pw_y = sd_y + 5;
                let pw_c = 0x00_00_66_99;
                s_fill(s, sw, pw_x + 3, pw_y, 2, 4, pw_c);
                s_fill(s, sw, pw_x + 1, pw_y + 3, 1, 1, pw_c);
                s_fill(s, sw, pw_x + 6, pw_y + 3, 1, 1, pw_c);
                s_fill(s, sw, pw_x, pw_y + 4, 1, 3, pw_c);
                s_fill(s, sw, pw_x + 7, pw_y + 4, 1, 3, pw_c);
                s_fill(s, sw, pw_x + 1, pw_y + 7, 6, 1, pw_c);
                s_draw_str_small(
                    s,
                    sw,
                    pw_x + 12,
                    sd_y + 6,
                    "Shut down",
                    if sd_hot { WHITE } else { 0x00_88_CC_FF },
                    sd_bg,
                    sd_x + sd_w - 4,
                );

                let item_h = if prefs.compact { 32i32 } else { 40i32 };
                let items_y = menu_y + left_hdr_h + 8;
                let pinned_apps = &self.start_menu_pinned;
                let pinned_limit = pinned_apps.len().min(start_menu_pinned_limit(
                    menu_h, bottom_h, left_hdr_h, item_h,
                ));
                let mut left_hov: Option<usize> = None;
                if mx_i > menu_x
                    && mx_i < menu_x + left_w
                    && my_i >= items_y
                    && my_i < items_y + item_h * pinned_limit as i32
                {
                    left_hov = Some(((my_i - items_y) / item_h) as usize);
                }

                for (i, name) in pinned_apps.iter().take(pinned_limit).enumerate() {
                    let iy = items_y + i as i32 * item_h;
                    let is_hov = left_hov == Some(i);
                    let row_bg = if is_hov { 0x00_00_14_30 } else { 0x00_00_07_18 };
                    let kind = start_item_kind(name);
                    let acc = launcher_kind_accent(&kind);
                    let glyph = launcher_kind_glyph(&kind);
                    if is_hov {
                        s_fill(s, sw, menu_x + 1, iy, left_w - 1, item_h - 1, row_bg);
                        s_fill(s, sw, menu_x + 1, iy + 8, 3, item_h - 17, ACCENT);
                    }

                    let icon_x = menu_x + 10;
                    let icon_y = iy + (item_h - 24) / 2;
                    let icon_bg = blend_color(0x00_00_0B_20, acc, 55);
                    s_fill(s, sw, icon_x, icon_y, 24, 24, icon_bg);
                    s_fill(s, sw, icon_x, icon_y, 24, 3, acc);
                    s_draw_str_small(
                        s,
                        sw,
                        icon_x + 5,
                        icon_y + 9,
                        glyph,
                        acc,
                        icon_bg,
                        icon_x + 22,
                    );
                    let text_left = menu_x + 40;
                    let text_right = menu_x + left_w - 24;
                    let display_name = start_item_label(name);
                    let text_w = display_name.chars().count() as i32 * 8;
                    let text_x = text_left + ((text_right - text_left - text_w).max(0) / 2);
                    let text_y = iy + (item_h - 8) / 2;
                    s_draw_str_small(
                        s,
                        sw,
                        text_x,
                        text_y,
                        &display_name,
                        if is_hov { WHITE } else { 0x00_AA_DD_FF },
                        row_bg,
                        text_right,
                    );
                    if i + 1 < pinned_limit {
                        s_fill(
                            s,
                            sw,
                            menu_x + 8,
                            iy + item_h - 1,
                            left_w - 16,
                            1,
                            if is_hov { 0x00_00_10_24 } else { 0x00_00_1A_36 },
                        );
                    }
                }

                let all_y = bar_y - item_h;
                let all_hot = mx_i > menu_x
                    && mx_i < menu_x + left_w
                    && my_i >= all_y
                    && my_i < all_y + item_h;
                s_fill(s, sw, menu_x + 8, all_y, left_w - 16, 1, 0x00_00_22_44);
                if all_hot {
                    s_fill(
                        s,
                        sw,
                        menu_x + 1,
                        all_y,
                        left_w - 1,
                        item_h - 1,
                        0x00_00_14_30,
                    );
                }
                let all_bg = if all_hot {
                    0x00_00_14_30
                } else {
                    0x00_00_07_18
                };
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 10,
                    all_y + 16,
                    "All Programs",
                    if all_hot { WHITE } else { 0x00_88_CC_FF },
                    all_bg,
                    menu_x + left_w - 20,
                );
                let chv_x = menu_x + left_w - 14;
                let chv_mid = all_y + item_h / 2;
                let chv_col = if all_hot { ACCENT } else { 0x00_00_33_55 };
                s_fill(s, sw, chv_x, chv_mid - 3, 1, 4, chv_col);
                s_fill(s, sw, chv_x + 1, chv_mid - 1, 1, 2, chv_col);

                let banner_x = rc_x + 6;
                let banner_y = menu_y + 10;
                let banner_w = rc_w - 12;
                let banner_h = 84i32;
                let banner_bg = 0x00_00_0A_20;
                s_fill(s, sw, banner_x, banner_y, banner_w, banner_h, banner_bg);
                draw_rect_border(s, sw, banner_x, banner_y, banner_w, banner_h, 0x00_00_33_66);
                let av_x = banner_x + 8;
                let av_y = banner_y + 10;
                s_fill(s, sw, av_x, av_y, 40, 40, 0x00_00_10_28);
                draw_rect_border(s, sw, av_x, av_y, 40, 40, ACCENT);
                s_fill(s, sw, av_x + 14, av_y + 6, 12, 10, 0x00_00_55_88);
                s_fill(s, sw, av_x + 9, av_y + 20, 22, 14, 0x00_00_44_77);
                let clock_right = banner_x + banner_w - 10;
                let time_x = clock_right - banner_time.chars().count() as i32 * 8;
                let settings_w = 104i32;
                let settings_h = 20i32;
                let settings_x = banner_x + 56;
                let settings_y = banner_y + 38;
                let date_x = av_x + 48;
                let date_y = av_y + 16;
                s_draw_str_small(
                    s,
                    sw,
                    av_x + 48,
                    av_y + 4,
                    "user",
                    0x00_CC_EE_FF,
                    banner_bg,
                    banner_x + banner_w - 10,
                );
                s_draw_str_small(
                    s,
                    sw,
                    time_x,
                    av_y + 4,
                    &banner_time,
                    0x00_CC_EE_FF,
                    banner_bg,
                    clock_right,
                );
                s_draw_str_small(
                    s,
                    sw,
                    date_x,
                    date_y,
                    &banner_date,
                    0x00_44_88_BB,
                    banner_bg,
                    clock_right,
                );

                let settings_hot = mx_i >= settings_x
                    && mx_i < settings_x + settings_w
                    && my_i >= settings_y
                    && my_i < settings_y + settings_h;
                let settings_bg = if settings_hot {
                    0x00_00_14_30
                } else {
                    0x00_00_07_18
                };
                s_fill(
                    s,
                    sw,
                    settings_x,
                    settings_y,
                    settings_w,
                    settings_h,
                    settings_bg,
                );
                s_draw_str_small(
                    s,
                    sw,
                    settings_x + ((settings_w - ("Settings".len() as i32 * 8)) / 2),
                    settings_y + 6,
                    "Settings",
                    if settings_hot { WHITE } else { 0x00_AA_DD_FF },
                    settings_bg,
                    settings_x + settings_w - 6,
                );
                draw_rect_border(
                    s,
                    sw,
                    settings_x,
                    settings_y,
                    settings_w,
                    settings_h,
                    if settings_hot { ACCENT } else { 0x00_00_33_66 },
                );
                s_fill(s, sw, settings_x, settings_y, settings_w, 2, ACCENT);

                let link_h = 22i32;
                let links_y = banner_y + banner_h + 8;
                let entries = &self.start_menu_entries;
                let mut last_section = "";
                let mut row_y = links_y;
                for entry in entries.iter() {
                    if entry.section != last_section {
                        if row_y + START_MENU_SECTION_H > bar_y - 8 {
                            break;
                        }
                        s_draw_str_small(
                            s,
                            sw,
                            rc_x + 10,
                            row_y + 2,
                            entry.section,
                            0x00_44_77_99,
                            0x00_00_05_12,
                            rc_x + rc_w - 4,
                        );
                        row_y += START_MENU_SECTION_H;
                        last_section = entry.section;
                    }
                    let ly = row_y;
                    if ly + link_h > bar_y - 8 {
                        break;
                    }
                    let is_hov =
                        mx_i >= rc_x && mx_i < rc_x + rc_w && my_i >= ly && my_i < ly + link_h;
                    let link_bg = if is_hov { 0x00_00_14_30 } else { 0x00_00_05_12 };
                    if is_hov {
                        s_fill(s, sw, rc_x, ly, rc_w, link_h - 1, link_bg);
                        s_fill(s, sw, rc_x, ly + 6, 2, link_h - 13, ACCENT);
                    }
                    let glyph = launcher_kind_glyph(&entry.kind);
                    s_draw_str_small(
                        s,
                        sw,
                        rc_x + 10,
                        ly + 7,
                        glyph,
                        launcher_kind_accent(&entry.kind),
                        link_bg,
                        rc_x + 28,
                    );
                    s_draw_str_small(
                        s,
                        sw,
                        rc_x + 34,
                        ly + 7,
                        &entry.label,
                        if is_hov { WHITE } else { 0x00_88_CC_FF },
                        link_bg,
                        rc_x + rc_w - 72,
                    );
                    s_draw_str_small(
                        s,
                        sw,
                        rc_x + rc_w - 68,
                        ly + 7,
                        &entry.detail,
                        0x00_44_77_99,
                        link_bg,
                        rc_x + rc_w - 4,
                    );
                    row_y += link_h;
                }

                if prefs.show_widgets {
                    draw_start_menu_widgets(
                        s,
                        sw,
                        banner_x + 56,
                        banner_y + 62,
                        (banner_w - 66).max(64),
                        16,
                        &self.start_menu_widget_line,
                    );
                }
            }

            // ── Taskbar window tabs — icon-first strip ───────────────────────────
            let taskbar_btn_x0 = START_BTN_W + 8;
            let show_desktop_x = sw as i32 - TASKBAR_CLOCK_W - SHOW_DESKTOP_W - 8;
            let mut taskbar_slot = 0usize;
            for i in 0..self.windows.len() {
                if self
                    .window_workspaces
                    .get(i)
                    .copied()
                    .unwrap_or(0)
                    .min(WORKSPACE_COUNT - 1)
                    != current_workspace
                {
                    continue;
                }
                let bx = taskbar_btn_x0 + taskbar_slot as i32 * (BUTTON_W + 6);
                if bx + BUTTON_W > show_desktop_x - 6 {
                    break;
                }
                taskbar_slot += 1;

                let focused = self.focused == Some(i);
                let minimized = self.windows[i].is_minimized();
                let hovered = hovered_taskbar_window == Some(i);
                let title = self.windows[i].window().title;
                let accent = window_accent(title);
                let glyph = window_glyph(title);

                let bh = TASKBAR_H - 4;
                let by = taskbar_y + 2;
                let bg = if focused {
                    0x00_00_28_55
                } else if hovered {
                    0x00_00_16_32
                } else {
                    0x00_00_00_00
                };

                if focused || hovered {
                    s_fill(s, sw, bx, by, BUTTON_W, bh, bg);
                }
                if focused {
                    // 3px bottom accent bar
                    s_fill(s, sw, bx, taskbar_y + TASKBAR_H - 3, BUTTON_W, 3, accent);
                    // Subtle top glow line
                    s_fill(
                        s,
                        sw,
                        bx,
                        taskbar_y + 2,
                        BUTTON_W,
                        1,
                        blend_color(accent, BLACK, 170),
                    );
                } else if hovered {
                    s_fill(
                        s,
                        sw,
                        bx,
                        taskbar_y + TASKBAR_H - 2,
                        BUTTON_W,
                        1,
                        0x00_00_55_99,
                    );
                }
                if minimized {
                    s_fill(
                        s,
                        sw,
                        bx + BUTTON_W / 2 - 4,
                        taskbar_y + TASKBAR_H - 4,
                        8,
                        2,
                        0x00_00_66_AA,
                    );
                }

                let icon_x = bx + 10;
                let icon_y = by + (bh - 16) / 2;
                let icon_bg = if focused || hovered {
                    blend_color(bg, accent, 70)
                } else {
                    blend_color(0x00_00_0B_20, accent, 65)
                };
                s_fill(s, sw, icon_x, icon_y, 16, 16, icon_bg);
                draw_rect_border(
                    s,
                    sw,
                    icon_x,
                    icon_y,
                    16,
                    16,
                    blend_color(accent, WHITE, 80),
                );
                s_draw_str_small(
                    s,
                    sw,
                    icon_x + 3,
                    icon_y + 4,
                    glyph,
                    accent,
                    icon_bg,
                    icon_x + 14,
                );

                let trunc = if title.len() > 14 {
                    &title[..14]
                } else {
                    title
                };
                let text_w = trunc.len() as i32 * 8;
                let text_x = icon_x + 24;
                s_draw_str_small(
                    s,
                    sw,
                    text_x,
                    by + (bh - 8) / 2,
                    trunc,
                    if focused {
                        0x00_CC_EE_FF
                    } else if hovered {
                        0x00_66_BB_EE
                    } else {
                        0x00_44_88_BB
                    },
                    bg,
                    (text_x + text_w).min(bx + BUTTON_W - 8),
                );
            }

            if let Some(idx) = hovered_taskbar_window {
                if idx < self.windows.len() {
                    let slot = self
                        .windows
                        .iter()
                        .enumerate()
                        .filter(|(win_idx, _)| {
                            self.window_workspaces
                                .get(*win_idx)
                                .copied()
                                .unwrap_or(0)
                                .min(WORKSPACE_COUNT - 1)
                                == current_workspace
                        })
                        .position(|(win_idx, _)| win_idx == idx)
                        .unwrap_or(0);
                    let bx = taskbar_btn_x0 + slot as i32 * (BUTTON_W + 6);
                    draw_taskbar_preview(s, sw, taskbar_y, bx, &self.windows[idx]);
                }
            }

            // ── Clock / system tray — refined phosphor readout ────────────────────
            let clock_sep_x = sw as i32 - TASKBAR_CLOCK_W - 4;
            // Thin left separator
            s_fill(
                s,
                sw,
                clock_sep_x,
                taskbar_y + 4,
                1,
                TASKBAR_H - 8,
                0x00_00_44_88,
            );

            let clk_x = clock_sep_x + 4;
            let clk_w = TASKBAR_CLOCK_W - 4;
            let tray_icon_gap = 20i32;
            let clock_box_x = clk_x + TASKBAR_TRAY_W;
            let clock_box_w = clk_w - TASKBAR_TRAY_W;
            let usb_lines = crate::usb::status_lines();
            let (usb_keyboard, usb_mouse) = crate::usb::input_presence();
            let usb_present = !usb_lines.is_empty();
            let usb_active = usb_lines
                .iter()
                .any(|line| line.contains("active init ready"));
            let pulse_step = (crate::interrupts::TIMER_HZ / 8).max(1) as u64;
            let pulse = ((uptime_ticks / pulse_step) % 28) as u32;
            let usb_col = if usb_active {
                blend_color(0x00_00_EE_FF, ACCENT_HOV, pulse * 4)
            } else if usb_present {
                0x00_55_AA_DD
            } else {
                0x00_22_44_66
            };
            let kbd_col = if usb_keyboard {
                0x00_00_FF_88
            } else {
                0x00_33_55_44
            };
            let mouse_col = if usb_mouse {
                0x00_FF_DD_55
            } else {
                0x00_55_55_44
            };

            // Center three icons horizontally in tray zone, vertically in taskbar.
            let icon_span = tray_icon_gap * 2 + 13;
            let tray_x = clk_x + (TASKBAR_TRAY_W - icon_span) / 2;
            let tray_y = taskbar_y + (TASKBAR_H - 12) / 2;
            let usb_hot = mx_i >= tray_x
                && mx_i < tray_x + 12
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H;
            let kbd_hot = mx_i >= tray_x + tray_icon_gap
                && mx_i < tray_x + tray_icon_gap + 14
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H;
            let mse_hot = mx_i >= tray_x + tray_icon_gap * 2
                && mx_i < tray_x + tray_icon_gap * 2 + 14
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H;
            if usb_hot {
                s_fill(
                    s,
                    sw,
                    tray_x - 3,
                    taskbar_y + 3,
                    17,
                    TASKBAR_H - 6,
                    0x00_00_18_38,
                );
            }
            if kbd_hot {
                s_fill(
                    s,
                    sw,
                    tray_x + tray_icon_gap - 3,
                    taskbar_y + 3,
                    20,
                    TASKBAR_H - 6,
                    0x00_00_18_38,
                );
            }
            if mse_hot {
                s_fill(
                    s,
                    sw,
                    tray_x + tray_icon_gap * 2 - 3,
                    taskbar_y + 3,
                    17,
                    TASKBAR_H - 6,
                    0x00_00_18_38,
                );
            }

            let clk_bg = 0x00_00_00_00;

            // Small tray icons: controller, keyboard, mouse.
            draw_usb_tray_icon(s, sw, tray_x, tray_y, usb_col);
            draw_keyboard_tray_icon(s, sw, tray_x + tray_icon_gap, tray_y, kbd_col);
            draw_mouse_tray_icon(s, sw, tray_x + tray_icon_gap * 2, tray_y, mouse_col);

            // Separator between icons and clock.
            s_fill(
                s,
                sw,
                clock_box_x - 5,
                taskbar_y + 7,
                1,
                TASKBAR_H - 14,
                0x00_00_33_66,
            );

            // Two-line clock: uptime HH:MM on top, brand below.
            {
                let secs = uptime_ticks / crate::interrupts::TIMER_HZ as u64;
                let h = (secs / 3600) % 24;
                let m = (secs / 60) % 60;
                let buf = [
                    b'0' + (h / 10) as u8,
                    b'0' + (h % 10) as u8,
                    b':',
                    b'0' + (m / 10) as u8,
                    b'0' + (m % 10) as u8,
                ];
                if let Ok(time_str) = core::str::from_utf8(&buf) {
                    let time_w = 5 * 8;
                    let time_x = clock_box_x + (clock_box_w - time_w) / 2;
                    // Phosphor glow — dim halo 1px around digits
                    s_draw_str_small(
                        s,
                        sw,
                        time_x - 1,
                        taskbar_y + 7,
                        time_str,
                        0x00_00_44_77, // dim glow behind
                        clk_bg,
                        clock_box_x + clock_box_w,
                    );
                    s_draw_str_small(
                        s,
                        sw,
                        time_x,
                        taskbar_y + 8,
                        time_str,
                        0x00_00_FF_FF, // bright cyan phosphor clock digits
                        clk_bg,
                        clock_box_x + clock_box_w,
                    );
                }
            }
            {
                let workspace = workspace_label(current_workspace);
                s_draw_str_small(
                    s,
                    sw,
                    clock_box_x + 8,
                    taskbar_y + 22,
                    workspace,
                    ACCENT_HOV,
                    clk_bg,
                    clock_box_x + clock_box_w,
                );
                let brand_w = 6 * 8;
                let brand_x = clock_box_x + clock_box_w - brand_w - 8;
                s_draw_str_small(
                    s,
                    sw,
                    brand_x,
                    taskbar_y + 22,
                    "coolOS",
                    0x00_00_88_BB,
                    clk_bg,
                    clock_box_x + clock_box_w,
                );
            }
            let unread = crate::notifications::unread_count();
            if unread > 0 {
                let badge_x = clock_box_x + clock_box_w - 22;
                let badge_y = taskbar_y + 5;
                s_fill(s, sw, badge_x, badge_y, 16, 10, 0x00_BB_22_22);
                draw_rect_border(s, sw, badge_x, badge_y, 16, 10, 0x00_FF_88_88);
                let count_char = if unread > 9 {
                    '+'
                } else {
                    (b'0' + unread as u8) as char
                };
                s_draw_char_small(
                    s,
                    sw,
                    badge_x + 4,
                    badge_y + 1,
                    count_char,
                    WHITE,
                    0x00_BB_22_22,
                );
            }

            // ── Context menu ──────────────────────────────────────────────────────
            if let Some(ref cm) = self.context_menu {
                draw_desktop_context_menu(
                    s,
                    sw,
                    cm,
                    mx_i,
                    my_i,
                    self.desktop_show_icons,
                    self.desktop_compact_spacing,
                    self.desktop_sort,
                    sw as i32,
                    taskbar_y,
                );
            }

            if let Some(ref menu) = self.taskbar_menu {
                draw_taskbar_menu(s, sw, menu, &self.windows, mx_i, my_i);
            }

            if self.notification_center_open {
                draw_notification_center(s, sw, taskbar_y);
            } else {
                draw_notification_toasts(s, sw, taskbar_y, uptime_ticks);
            }

            if let Some(ref launcher) = self.launcher {
                draw_launcher_overlay(s, sw, taskbar_y, launcher);
            }

            if self.task_switcher_until_tick > uptime_ticks {
                draw_task_switcher_overlay(
                    s,
                    sw,
                    taskbar_y,
                    &self.windows,
                    &self.window_workspaces,
                    &self.z_order,
                    self.focused,
                    self.current_workspace,
                    &self.task_switcher_query,
                );
            }

            if let Some(ref dialog) = self.dialog {
                draw_shell_dialog(s, sw, taskbar_y, dialog);
            }

            if let Some(ref file_drag) = self.file_drag {
                draw_file_drag_badge(s, sw, mx_i + 16, my_i + 18, file_drag.paths.len());
            }

            // ── Cursor — switches to resize cursor over resize handles ────────────
            let (cursor_outline, cursor_shape) = if resize_hover {
                (&CURSOR_RESIZE_OUTLINE, &CURSOR_RESIZE_SHAPE)
            } else {
                (&CURSOR_OUTLINE, &CURSOR_SHAPE)
            };

            // Draw outline first (black), then fill (white)
            for (row, &mask) in cursor_outline.iter().enumerate() {
                for bit in 0..16usize {
                    if mask & (0x8000u16 >> bit) != 0 {
                        s_put(
                            s,
                            sw,
                            sh,
                            mx as i32 + bit as i32,
                            my as i32 + row as i32,
                            BLACK,
                        );
                    }
                }
            }
            for (row, &mask) in cursor_shape.iter().enumerate() {
                for bit in 0..16usize {
                    if mask & (0x8000u16 >> bit) != 0 {
                        s_put(
                            s,
                            sw,
                            sh,
                            mx as i32 + bit as i32,
                            my as i32 + row as i32,
                            WHITE,
                        );
                    }
                }
            }
        } // end shadow borrow — rendering done

        self.compute_damage_spans(sw, sh);

        // ── Blit damaged shadow spans → hardware framebuffer ─────────────────
        let hw_base = crate::framebuffer::base();
        let hw_stride = crate::framebuffer::stride();
        let hw_bpp = crate::framebuffer::bpp();
        let hw_fmt = crate::framebuffer::fmt();
        let is_rgb = hw_fmt == crate::framebuffer::PixFmt::Rgb;
        if hw_base != 0 {
            match hw_bpp {
                4 => {
                    for row in 0..sh {
                        let (x0, x1) = self.damage_spans[row];
                        if x0 >= x1 {
                            continue;
                        }
                        let src = &self.shadow[row * sw + x0..row * sw + x1];
                        let row_base = hw_base + (row * hw_stride * 4) as u64;
                        let dst = row_base as *mut u32;
                        if !is_rgb {
                            unsafe {
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst.add(x0), x1 - x0);
                            }
                        } else {
                            for col in x0..x1 {
                                let c = self.shadow[row * sw + col];
                                let hw = ((c & 0xFF) << 16) | (c & 0x00FF00) | (c >> 16 & 0xFF);
                                unsafe {
                                    dst.add(col).write_volatile(hw);
                                }
                            }
                        }
                    }
                }
                3 => {
                    let row_bytes = sw * 3;
                    if self.blit_scratch.len() < row_bytes {
                        self.blit_scratch.resize(row_bytes, 0);
                    }
                    let scratch = &mut self.blit_scratch[..row_bytes];
                    for row in 0..sh {
                        let (x0, x1) = self.damage_spans[row];
                        if x0 >= x1 {
                            continue;
                        }
                        let src = &self.shadow[row * sw + x0..row * sw + x1];
                        let row_base = hw_base + (row * hw_stride * 3 + x0 * 3) as u64;
                        if !is_rgb {
                            for (col, c) in src.iter().copied().enumerate() {
                                scratch[col * 3] = c as u8;
                                scratch[col * 3 + 1] = (c >> 8) as u8;
                                scratch[col * 3 + 2] = (c >> 16) as u8;
                            }
                        } else {
                            for (col, c) in src.iter().copied().enumerate() {
                                scratch[col * 3] = (c >> 16) as u8;
                                scratch[col * 3 + 1] = (c >> 8) as u8;
                                scratch[col * 3 + 2] = c as u8;
                            }
                        }
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                scratch.as_ptr(),
                                row_base as *mut u8,
                                (x1 - x0) * 3,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        self.update_compositor_telemetry(frame_start_tick);
    }

    fn update_compositor_telemetry(&mut self, frame_start_tick: u64) {
        let now = crate::interrupts::ticks();
        let frame_ticks = now.wrapping_sub(frame_start_tick);
        self.frame_ticks_peak = self.frame_ticks_peak.max(frame_ticks);
        self.fps_window_frames = self.fps_window_frames.saturating_add(1);
        if self.fps_window_start_tick == 0 {
            self.fps_window_start_tick = now;
        }
        let elapsed = now.wrapping_sub(self.fps_window_start_tick);
        if elapsed >= crate::interrupts::TIMER_HZ as u64 {
            let fps = self
                .fps_window_frames
                .saturating_mul(crate::interrupts::TIMER_HZ as u64)
                / elapsed.max(1);
            COMPOSITOR_FPS.store(fps, Ordering::Relaxed);
            COMPOSITOR_FRAME_TICKS_PEAK.store(self.frame_ticks_peak, Ordering::Relaxed);
            self.fps_window_frames = 0;
            self.fps_window_start_tick = now;
            self.frame_ticks_peak = 0;
        }
        COMPOSITOR_FRAME_TICKS_LAST.store(frame_ticks, Ordering::Relaxed);
        COMPOSITOR_DAMAGE_ROWS.store(self.damage_rows_last as u64, Ordering::Relaxed);
        COMPOSITOR_DAMAGE_PIXELS.store(self.damage_pixels_last as u64, Ordering::Relaxed);
        COMPOSITOR_FRAMES.store(self.damage_frames, Ordering::Relaxed);
    }

    fn compute_damage_spans(&mut self, sw: usize, sh: usize) {
        let total = sw.saturating_mul(sh);
        if self.prev_shadow.len() != total {
            self.prev_shadow.resize(total, 0);
            self.full_damage_next = true;
        }
        if self.damage_spans.len() != sh {
            self.damage_spans.resize(sh, (0, 0));
            self.full_damage_next = true;
        }
        if self.reported_damage_spans.len() != sh {
            self.reported_damage_spans.resize(sh, (0, 0));
        }

        let mut rows = 0usize;
        let mut pixels = 0usize;
        for row in 0..sh {
            let start_idx = row * sw;
            let end_idx = start_idx + sw;
            let span = if self.full_damage_next {
                (0, sw)
            } else {
                let cur = &self.shadow[start_idx..end_idx];
                let prev = &self.prev_shadow[start_idx..end_idx];
                let mut first = sw;
                let mut last = 0usize;
                for col in 0..sw {
                    if cur[col] != prev[col] {
                        if first == sw {
                            first = col;
                        }
                        last = col + 1;
                    }
                }
                if first == sw {
                    (0, 0)
                } else {
                    (first, last)
                }
            };
            let reported = self.reported_damage_spans[row];
            let span = merge_spans(span, reported);
            self.damage_spans[row] = span;
            if span.0 < span.1 {
                rows += 1;
                pixels += span.1 - span.0;
                self.prev_shadow[start_idx..end_idx]
                    .copy_from_slice(&self.shadow[start_idx..end_idx]);
            }
        }

        self.full_damage_next = false;
        self.damage_rows_last = rows;
        self.damage_pixels_last = pixels;
        self.damage_frames = self.damage_frames.saturating_add(1);
        if rows > 0 && self.damage_frames % 60 == 0 {
            crate::profiler::record(
                "compositor",
                "damage",
                &format!("rows={} pixels={}", rows, pixels),
            );
        }
    }

    fn collect_app_dirty_spans(&mut self, sw: usize, sh: usize) {
        if self.reported_damage_spans.len() != sh {
            self.reported_damage_spans.resize(sh, (0, 0));
        }
        for span in self.reported_damage_spans.iter_mut() {
            *span = (0, 0);
        }
        let current_workspace = self.current_workspace;
        for (idx, app) in self.windows.iter_mut().enumerate() {
            if self
                .window_workspaces
                .get(idx)
                .copied()
                .unwrap_or(0)
                .min(WORKSPACE_COUNT - 1)
                != current_workspace
                || app.window().minimized
            {
                continue;
            }
            let win_x = app.window().x;
            let win_y = app.window().y + TITLE_H;
            let rects = app.window_mut().take_dirty_regions();
            for rect in rects {
                let x0 = (win_x + rect.x).clamp(0, sw as i32) as usize;
                let x1 = (win_x + rect.x + rect.w).clamp(0, sw as i32) as usize;
                let y0 = (win_y + rect.y).clamp(0, sh as i32) as usize;
                let y1 = (win_y + rect.y + rect.h).clamp(0, sh as i32) as usize;
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                for row in y0..y1 {
                    self.reported_damage_spans[row] =
                        merge_spans(self.reported_damage_spans[row], (x0, x1));
                }
            }
        }
    }

    fn save_focused_screenshot(&self, path: &str) -> Result<(), &'static str> {
        let Some(win_idx) = self.focused else {
            return Err("no focused window");
        };
        let Some(app) = self.windows.get(win_idx) else {
            return Err("focused window missing");
        };
        let win = app.window();
        let width = win.width.max(1) as usize;
        let height = (win.height - TITLE_H).max(1) as usize;
        if win.buf.len() < width.saturating_mul(height) {
            return Err("window buffer incomplete");
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"P6\n");
        push_usize_bytes(&mut bytes, width);
        bytes.push(b' ');
        push_usize_bytes(&mut bytes, height);
        bytes.extend_from_slice(b"\n255\n");
        for pixel in win.buf.iter().take(width * height) {
            bytes.push(((pixel >> 16) & 0xFF) as u8);
            bytes.push(((pixel >> 8) & 0xFF) as u8);
            bytes.push((pixel & 0xFF) as u8);
        }

        let _ = crate::fat32::create_dir("/LOGS");
        match crate::fat32::create_file(path) {
            Ok(()) | Err(crate::fat32::FsError::AlreadyExists) => {}
            Err(err) => return Err(err.as_str()),
        }
        crate::fat32::write_file(path, &bytes).map_err(|err| err.as_str())
    }

    fn handle_dialog_click(&mut self, px: i32, py: i32, sw: i32, taskbar_y: i32) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        let (x, y, w, h) = shell_dialog_rect(sw, taskbar_y, &dialog);
        let button_y = y + h - 34;
        let button_h = 22;

        if dialog.kind == ShellDialogKind::Crash
            && py >= button_y
            && py < button_y + button_h
            && px >= x + 18
            && px < x + w - 18
        {
            let view_x = x + 18;
            let restart_x = view_x + 104;
            let copy_x = restart_x + 104;
            if px >= view_x && px < view_x + 94 {
                self.dialog = None;
                let off = self.windows.len() as i32 * 16;
                self.launch_app("Crash Viewer", x + off / 2, y + off / 2);
                return;
            }
            if px >= restart_x && px < restart_x + 94 {
                self.dialog = None;
                if let Some(target) = dialog.restart_target.as_ref() {
                    if target.starts_with('/') {
                        match crate::elf::spawn_elf_process(target) {
                            Ok(_) => {
                                crate::crashdump::record_restart(target);
                                crate::notifications::push("App restarted", target);
                            }
                            Err(err) => crate::notifications::push("Restart failed", err.as_str()),
                        }
                    } else {
                        crate::crashdump::record_restart(target);
                        self.launch_app(target, x + 18, y + 18);
                    }
                } else {
                    crate::notifications::push("Restart unavailable", "no app target recorded");
                }
                return;
            }
            if px >= copy_x && px < copy_x + 94 {
                crate::clipboard::set_text(&dialog.body);
                crate::notifications::push("Crash details copied", "details copied to clipboard");
                return;
            }
        }

        self.dialog = None;
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn front_to_back_hit(&self, px: i32, py: i32) -> Option<usize> {
        for z_pos in (0..self.z_order.len()).rev() {
            let wi = self.z_order[z_pos];
            if wi < self.windows.len()
                && self.is_window_on_current_workspace(wi)
                && !self.windows[wi].window().minimized
                && self.windows[wi].window().hit(px, py)
            {
                return Some(z_pos);
            }
        }
        None
    }

    /// Draw a single window — Windows 11 Dark Mode chrome.
    fn draw_window(
        s: &mut [u32],
        sw: usize,
        w: &Window,
        focused: bool,
        cursor_x: i32,
        cursor_y: i32,
    ) {
        // ── Drop shadow ───────────────────────────────────────────────────────
        // Six-sided soft shadow with smooth cubic falloff
        const SHADOW_R: i32 = 8;
        for d in 1..=SHADOW_R {
            let t = (SHADOW_R - d + 1) as u32;
            let alpha = (t * t * 3 / SHADOW_R as u32).min(48); // cubic falloff
            let shadow_col = alpha << 24;
            let sx = w.x + w.width + d - 1;
            let sy = w.y + w.height + d - 1;
            s_fill_alpha(s, sw, sx, w.y + d, 1, w.height, shadow_col);
            s_fill_alpha(s, sw, w.x + d, sy, w.width, 1, shadow_col);
            s_fill_alpha(s, sw, w.x - d, w.y + d, 1, w.height, shadow_col);
            s_fill_alpha(s, sw, w.x + d, w.y - d, w.width, 1, shadow_col);
        }

        if focused {
            // Outer dim glow ring (3px out)
            let outer_glow = blend_color(ACCENT, BLACK, 190);
            draw_rect_border(
                s,
                sw,
                w.x - 3,
                w.y - 3,
                w.width + 6,
                w.height + 6,
                outer_glow,
            );
            // Inner bright 2px border
            let glow = blend_color(ACCENT, BLACK, 100);
            draw_rect_border(s, sw, w.x - 2, w.y - 2, w.width + 4, w.height + 4, glow);
            draw_rect_border(
                s,
                sw,
                w.x - 1,
                w.y - 1,
                w.width + 2,
                w.height + 2,
                WIN_BDR_F,
            );
        }

        // ── Title bar — CRT phosphor chrome ──────────────────────────────────
        let title_bg = if focused { WIN_BAR_F } else { WIN_BAR_U };
        // 3-stop gradient: highlight → mid navy → base
        let title_top = if focused {
            blend_color(ACCENT, WIN_BAR_F, 180) // bright phosphor highlight
        } else {
            blend_color(WIN_BAR_F, WIN_BAR_U, 80)
        };
        let title_mid = if focused {
            blend_color(ACCENT, WIN_BAR_F, 230)
        } else {
            WIN_BAR_U
        };
        for row in 0..TITLE_H {
            let t = (row * 255 / TITLE_H.max(1)) as u32;
            let shade = if t < 128 {
                blend_color(title_top, title_mid, t * 2)
            } else {
                blend_color(title_mid, title_bg, (t - 128) * 2)
            };
            s_fill(s, sw, w.x, w.y + row, w.width, 1, shade);
        }

        // ── Window border ─────────────────────────────────────────────────────
        let bord = if focused { WIN_BDR_F } else { WIN_BDR_U };
        let bord_inner = if focused {
            blend_color(ACCENT, WIN_BAR_F, 210)
        } else {
            blend_color(WIN_BDR_U, BLACK, 160)
        };
        s_fill(s, sw, w.x - 1, w.y - 1, w.width + 2, 1, bord); // top outer
        s_fill(s, sw, w.x, w.y, w.width, 1, bord_inner); // top inner shine
        s_fill(s, sw, w.x - 1, w.y + w.height, w.width + 2, 1, bord); // bottom
        s_fill(s, sw, w.x - 1, w.y, 1, w.height, bord); // left
        s_fill(s, sw, w.x + w.width, w.y, 1, w.height, bord); // right

        // ── Title icon + text ─────────────────────────────────────────────────
        let accent = window_accent(w.title);
        let glyph = window_glyph(w.title);
        let title_content_y = w.y + 3;
        let title_content_h = TITLE_H - 3;
        let icon_x = w.x + 8;
        let icon_y = title_content_y + (title_content_h - 18) / 2;
        let icon_bg = blend_color(title_bg, accent, if focused { 120 } else { 75 });
        // Slightly taller icon badge to fill the taller title bar
        s_fill(s, sw, icon_x, icon_y, 18, 18, icon_bg);
        s_fill(s, sw, icon_x, icon_y, 18, 3, accent); // accent top stripe on badge
        draw_rect_border(
            s,
            sw,
            icon_x,
            icon_y,
            18,
            18,
            blend_color(accent, WHITE, 80),
        );
        s_draw_str_small(
            s,
            sw,
            icon_x + 4,
            icon_y + 5,
            glyph,
            accent,
            icon_bg,
            icon_x + 16,
        );

        let max_title_x = w.x + w.width - WIN_BTN_W * 3 - 10;
        let title_fg = if focused {
            0x00_CC_EE_FF
        } else {
            0x00_00_55_88
        };
        s_draw_str_small(
            s,
            sw,
            w.x + 34,
            title_content_y + (title_content_h - 8) / 2,
            w.title,
            title_fg,
            title_bg,
            max_title_x,
        );

        // ── Caption buttons — CRT phosphor style ──────────────────────────────
        let btn_y = w.y + 1;
        let btn_h = TITLE_H - 2;
        let cap_glyph_mid_y = btn_y + 3 + (btn_h - 3) / 2;

        // Hover detection — only fire when cursor is over this window's title row
        let in_btn_row = cursor_y >= btn_y && cursor_y < btn_y + btn_h;
        let min_x = w.x + w.width - WIN_BTN_W * 3;
        let max_x = w.x + w.width - WIN_BTN_W * 2;
        let cls_x = w.x + w.width - WIN_BTN_W;
        let hover_min = in_btn_row && cursor_x >= min_x && cursor_x < min_x + WIN_BTN_W;
        let hover_max = in_btn_row && cursor_x >= max_x && cursor_x < max_x + WIN_BTN_W;
        let hover_close = in_btn_row && cursor_x >= cls_x && cursor_x < cls_x + WIN_BTN_W;

        // Minimize  ─
        s_fill(
            s,
            sw,
            min_x,
            btn_y,
            WIN_BTN_W,
            btn_h,
            if hover_min { CAP_HOV } else { CAP_NORMAL },
        );
        let min_glyph = if hover_min { WHITE } else { 0x00_00_99_FF };
        s_fill(
            s,
            sw,
            min_x + WIN_BTN_W / 2 - 4,
            cap_glyph_mid_y,
            8,
            1,
            min_glyph,
        );

        // Maximize  □
        s_fill(
            s,
            sw,
            max_x,
            btn_y,
            WIN_BTN_W,
            btn_h,
            if hover_max { CAP_HOV } else { CAP_NORMAL },
        );
        let max_glyph = if hover_max { WHITE } else { 0x00_00_99_FF };
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 - 4,
            cap_glyph_mid_y - 4,
            8,
            1,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 - 4,
            cap_glyph_mid_y + 3,
            8,
            1,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 - 4,
            cap_glyph_mid_y - 4,
            1,
            8,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 + 3,
            cap_glyph_mid_y - 4,
            1,
            8,
            max_glyph,
        );

        // Close  ✕ — pixel diagonals
        let cx_c = cls_x + WIN_BTN_W / 2;
        let cy_c = cap_glyph_mid_y;
        let sh_wnd = s.len() / sw;
        s_fill(
            s,
            sw,
            cls_x,
            btn_y,
            WIN_BTN_W,
            btn_h,
            if hover_close { CLOSE_HOV } else { CLOSE_REST },
        );
        let cls_glyph = if hover_close { WHITE } else { 0x00_FF_44_44 };
        for i in -3i32..=3 {
            s_put(s, sw, sh_wnd, cx_c + i, cy_c + i, cls_glyph);
            s_put(s, sw, sh_wnd, cx_c + i + 1, cy_c + i, cls_glyph);
            s_put(s, sw, sh_wnd, cx_c + i, cy_c - i, cls_glyph);
            s_put(s, sw, sh_wnd, cx_c + i + 1, cy_c - i, cls_glyph);
        }

        // Accent stripe drawn last so it runs end-to-end over the caption buttons too
        if focused {
            s_fill(s, sw, w.x, w.y, w.width, 3, ACCENT); // 3px bright stripe
            s_fill(
                s,
                sw,
                w.x,
                w.y + 3,
                w.width,
                2,
                blend_color(ACCENT, WIN_BAR_F, 160),
            );
            s_fill(
                s,
                sw,
                w.x,
                w.y + 5,
                w.width,
                1,
                blend_color(ACCENT, WIN_BAR_F, 220),
            );
        }

        // ── Content area ──────────────────────────────────────────────────────
        let content_y = w.y + TITLE_H;
        let content_h = (w.height - TITLE_H).max(0) as usize;
        let cw = w.width as usize;
        let sh = s.len() / sw;
        let dst_x0 = w.x.max(0) as usize;
        let dst_x1 = (w.x + w.width).min(sw as i32).max(0) as usize;
        let dst_y0 = content_y.max(0) as usize;
        let dst_y1 = (content_y + content_h as i32).min(sh as i32).max(0) as usize;

        if dst_x0 < dst_x1 && dst_y0 < dst_y1 {
            let src_x0 = (dst_x0 as i32 - w.x) as usize;
            let src_y0 = (dst_y0 as i32 - content_y) as usize;
            let visible_w = dst_x1 - dst_x0;

            for dst_y in dst_y0..dst_y1 {
                let src_row = src_y0 + (dst_y - dst_y0);
                let row_start = src_row * cw + src_x0;
                let src = &w.buf[row_start..row_start + visible_w];
                let dst = &mut s[dst_y * sw + dst_x0..dst_y * sw + dst_x1];
                let dim_scanline = src_row % 3 == 2;

                if !src.contains(&WIN_TRANSPARENT) && !dim_scanline {
                    dst.copy_from_slice(src);
                    continue;
                }

                for (out, &pixel) in dst.iter_mut().zip(src.iter()) {
                    let base = if pixel == WIN_TRANSPARENT {
                        WIN_CONTENT
                    } else {
                        pixel
                    };
                    *out = if dim_scanline {
                        let r = ((base >> 16) & 0xFF) * 220 / 255;
                        let g = ((base >> 8) & 0xFF) * 220 / 255;
                        let b = (base & 0xFF) * 220 / 255;
                        (r << 16) | (g << 8) | b
                    } else {
                        base
                    };
                }
            }
        }

        // ── Scrollbar ─────────────────────────────────────────────────────────
        let view_h = content_h as i32;
        if w.scroll.needs_scrollbar(view_h) {
            let sb_x = w.x + w.width - SCROLLBAR_W;
            let track_h = view_h;
            // Track background
            s_fill(s, sw, sb_x, content_y, SCROLLBAR_W, track_h, 0x00_00_03_0A);
            // Left edge separator (2px with gradient)
            s_fill(s, sw, sb_x, content_y, 1, track_h, 0x00_00_22_44);
            s_fill(s, sw, sb_x + 1, content_y, 1, track_h, 0x00_00_0C_18);
            // Thumb
            let (thumb_y, thumb_h) = w.scroll.thumb_rect(view_h, track_h);
            let thumb_col = if focused {
                0x00_00_55_AA
            } else {
                0x00_00_28_55
            };
            let thumb_highlight = if focused {
                0x00_00_88_DD
            } else {
                0x00_00_44_77
            };
            // Thumb body
            s_fill(
                s,
                sw,
                sb_x + 2,
                content_y + thumb_y,
                SCROLLBAR_W - 4,
                thumb_h,
                thumb_col,
            );
            // Thumb center highlight stripe
            if thumb_h >= 4 {
                s_fill(
                    s,
                    sw,
                    sb_x + SCROLLBAR_W / 2 - 1,
                    content_y + thumb_y + 2,
                    2,
                    thumb_h - 4,
                    thumb_highlight,
                );
            }
            // Thumb top/bottom edge lines
            s_fill(
                s,
                sw,
                sb_x + 2,
                content_y + thumb_y,
                SCROLLBAR_W - 4,
                1,
                thumb_highlight,
            );
            s_fill(
                s,
                sw,
                sb_x + 2,
                content_y + thumb_y + thumb_h - 1,
                SCROLLBAR_W - 4,
                1,
                blend_color(thumb_col, BLACK, 80),
            );
        }

        // ── Resize handle — diagonal dot-grip in bottom-right corner ──────────
        {
            let hx = w.x + w.width - RESIZE_HANDLE;
            let hy = w.y + w.height - RESIZE_HANDLE;
            let gc = if focused {
                0x00_00_77_BB
            } else {
                0x00_00_2A_55
            };
            let gc_dim = if focused {
                0x00_00_33_66
            } else {
                0x00_00_14_28
            };
            // Four 2×2 dots — stair-stepping toward corner
            s_fill(s, sw, hx + 7, hy + 7, 2, 2, gc);
            s_fill(s, sw, hx + 5, hy + 7, 2, 2, gc_dim);
            s_fill(s, sw, hx + 7, hy + 5, 2, 2, gc_dim);
            s_fill(s, sw, hx + 3, hy + 7, 2, 2, gc_dim);
            s_fill(s, sw, hx + 7, hy + 3, 2, 2, gc_dim);
        }
    }
}

fn key_event_packet(c: char) -> [u8; EVENT_PACKET_SIZE] {
    let mut packet = [0u8; EVENT_PACKET_SIZE];
    let mut utf8 = [0u8; 4];
    let encoded = c.encode_utf8(&mut utf8);
    packet[0] = EVENT_KIND_KEY_CHAR;
    packet[1] = encoded.len() as u8;
    packet[2..2 + encoded.len()].copy_from_slice(encoded.as_bytes());
    packet
}

fn mouse_event_packet(buttons: u8, lx: i32, ly: i32) -> [u8; EVENT_PACKET_SIZE] {
    let mut packet = [0u8; EVENT_PACKET_SIZE];
    let x = lx.clamp(0, u16::MAX as i32) as u16;
    let y = ly.clamp(0, u16::MAX as i32) as u16;
    packet[0] = EVENT_KIND_MOUSE_DOWN;
    packet[1] = buttons;
    packet[2..4].copy_from_slice(&x.to_le_bytes());
    packet[4..6].copy_from_slice(&y.to_le_bytes());
    packet
}

fn ctx_submenu_entries(submenu: DesktopContextSubmenu) -> &'static [ContextEntryDef] {
    match submenu {
        DesktopContextSubmenu::View => CTX_VIEW_MENU,
        DesktopContextSubmenu::SortBy => CTX_SORT_MENU,
        DesktopContextSubmenu::New => CTX_NEW_MENU,
    }
}

fn ctx_entry_h(entry: ContextEntryDef) -> i32 {
    match entry.kind {
        ContextEntryKind::Separator => CTX_SEP_H,
        _ => CTX_ITEM_H,
    }
}

fn ctx_menu_height(entries: &[ContextEntryDef]) -> i32 {
    CTX_HEADER_H + CTX_PAD * 2 + entries.iter().map(|entry| ctx_entry_h(*entry)).sum::<i32>()
}

fn ctx_entry_y(entries: &[ContextEntryDef], menu_y: i32, target_idx: usize) -> i32 {
    let mut y = menu_y + CTX_HEADER_H + CTX_PAD;
    for (idx, entry) in entries.iter().enumerate() {
        if idx == target_idx {
            return y;
        }
        y += ctx_entry_h(*entry);
    }
    y
}

fn ctx_menu_hit_index(
    entries: &[ContextEntryDef],
    menu_x: i32,
    menu_y: i32,
    menu_w: i32,
    px: i32,
    py: i32,
) -> Option<usize> {
    if px < menu_x
        || px >= menu_x + menu_w
        || py < menu_y
        || py >= menu_y + ctx_menu_height(entries)
    {
        return None;
    }

    let mut y = menu_y + CTX_HEADER_H + CTX_PAD;
    for (idx, entry) in entries.iter().enumerate() {
        let h = ctx_entry_h(*entry);
        if py >= y && py < y + h {
            return match entry.kind {
                ContextEntryKind::Separator => None,
                _ => Some(idx),
            };
        }
        y += h;
    }
    None
}

fn ctx_submenu_rect(
    menu_x: i32,
    menu_y: i32,
    submenu: DesktopContextSubmenu,
    sw: i32,
    taskbar_y: i32,
) -> (i32, i32, i32, i32) {
    let parent_idx = DESKTOP_CONTEXT_MENU
        .iter()
        .position(|entry| entry.kind == ContextEntryKind::Submenu(submenu))
        .unwrap_or(0);
    let entries = ctx_submenu_entries(submenu);
    let h = ctx_menu_height(entries);
    let parent_y = ctx_entry_y(DESKTOP_CONTEXT_MENU, menu_y, parent_idx);
    let mut x = menu_x + CTX_W - 6;
    if x + CTX_SUB_W > sw {
        x = (menu_x - CTX_SUB_W + 6).max(0);
    }
    let mut y = (parent_y - 4).max(0);
    if y + h > taskbar_y {
        y = (taskbar_y - h).max(0);
    }
    (x, y, CTX_SUB_W, h)
}

fn create_root_item(
    prefix: &str,
    ext: Option<&str>,
    is_dir: bool,
) -> Result<String, crate::fat32::FsError> {
    let entries = crate::vfs::vfs_list_dir("/").unwrap_or_default();
    for n in 1..10_000usize {
        let mut name = String::from(prefix);
        push_decimal(&mut name, n as u64);
        if let Some(ext) = ext {
            name.push('.');
            name.push_str(ext);
        }
        if entries
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        let mut path = String::from("/");
        path.push_str(&name);
        if is_dir {
            crate::vfs::vfs_create_dir(&path)?;
        } else {
            crate::vfs::vfs_create_file(&path)?;
        }
        return Ok(path);
    }
    Err(crate::fat32::FsError::NoSpace)
}

fn push_decimal(out: &mut String, mut n: u64) {
    if n == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while n > 0 {
        digits[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        out.push(digits[idx] as char);
    }
}

#[derive(Clone, Copy)]
struct WallpaperPalette {
    tl: u32,
    tr: u32,
    bl: u32,
    br: u32,
    bloom: u32,
    star_tint: u32,
}

fn wallpaper_palette(preset: WallpaperPreset) -> WallpaperPalette {
    match preset {
        WallpaperPreset::Phosphor => WallpaperPalette {
            tl: DESK_TL,
            tr: DESK_TR,
            bl: DESK_BL,
            br: DESK_BR,
            bloom: BLOOM_1,
            star_tint: DESK_TR,
        },
        WallpaperPreset::Aurora => WallpaperPalette {
            tl: 0x00_00_05_0C,
            tr: 0x00_00_10_12,
            bl: 0x00_00_03_08,
            br: 0x00_00_08_10,
            bloom: 0x00_11_BB_AA,
            star_tint: 0x00_00_10_12,
        },
        WallpaperPreset::Midnight => WallpaperPalette {
            tl: 0x00_03_01_0C,
            tr: 0x00_02_02_10,
            bl: 0x00_01_00_08,
            br: 0x00_01_01_0A,
            bloom: 0x00_22_44_AA,
            star_tint: 0x00_02_02_10,
        },
    }
}

fn build_wallpaper(
    w: usize,
    taskbar_y: usize,
    preset: WallpaperPreset,
    show_progress: bool,
) -> Vec<u32> {
    let palette = wallpaper_palette(preset);
    if show_progress {
        crate::boot_splash::show(
            "allocating desktop buffers",
            15,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
    }
    let mut wallpaper = alloc::vec![0u32; w * crate::framebuffer::height()];
    if show_progress {
        crate::boot_splash::show(
            "painting desktop background",
            16,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
    }

    if w > 0 && taskbar_y > 0 {
        let (fw, fh) = (w as f32, taskbar_y as f32);
        let glow_mark = taskbar_y / 3;
        let scanline_mark = taskbar_y * 2 / 3;
        let mut glow_stage_shown = false;
        let mut scanline_stage_shown = false;
        for y in 0..taskbar_y {
            if show_progress && !glow_stage_shown && y >= glow_mark {
                crate::boot_splash::show(
                    "charging phosphor glow",
                    17,
                    crate::boot_splash::BOOT_PROGRESS_TOTAL,
                );
                glow_stage_shown = true;
            }
            if show_progress && !scanline_stage_shown && y >= scanline_mark {
                crate::boot_splash::show(
                    "laying scanlines",
                    18,
                    crate::boot_splash::BOOT_PROGRESS_TOTAL,
                );
                scanline_stage_shown = true;
            }

            let ty = y as f32 / fh;
            for x in 0..w {
                let tx = x as f32 / fw;
                let r = bilinear_u8(
                    (palette.tl >> 16) as u8,
                    (palette.tr >> 16) as u8,
                    (palette.bl >> 16) as u8,
                    (palette.br >> 16) as u8,
                    tx,
                    ty,
                );
                let g = bilinear_u8(
                    (palette.tl >> 8) as u8,
                    (palette.tr >> 8) as u8,
                    (palette.bl >> 8) as u8,
                    (palette.br >> 8) as u8,
                    tx,
                    ty,
                );
                let b = bilinear_u8(
                    palette.tl as u8,
                    palette.tr as u8,
                    palette.bl as u8,
                    palette.br as u8,
                    tx,
                    ty,
                );
                let dx = tx - 0.50;
                let dy = ty - 0.45;
                let dist_sq = dx * dx + dy * dy;
                let t_b = 1.0f32 - (dist_sq / 0.20f32).min(1.0f32);
                let bloom = t_b * t_b * t_b * 1.4f32;
                let br =
                    (r as f32 + bloom * ((palette.bloom >> 16) as u8 as f32)).min(255.0) as u32;
                let bg = (g as f32 + bloom * ((palette.bloom >> 8) as u8 as f32)).min(255.0) as u32;
                let bb = (b as f32 + bloom * (palette.bloom as u8 as f32)).min(255.0) as u32;

                let scan: u32 = match y % 3 {
                    0 => 255,
                    1 => 210,
                    _ => 175,
                };
                let dot_boost: u32 = if x % 3 == 2 { 14 } else { 0 };
                let fr = br * scan / 255;
                let fg = bg * scan / 255;
                let fb = (bb * scan / 255).saturating_add(dot_boost).min(255);
                wallpaper[y * w + x] = (fr << 16) | (fg << 8) | fb;
            }
        }
    }

    if show_progress {
        crate::boot_splash::show(
            "finishing wallpaper",
            19,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
    }

    if taskbar_y > 0 && w > 0 {
        if show_progress {
            crate::boot_splash::show(
                "placing starfield",
                20,
                crate::boot_splash::BOOT_PROGRESS_TOTAL,
            );
        }
        let mut seed: u32 = match preset {
            WallpaperPreset::Phosphor => 0xC001_D00D,
            WallpaperPreset::Aurora => 0xA11E_7A1A,
            WallpaperPreset::Midnight => 0x0B5C_0DED,
        };
        let star_count = ((w * taskbar_y) / 12_000).max(48);
        for _ in 0..star_count {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let sx = (seed as usize) % w;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let sy = (seed as usize) % taskbar_y;
            let core = if seed & 3 == 0 {
                0x00_FF_FF_FF
            } else if seed & 3 == 1 {
                0x00_EE_FF_FF
            } else {
                0x00_88_CC_FF
            };
            let glow = blend_color(core, palette.star_tint, 160);
            let dim_glow = blend_color(core, palette.star_tint, 210);
            wallpaper[sy * w + sx] = core;
            if sx + 1 < w {
                wallpaper[sy * w + sx + 1] = glow;
            }
            if sx > 0 {
                wallpaper[sy * w + sx - 1] = dim_glow;
            }
            if sy + 1 < taskbar_y {
                wallpaper[(sy + 1) * w + sx] = glow;
            }
            if sy > 0 {
                wallpaper[(sy - 1) * w + sx] = dim_glow;
            }
        }
    }

    wallpaper
}

fn draw_desktop_context_menu(
    s: &mut [u32],
    sw: usize,
    cm: &ContextMenu,
    mx: i32,
    my: i32,
    show_desktop_icons: bool,
    compact_spacing: bool,
    desktop_sort: DesktopSortMode,
    screen_w: i32,
    taskbar_y: i32,
) {
    draw_context_panel(
        s,
        sw,
        cm.x,
        cm.y,
        CTX_W,
        DESKTOP_CONTEXT_MENU,
        ctx_menu_hit_index(DESKTOP_CONTEXT_MENU, cm.x, cm.y, CTX_W, mx, my),
        None,
        show_desktop_icons,
        compact_spacing,
        desktop_sort,
    );

    if let Some(submenu) = cm.submenu {
        let (sub_x, sub_y, sub_w, _sub_h) =
            ctx_submenu_rect(cm.x, cm.y, submenu, screen_w, taskbar_y);
        let entries = ctx_submenu_entries(submenu);
        draw_context_panel(
            s,
            sw,
            sub_x,
            sub_y,
            sub_w,
            entries,
            ctx_menu_hit_index(entries, sub_x, sub_y, sub_w, mx, my),
            Some(submenu),
            show_desktop_icons,
            compact_spacing,
            desktop_sort,
        );
    }
}

fn draw_context_panel(
    s: &mut [u32],
    sw: usize,
    menu_x: i32,
    menu_y: i32,
    menu_w: i32,
    entries: &[ContextEntryDef],
    hovered: Option<usize>,
    submenu: Option<DesktopContextSubmenu>,
    show_desktop_icons: bool,
    compact_spacing: bool,
    desktop_sort: DesktopSortMode,
) {
    let menu_h = ctx_menu_height(entries);
    let bg = 0x00_00_09_1E;
    let bg_inner = 0x00_00_07_18;
    let border = ACCENT;
    let inner = 0x00_00_33_55;
    let top_glow = 0x00_00_14_30;
    let hover_bg = 0x00_00_12_2C;
    let hover_border = 0x00_00_66_99;
    let text = 0x00_88_CC_FF;
    let text_hot = WHITE;
    let muted = 0x00_44_77_99;
    let sep = 0x00_00_1A_33;

    s_fill(s, sw, menu_x + 4, menu_y + 4, menu_w, menu_h, 0x00_00_00_24);
    s_fill(s, sw, menu_x, menu_y, menu_w, menu_h, bg);
    draw_rect_border(s, sw, menu_x, menu_y, menu_w, menu_h, border);
    draw_rect_border(s, sw, menu_x + 1, menu_y + 1, menu_w - 2, menu_h - 2, inner);
    s_fill(s, sw, menu_x + 1, menu_y + 1, menu_w - 2, 3, top_glow);
    s_fill(
        s,
        sw,
        menu_x + 1,
        menu_y + menu_h - 2,
        menu_w - 2,
        1,
        0x00_00_05_10,
    );

    let mut row_y = menu_y + CTX_HEADER_H + CTX_PAD;
    for (idx, entry) in entries.iter().enumerate() {
        match entry.kind {
            ContextEntryKind::Separator => {
                s_fill(
                    s,
                    sw,
                    menu_x + 14,
                    row_y + CTX_SEP_H / 2,
                    menu_w - 28,
                    1,
                    sep,
                );
                row_y += CTX_SEP_H;
            }
            _ => {
                let hot = hovered == Some(idx) && entry.enabled;
                let text_y = row_y + (CTX_ITEM_H - 8) / 2;
                let mark_y = row_y + (CTX_ITEM_H - 8) / 2 - 1;
                if hot {
                    s_fill(s, sw, menu_x + 2, row_y, menu_w - 4, CTX_ITEM_H, hover_bg);
                    draw_rect_border(
                        s,
                        sw,
                        menu_x + 2,
                        row_y,
                        menu_w - 4,
                        CTX_ITEM_H,
                        hover_border,
                    );
                    s_fill(s, sw, menu_x + 2, row_y + 4, 3, CTX_ITEM_H - 8, ACCENT);
                }

                if let Some(mark) = ctx_menu_mark(
                    entry.kind,
                    submenu,
                    show_desktop_icons,
                    compact_spacing,
                    desktop_sort,
                ) {
                    match mark {
                        MenuMark::Check => draw_menu_check(s, sw, menu_x + 8, mark_y, ACCENT_HOV),
                        MenuMark::Dot => s_fill(s, sw, menu_x + 11, mark_y + 2, 5, 5, ACCENT_HOV),
                    }
                }

                let fg = if !entry.enabled {
                    muted
                } else if hot {
                    text_hot
                } else {
                    text
                };
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 24,
                    text_y,
                    entry.label,
                    fg,
                    if hot { hover_bg } else { bg_inner },
                    menu_x + menu_w - 18,
                );

                if let ContextEntryKind::Submenu(_) = entry.kind {
                    draw_menu_chevron(
                        s,
                        sw,
                        menu_x + menu_w - 16,
                        text_y,
                        if !entry.enabled {
                            muted
                        } else if hot {
                            text_hot
                        } else {
                            text
                        },
                    );
                }

                row_y += CTX_ITEM_H;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MenuMark {
    Check,
    Dot,
}

fn ctx_menu_mark(
    kind: ContextEntryKind,
    submenu: Option<DesktopContextSubmenu>,
    show_desktop_icons: bool,
    compact_spacing: bool,
    desktop_sort: DesktopSortMode,
) -> Option<MenuMark> {
    match (submenu, kind) {
        (
            Some(DesktopContextSubmenu::View),
            ContextEntryKind::Action(DesktopContextCommand::ToggleDesktopIcons),
        ) if show_desktop_icons => Some(MenuMark::Check),
        (
            Some(DesktopContextSubmenu::View),
            ContextEntryKind::Action(DesktopContextCommand::ToggleCompactSpacing),
        ) if compact_spacing => Some(MenuMark::Check),
        (
            Some(DesktopContextSubmenu::SortBy),
            ContextEntryKind::Action(DesktopContextCommand::SortByName),
        ) if desktop_sort == DesktopSortMode::Name => Some(MenuMark::Dot),
        (
            Some(DesktopContextSubmenu::SortBy),
            ContextEntryKind::Action(DesktopContextCommand::SortByType),
        ) if desktop_sort == DesktopSortMode::Type => Some(MenuMark::Dot),
        _ => None,
    }
}

fn draw_menu_chevron(s: &mut [u32], sw: usize, x: i32, y: i32, color: u32) {
    s_fill(s, sw, x, y, 1, 2, color);
    s_fill(s, sw, x + 1, y + 1, 1, 2, color);
    s_fill(s, sw, x + 2, y + 2, 1, 2, color);
    s_fill(s, sw, x + 1, y + 3, 1, 2, color);
    s_fill(s, sw, x, y + 4, 1, 2, color);
}

fn draw_menu_check(s: &mut [u32], sw: usize, x: i32, y: i32, color: u32) {
    s_fill(s, sw, x, y + 3, 2, 2, color);
    s_fill(s, sw, x + 2, y + 5, 2, 2, color);
    s_fill(s, sw, x + 4, y + 3, 2, 2, color);
    s_fill(s, sw, x + 6, y + 1, 2, 2, color);
}

fn draw_task_switcher_overlay(
    s: &mut [u32],
    sw: usize,
    taskbar_y: i32,
    windows: &[AppWindow],
    window_workspaces: &[usize],
    z_order: &[usize],
    focused: Option<usize>,
    current_workspace: usize,
    query: &str,
) {
    let query_lower = query.to_ascii_lowercase();
    let visible_count = z_order
        .iter()
        .rev()
        .filter(|&&idx| {
            idx < windows.len()
                && window_workspaces
                    .get(idx)
                    .copied()
                    .unwrap_or(0)
                    .min(WORKSPACE_COUNT - 1)
                    == current_workspace
                && !windows[idx].window().minimized
                && (query_lower.is_empty()
                    || windows[idx]
                        .window()
                        .title
                        .to_ascii_lowercase()
                        .contains(&query_lower))
        })
        .count();
    if visible_count == 0 {
        return;
    }

    let shown = visible_count.min(6);
    let item_w = 126i32;
    let item_h = 64i32;
    let gap = 10i32;
    let panel_w = 32 + shown as i32 * item_w + (shown.saturating_sub(1)) as i32 * gap;
    let panel_h = 112i32;
    let panel_x = ((sw as i32 - panel_w) / 2).max(0);
    let panel_y = ((taskbar_y - panel_h) / 2).max(0);

    s_fill_alpha(
        s,
        sw,
        panel_x - 6,
        panel_y - 6,
        panel_w + 12,
        panel_h + 12,
        0x44_00_00_00,
    );
    s_fill(s, sw, panel_x, panel_y, panel_w, panel_h, 0x00_00_07_18);
    s_fill(s, sw, panel_x, panel_y, panel_w, 3, ACCENT);
    draw_rect_border(s, sw, panel_x, panel_y, panel_w, panel_h, 0x00_00_66_BB);
    draw_rect_border(
        s,
        sw,
        panel_x + 1,
        panel_y + 1,
        panel_w - 2,
        panel_h - 2,
        0x00_00_22_44,
    );
    s_draw_str_small(
        s,
        sw,
        panel_x + 16,
        panel_y + 12,
        "TASK SWITCHER",
        0x00_CC_EE_FF,
        0x00_00_07_18,
        panel_x + panel_w - 16,
    );
    s_draw_str_small(
        s,
        sw,
        panel_x + 16,
        panel_y + 26,
        "Alt+Tab cycles windows",
        0x00_55_88_AA,
        0x00_00_07_18,
        panel_x + panel_w - 16,
    );
    if !query.is_empty() {
        let mut search = String::from("Search: ");
        search.push_str(query);
        s_draw_str_small(
            s,
            sw,
            panel_x + 170,
            panel_y + 26,
            &search,
            ACCENT_HOV,
            0x00_00_07_18,
            panel_x + panel_w - 16,
        );
    }

    let mut drawn = 0usize;
    for &win_idx in z_order.iter().rev() {
        if drawn >= shown {
            break;
        }
        if win_idx >= windows.len()
            || window_workspaces
                .get(win_idx)
                .copied()
                .unwrap_or(0)
                .min(WORKSPACE_COUNT - 1)
                != current_workspace
            || windows[win_idx].window().minimized
            || (!query_lower.is_empty()
                && !windows[win_idx]
                    .window()
                    .title
                    .to_ascii_lowercase()
                    .contains(&query_lower))
        {
            continue;
        }

        let win = windows[win_idx].window();
        let x = panel_x + 16 + drawn as i32 * (item_w + gap);
        let y = panel_y + 42;
        let selected = focused == Some(win_idx);
        let accent = window_accent(win.title);
        let bg = if selected {
            0x00_00_1E_3C
        } else {
            0x00_00_0B_20
        };
        let border = if selected { ACCENT_HOV } else { 0x00_00_33_66 };

        s_fill(s, sw, x, y, item_w, item_h, bg);
        s_fill(s, sw, x, y, item_w, 3, accent);
        draw_rect_border(s, sw, x, y, item_w, item_h, border);
        draw_live_window_thumbnail(s, sw, x + 8, y + 10, 38, 28, win);

        let icon_bg = blend_color(bg, accent, 90);
        s_fill(s, sw, x + 12, y + 42, 22, 14, icon_bg);
        draw_rect_border(
            s,
            sw,
            x + 12,
            y + 42,
            22,
            14,
            blend_color(accent, WHITE, 90),
        );
        s_draw_str_small(
            s,
            sw,
            x + 15,
            y + 45,
            window_glyph(win.title),
            accent,
            icon_bg,
            x + 32,
        );

        let title = if win.title.len() > 11 {
            &win.title[..11]
        } else {
            win.title
        };
        s_draw_str_small(
            s,
            sw,
            x + 48,
            y + 18,
            title,
            if selected { WHITE } else { 0x00_88_CC_FF },
            bg,
            x + item_w - 8,
        );
        s_draw_str_small(
            s,
            sw,
            x + 48,
            y + 34,
            if selected { "active" } else { "window" },
            if selected { ACCENT_HOV } else { 0x00_44_77_99 },
            bg,
            x + item_w - 8,
        );

        drawn += 1;
    }
}

fn draw_file_drag_badge(s: &mut [u32], sw: usize, x: i32, y: i32, count: usize) {
    let w = 132i32;
    let h = 34i32;
    let x = x.min(sw as i32 - w - 4).max(4);
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    let y = y.min(sh as i32 - h - 4).max(4);
    let bg = 0x00_00_08_18;
    s_fill_alpha(s, sw, x + 4, y + 4, w, h, 0x44_00_00_00);
    s_fill(s, sw, x, y, w, h, bg);
    s_fill(s, sw, x, y, 3, h, ACCENT);
    draw_rect_border(s, sw, x, y, w, h, 0x00_00_66_BB);
    s_draw_str_small(s, sw, x + 12, y + 7, "DROP FILES", WHITE, bg, x + w - 8);
    let text = if count == 1 {
        String::from("1 item")
    } else {
        format!("{} items", count)
    };
    s_draw_str_small(s, sw, x + 12, y + 19, &text, 0x00_66_AA_DD, bg, x + w - 8);
}

fn draw_live_window_thumbnail(
    s: &mut [u32],
    sw: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    win: &Window,
) {
    s_fill(s, sw, x, y, w, h, 0x00_00_04_10);
    draw_rect_border(s, sw, x, y, w, h, 0x00_00_44_88);
    let src_w = win.width.max(1) as usize;
    let src_h = (win.height - TITLE_H).max(1) as usize;
    if win.buf.is_empty() {
        return;
    }
    for ty in 1..h.saturating_sub(1) {
        let sy = (ty as usize * src_h / h.max(1) as usize).min(src_h - 1);
        for tx in 1..w.saturating_sub(1) {
            let sx = (tx as usize * src_w / w.max(1) as usize).min(src_w - 1);
            let src = sy * src_w + sx;
            if src < win.buf.len() {
                s_put(s, sw, usize::MAX, x + tx, y + ty, win.buf[src]);
            }
        }
    }
}

fn draw_shell_dialog(s: &mut [u32], sw: usize, taskbar_y: i32, dialog: &ShellDialog) {
    let (x, y, w, h) = shell_dialog_rect(sw as i32, taskbar_y, dialog);
    let bg = 0x00_07_0D_1C;
    s_fill_alpha(s, sw, 0, 0, sw as i32, taskbar_y, 0x44_00_00_00);
    s_fill_alpha(s, sw, x + 6, y + 6, w, h, 0x55_00_00_00);
    s_fill(s, sw, x, y, w, h, bg);
    s_fill(s, sw, x, y, w, 4, 0x00_FF_66_66);
    draw_rect_border(s, sw, x, y, w, h, 0x00_FF_88_88);
    s_draw_str_small(s, sw, x + 18, y + 18, &dialog.title, WHITE, bg, x + w - 18);
    s_draw_str_small(
        s,
        sw,
        x + 18,
        y + 44,
        &dialog.body,
        0x00_CC_EE_FF,
        bg,
        x + w - 18,
    );
    s_draw_str_small(
        s,
        sw,
        x + 18,
        y + if dialog.kind == ShellDialogKind::Crash {
            h - 56
        } else {
            h - 26
        },
        if dialog.kind == ShellDialogKind::Crash {
            "app failure captured"
        } else {
            "click anywhere to dismiss"
        },
        0x00_66_99_BB,
        bg,
        x + w - 18,
    );
    if dialog.kind == ShellDialogKind::Crash {
        let button_y = y + h - 34;
        draw_dialog_button(s, sw, x + 18, button_y, "View Dump", 0x00_FF_88_88);
        draw_dialog_button(s, sw, x + 122, button_y, "Restart", 0x00_FF_DD_55);
        draw_dialog_button(s, sw, x + 226, button_y, "Copy", 0x00_55_FF_BB);
        draw_dialog_button(s, sw, x + w - 112, button_y, "Dismiss", 0x00_66_AA_DD);
    }
}

fn shell_dialog_rect(sw: i32, taskbar_y: i32, dialog: &ShellDialog) -> (i32, i32, i32, i32) {
    let w = 460i32;
    let h = if dialog.kind == ShellDialogKind::Crash {
        168i32
    } else {
        132i32
    };
    let x = ((sw - w) / 2).max(8);
    let y = ((taskbar_y - h) / 2).max(8);
    (x, y, w, h)
}

fn draw_dialog_button(s: &mut [u32], sw: usize, x: i32, y: i32, label: &str, accent: u32) {
    let w = 94i32;
    let h = 22i32;
    let bg = 0x00_00_0C_20;
    s_fill(s, sw, x, y, w, h, bg);
    s_fill(s, sw, x, y, w, 2, accent);
    draw_rect_border(s, sw, x, y, w, h, blend_color(accent, 0x00_00_08_18, 90));
    let text_w = label.chars().count() as i32 * 8;
    s_draw_str_small(
        s,
        sw,
        x + ((w - text_w) / 2).max(4),
        y + 7,
        label,
        0x00_DD_FF_FF,
        bg,
        x + w - 4,
    );
}

fn draw_taskbar_preview(s: &mut [u32], sw: usize, taskbar_y: i32, button_x: i32, app: &AppWindow) {
    let win = app.window();
    let preview_w = 208i32;
    let preview_h = 74i32;
    let x = (button_x + BUTTON_W / 2 - preview_w / 2)
        .max(4)
        .min(sw as i32 - preview_w - 4);
    let y = (taskbar_y - preview_h - 8).max(0);
    let accent = window_accent(win.title);
    s_fill_alpha(s, sw, x + 5, y + 5, preview_w, preview_h, 0x40_00_00_00);
    s_fill(s, sw, x, y, preview_w, preview_h, 0x00_00_08_18);
    s_fill(s, sw, x, y, preview_w, 3, accent);
    draw_rect_border(s, sw, x, y, preview_w, preview_h, 0x00_00_66_BB);
    draw_live_window_thumbnail(s, sw, x + 10, y + 16, 92, 40, win);
    let icon_bg = blend_color(0x00_00_08_18, accent, 90);
    s_fill(s, sw, x + 116, y + 16, 32, 32, icon_bg);
    draw_rect_border(
        s,
        sw,
        x + 116,
        y + 16,
        32,
        32,
        blend_color(accent, WHITE, 88),
    );
    s_draw_str_small(
        s,
        sw,
        x + 123,
        y + 28,
        window_glyph(win.title),
        accent,
        icon_bg,
        x + 146,
    );
    s_draw_str_small(
        s,
        sw,
        x + 10,
        y + 7,
        win.title,
        WHITE,
        0x00_00_08_18,
        x + 108,
    );
    let state = if win.minimized { "minimized" } else { "open" };
    s_draw_str_small(
        s,
        sw,
        x + 116,
        y + 32,
        state,
        0x00_66_AA_DD,
        0x00_00_08_18,
        x + preview_w - 10,
    );
    let bounds = format!("{}x{} @ {},{}", win.width, win.height, win.x, win.y);
    s_draw_str_small(
        s,
        sw,
        x + 10,
        y + 60,
        &bounds,
        0x00_44_88_BB,
        0x00_00_08_18,
        x + preview_w - 10,
    );
}

fn draw_taskbar_menu(
    s: &mut [u32],
    sw: usize,
    menu: &TaskbarMenu,
    windows: &[AppWindow],
    mx: i32,
    my: i32,
) {
    if menu.window >= windows.len() {
        return;
    }
    let bg = 0x00_00_08_18;
    s_fill_alpha(
        s,
        sw,
        menu.x + 4,
        menu.y + 4,
        TASKBAR_MENU_W,
        TASKBAR_MENU_H,
        0x44_00_00_00,
    );
    s_fill(s, sw, menu.x, menu.y, TASKBAR_MENU_W, TASKBAR_MENU_H, bg);
    s_fill(s, sw, menu.x, menu.y, TASKBAR_MENU_W, 3, ACCENT);
    draw_rect_border(
        s,
        sw,
        menu.x,
        menu.y,
        TASKBAR_MENU_W,
        TASKBAR_MENU_H,
        0x00_00_66_BB,
    );
    let labels = [
        if windows[menu.window].is_minimized() {
            "Restore"
        } else {
            "Minimize"
        },
        "Maximize",
        "Close",
    ];
    for (i, label) in labels.iter().enumerate() {
        let row_y = menu.y + 5 + i as i32 * TASKBAR_MENU_ROW_H;
        let hot = mx >= menu.x + 4
            && mx < menu.x + TASKBAR_MENU_W - 4
            && my >= row_y
            && my < row_y + TASKBAR_MENU_ROW_H;
        let row_bg = if hot { 0x00_00_18_34 } else { bg };
        if hot {
            s_fill(
                s,
                sw,
                menu.x + 3,
                row_y,
                TASKBAR_MENU_W - 6,
                TASKBAR_MENU_ROW_H,
                row_bg,
            );
            s_fill(
                s,
                sw,
                menu.x + 4,
                row_y + 5,
                2,
                TASKBAR_MENU_ROW_H - 10,
                ACCENT,
            );
        }
        s_draw_str_small(
            s,
            sw,
            menu.x + 14,
            row_y + 8,
            label,
            if hot { WHITE } else { 0x00_88_CC_FF },
            row_bg,
            menu.x + TASKBAR_MENU_W - 10,
        );
    }
}

fn draw_notification_center(s: &mut [u32], sw: usize, taskbar_y: i32) {
    let panel_w = 340i32;
    let panel_h = 260i32;
    let x = (sw as i32 - panel_w - 12).max(0);
    let y = (taskbar_y - panel_h - 10).max(0);
    let bg = 0x00_00_08_18;
    s_fill_alpha(s, sw, x + 5, y + 5, panel_w, panel_h, 0x50_00_00_00);
    s_fill(s, sw, x, y, panel_w, panel_h, bg);
    s_fill(s, sw, x, y, panel_w, 3, ACCENT);
    draw_rect_border(s, sw, x, y, panel_w, panel_h, 0x00_00_66_BB);
    s_draw_str_small(
        s,
        sw,
        x + 14,
        y + 14,
        "NOTIFICATIONS",
        WHITE,
        bg,
        x + panel_w - 12,
    );
    s_draw_str_small(
        s,
        sw,
        x + 14,
        y + 28,
        "Ctrl+Alt+M toggles this panel",
        0x00_55_88_AA,
        bg,
        x + panel_w - 12,
    );
    let list = crate::notifications::latest(7);
    if list.is_empty() {
        s_draw_str_small(
            s,
            sw,
            x + 14,
            y + 62,
            "No system events yet.",
            0x00_66_AA_DD,
            bg,
            x + panel_w - 12,
        );
        return;
    }
    for (i, note) in list.iter().enumerate() {
        let row_y = y + 54 + i as i32 * 28;
        let row_bg = if note.unread {
            0x00_00_18_34
        } else {
            0x00_00_0C_20
        };
        s_fill(s, sw, x + 10, row_y, panel_w - 20, 24, row_bg);
        if note.unread {
            s_fill(s, sw, x + 10, row_y + 5, 2, 14, ACCENT_HOV);
        }
        s_draw_str_small(
            s,
            sw,
            x + 18,
            row_y + 4,
            &note.title,
            if note.unread { WHITE } else { 0x00_AA_DD_FF },
            row_bg,
            x + panel_w - 18,
        );
        s_draw_str_small(
            s,
            sw,
            x + 18,
            row_y + 15,
            &note.body,
            0x00_66_AA_DD,
            row_bg,
            x + panel_w - 18,
        );
    }
}

fn draw_notification_toasts(s: &mut [u32], sw: usize, taskbar_y: i32, ticks: u64) {
    let list = crate::notifications::latest(2);
    if list.is_empty() {
        return;
    }
    let timeout = crate::interrupts::ticks_for_millis(6500);
    let toast_w = 294i32;
    let toast_h = 50i32;
    let mut drawn = 0i32;
    for note in list.iter() {
        if ticks.wrapping_sub(note.tick) > timeout {
            continue;
        }
        let x = (sw as i32 - toast_w - 12).max(0);
        let y = (taskbar_y - 12 - (drawn + 1) * (toast_h + 8)).max(0);
        let bg = 0x00_00_08_18;
        s_fill_alpha(s, sw, x + 4, y + 4, toast_w, toast_h, 0x44_00_00_00);
        s_fill(s, sw, x, y, toast_w, toast_h, bg);
        s_fill(s, sw, x, y, 3, toast_h, ACCENT);
        draw_rect_border(s, sw, x, y, toast_w, toast_h, 0x00_00_66_BB);
        s_draw_str_small(
            s,
            sw,
            x + 12,
            y + 10,
            &note.title,
            WHITE,
            bg,
            x + toast_w - 10,
        );
        s_draw_str_small(
            s,
            sw,
            x + 12,
            y + 26,
            &note.body,
            0x00_66_AA_DD,
            bg,
            x + toast_w - 10,
        );
        drawn += 1;
    }
}

fn draw_launcher_overlay(s: &mut [u32], sw: usize, taskbar_y: i32, state: &LauncherState) {
    let panel_w = 680i32.min(sw as i32 - 24);
    let panel_h = 340i32;
    let x = ((sw as i32 - panel_w) / 2).max(0);
    let y = ((taskbar_y - panel_h) / 4).max(8);
    let bg = 0x00_00_08_18;
    s_fill_alpha(s, sw, 0, 0, sw as i32, taskbar_y, 0x28_00_00_00);
    s_fill_alpha(s, sw, x + 7, y + 7, panel_w, panel_h, 0x55_00_00_00);
    s_fill(s, sw, x, y, panel_w, panel_h, bg);
    s_fill(s, sw, x, y, panel_w, 3, ACCENT);
    draw_rect_border(s, sw, x, y, panel_w, panel_h, 0x00_00_77_CC);
    s_draw_str_small(
        s,
        sw,
        x + 16,
        y + 14,
        "LAUNCHER / COMMAND PALETTE",
        WHITE,
        bg,
        x + panel_w - 16,
    );
    s_draw_str_small(
        s,
        sw,
        x + panel_w - 178,
        y + 14,
        "Ctrl+Space  > commands",
        0x00_55_88_AA,
        bg,
        x + panel_w - 16,
    );

    let search_x = x + 16;
    let search_y = y + 38;
    let search_h = 30i32;
    s_fill(
        s,
        sw,
        search_x,
        search_y,
        panel_w - 32,
        search_h,
        0x00_00_03_0C,
    );
    draw_rect_border(
        s,
        sw,
        search_x,
        search_y,
        panel_w - 32,
        search_h,
        0x00_00_66_BB,
    );
    let query = if state.query.is_empty() {
        "type app, file, command, @category, or > reboot..."
    } else {
        &state.query
    };
    s_draw_str_small(
        s,
        sw,
        search_x + 12,
        search_y + 11,
        query,
        if state.query.is_empty() {
            0x00_44_77_99
        } else {
            WHITE
        },
        0x00_00_03_0C,
        search_x + panel_w - 44,
    );

    let matches = launcher_matches(&state.query);
    if matches.is_empty() {
        s_draw_str_small(
            s,
            sw,
            x + 18,
            y + 88,
            "No matching app, file, or command.",
            0x00_66_AA_DD,
            bg,
            x + panel_w - 18,
        );
        return;
    }

    let max_rows = 8usize.min(matches.len());
    for i in 0..max_rows {
        let entry = &matches[i];
        let row_y = y + 82 + i as i32 * 29;
        let selected = i == state.selected.min(matches.len() - 1);
        let row_bg = if selected { 0x00_00_1A_38 } else { bg };
        if selected {
            s_fill(s, sw, x + 10, row_y, panel_w - 20, 25, row_bg);
            s_fill(s, sw, x + 10, row_y + 5, 3, 14, ACCENT);
        }
        let glyph = launcher_kind_glyph(&entry.kind);
        s_draw_str_small(
            s,
            sw,
            x + 22,
            row_y + 8,
            glyph,
            if selected {
                launcher_kind_accent(&entry.kind)
            } else {
                0x00_55_88_AA
            },
            row_bg,
            x + 48,
        );
        s_draw_str_small(
            s,
            sw,
            x + 54,
            row_y + 4,
            &entry.label,
            if selected { WHITE } else { 0x00_AA_DD_FF },
            row_bg,
            x + panel_w - 210,
        );
        s_draw_str_small(
            s,
            sw,
            x + 54,
            row_y + 15,
            &entry.detail,
            0x00_55_88_AA,
            row_bg,
            x + panel_w - 210,
        );
        if selected {
            s_draw_str_small(
                s,
                sw,
                x + panel_w - 202,
                row_y + 8,
                "Enter open  C-P pin  C-L loc  C-C copy",
                0x00_44_88_AA,
                row_bg,
                x + panel_w - 18,
            );
        }
    }
}

fn launcher_kind_glyph(kind: &LauncherMatchKind) -> &'static str {
    match kind {
        LauncherMatchKind::App(app) => window_glyph(app),
        LauncherMatchKind::Path(_) => "FS",
        LauncherMatchKind::Command(_) => "$>",
        LauncherMatchKind::Inline(action) if action.starts_with("settings:") => "DS",
        LauncherMatchKind::Inline(action) if action.starts_with("category:") => "AP",
        LauncherMatchKind::Inline(action) if action == "refresh-index" => "IX",
        LauncherMatchKind::Inline(action)
            if action == "shutdown" || action == "reboot" || action == "sleep" =>
        {
            "PW"
        }
        LauncherMatchKind::Inline(_) => "!!",
    }
}

fn launcher_kind_accent(kind: &LauncherMatchKind) -> u32 {
    match kind {
        LauncherMatchKind::App(app) => window_accent(app),
        LauncherMatchKind::Path(_) => 0x00_55_DD_FF,
        LauncherMatchKind::Command(_) => 0x00_00_FF_88,
        LauncherMatchKind::Inline(action) if action.starts_with("settings:") => 0x00_66_CC_FF,
        LauncherMatchKind::Inline(action)
            if action == "shutdown" || action == "reboot" || action == "sleep" =>
        {
            0x00_FF_DD_55
        }
        LauncherMatchKind::Inline(_) => ACCENT_HOV,
    }
}

fn launcher_matches(query: &str) -> Vec<LauncherMatch> {
    let query = query.trim();
    if query.starts_with('>') {
        return command_palette_matches(query);
    }

    let mut matches = Vec::new();
    let category_filter = query
        .strip_prefix('@')
        .map(str::trim)
        .filter(|category| !category.is_empty());
    let search_query = if category_filter.is_some() { "" } else { query };

    for app in crate::app_metadata::APPS {
        let category_match = category_filter
            .map(|category| app.category.label().eq_ignore_ascii_case(category))
            .unwrap_or(false);
        let detail = app_launcher_detail(app);
        let mut score = if category_match { Some(80) } else { None };
        if score.is_none() {
            score = launcher_score(app.name, &detail, search_query);
        }
        if score.is_none() {
            for alias in app.aliases {
                if let Some(alias_score) = launcher_score(alias, &detail, search_query) {
                    score = Some(alias_score.saturating_sub(1));
                    break;
                }
            }
        }
        if let Some(score) = score {
            matches.push(LauncherMatch {
                label: String::from(app.name),
                detail,
                kind: LauncherMatchKind::App(String::from(app.name)),
                score: score + recent_app_boost(app.name),
            });
        }
    }

    for manifest in crate::app_metadata::installed_app_manifests() {
        let detail = manifest_launcher_detail(&manifest);
        let category_match = category_filter
            .map(|category| manifest.category.eq_ignore_ascii_case(category))
            .unwrap_or(false);
        if category_match || launcher_score(&manifest.name, &detail, search_query).is_some() {
            let score = if category_match {
                70
            } else {
                launcher_score(&manifest.name, &detail, search_query).unwrap_or(1)
            };
            matches.push(LauncherMatch {
                label: manifest.name.clone(),
                detail,
                kind: LauncherMatchKind::App(manifest.name.clone()),
                score,
            });
        }
    }
    if category_filter.is_some() {
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
        matches.truncate(12);
        return matches;
    }

    for &entry in crate::app_metadata::LAUNCHER_ENTRIES.iter() {
        if let Some(score) = launcher_score(entry.label, entry.detail, search_query) {
            matches.push(LauncherMatch {
                label: String::from(entry.label),
                detail: String::from(entry.detail),
                kind: match entry.kind {
                    crate::app_metadata::LauncherKind::App(app) => {
                        LauncherMatchKind::App(String::from(app))
                    }
                    crate::app_metadata::LauncherKind::Path(path) => {
                        LauncherMatchKind::Path(String::from(path))
                    }
                    crate::app_metadata::LauncherKind::Command(command) => {
                        LauncherMatchKind::Command(String::from(command))
                    }
                },
                score,
            });
        }
    }

    for app in crate::app_lifecycle::recent_apps().iter() {
        if let Some(score) = launcher_score(app, "recent app", search_query) {
            matches.push(LauncherMatch {
                label: app.clone(),
                detail: String::from("recent app"),
                kind: LauncherMatchKind::App(app.clone()),
                score: score + 10,
            });
        }
    }
    for command in crate::app_lifecycle::recent_commands().iter() {
        if let Some(score) = launcher_score(command, "recent command", search_query) {
            matches.push(LauncherMatch {
                label: command.clone(),
                detail: String::from("recent command"),
                kind: LauncherMatchKind::Command(command.clone()),
                score: score + 4,
            });
        }
    }
    for file in crate::app_lifecycle::recent_files().iter() {
        if let Some(score) = launcher_score(file, "recent file", search_query) {
            matches.push(LauncherMatch {
                label: file_name(file),
                detail: String::from("recent file"),
                kind: LauncherMatchKind::Path(file.clone()),
                score: score + 3,
            });
        }
    }
    for search in crate::app_lifecycle::recent_searches().iter() {
        if let Some(score) = launcher_score(search, "recent search", search_query) {
            matches.push(LauncherMatch {
                label: search.clone(),
                detail: String::from("recent search"),
                kind: LauncherMatchKind::Inline({
                    let mut action = String::from("search:");
                    action.push_str(search);
                    action
                }),
                score: score + 2,
            });
        }
    }
    for category in crate::app_metadata::APP_CATEGORIES {
        let label = category.label();
        if let Some(score) = launcher_score(label, "app category", search_query) {
            let mut action = String::from("category:");
            action.push_str(label);
            matches.push(LauncherMatch {
                label: {
                    let mut out = String::from(label);
                    out.push_str(" apps");
                    out
                },
                detail: String::from("app category"),
                kind: LauncherMatchKind::Inline(action),
                score: score + 1,
            });
        }
    }
    for shortcut in settings_shortcuts() {
        if let Some(score) = launcher_score(shortcut.0, "settings shortcut", search_query) {
            matches.push(LauncherMatch {
                label: String::from(shortcut.0),
                detail: String::from("settings shortcut"),
                kind: LauncherMatchKind::Inline({
                    let mut action = String::from("settings:");
                    action.push_str(shortcut.1);
                    action
                }),
                score: score + 5,
            });
        }
    }
    for action in power_actions() {
        if let Some(score) = launcher_score(action.0, "power/session action", search_query) {
            matches.push(LauncherMatch {
                label: String::from(action.0),
                detail: String::from("power/session"),
                kind: LauncherMatchKind::Inline(String::from(action.1)),
                score: score + 4,
            });
        }
    }
    for entry in crate::search_index::search(search_query, 8).iter() {
        matches.push(LauncherMatch {
            label: entry.name.clone(),
            detail: entry.path.clone(),
            kind: LauncherMatchKind::Path(entry.path.clone()),
            score: crate::search_index::fuzzy_score(&entry.name, search_query).unwrap_or(1) + 2,
        });
    }
    if launcher_score("Refresh search index", "inline action", search_query).is_some() {
        matches.push(LauncherMatch {
            label: String::from("Refresh search index"),
            detail: String::from("inline action"),
            kind: LauncherMatchKind::Inline(String::from("refresh-index")),
            score: if search_query.is_empty() { 1 } else { 24 },
        });
    }
    if launcher_score(
        "Test crash dialog",
        "diagnostics inline action",
        search_query,
    )
    .is_some()
    {
        matches.push(LauncherMatch {
            label: String::from("Test crash dialog"),
            detail: String::from("diagnostics inline action"),
            kind: LauncherMatchKind::Inline(String::from("test-crash-dialog")),
            score: if search_query.is_empty() { 1 } else { 90 },
        });
    }
    matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.label.cmp(&b.label)));
    matches.truncate(12);
    matches
}

fn launcher_score(label: &str, detail: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(1);
    }
    let label_score = crate::search_index::fuzzy_score(label, query).unwrap_or(0);
    let detail_score = crate::search_index::fuzzy_score(detail, query)
        .unwrap_or(0)
        .saturating_sub(3);
    let score = label_score.max(detail_score);
    if score == 0 {
        None
    } else {
        Some(score)
    }
}

fn command_palette_matches(query: &str) -> Vec<LauncherMatch> {
    let command = query.trim_start_matches('>').trim();
    if command.is_empty() {
        return alloc::vec![
            launcher_inline("Reboot", "power action", "reboot", 40),
            launcher_inline("Shutdown", "power action", "shutdown", 38),
            launcher_command("Run fsck", "filesystem check", "fsck", 36),
            launcher_command("HTTP example.com", "userspace HTTP", "http example.com", 34),
            launcher_path("Open /Documents", "open folder", "/Documents", 32),
        ];
    }
    if let Some(rest) = command.strip_prefix("open ") {
        return alloc::vec![launcher_path(rest.trim(), "open path", rest.trim(), 70)];
    }
    if let Some(rest) = command.strip_prefix("http ") {
        let mut cmd = String::from("http ");
        cmd.push_str(rest.trim());
        return alloc::vec![launcher_command(&cmd, "HTTP client", &cmd, 70)];
    }
    if let Some(rest) = command.strip_prefix("dns ") {
        let mut cmd = String::from("dns ");
        cmd.push_str(rest.trim());
        return alloc::vec![launcher_command(&cmd, "DNS resolver", &cmd, 70)];
    }
    if let Some(rest) = command.strip_prefix("settings ") {
        let page = rest.trim();
        return alloc::vec![launcher_inline(
            command,
            "settings page",
            &settings_action(page),
            70
        )];
    }
    match command {
        "reboot" | "restart" => {
            alloc::vec![launcher_inline("Reboot", "power action", "reboot", 80)]
        }
        "shutdown" | "poweroff" => {
            alloc::vec![launcher_inline("Shutdown", "power action", "shutdown", 80)]
        }
        "sleep" => alloc::vec![launcher_inline("Sleep", "power action", "sleep", 80)],
        "lock" => alloc::vec![launcher_inline("Lock", "session action", "lock", 80)],
        "logout" => alloc::vec![launcher_inline("Logout", "session action", "logout", 80)],
        "restore" | "restore session" => alloc::vec![launcher_inline(
            "Restore session",
            "session action",
            "restore-session",
            80,
        )],
        "restart desktop" => alloc::vec![launcher_inline(
            "Restart desktop",
            "desktop action",
            "restart-desktop",
            80,
        )],
        "crash dialog" | "test crash dialog" => alloc::vec![launcher_inline(
            "Test crash dialog",
            "diagnostics action",
            "test-crash-dialog",
            80,
        )],
        _ => alloc::vec![launcher_command(command, "terminal command", command, 50)],
    }
}

fn launcher_inline(label: &str, detail: &str, action: &str, score: usize) -> LauncherMatch {
    LauncherMatch {
        label: String::from(label),
        detail: String::from(detail),
        kind: LauncherMatchKind::Inline(String::from(action)),
        score,
    }
}

fn launcher_command(label: &str, detail: &str, command: &str, score: usize) -> LauncherMatch {
    LauncherMatch {
        label: String::from(label),
        detail: String::from(detail),
        kind: LauncherMatchKind::Command(String::from(command)),
        score,
    }
}

fn launcher_path(label: &str, detail: &str, path: &str, score: usize) -> LauncherMatch {
    LauncherMatch {
        label: String::from(label),
        detail: String::from(detail),
        kind: LauncherMatchKind::Path(String::from(path)),
        score,
    }
}

fn app_launcher_detail(app: &crate::app_metadata::AppMetadata) -> String {
    let mut detail = String::from(app.category.label());
    detail.push_str(" app, permission ");
    detail.push_str(app.permission);
    if !app.associations.is_empty() {
        detail.push_str(", opens ");
        for (idx, assoc) in app.associations.iter().enumerate() {
            if idx > 0 {
                detail.push(',');
            }
            detail.push_str(assoc);
        }
    }
    detail
}

fn manifest_launcher_detail(manifest: &crate::app_metadata::AppManifest) -> String {
    let mut detail = String::from("/APPS manifest ");
    detail.push_str(&manifest.icon);
    detail.push(' ');
    detail.push_str(&manifest.category);
    detail.push_str(", permission ");
    detail.push_str(&manifest.permission);
    detail.push_str(", command ");
    detail.push_str(&manifest.command);
    detail.push_str(", id ");
    detail.push_str(&manifest.id);
    if !manifest.associations.is_empty() {
        detail.push_str(", opens ");
        for (idx, assoc) in manifest.associations.iter().enumerate() {
            if idx > 0 {
                detail.push(',');
            }
            detail.push_str(assoc);
        }
    }
    detail
}

fn recent_app_boost(app: &str) -> usize {
    crate::app_lifecycle::recent_apps()
        .iter()
        .position(|recent| recent.eq_ignore_ascii_case(app))
        .map(|idx| 10usize.saturating_sub(idx))
        .unwrap_or(0)
}

fn settings_shortcuts() -> &'static [(&'static str, &'static str)] {
    &[
        ("Display settings", "desktop"),
        ("Accessibility settings", "accessibility"),
        ("Diagnostics settings", "diagnostics"),
        ("Network settings", "network"),
        ("Storage settings", "storage"),
        ("Log viewer settings", "logs"),
        ("Power settings", "power"),
        ("Updates", "logs"),
    ]
}

fn power_actions() -> &'static [(&'static str, &'static str)] {
    &[
        ("Shutdown", "shutdown"),
        ("Reboot", "reboot"),
        ("Sleep", "sleep"),
        ("Lock", "lock"),
        ("Logout", "logout"),
        ("Restart desktop", "restart-desktop"),
        ("Restore session", "restore-session"),
    ]
}

fn settings_action(page: &str) -> String {
    let mut action = String::from("settings:");
    action.push_str(page);
    action
}

fn build_start_menu_entries() -> Vec<StartMenuEntry> {
    let prefs = crate::app_lifecycle::start_menu_prefs();
    let mut out = Vec::new();
    if prefs.show_recent {
        for app in crate::app_lifecycle::recent_apps().iter().take(3) {
            out.push(StartMenuEntry {
                section: "RECENT",
                label: app.clone(),
                detail: String::from("app"),
                kind: LauncherMatchKind::App(app.clone()),
            });
        }
        for file in crate::app_lifecycle::recent_files().iter().take(3) {
            out.push(StartMenuEntry {
                section: "RECENT",
                label: file_name(file),
                detail: String::from("file"),
                kind: LauncherMatchKind::Path(file.clone()),
            });
        }
        for command in crate::app_lifecycle::recent_commands().iter().take(2) {
            out.push(StartMenuEntry {
                section: "RECENT",
                label: command.clone(),
                detail: String::from("cmd"),
                kind: LauncherMatchKind::Command(command.clone()),
            });
        }
    }

    for &place in FileManagerApp::START_MENU_LINKS.iter().take(4) {
        out.push(StartMenuEntry {
            section: "PLACES",
            label: String::from(place),
            detail: String::from("folder"),
            kind: LauncherMatchKind::Path(FileManagerApp::shell_link_path(place)),
        });
    }

    for shortcut in settings_shortcuts().iter().take(7) {
        out.push(StartMenuEntry {
            section: "SETTINGS",
            label: String::from(shortcut.0),
            detail: String::from("page"),
            kind: LauncherMatchKind::Inline(settings_action(shortcut.1)),
        });
    }
    for category in crate::app_metadata::APP_CATEGORIES.iter().take(6) {
        let mut action = String::from("category:");
        action.push_str(category.label());
        out.push(StartMenuEntry {
            section: "CATEGORIES",
            label: {
                let mut label = String::from(category.label());
                label.push_str(" apps");
                label
            },
            detail: String::from("apps"),
            kind: LauncherMatchKind::Inline(action),
        });
    }
    for action in power_actions().iter().take(5) {
        out.push(StartMenuEntry {
            section: "POWER",
            label: String::from(action.0),
            detail: String::from("session"),
            kind: LauncherMatchKind::Inline(String::from(action.1)),
        });
    }
    out
}

fn start_menu_pinned_limit(menu_h: i32, bottom_h: i32, left_hdr_h: i32, item_h: i32) -> usize {
    ((menu_h - bottom_h - left_hdr_h - item_h - 18).max(0) / item_h.max(1)) as usize
}

fn start_menu_entry_at(
    entries: &[StartMenuEntry],
    rel_y: i32,
    item_h: i32,
    max_h: i32,
) -> Option<usize> {
    if rel_y < 0 {
        return None;
    }
    let mut y = 0i32;
    let mut last_section = "";
    for (idx, entry) in entries.iter().enumerate() {
        if entry.section != last_section {
            if y + START_MENU_SECTION_H > max_h {
                return None;
            }
            if rel_y >= y && rel_y < y + START_MENU_SECTION_H {
                return None;
            }
            y += START_MENU_SECTION_H;
            last_section = entry.section;
        }
        if y + item_h > max_h {
            return None;
        }
        if rel_y >= y && rel_y < y + item_h {
            return Some(idx);
        }
        y += item_h;
    }
    None
}

fn start_item_kind(item: &str) -> LauncherMatchKind {
    if let Some(path) = item.strip_prefix("path:") {
        LauncherMatchKind::Path(String::from(path.trim()))
    } else if let Some(command) = item.strip_prefix("cmd:") {
        LauncherMatchKind::Command(String::from(command.trim()))
    } else if let Some(action) = item.strip_prefix("setting:") {
        LauncherMatchKind::Inline(settings_action(action.trim()))
    } else if let Some(action) = item.strip_prefix("inline:") {
        LauncherMatchKind::Inline(String::from(action.trim()))
    } else if item.starts_with('/') {
        LauncherMatchKind::Path(String::from(item))
    } else if crate::app_metadata::app_by_id_or_command(item).is_some()
        || crate::app_metadata::app_by_name(item).is_some()
    {
        let app = crate::app_metadata::app_by_id_or_command(item)
            .or_else(|| crate::app_metadata::app_by_name(item));
        LauncherMatchKind::App(String::from(app.map(|meta| meta.name).unwrap_or(item)))
    } else {
        LauncherMatchKind::App(String::from(item))
    }
}

fn start_item_label(item: &str) -> String {
    if let Some(path) = item.strip_prefix("path:") {
        file_name(path.trim())
    } else if let Some(command) = item.strip_prefix("cmd:") {
        let mut label = String::from("Run ");
        label.push_str(command.trim());
        label
    } else if let Some(page) = item.strip_prefix("setting:") {
        let mut label = String::from(page.trim());
        label.push_str(" settings");
        label
    } else if let Some(action) = item.strip_prefix("inline:") {
        String::from(action.trim())
    } else {
        String::from(item)
    }
}

fn launcher_pin_label(entry: &LauncherMatch) -> String {
    match &entry.kind {
        LauncherMatchKind::App(app) => app.clone(),
        LauncherMatchKind::Path(path) => {
            let mut item = String::from("path:");
            item.push_str(path);
            item
        }
        LauncherMatchKind::Command(command) => {
            let mut item = String::from("cmd:");
            item.push_str(command);
            item
        }
        LauncherMatchKind::Inline(action) if action.starts_with("settings:") => {
            let mut item = String::from("setting:");
            item.push_str(action.trim_start_matches("settings:"));
            item
        }
        LauncherMatchKind::Inline(action) => {
            let mut item = String::from("inline:");
            item.push_str(action);
            item
        }
    }
}

fn launcher_copy_text(entry: &LauncherMatch) -> String {
    match &entry.kind {
        LauncherMatchKind::App(app) => app.clone(),
        LauncherMatchKind::Path(path) => path.clone(),
        LauncherMatchKind::Command(command) => command.clone(),
        LauncherMatchKind::Inline(action) => action.clone(),
    }
}

fn parent_path(path: &str) -> &str {
    if path == "/" {
        return "/";
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((parent, _)) => parent,
    }
}

fn ctrl_number_slot(c: char) -> Option<usize> {
    match c {
        '1' => Some(0),
        '2' => Some(1),
        '3' => Some(2),
        '4' => Some(3),
        '5' => Some(4),
        '6' => Some(5),
        '7' => Some(6),
        '8' => Some(7),
        '9' => Some(8),
        '0' => Some(9),
        _ => None,
    }
}

fn start_menu_widget_status_line() -> String {
    let mut line = String::new();
    if let Some(stats) = crate::fat32::stats() {
        line.push_str("disk ");
        push_decimal(&mut line, stats.free_clusters as u64);
        line.push_str(" free");
    } else {
        line.push_str("disk ?");
    }
    line.push_str("  net ");
    line.push_str(if crate::net::protocol_lines().is_empty() {
        "idle"
    } else {
        "ok"
    });
    line.push_str("  job ");
    push_decimal(&mut line, crate::jobs::recent(6).len() as u64);
    line
}

fn draw_start_menu_widgets(s: &mut [u32], sw: usize, x: i32, y: i32, w: i32, h: i32, line: &str) {
    let bg = 0x00_00_05_12;
    s_fill(s, sw, x, y, w, h, bg);
    draw_rect_border(s, sw, x, y, w, h, 0x00_00_33_66);
    s_draw_str_small(s, sw, x + 6, y + 5, line, 0x00_66_AA_DD, bg, x + w - 6);
}

fn file_name(path: &str) -> String {
    String::from(path.rsplit('/').next().unwrap_or(path))
}

fn parse_i32_field(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

fn parse_usize_field(value: &str) -> Option<usize> {
    value.parse::<usize>().ok()
}

fn workspace_label(workspace: usize) -> &'static str {
    match workspace {
        0 => "WS1",
        1 => "WS2",
        2 => "WS3",
        3 => "WS4",
        _ => "WS?",
    }
}

fn push_i32_decimal(out: &mut String, n: i32) {
    if n < 0 {
        out.push('-');
        push_decimal(out, n.unsigned_abs() as u64);
    } else {
        push_decimal(out, n as u64);
    }
}

lazy_static! {
    pub static ref WM: Mutex<WindowManager> = Mutex::new(WindowManager::new());
}

// ── Shadow-buffer helpers ─────────────────────────────────────────────────────

#[inline(always)]
fn s_put(s: &mut [u32], sw: usize, sh: usize, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 {
        let (x, y) = (x as usize, y as usize);
        if x < sw && (sh == usize::MAX || y < sh) && y * sw + x < s.len() {
            s[y * sw + x] = color;
        }
    }
}

fn s_fill(s: &mut [u32], sw: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    let x0 = (x.max(0) as usize).min(sw);
    let y0 = y.max(0) as usize;
    let x1 = ((x + w).max(0) as usize).min(sw);
    let y1 = ((y + h).max(0) as usize).min(sh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for row in y0..y1 {
        let base = row * sw;
        s[base + x0..base + x1].fill(color);
    }
}

/// Additive-alpha fill — darkens existing pixels by a fraction derived from `shadow`'s alpha byte.
#[inline(always)]
fn s_fill_alpha(s: &mut [u32], sw: usize, x: i32, y: i32, w: i32, h: i32, shadow: u32) {
    // Scale darkening by the alpha embedded in bits [31:24] of the colour word.
    let amount = ((shadow >> 24) & 0xFF) as u32;
    if amount == 0 {
        return;
    }
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    let x0 = (x.max(0) as usize).min(sw);
    let y0 = y.max(0) as usize;
    let x1 = ((x + w).max(0) as usize).min(sw);
    let y1 = ((y + h).max(0) as usize).min(sh);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for row in y0..y1 {
        for col in x0..x1 {
            let idx = row * sw + col;
            let p = s[idx];
            let r = ((p >> 16) & 0xFF).saturating_sub(amount);
            let g = ((p >> 8) & 0xFF).saturating_sub(amount);
            let b = (p & 0xFF).saturating_sub(amount);
            s[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

/// Render text at raw 1× scale (8 × 8 px per glyph) — used for compact labels.
fn s_draw_char_small(s: &mut [u32], sw: usize, x: i32, y: i32, c: char, fg: u32, bg: u32) {
    use font8x8::UnicodeFonts;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    let large_text = crate::accessibility::snapshot().large_text;
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let ink = byte & (1 << bit) != 0;
            let color = if ink { fg } else { bg };
            let px = x + bit as i32;
            let py = y + gy as i32;
            if px >= 0 && py >= 0 {
                let (px, py) = (px as usize, py as usize);
                if px < sw && py < sh {
                    s[py * sw + px] = color;
                    if large_text && ink && px + 1 < sw {
                        s[py * sw + px + 1] = fg;
                    }
                }
            }
        }
    }
}

fn s_draw_str_small(
    s: &mut [u32],
    sw: usize,
    x: i32,
    y: i32,
    text: &str,
    fg: u32,
    bg: u32,
    max_x: i32,
) {
    let mut cx = x;
    for c in text.chars() {
        if cx + 8 > max_x {
            break;
        }
        s_draw_char_small(s, sw, cx, y, c, fg, bg);
        cx += 8;
    }
}

/// Draw a 1-pixel-wide unfilled rectangle border.
fn draw_rect_border(s: &mut [u32], sw: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    s_fill(s, sw, x, y, w, 1, color); // top
    s_fill(s, sw, x, y + h - 1, w, 1, color); // bottom
    s_fill(s, sw, x, y, 1, h, color); // left
    s_fill(s, sw, x + w - 1, y, 1, h, color); // right
}

fn draw_usb_tray_icon(s: &mut [u32], sw: usize, x: i32, y: i32, color: u32) {
    s_fill(s, sw, x + 5, y, 2, 4, color);
    s_fill(s, sw, x + 3, y + 3, 6, 2, color);
    s_fill(s, sw, x + 2, y + 5, 2, 4, color);
    s_fill(s, sw, x + 8, y + 5, 2, 4, color);
    s_fill(s, sw, x + 4, y + 7, 4, 2, color);
    s_fill(s, sw, x + 5, y + 9, 2, 3, color);
    s_fill(s, sw, x + 4, y + 11, 4, 1, blend_color(color, WHITE, 110));
}

fn draw_keyboard_tray_icon(s: &mut [u32], sw: usize, x: i32, y: i32, color: u32) {
    draw_rect_border(s, sw, x, y + 2, 13, 8, color);
    s_fill(s, sw, x + 2, y + 4, 9, 1, color);
    s_fill(s, sw, x + 2, y + 6, 2, 1, color);
    s_fill(s, sw, x + 5, y + 6, 2, 1, color);
    s_fill(s, sw, x + 8, y + 6, 2, 1, color);
    s_fill(s, sw, x + 3, y + 10, 7, 2, color);
}

fn draw_mouse_tray_icon(s: &mut [u32], sw: usize, x: i32, y: i32, color: u32) {
    draw_rect_border(s, sw, x + 2, y, 9, 12, color);
    s_fill(s, sw, x + 6, y + 2, 1, 2, color);
    s_fill(s, sw, x + 3, y + 4, 7, 1, color);
}

/// Blend two u32 colours: t=0 → a, t=255 → b.
#[inline(always)]
fn blend_color(a: u32, b: u32, t: u32) -> u32 {
    let lerp = |ca: u32, cb: u32| -> u32 {
        if cb >= ca {
            (ca + (cb - ca) * t / 255).min(255)
        } else {
            ca - (ca - cb) * t / 255
        }
    };
    let r = lerp((a >> 16) & 0xFF, (b >> 16) & 0xFF);
    let g = lerp((a >> 8) & 0xFF, (b >> 8) & 0xFF);
    let bl = lerp(a & 0xFF, b & 0xFF);
    (r << 16) | (g << 8) | bl
}

/// Bilinear interpolation for a single u8 channel across four corners.
#[inline(always)]
fn bilinear_u8(tl: u8, tr: u8, bl: u8, br: u8, tx: f32, ty: f32) -> u8 {
    let top = tl as f32 * (1.0 - tx) + tr as f32 * tx;
    let bot = bl as f32 * (1.0 - tx) + br as f32 * tx;
    (top * (1.0 - ty) + bot * ty) as u8
}
