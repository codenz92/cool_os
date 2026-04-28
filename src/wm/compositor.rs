/// Window compositor — desktop, windows, taskbar, cursor, context menu.
/// All rendering targets a `Vec<u32>` shadow buffer; one blit per frame.
///
/// Visual theme: Retro-Futuristic CRT Phosphor Blue
extern crate alloc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::apps::{ColorPickerApp, FileManagerApp, SysMonApp, TerminalApp, TextViewerApp};
use crate::framebuffer::{BLACK, WHITE};
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

// ── Colors — Retro-Futuristic CRT Phosphor Blue ───────────────────────────────

// Taskbar / shell
// Accent (CRT phosphor blue)
const ACCENT: u32 = 0x00_00_BB_FF; // #00BBFF  bright phosphor blue (boosted)
const ACCENT_HOV: u32 = 0x00_44_CC_FF; // #44CCFF  lit hover (richer)
const ACCENT_PRESS: u32 = 0x00_00_77_CC; // #0077CC  depressed

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

const CTX_W: i32 = 224;
const CTX_ITEM_H: i32 = 32;
const CTX_HEADER_H: i32 = 28; // non-clickable header strip
const CTX_PAD: i32 = 4; // top/bottom padding inside menu body
const CTX_ITEMS: &[&str] = &[
    "Terminal",
    "System Mon",
    "Text Viewer",
    "Color Pick",
    "File Manager",
];

struct ContextMenu {
    x: i32,
    y: i32,
}

// ── Desktop icons ──────────────────────────────────────────────────────────────

const ICON_SIZE: i32 = 52;
const ICON_LABEL_H: i32 = 14;

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

fn desktop_icons() -> [DesktopIcon; 5] {
    [
        DesktopIcon {
            x: 20,
            y: 20,
            label: "Terminal",
            app: "Terminal",
        },
        DesktopIcon {
            x: 188,
            y: 20,
            label: "Monitor",
            app: "System Mon",
        },
        DesktopIcon {
            x: 356,
            y: 20,
            label: "Files",
            app: "File Manager",
        },
        DesktopIcon {
            x: 20,
            y: 118,
            label: "Viewer",
            app: "Text Viewer",
        },
        DesktopIcon {
            x: 188,
            y: 118,
            label: "Colors",
            app: "Color Pick",
        },
    ]
}

fn desktop_icon_hit(px: i32, py: i32) -> Option<usize> {
    desktop_icons().iter().position(|icon| icon.hit(px, py))
}

const START_PINNED_APPS: [&str; 5] = [
    "Terminal",
    "System Monitor",
    "Text Viewer",
    "Color Picker",
    "File Manager",
];

fn canonical_app_title(name: &str) -> &str {
    match name {
        "Terminal" => "Terminal",
        "System Mon" | "System Monitor" => "System Monitor",
        "Text View" | "Text Viewer" => "Text Viewer",
        "Color Pick" | "Color Picker" => "Color Picker",
        "File Mgr" | "File Manager" => "File Manager",
        _ => name,
    }
}

fn window_accent(title: &str) -> u32 {
    match canonical_app_title(title) {
        "Terminal" => ICON_TERM_ACC,
        "System Monitor" => ICON_MON_ACC,
        "Text Viewer" => ICON_TXT_ACC,
        "Color Picker" => ICON_COL_ACC,
        "File Manager" => 0x00_55_DD_FF,
        _ => ACCENT,
    }
}

fn window_glyph(title: &str) -> &'static str {
    match canonical_app_title(title) {
        "Terminal" => "T>",
        "System Monitor" => "M#",
        "Text Viewer" => "Tx",
        "Color Picker" => "CP",
        "File Manager" => "FM",
        _ => "[]",
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

// ── AppWindow ─────────────────────────────────────────────────────────────────

pub enum AppWindow {
    Terminal(TerminalApp),
    SysMon(SysMonApp),
    TextViewer(TextViewerApp),
    ColorPicker(ColorPickerApp),
    FileManager(FileManagerApp),
}

impl AppWindow {
    pub fn window(&self) -> &Window {
        match self {
            AppWindow::Terminal(t) => &t.window,
            AppWindow::SysMon(s) => &s.window,
            AppWindow::TextViewer(v) => &v.window,
            AppWindow::ColorPicker(c) => &c.window,
            AppWindow::FileManager(f) => &f.window,
        }
    }
    pub fn window_mut(&mut self) -> &mut Window {
        match self {
            AppWindow::Terminal(t) => &mut t.window,
            AppWindow::SysMon(s) => &mut s.window,
            AppWindow::TextViewer(v) => &mut v.window,
            AppWindow::ColorPicker(c) => &mut c.window,
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
    }
    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        match self {
            AppWindow::ColorPicker(cp) => cp.handle_click(lx, ly),
            AppWindow::FileManager(fm) => fm.handle_click(lx, ly),
            _ => {}
        }
    }
    pub fn handle_dbl_click(&mut self, lx: i32, ly: i32) {
        if let AppWindow::FileManager(fm) = self {
            fm.handle_dbl_click(lx, ly);
        }
    }
    pub fn take_open_request(&mut self) -> Option<alloc::string::String> {
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
    }
    pub fn update(&mut self) {
        match self {
            AppWindow::Terminal(t) => t.update(),
            AppWindow::SysMon(s) => s.update(),
            AppWindow::TextViewer(v) => v.update(),
            AppWindow::ColorPicker(c) => c.update(),
            AppWindow::FileManager(f) => f.update(),
        }
    }
    pub fn is_minimized(&self) -> bool {
        self.window().minimized
    }
}

// ── Window manager ────────────────────────────────────────────────────────────

pub struct WindowManager {
    pub windows: Vec<AppWindow>,
    z_order: Vec<usize>,
    focused: Option<usize>,
    key_sink_fd: Option<usize>,
    key_sink_window: Option<usize>,
    drag: Option<DragState>,
    resize: Option<ResizeState>,
    scroll_drag: Option<ScrollDragState>,
    prev_left: bool,
    prev_right: bool,
    context_menu: Option<ContextMenu>,
    icon_selected: Option<usize>,
    pressed_icon: Option<usize>,
    start_menu_open: bool,
    last_click_tick: u64,
    last_click_window: Option<usize>,
    last_click_x: i32,
    last_click_y: i32,
    /// Shadow buffer — screen_width × screen_height u32 pixels.
    shadow: Vec<u32>,
    shadow_width: usize,
    shadow_height: usize,
    blit_scratch: Vec<u8>,
    /// Pre-baked wallpaper pixels — computed once in new(), blitted each frame.
    wallpaper: Vec<u32>,
}

impl WindowManager {
    pub fn new() -> Self {
        let w = crate::framebuffer::width();
        let h = crate::framebuffer::height();
        let taskbar_y = h - TASKBAR_H as usize;
        crate::boot_splash::show(
            "allocating desktop buffers",
            15,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
        let mut wallpaper = alloc::vec![0u32; w * h];
        crate::boot_splash::show(
            "painting desktop background",
            16,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );
        let (fw, fh) = (w as f32, taskbar_y as f32);
        let glow_mark = taskbar_y / 3;
        let scanline_mark = taskbar_y * 2 / 3;
        let mut glow_stage_shown = false;
        let mut scanline_stage_shown = false;
        for y in 0..taskbar_y {
            if !glow_stage_shown && y >= glow_mark {
                crate::boot_splash::show(
                    "charging phosphor glow",
                    17,
                    crate::boot_splash::BOOT_PROGRESS_TOTAL,
                );
                glow_stage_shown = true;
            }
            if !scanline_stage_shown && y >= scanline_mark {
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
                    (DESK_TL >> 16) as u8,
                    (DESK_TR >> 16) as u8,
                    (DESK_BL >> 16) as u8,
                    (DESK_BR >> 16) as u8,
                    tx,
                    ty,
                );
                let g = bilinear_u8(
                    (DESK_TL >> 8) as u8,
                    (DESK_TR >> 8) as u8,
                    (DESK_BL >> 8) as u8,
                    (DESK_BR >> 8) as u8,
                    tx,
                    ty,
                );
                let b = bilinear_u8(
                    DESK_TL as u8,
                    DESK_TR as u8,
                    DESK_BL as u8,
                    DESK_BR as u8,
                    tx,
                    ty,
                );
                let dx = tx - 0.50;
                let dy = ty - 0.45;
                let dist_sq = dx * dx + dy * dy;
                let t_b = 1.0f32 - (dist_sq / 0.20f32).min(1.0f32); // tighter, brighter bloom
                let bloom = t_b * t_b * t_b * 1.4f32; // boosted bloom intensity
                let br = (r as f32 + bloom * ((BLOOM_1 >> 16) as u8 as f32)).min(255.0) as u32;
                let bg = (g as f32 + bloom * ((BLOOM_1 >> 8) as u8 as f32)).min(255.0) as u32;
                let bb = (b as f32 + bloom * (BLOOM_1 as u8 as f32)).min(255.0) as u32;

                // ── CRT scanline — stronger 3-step phosphor falloff ─────────────
                let scan: u32 = match y % 3 {
                    0 => 255, // bright phosphor line
                    1 => 210, // soft shoulder (stronger shadow vs before)
                    _ => 175, // dim valley (deeper shadow for contrast)
                };

                // ── Phosphor triad dot-mask — column 2 of every 3 gets blue boost ─
                let dot_boost: u32 = if x % 3 == 2 { 14 } else { 0 };

                let fr = br * scan / 255;
                let fg = bg * scan / 255;
                let fb = (bb * scan / 255).saturating_add(dot_boost).min(255);

                wallpaper[y * w + x] = (fr << 16) | (fg << 8) | fb;
            }
        }
        crate::boot_splash::show(
            "finishing wallpaper",
            19,
            crate::boot_splash::BOOT_PROGRESS_TOTAL,
        );

        if taskbar_y > 0 && w > 0 {
            crate::boot_splash::show(
                "placing starfield",
                20,
                crate::boot_splash::BOOT_PROGRESS_TOTAL,
            );
            let mut seed = 0xC001_D00Du32;
            let star_count = ((w * taskbar_y) / 12_000).max(48); // denser star field
            for _ in 0..star_count {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let sx = (seed as usize) % w;
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let sy = (seed as usize) % taskbar_y;
                let core = if seed & 3 == 0 {
                    0x00_FF_FF_FF // bright white star
                } else if seed & 3 == 1 {
                    0x00_EE_FF_FF // cool white
                } else {
                    0x00_88_CC_FF // blue phosphor
                };
                let glow = blend_color(core, DESK_TR, 160);
                let dim_glow = blend_color(core, DESK_TR, 210);
                wallpaper[sy * w + sx] = core;
                // 4-point cross halo
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

        WindowManager {
            windows: Vec::new(),
            z_order: Vec::new(),
            focused: None,
            key_sink_fd: None,
            key_sink_window: None,
            drag: None,
            resize: None,
            scroll_drag: None,
            prev_left: false,
            prev_right: false,
            context_menu: None,
            icon_selected: None,
            pressed_icon: None,
            start_menu_open: false,
            last_click_tick: 0,
            last_click_window: None,
            last_click_x: 0,
            last_click_y: 0,
            shadow,
            shadow_width: w,
            shadow_height: h,
            blit_scratch: alloc::vec![0u8; w * 3],
            wallpaper,
        }
    }

    pub fn add_window(&mut self, w: AppWindow) {
        let idx = self.windows.len();
        self.windows.push(w);
        self.z_order.push(idx);
        self.focused = Some(idx);
    }

    fn launch_app(&mut self, name: &str, wx: i32, wy: i32) {
        match canonical_app_title(name) {
            "Terminal" => self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy))),
            "System Monitor" => self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy))),
            "Text Viewer" => self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy))),
            "Color Picker" => self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy))),
            "File Manager" => self.add_window(AppWindow::FileManager(FileManagerApp::new(wx, wy))),
            _ => {}
        }
    }

    fn toggle_show_desktop(&mut self) {
        let any_visible = self.windows.iter().any(|w| !w.window().minimized);
        if any_visible {
            for w in self.windows.iter_mut() {
                w.window_mut().minimize();
            }
            self.focused = None;
        } else {
            for w in self.windows.iter_mut() {
                w.window_mut().restore();
            }
            self.focused = self.z_order.last().copied();
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

    /// Full composite frame into shadow, then blit to hardware framebuffer.
    pub fn compose(&mut self) {
        // Drain buffered keystrokes.
        while let Some(c) = crate::keyboard::pop() {
            self.handle_key(c);
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

        // Start button click — flush left, full height
        let taskbar_click = left_pressed && my_i >= taskbar_y && mx_i < START_BTN_W;
        if taskbar_click {
            self.start_menu_open = !self.start_menu_open;
            self.context_menu = None;
            left_press_consumed = true;
            crate::wm::request_repaint();
        }

        if right_pressed && self.front_to_back_hit(mx_i, my_i).is_none() {
            let cx = mx_i.min(sw as i32 - CTX_W);
            let ctx_total_h = CTX_HEADER_H + CTX_PAD * 2 + CTX_ITEM_H * CTX_ITEMS.len() as i32;
            let cy = my_i.min(taskbar_y - ctx_total_h);
            self.context_menu = Some(ContextMenu { x: cx, y: cy });
        }

        if left_pressed {
            if self.context_menu.is_some() {
                left_press_consumed = true;
                let clicked: Option<&str> = {
                    let cm = self.context_menu.as_ref().unwrap();
                    CTX_ITEMS.iter().enumerate().find_map(|(i, &label)| {
                        let item_y = cm.y + CTX_HEADER_H + CTX_PAD + i as i32 * CTX_ITEM_H;
                        if mx_i >= cm.x
                            && mx_i < cm.x + CTX_W
                            && my_i >= item_y
                            && my_i < item_y + CTX_ITEM_H
                        {
                            Some(label)
                        } else {
                            None
                        }
                    })
                };
                self.context_menu = None;
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(sw as i32 - 200);
                let wy = (10 + off).min(taskbar_y - 80);
                if let Some(label) = clicked {
                    self.launch_app(label, wx, wy);
                }
            } else {
                if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                    left_press_consumed = true;
                    let win_idx = self.z_order[z_pos];
                    self.z_order.remove(z_pos);
                    self.z_order.push(win_idx);
                    self.focused = Some(win_idx);

                    let w = self.windows[win_idx].window();
                    if w.hit_close(mx_i, my_i) {
                        if self.key_sink_window == Some(win_idx) {
                            self.stop_key_sink();
                        } else if let Some(target) = self.key_sink_window {
                            if target > win_idx {
                                self.key_sink_window = Some(target - 1);
                            }
                        }
                        self.windows.remove(win_idx);
                        self.z_order.retain(|&i| i != win_idx);
                        for z in self.z_order.iter_mut() {
                            if *z > win_idx {
                                *z -= 1;
                            }
                        }
                        self.focused = self.z_order.last().copied();
                        self.drag = None;
                    } else if w.hit_minimize(mx_i, my_i) {
                        self.windows[win_idx].window_mut().minimize();
                        crate::wm::request_repaint();
                    } else if w.hit_maximize(mx_i, my_i) {
                        let sw = self.shadow_width as i32;
                        let sh = self.shadow_height as i32;
                        self.windows[win_idx].window_mut().maximize(sw, sh);
                        crate::wm::request_repaint();
                    } else if self.windows[win_idx].window().hit_title(mx_i, my_i) {
                        self.drag = Some(DragState {
                            window: win_idx,
                            off_x: mx_i - self.windows[win_idx].window().x,
                            off_y: my_i - self.windows[win_idx].window().y,
                        });
                    } else if self.windows[win_idx].window().hit_resize(mx_i, my_i) {
                        let w = self.windows[win_idx].window();
                        self.resize = Some(ResizeState {
                            window: win_idx,
                            start_w: w.width,
                            start_h: w.height,
                            start_mx: mx_i,
                            start_my: my_i,
                        });
                    } else if self.windows[win_idx].window().hit_scrollbar(mx_i, my_i) {
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
                        self.windows[win_idx].handle_click(lx, ly);
                        let is_double_click = self.last_click_window == Some(win_idx)
                            && uptime_ticks.wrapping_sub(self.last_click_tick)
                                <= crate::interrupts::ticks_for_millis(500)
                            && (self.last_click_x - lx).abs() <= 6
                            && (self.last_click_y - ly).abs() <= 6;
                        if is_double_click {
                            self.windows[win_idx].handle_dbl_click(lx, ly);
                            if let Some(path) = self.windows[win_idx].take_open_request() {
                                let off = self.windows.len() as i32 * 16;
                                let wx = (20 + off).min(sw as i32 - 220);
                                let wy = (20 + off).min(taskbar_y - 120);
                                match TextViewerApp::open_file(wx, wy, &path) {
                                    Ok(viewer) => self.add_window(AppWindow::TextViewer(viewer)),
                                    Err(err) => {
                                        if let Some(term) =
                                            self.windows.iter_mut().find_map(|w| match w {
                                                AppWindow::Terminal(t) => Some(t),
                                                _ => None,
                                            })
                                        {
                                            term.print_str("open failed: ");
                                            term.print_str(err);
                                            term.print_char('\n');
                                        }
                                    }
                                }
                                crate::wm::request_repaint();
                            }
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
                    let t_gap = 20i32;
                    let t_start = tray_clk_x + (TASKBAR_TRAY_W - (t_gap * 2 + 13)) / 2;
                    if mx_i >= t_start && mx_i < t_start + t_gap * 2 + 14 {
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
                        let taskbar_btn_x0 = START_BTN_W + 8;
                        if mx_i >= taskbar_btn_x0 && mx_i < show_desktop_x - 6 {
                            let btn_idx = ((mx_i - taskbar_btn_x0) / (BUTTON_W + 6)) as usize;
                            let bx = taskbar_btn_x0 + btn_idx as i32 * (BUTTON_W + 6);
                            if btn_idx < self.windows.len() && mx_i < bx + BUTTON_W {
                                if self.windows[btn_idx].is_minimized() {
                                    self.windows[btn_idx].window_mut().restore();
                                }
                                if let Some(z_pos) = self.z_order.iter().position(|&i| i == btn_idx)
                                {
                                    self.z_order.remove(z_pos);
                                    self.z_order.push(btn_idx);
                                    self.focused = Some(btn_idx);
                                }
                                crate::wm::request_repaint();
                            }
                        }
                    }
                }
            }
        }

        if left_released {
            self.drag = None;
            self.resize = None;
            self.scroll_drag = None;
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
            let menu_w = 460i32;
            let menu_h = 320i32;
            let left_w = 240i32;
            let bottom_h = 36i32;
            let left_hdr_h = 32i32;
            let menu_x = 2i32;
            let menu_y = taskbar_y - menu_h;
            let bar_y = menu_y + menu_h - bottom_h;
            if mx_i >= menu_x && mx_i < menu_x + menu_w && my_i >= menu_y && my_i < taskbar_y {
                left_press_consumed = true;
                // Left column — app list rows
                if mx_i < menu_x + left_w {
                    let item_h = 40i32;
                    let items_y = menu_y + left_hdr_h + 8;
                    if my_i >= items_y && my_i < items_y + item_h * START_PINNED_APPS.len() as i32 {
                        let item_idx = ((my_i - items_y) / item_h) as usize;
                        if item_idx < START_PINNED_APPS.len() {
                            let off = self.windows.len() as i32 * 16;
                            let wx = (10 + off).min(sw as i32 - 200);
                            let wy = (10 + off).min(bar_y - 80);
                            self.launch_app(START_PINNED_APPS[item_idx], wx, wy);
                            self.start_menu_open = false;
                            crate::wm::request_repaint();
                        }
                    }
                }
            }
        }

        // Desktop icon click.
        if left_pressed {
            let icon_hit = desktop_icon_hit(mx_i, my_i);
            let desktop_hit = !left_press_consumed
                && my_i < taskbar_y
                && self.context_menu.is_none()
                && !self.start_menu_open;
            self.pressed_icon = if desktop_hit { icon_hit } else { None };
            if let Some(i) = self.pressed_icon {
                self.icon_selected = Some(i);
                self.context_menu = None;
                crate::wm::request_repaint();
            }
        }

        if left_released {
            if let Some(icon_idx) = self.pressed_icon.take() {
                if desktop_icon_hit(mx_i, my_i) == Some(icon_idx)
                    && my_i < taskbar_y
                    && self.front_to_back_hit(mx_i, my_i).is_none()
                    && self.context_menu.is_none()
                    && !self.start_menu_open
                {
                    let icon = &desktop_icons()[icon_idx];
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

        // ── Render ────────────────────────────────────────────────────────────
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
        {
            let s: &mut [u32] = self.shadow.as_mut_slice();

            for w in self.windows.iter_mut() {
                w.update();
            }

            // ── Desktop icons — drawn BEFORE windows so windows can cover them ────
            let icon_data: [(u32, u32); 5] = [
                (ICON_TERM_BG, ICON_TERM_ACC),  // Terminal
                (ICON_MON_BG, ICON_MON_ACC),    // Monitor
                (0x00_00_0E_20, 0x00_55_DD_FF), // Files (File Manager)
                (ICON_TXT_BG, ICON_TXT_ACC),    // Viewer (Text Viewer)
                (ICON_COL_BG, ICON_COL_ACC),    // Colors (Color Picker)
            ];
            for (i, icon) in desktop_icons().iter().enumerate() {
                let selected = self.icon_selected == Some(i);
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

            // ── Windows — drawn AFTER icons so they appear in front ───────────────
            let z: Vec<usize> = self.z_order.clone();
            for &wi in &z {
                if wi < self.windows.len() {
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
                ACCENT_PRESS
            } else if start_hot {
                0x00_00_22_48
            } else {
                0x00_00_0E_22
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
            s_fill(s, sw, btn_x, btn_y, btn_w, btn_h, start_bg);
            // Double border for depth
            draw_rect_border(
                s,
                sw,
                btn_x,
                btn_y,
                btn_w,
                btn_h,
                if start_active { ACCENT_HOV } else { ACCENT },
            );
            draw_rect_border(
                s,
                sw,
                btn_x + 1,
                btn_y + 1,
                btn_w - 2,
                btn_h - 2,
                if start_active {
                    blend_color(ACCENT, WIN_BAR_F, 200)
                } else {
                    0x00_00_22_44
                },
            );

            let tile_x = btn_x + 5;
            let tile_y = btn_y + 5;
            let tile_bg = if start_active {
                0x00_00_16_2E
            } else {
                0x00_00_0A_1C
            };
            s_fill(s, sw, tile_x, tile_y, 18, 18, tile_bg);
            s_fill(
                s,
                sw,
                tile_x,
                tile_y,
                18,
                3,
                if start_active {
                    ACCENT
                } else {
                    blend_color(ACCENT, BLACK, 160)
                },
            );
            draw_rect_border(
                s,
                sw,
                tile_x,
                tile_y,
                18,
                18,
                if start_active { ACCENT } else { 0x00_00_55_99 },
            );
            let icon_col = if start_active { WHITE } else { ACCENT_HOV };
            // Four-pane launcher glyph.
            s_fill(s, sw, tile_x + 3, tile_y + 3, 5, 5, icon_col);
            s_fill(s, sw, tile_x + 10, tile_y + 3, 5, 5, icon_col);
            s_fill(s, sw, tile_x + 3, tile_y + 10, 5, 5, icon_col);
            s_fill(s, sw, tile_x + 10, tile_y + 10, 5, 5, icon_col);

            // ── Start menu — shell hub with pinned and quick-launch areas ─────────
            if self.start_menu_open {
                let menu_w = 460i32;
                let menu_h = 320i32;
                let left_w = 240i32;
                let right_w = menu_w - left_w;
                let bottom_h = 36i32;
                let left_hdr_h = 32i32;
                let menu_x = 2i32;
                let menu_y = taskbar_y - menu_h;
                let bar_y = menu_y + menu_h - bottom_h;
                let rc_x = menu_x + left_w + 1;
                let rc_w = right_w - 2;
                let usb_lines = crate::usb::status_lines();
                let (usb_keyboard, usb_mouse) = crate::usb::input_presence();
                let usb_live = usb_lines
                    .iter()
                    .any(|line| line.contains("active init ready"));

                s_fill(s, sw, menu_x + 4, menu_y + 4, menu_w, menu_h, 0x00_00_00_18);
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
                    "PINNED",
                    0x00_00_EE_FF,
                    left_hdr_bg,
                    menu_x + left_w - 12,
                );
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 10,
                    left_hdr_y + 20,
                    "quick access",
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
                    "Search programs and files",
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

                let item_h = 40i32;
                let items_y = menu_y + left_hdr_h + 8;
                let mut left_hov: Option<usize> = None;
                if mx_i > menu_x
                    && mx_i < menu_x + left_w
                    && my_i >= items_y
                    && my_i < items_y + item_h * START_PINNED_APPS.len() as i32
                {
                    left_hov = Some(((my_i - items_y) / item_h) as usize);
                }

                for (i, &name) in START_PINNED_APPS.iter().enumerate() {
                    let iy = items_y + i as i32 * item_h;
                    let is_hov = left_hov == Some(i);
                    let row_bg = if is_hov { 0x00_00_14_30 } else { 0x00_00_07_18 };
                    let acc = window_accent(name);
                    let glyph = window_glyph(name);
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
                    s_draw_str_small(
                        s,
                        sw,
                        menu_x + 40,
                        iy + 10,
                        name,
                        if is_hov { WHITE } else { 0x00_AA_DD_FF },
                        row_bg,
                        menu_x + left_w - 24,
                    );
                    if i + 1 < START_PINNED_APPS.len() {
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
                    av_x + 48,
                    av_y + 16,
                    "coolOS shell",
                    0x00_44_88_BB,
                    banner_bg,
                    banner_x + banner_w - 10,
                );

                let chip_y = banner_y + 56;
                let chip_w = 54;
                let chip_h = 16;
                let chip_gap = 6;
                let chip_bg = 0x00_00_07_18;
                s_fill(s, sw, banner_x + 8, chip_y, chip_w, chip_h, chip_bg);
                draw_rect_border(s, sw, banner_x + 8, chip_y, chip_w, chip_h, 0x00_00_33_66);
                s_draw_str_small(
                    s,
                    sw,
                    banner_x + 18,
                    chip_y + 4,
                    "USB",
                    if usb_live {
                        0x00_00_FF_AA
                    } else {
                        0x00_44_88_BB
                    },
                    chip_bg,
                    banner_x + 8 + chip_w - 4,
                );
                let chip2_x = banner_x + 8 + chip_w + chip_gap;
                s_fill(s, sw, chip2_x, chip_y, chip_w, chip_h, chip_bg);
                draw_rect_border(s, sw, chip2_x, chip_y, chip_w, chip_h, 0x00_00_33_66);
                s_draw_str_small(
                    s,
                    sw,
                    chip2_x + 12,
                    chip_y + 4,
                    "KBD",
                    if usb_keyboard {
                        0x00_00_FF_AA
                    } else {
                        0x00_44_88_BB
                    },
                    chip_bg,
                    chip2_x + chip_w - 4,
                );
                let chip3_x = chip2_x + chip_w + chip_gap;
                s_fill(s, sw, chip3_x, chip_y, chip_w, chip_h, chip_bg);
                draw_rect_border(s, sw, chip3_x, chip_y, chip_w, chip_h, 0x00_00_33_66);
                s_draw_str_small(
                    s,
                    sw,
                    chip3_x + 12,
                    chip_y + 4,
                    "MSE",
                    if usb_mouse {
                        0x00_FF_DD_55
                    } else {
                        0x00_44_88_BB
                    },
                    chip_bg,
                    chip3_x + chip_w - 4,
                );

                const SYS_LINKS: [&str; 9] = [
                    "Documents",
                    "Downloads",
                    "Pictures",
                    "Music",
                    "Videos",
                    "Desktop",
                    "Home",
                    "Shared",
                    "Trash",
                ];
                let link_h = 24i32;
                let links_y = banner_y + banner_h + 8;
                for (i, &link_name) in SYS_LINKS.iter().enumerate() {
                    let ly = links_y + i as i32 * link_h;
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
                    s_draw_str_small(
                        s,
                        sw,
                        rc_x + 10,
                        ly + 8,
                        link_name,
                        if is_hov { WHITE } else { 0x00_88_CC_FF },
                        link_bg,
                        rc_x + rc_w - 4,
                    );
                }
            }

            // ── Taskbar window tabs — icon-first strip ───────────────────────────
            let taskbar_btn_x0 = START_BTN_W + 8;
            let show_desktop_x = sw as i32 - TASKBAR_CLOCK_W - SHOW_DESKTOP_W - 8;
            let hovered_btn = if mx_i >= taskbar_btn_x0
                && mx_i < show_desktop_x - 6
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H
            {
                let idx = ((mx_i - taskbar_btn_x0) / (BUTTON_W + 6)) as usize;
                let bx = taskbar_btn_x0 + idx as i32 * (BUTTON_W + 6);
                if mx_i < bx + BUTTON_W {
                    Some(idx)
                } else {
                    None
                }
            } else {
                None
            };

            for i in 0..self.windows.len() {
                let bx = taskbar_btn_x0 + i as i32 * (BUTTON_W + 6);
                if bx + BUTTON_W > show_desktop_x - 6 {
                    break;
                }

                let focused = self.focused == Some(i);
                let minimized = self.windows[i].is_minimized();
                let hovered = hovered_btn == Some(i);
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
                let brand_w = 6 * 8;
                let brand_x = clock_box_x + (clock_box_w - brand_w) / 2;
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

            // ── Context menu ──────────────────────────────────────────────────────
            if let Some(ref cm) = self.context_menu {
                let menu_h = CTX_HEADER_H + CTX_PAD * 2 + CTX_ITEM_H * CTX_ITEMS.len() as i32;

                // Drop shadow
                s_fill(s, sw, cm.x + 4, cm.y + 4, CTX_W, menu_h, 0x00_00_00_24);

                // Background — darker, richer navy
                s_fill(s, sw, cm.x, cm.y, CTX_W, menu_h, 0x00_00_09_1E);

                // Outer border + inner inset
                draw_rect_border(s, sw, cm.x, cm.y, CTX_W, menu_h, 0x00_00_BB_EE);
                draw_rect_border(
                    s,
                    sw,
                    cm.x + 1,
                    cm.y + 1,
                    CTX_W - 2,
                    menu_h - 2,
                    0x00_00_33_55,
                );

                // ── Header strip ──────────────────────────────────────────────────
                let hdr_bg = 0x00_00_0E_28;
                s_fill(
                    s,
                    sw,
                    cm.x + 1,
                    cm.y + 1,
                    CTX_W - 2,
                    CTX_HEADER_H - 1,
                    hdr_bg,
                );
                // Accent pip + "Open App" label
                s_fill(s, sw, cm.x + 10, cm.y + 12, 4, 4, ACCENT);
                s_draw_str_small(
                    s,
                    sw,
                    cm.x + 18,
                    cm.y + 10,
                    "Open App",
                    0x00_44_88_BB,
                    hdr_bg,
                    cm.x + CTX_W - 4,
                );
                // Header bottom rule
                s_fill(
                    s,
                    sw,
                    cm.x + 1,
                    cm.y + CTX_HEADER_H - 1,
                    CTX_W - 2,
                    1,
                    0x00_00_33_66,
                );

                // Per-app accent colours, matching the desktop icon set
                const CTX_ACCENTS: [u32; 5] = [
                    ICON_TERM_ACC,
                    ICON_MON_ACC,
                    ICON_TXT_ACC,
                    ICON_COL_ACC,
                    0x00_55_DD_FF,
                ];
                const CTX_GLYPHS: [&str; 5] = ["T>", "M#", "Tx", "CP", "FM"];

                for (i, &label) in CTX_ITEMS.iter().enumerate() {
                    let item_y = cm.y + CTX_HEADER_H + CTX_PAD + i as i32 * CTX_ITEM_H;
                    let hot = mx_i >= cm.x
                        && mx_i < cm.x + CTX_W
                        && my_i >= item_y
                        && my_i < item_y + CTX_ITEM_H;
                    let acc = CTX_ACCENTS[i % CTX_ACCENTS.len()];

                    // Hover: subtle tint row + left accent bar (no full solid fill)
                    if hot {
                        let hov_bg = 0x00_00_12_2C;
                        s_fill(s, sw, cm.x + 1, item_y, CTX_W - 2, CTX_ITEM_H - 1, hov_bg);
                        s_fill(s, sw, cm.x + 1, item_y + 5, 3, CTX_ITEM_H - 11, ACCENT);
                    }

                    // Coloured icon tile identifying the app.
                    let icon_x = cm.x + 8;
                    let icon_y = item_y + 5;
                    let icon_bg = blend_color(0x00_00_0B_20, acc, 65);
                    s_fill(s, sw, icon_x, icon_y, 20, 20, icon_bg);
                    draw_rect_border(s, sw, icon_x, icon_y, 20, 20, blend_color(acc, WHITE, 90));
                    s_draw_str_small(
                        s,
                        sw,
                        icon_x + 4,
                        icon_y + 6,
                        CTX_GLYPHS[i],
                        acc,
                        icon_bg,
                        icon_x + 18,
                    );

                    // Label
                    let text_col = if hot { WHITE } else { 0x00_88_CC_FF };
                    let text_bg = if hot { 0x00_00_12_2C } else { 0x00_00_07_18 };
                    s_draw_str_small(
                        s,
                        sw,
                        cm.x + 34,
                        item_y + (CTX_ITEM_H - 8) / 2,
                        label,
                        text_col,
                        text_bg,
                        cm.x + CTX_W - 4,
                    );

                    // Separator (not after last item)
                    if i + 1 < CTX_ITEMS.len() {
                        let sep_col = if hot { 0x00_00_10_22 } else { 0x00_00_1A_33 };
                        s_fill(
                            s,
                            sw,
                            cm.x + 8,
                            item_y + CTX_ITEM_H - 1,
                            CTX_W - 16,
                            1,
                            sep_col,
                        );
                    }
                }
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

        // ── Blit shadow → hardware framebuffer ───────────────────────────────
        let hw_base = crate::framebuffer::base();
        let hw_stride = crate::framebuffer::stride();
        let hw_bpp = crate::framebuffer::bpp();
        let hw_fmt = crate::framebuffer::fmt();
        let is_rgb = hw_fmt == crate::framebuffer::PixFmt::Rgb;
        if hw_base != 0 {
            match hw_bpp {
                4 => {
                    for row in 0..sh {
                        let src = &self.shadow[row * sw..row * sw + sw];
                        let row_base = hw_base + (row * hw_stride * 4) as u64;
                        let dst = row_base as *mut u32;
                        if !is_rgb {
                            unsafe {
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, sw);
                            }
                        } else {
                            for col in 0..sw {
                                let c = src[col];
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
                        let src = &self.shadow[row * sw..row * sw + sw];
                        let row_base = hw_base + (row * hw_stride * 3) as u64;
                        if !is_rgb {
                            for col in 0..sw {
                                let c = src[col];
                                scratch[col * 3] = c as u8;
                                scratch[col * 3 + 1] = (c >> 8) as u8;
                                scratch[col * 3 + 2] = (c >> 16) as u8;
                            }
                        } else {
                            for col in 0..sw {
                                let c = src[col];
                                scratch[col * 3] = (c >> 16) as u8;
                                scratch[col * 3 + 1] = (c >> 8) as u8;
                                scratch[col * 3 + 2] = c as u8;
                            }
                        }
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                scratch.as_ptr(),
                                row_base as *mut u8,
                                row_bytes,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn front_to_back_hit(&self, px: i32, py: i32) -> Option<usize> {
        for z_pos in (0..self.z_order.len()).rev() {
            let wi = self.z_order[z_pos];
            if wi < self.windows.len()
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
        let icon_x = w.x + 8;
        let icon_y = w.y + 5;
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
            w.y + (TITLE_H - 8) / 2,
            w.title,
            title_fg,
            title_bg,
            max_title_x,
        );

        // ── Caption buttons — CRT phosphor style ──────────────────────────────
        let btn_y = w.y + 1;
        let btn_h = TITLE_H - 2;

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
            btn_y + btn_h / 2 + 1,
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
            btn_y + btn_h / 2 - 4,
            8,
            1,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 + 3,
            8,
            1,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 - 4,
            1,
            8,
            max_glyph,
        );
        s_fill(
            s,
            sw,
            max_x + WIN_BTN_W / 2 + 3,
            btn_y + btn_h / 2 - 4,
            1,
            8,
            max_glyph,
        );

        // Close  ✕ — pixel diagonals
        let cx_c = cls_x + WIN_BTN_W / 2;
        let cy_c = btn_y + btn_h / 2;
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
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            let px = x + bit as i32;
            let py = y + gy as i32;
            if px >= 0 && py >= 0 {
                let (px, py) = (px as usize, py as usize);
                if px < sw && py < sh {
                    s[py * sw + px] = color;
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
