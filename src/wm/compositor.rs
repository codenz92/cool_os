/// Window compositor — desktop, windows, taskbar, cursor, context menu.
/// All rendering targets a `Vec<u32>` shadow buffer; one blit per frame.
///
/// Visual theme: Retro-Futuristic CRT Phosphor Blue
extern crate alloc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::apps::{ColorPickerApp, SysMonApp, TerminalApp, TextViewerApp};
use crate::framebuffer::{BLACK, CHAR_W, WHITE};
use crate::wm::window::{Window, TITLE_H};

// ── Layout constants ──────────────────────────────────────────────────────────

const TASKBAR_H: i32 = 40; // Win11: 40px tall taskbar
const START_BTN_W: i32 = 86; // 5 chars × 16px + 4px pad each side
const TASKBAR_CLOCK_W: i32 = 108; // "00:00" (5×16=80) + padding; "coolOS" (6×16=96) + padding
const BUTTON_W: i32 = 160;
const WIN_BTN_W: i32 = crate::wm::window::WIN_BTN_W;
const EVENT_PACKET_SIZE: usize = 8;
const EVENT_KIND_KEY_CHAR: u8 = 1;
const EVENT_KIND_MOUSE_DOWN: u8 = 2;

// ── Colors — Retro-Futuristic CRT Phosphor Blue ───────────────────────────────

// Taskbar / shell
const TASKBAR_BG: u32 = 0x00_00_07_14; // #000714  deep CRT navy-black
const TASKBAR_BORD: u32 = 0x00_00_66_CC; // #0066CC  phosphor blue hairline

// Accent (CRT phosphor blue)
const ACCENT: u32 = 0x00_00_99_FF; // #0099FF  bright phosphor blue
const ACCENT_HOV: u32 = 0x00_33_BB_FF; // #33BBFF  lit hover
const ACCENT_PRESS: u32 = 0x00_00_66_CC; // #0066CC  depressed

// Window chrome (CRT dark mode)
const WIN_BAR_F: u32 = 0x00_00_10_28; // #001028  focused title bar — deep navy
const WIN_BAR_U: u32 = 0x00_00_07_14; // #000714  unfocused — near-black
const WIN_CONTENT: u32 = 0x00_00_09_1C; // #00091C  window body
const WIN_BDR_F: u32 = 0x00_00_99_FF; // #0099FF  focused border — phosphor glow
const WIN_BDR_U: u32 = 0x00_00_33_66; // #003366  unfocused — dim blue

// Window caption buttons
const CAP_NORMAL: u32 = 0x00_00_10_28; // same as title bar
const CAP_HOV: u32 = 0x00_00_22_44; // slightly lighter navy
const CLOSE_REST: u32 = 0x00_00_10_28; // close resting
const CLOSE_HOV: u32 = 0x00_BB_11_11; // #BB1111  red-CRT close

// Desktop wallpaper — deep space phosphor
const DESK_TL: u32 = 0x00_00_02_08; // top-left  pitch black with blue ghost
const DESK_TR: u32 = 0x00_00_03_0C; // top-right
const DESK_BL: u32 = 0x00_00_01_06; // bottom-left
const DESK_BR: u32 = 0x00_00_02_0A; // bottom-right
                                    // CRT phosphor glow toward screen centre
const BLOOM_1: u32 = 0x00_00_44_AA; // primary phosphor blue bloom
const BLOOM_2: u32 = 0x00_00_22_66; // secondary deep bloom

// Desktop icons — CRT phosphor colour set
const ICON_TERM_BG: u32 = 0x00_00_16_08; // terminal — dark green-black
const ICON_TERM_ACC: u32 = 0x00_00_FF_88; // #00FF88  phosphor green
const ICON_MON_BG: u32 = 0x00_00_0E_1E; // monitor — deep blue-black
const ICON_MON_ACC: u32 = 0x00_00_EE_FF; // #00EEFF  cyan phosphor
const ICON_TXT_BG: u32 = 0x00_00_0A_22; // text — navy
const ICON_TXT_ACC: u32 = 0x00_00_99_FF; // #0099FF  blue phosphor
const ICON_COL_BG: u32 = 0x00_10_00_1E; // colour — dark purple-black
const ICON_COL_ACC: u32 = 0x00_AA_44_FF; // #AA44FF  violet phosphor

const ICON_SEL: u32 = 0x00_00_33_77;

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

// ── Context menu ──────────────────────────────────────────────────────────────

const CTX_W: i32 = 210;
const CTX_ITEM_H: i32 = 28;
const CTX_ITEMS: &[&str] = &["Terminal", "System Mon", "Text Viewer", "Color Pick"];

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

fn desktop_icons() -> [DesktopIcon; 4] {
    // Column stride: ICON_SIZE(64) + 104px gap = 168 — gives 8-char label room before next tile.
    // Row stride:    ICON_SIZE(64) + ICON_LABEL_H(14) + 20px gap = 98.
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
            label: "Sys Mon",
            app: "System Mon",
        },
        DesktopIcon {
            x: 20,
            y: 118,
            label: "Text View",
            app: "Text Viewer",
        },
        DesktopIcon {
            x: 188,
            y: 118,
            label: "Color Pick",
            app: "Color Pick",
        },
    ]
}

// ── Drag state ────────────────────────────────────────────────────────────────

struct DragState {
    window: usize,
    off_x: i32,
    off_y: i32,
}

// ── AppWindow ─────────────────────────────────────────────────────────────────

pub enum AppWindow {
    Terminal(TerminalApp),
    SysMon(SysMonApp),
    TextViewer(TextViewerApp),
    ColorPicker(ColorPickerApp),
}

impl AppWindow {
    pub fn window(&self) -> &Window {
        match self {
            AppWindow::Terminal(t) => &t.window,
            AppWindow::SysMon(s) => &s.window,
            AppWindow::TextViewer(v) => &v.window,
            AppWindow::ColorPicker(c) => &c.window,
        }
    }
    pub fn window_mut(&mut self) -> &mut Window {
        match self {
            AppWindow::Terminal(t) => &mut t.window,
            AppWindow::SysMon(s) => &mut s.window,
            AppWindow::TextViewer(v) => &mut v.window,
            AppWindow::ColorPicker(c) => &mut c.window,
        }
    }
    pub fn handle_key(&mut self, c: char) {
        match self {
            AppWindow::Terminal(t) => t.handle_key(c),
            AppWindow::TextViewer(v) => v.handle_key(c),
            _ => {}
        }
    }
    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        if let AppWindow::ColorPicker(cp) = self {
            cp.handle_click(lx, ly);
        }
    }
    pub fn update(&mut self) {
        if let AppWindow::SysMon(s) = self {
            s.update();
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
    prev_left: bool,
    prev_right: bool,
    context_menu: Option<ContextMenu>,
    icon_selected: Option<usize>,
    start_menu_open: bool,
    /// Frame counter — used to drive the uptime clock display.
    /// Replace with a real RTC read once the kernel time API is wired up.
    tick: u64,
    /// Shadow buffer — screen_width × screen_height u32 pixels.
    shadow: Vec<u32>,
    shadow_width: usize,
    shadow_height: usize,
    /// Pre-baked wallpaper pixels — computed once in new(), blitted each frame.
    wallpaper: Vec<u32>,
}

impl WindowManager {
    pub fn new() -> Self {
        let w = crate::framebuffer::width();
        let h = crate::framebuffer::height();
        let taskbar_y = h - TASKBAR_H as usize;
        let mut wallpaper = alloc::vec![0u32; w * h];
        let (fw, fh) = (w as f32, taskbar_y as f32);
        for y in 0..taskbar_y {
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
                let dy = ty - 0.40;
                let dist_sq = dx * dx + dy * dy;
                let t_b = 1.0f32 - (dist_sq / 0.3025f32).min(1.0f32);
                let bloom = t_b * t_b * t_b;
                let br = (r as f32 + bloom * ((BLOOM_1 >> 16) as u8 as f32)).min(255.0) as u32;
                let bg = (g as f32 + bloom * ((BLOOM_1 >> 8) as u8 as f32)).min(255.0) as u32;
                let bb = (b as f32 + bloom * (BLOOM_1 as u8 as f32)).min(255.0) as u32;

                // ── CRT scanline — every even row dimmed ~18% ─────────────────────
                let scan: u32 = if y % 2 == 0 { 210 } else { 255 };

                // ── Phosphor triad dot-mask — column 2 of every 3 gets blue boost ─
                let dot_boost: u32 = if x % 3 == 2 { 10 } else { 0 };

                let fr = br * scan / 255;
                let fg = bg * scan / 255;
                let fb = (bb * scan / 255).saturating_add(dot_boost).min(255);

                wallpaper[y * w + x] = (fr << 16) | (fg << 8) | fb;
            }
        }

        WindowManager {
            windows: Vec::new(),
            z_order: Vec::new(),
            focused: None,
            key_sink_fd: None,
            key_sink_window: None,
            drag: None,
            prev_left: false,
            prev_right: false,
            context_menu: None,
            icon_selected: None,
            start_menu_open: false,
            tick: 0,
            shadow: alloc::vec![0u32; w * h],
            shadow_width: w,
            shadow_height: h,
            wallpaper,
        }
    }

    pub fn add_window(&mut self, w: AppWindow) {
        let idx = self.windows.len();
        self.windows.push(w);
        self.z_order.push(idx);
        self.focused = Some(idx);
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

        // Advance uptime counter and snapshot it for use inside the shadow borrow block.
        self.tick = self.tick.wrapping_add(1);
        let current_tick = self.tick;

        let (mx, my) = crate::mouse::pos();
        let (left, right) = crate::mouse::buttons();
        let mx_i = mx as i32;
        let my_i = my as i32;

        let left_pressed = left && !self.prev_left;
        let left_released = !left && self.prev_left;
        let right_pressed = right && !self.prev_right;

        // ── Input ─────────────────────────────────────────────────────────────

        // Start button click — flush left, full height
        let taskbar_click = left_pressed && my_i >= taskbar_y && mx_i < START_BTN_W + 4;
        if taskbar_click {
            self.start_menu_open = !self.start_menu_open;
            self.context_menu = None;
            crate::wm::request_repaint();
        }

        if right_pressed && self.front_to_back_hit(mx_i, my_i).is_none() {
            let cx = mx_i.min(sw as i32 - CTX_W);
            let cy = my_i.min(taskbar_y - CTX_ITEM_H * CTX_ITEMS.len() as i32);
            self.context_menu = Some(ContextMenu { x: cx, y: cy });
        }

        if left_pressed {
            if self.context_menu.is_some() {
                let clicked: Option<&str> = {
                    let cm = self.context_menu.as_ref().unwrap();
                    CTX_ITEMS.iter().enumerate().find_map(|(i, &label)| {
                        let item_y = cm.y + i as i32 * CTX_ITEM_H;
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
                match clicked {
                    Some("Terminal") => {
                        self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy)))
                    }
                    Some("System Mon") => {
                        self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy)))
                    }
                    Some("Text Viewer") => {
                        self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy)))
                    }
                    Some("Color Pick") => {
                        self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy)))
                    }
                    _ => {}
                }
            } else {
                if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
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
                    }
                }

                if my_i >= taskbar_y {
                    let btn_x = (mx_i - 2) / (BUTTON_W + 2);
                    if btn_x >= 0 {
                        let btn_x = btn_x as usize;
                        if btn_x < self.windows.len() {
                            if self.windows[btn_x].is_minimized() {
                                self.windows[btn_x].window_mut().restore();
                            }
                            if let Some(z_pos) = self.z_order.iter().position(|&i| i == btn_x) {
                                self.z_order.remove(z_pos);
                                self.z_order.push(btn_x);
                                self.focused = Some(btn_x);
                            }
                            crate::wm::request_repaint();
                        }
                    }
                }
            }
        }

        if left_released {
            self.drag = None;
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
        }

        self.prev_left = left;
        self.prev_right = right;

        // Start menu item click.
        if left_pressed && self.start_menu_open {
            let menu_w = 280i32;
            let menu_h = 240i32;
            let menu_x = 2i32;
            let menu_y = taskbar_y - menu_h;
            if mx_i >= menu_x && mx_i < menu_x + menu_w && my_i >= menu_y && my_i < taskbar_y {
                // Header region (40px) + search bar (36px) = 76px before items
                let items_start_y = menu_y + 76;
                if my_i >= items_start_y {
                    let item_idx = ((my_i - items_start_y) / 40) as usize;
                    let apps: [&str; 4] = ["Terminal", "System Mon", "Text Viewer", "Color Pick"];
                    if item_idx < apps.len() {
                        let off = self.windows.len() as i32 * 16;
                        let wx = (10 + off).min(sw as i32 - 200);
                        let wy = (10 + off).min(menu_y - 80);
                        match apps[item_idx] {
                            "Terminal" => {
                                self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy)))
                            }
                            "System Mon" => {
                                self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy)))
                            }
                            "Text Viewer" => {
                                self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy)))
                            }
                            "Color Pick" => {
                                self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy)))
                            }
                            _ => {}
                        }
                        self.start_menu_open = false;
                        crate::wm::request_repaint();
                    }
                }
            }
        }

        // Desktop icon click.
        if left_pressed {
            let icons = desktop_icons();
            for (i, icon) in icons.iter().enumerate() {
                if icon.hit(mx_i, my_i) {
                    self.icon_selected = Some(i);
                    self.context_menu = None;
                    crate::wm::request_repaint();
                }
            }
        }

        if left_released {
            let icons = desktop_icons();
            for icon in icons.iter() {
                if icon.hit(mx_i, my_i) {
                    let off = self.windows.len() as i32 * 16;
                    let wx = (10 + off).min(sw as i32 - 200);
                    let wy = (10 + off).min(taskbar_y - 80);
                    match icon.app {
                        "Terminal" => {
                            self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy)))
                        }
                        "System Mon" => self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy))),
                        "Text Viewer" => {
                            self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy)))
                        }
                        "Color Pick" => {
                            self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy)))
                        }
                        _ => {}
                    }
                    crate::wm::request_repaint();
                }
            }
        }

        // ── Render ────────────────────────────────────────────────────────────
        // Blit wallpaper before taking the exclusive &mut shadow borrow,
        // so the compiler sees two separate borrows of self.shadow / self.wallpaper.
        {
            let desk_pixels = taskbar_y as usize * sw;
            self.shadow[..desk_pixels].copy_from_slice(&self.wallpaper[..desk_pixels]);
        }
        {
            let s = &mut self.shadow;

            for w in self.windows.iter_mut() {
                w.update();
            }

            // ── Desktop icons — drawn BEFORE windows so windows can cover them ────
            let icon_data: [(u32, u32); 4] = [
                (ICON_TERM_BG, ICON_TERM_ACC),
                (ICON_MON_BG, ICON_MON_ACC),
                (ICON_TXT_BG, ICON_TXT_ACC),
                (ICON_COL_BG, ICON_COL_ACC),
            ];
            for (i, icon) in desktop_icons().iter().enumerate() {
                let selected = self.icon_selected == Some(i);
                let hot = mx_i >= icon.x
                    && mx_i < icon.x + ICON_SIZE
                    && my_i >= icon.y
                    && my_i < icon.y + ICON_SIZE;

                let (icon_bg, icon_acc) = icon_data[i];

                // Drop shadow
                s_fill(
                    s,
                    sw,
                    icon.x + 3,
                    icon.y + 3,
                    ICON_SIZE,
                    ICON_SIZE,
                    0x00_00_00_18,
                );

                // Tile background
                let tile_bg = if selected || hot {
                    blend_color(icon_bg, 0x00_FF_FF_FF, 30)
                } else {
                    icon_bg
                };
                s_fill(s, sw, icon.x, icon.y, ICON_SIZE, ICON_SIZE, tile_bg);

                // Accent top band
                s_fill(s, sw, icon.x, icon.y, ICON_SIZE, 4, icon_acc);

                // ── Per-app pixel-art icon ────────────────────────────────────────
                match i {
                    0 => {
                        // Terminal — pixelated ">" prompt + underscore cursor (scaled for 52px)
                        let bx = icon.x + 8;
                        let by = icon.y + 14;
                        // Top arm of >
                        s_fill(s, sw, bx, by, 6, 3, icon_acc);
                        s_fill(s, sw, bx + 6, by + 3, 6, 3, icon_acc);
                        // Apex
                        s_fill(s, sw, bx + 12, by + 6, 6, 3, icon_acc);
                        // Bottom arm of >
                        s_fill(s, sw, bx + 6, by + 9, 6, 3, icon_acc);
                        s_fill(s, sw, bx, by + 12, 6, 3, icon_acc);
                        // Cursor underscore
                        s_fill(s, sw, icon.x + 8, icon.y + 38, 22, 2, icon_acc);
                    }
                    1 => {
                        // System Monitor — bar chart (scaled for 52px)
                        let base_y = icon.y + 44;
                        let bar_w = 8i32;
                        s_fill(s, sw, icon.x + 6, base_y - 10, bar_w, 10, icon_acc);
                        s_fill(s, sw, icon.x + 18, base_y - 26, bar_w, 26, icon_acc);
                        s_fill(s, sw, icon.x + 30, base_y - 18, bar_w, 18, icon_acc);
                        // Baseline
                        s_fill(s, sw, icon.x + 4, base_y, 36, 2, icon_acc);
                    }
                    2 => {
                        // Text Viewer — document page (scaled for 52px)
                        draw_rect_border(s, sw, icon.x + 8, icon.y + 6, 36, 40, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 10, 28, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 16, 22, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 22, 28, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 28, 16, 2, icon_acc);
                        s_fill(s, sw, icon.x + 11, icon.y + 34, 22, 2, icon_acc);
                    }
                    _ => {
                        // Color Picker — four colour quadrants (scaled for 52px)
                        s_fill(s, sw, icon.x + 6, icon.y + 8, 16, 16, 0x00_FF_50_50); // red
                        s_fill(s, sw, icon.x + 26, icon.y + 8, 16, 16, 0x00_50_FF_50); // green
                        s_fill(s, sw, icon.x + 6, icon.y + 28, 16, 16, 0x00_50_50_FF); // blue
                        s_fill(s, sw, icon.x + 26, icon.y + 28, 16, 16, 0x00_FF_FF_50); // yellow
                                                                                        // Central accent pip
                        s_fill(s, sw, icon.x + 20, icon.y + 20, 10, 10, icon_acc);
                    }
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

                // Label below icon — centred under tile with readable pill background
                // Uses 8 px (1×) font so labels stay compact under the 64 px tile.
                let label_y = icon.y + ICON_SIZE + 8;
                let label_w = icon.label.len() as i32 * 8; // 8px per char at 1× scale
                let label_x = (icon.x + (ICON_SIZE - label_w) / 2).max(1);
                let label_bg = if selected { ICON_SEL } else { 0x00_00_04_0E };
                // Pill — sized for 8 px tall glyphs
                s_fill(
                    s,
                    sw,
                    label_x - 3,
                    label_y - 2,
                    label_w + 6,
                    12, // 8px glyph + 2px pad top + 2px pad bottom
                    label_bg,
                );
                s_draw_str_small(
                    s,
                    sw,
                    label_x,
                    label_y,
                    icon.label,
                    if selected {
                        0x00_CC_EE_FF
                    } else {
                        0x00_88_CC_FF
                    },
                    label_bg,
                    label_x + label_w + 1,
                );
            }

            // ── Windows — drawn AFTER icons so they appear in front ───────────────
            let z: Vec<usize> = self.z_order.clone();
            for &wi in &z {
                if wi < self.windows.len() {
                    let win = self.windows[wi].window();
                    if !win.minimized {
                        let focused = self.focused == Some(wi);
                        Self::draw_window(s, sw, win, focused);
                    }
                }
            }

            // ── Taskbar — frosted glass panel ────────────────────────────────────
            // Step 1: Darken + blue-tint the wallpaper underneath to fake frosted glass.
            {
                let t0 = taskbar_y as usize;
                let t1 = (t0 + TASKBAR_H as usize).min(s.len() / sw);
                for row in t0..t1 {
                    for col in 0..sw {
                        let p = s[row * sw + col];
                        let r = ((p >> 16) & 0xFF) * 16 / 100;
                        let g = ((p >> 8) & 0xFF) * 18 / 100;
                        let b = ((p & 0xFF) * 28 / 100).saturating_add(22).min(255);
                        s[row * sw + col] = (r << 16) | (g << 8) | b;
                    }
                }
            }
            // Step 2: Bright 2-px top accent border — phosphor blue glow line.
            s_fill(s, sw, 0, taskbar_y, sw as i32, 1, 0x00_00_99_EE);
            s_fill(s, sw, 0, taskbar_y + 1, sw as i32, 1, 0x00_00_33_66);

            // ── Start button — floating pill with power icon ──────────────────────
            let start_hot = mx_i >= 0
                && mx_i < START_BTN_W + 4
                && my_i >= taskbar_y
                && my_i < taskbar_y + TASKBAR_H;
            let start_pressed = left && start_hot;
            let start_active = self.start_menu_open || start_pressed;

            let btn_x = 0i32;
            let btn_w = START_BTN_W + 4; // 90px hit area

            // Pill: visually centered and smaller than the hit zone
            let pill_w = 58i32;
            let pill_h = 28i32;
            let pill_x = btn_x + (btn_w - pill_w) / 2;
            let pill_y = taskbar_y + (TASKBAR_H - pill_h) / 2;

            // Fill: none at rest, tint on hover, accent on active
            if start_active {
                s_fill(s, sw, pill_x, pill_y, pill_w, pill_h, ACCENT_PRESS);
            } else if start_hot {
                s_fill(s, sw, pill_x, pill_y, pill_w, pill_h, 0x00_00_20_48);
            }

            // Border: always visible — brighter when active
            let pill_bord = if start_active { ACCENT_HOV } else { ACCENT };
            draw_rect_border(s, sw, pill_x, pill_y, pill_w, pill_h, pill_bord);
            // Second inner border line when menu is open (depth effect)
            if self.start_menu_open {
                draw_rect_border(
                    s,
                    sw,
                    pill_x + 1,
                    pill_y + 1,
                    pill_w - 2,
                    pill_h - 2,
                    0x00_00_44_88,
                );
            }

            // ── Power button icon — 18×20px, centered in pill ─────────────────────
            // Each conceptual dot = 2×2 actual pixels
            // Grid (9 cols × 10 rows):
            //   col:  0 1 2 3 4 5 6 7 8
            //   row0: . . . . # . . . .   stem
            //   row1: . . . . # . . . .   stem
            //   row2: . . . . # . . . .   stem
            //   row3: . . # . . . # . .   arc upper
            //   row4: . # . . . . . # .   arc
            //   row5: # . . . . . . . #   arc sides
            //   row6: # . . . . . . . #   arc sides
            //   row7: # . . . . . . . #   arc sides
            //   row8: . # . . . . . # .   arc lower
            //   row9: . . # # # # # . .   arc bottom
            let ic = if start_active { WHITE } else { ACCENT };
            let ix = pill_x + (pill_w - 18) / 2; // = pill_x + 20
            let iy = pill_y + (pill_h - 20) / 2; // = pill_y + 4

            // Stem (rows 0-2, col 4)
            s_fill(s, sw, ix + 8, iy, 2, 6, ic);
            // Upper arc corners (row 3)
            s_fill(s, sw, ix + 4, iy + 6, 2, 2, ic); // col 2
            s_fill(s, sw, ix + 12, iy + 6, 2, 2, ic); // col 6
                                                      // Mid arc (row 4)
            s_fill(s, sw, ix + 2, iy + 8, 2, 2, ic); // col 1
            s_fill(s, sw, ix + 14, iy + 8, 2, 2, ic); // col 7
                                                      // Straight sides (rows 5-7, cols 0 and 8)
            s_fill(s, sw, ix + 0, iy + 10, 2, 6, ic);
            s_fill(s, sw, ix + 16, iy + 10, 2, 6, ic);
            // Lower arc (row 8)
            s_fill(s, sw, ix + 2, iy + 16, 2, 2, ic); // col 1
            s_fill(s, sw, ix + 14, iy + 16, 2, 2, ic); // col 7
                                                       // Bottom arc (row 9, cols 2-6 = 10px wide)
            s_fill(s, sw, ix + 4, iy + 18, 10, 2, ic);

            // Thin right-edge separator
            let sep_x = btn_w;
            s_fill(s, sw, sep_x, taskbar_y + 4, 1, TASKBAR_H - 8, 0x00_00_66_AA);

            // ── Start menu — Win11 dark acrylic style ─────────────────────────────
            if self.start_menu_open {
                let menu_w = 280i32;
                let menu_h = 240i32;
                let menu_x = 2i32;
                let menu_y = taskbar_y - menu_h;

                // Shadow
                s_fill(
                    s,
                    sw,
                    menu_x + 4,
                    menu_y + 4,
                    menu_w + 4,
                    menu_h + 4,
                    0x00_00_02_06,
                );

                // Background — deep CRT navy
                s_fill(s, sw, menu_x, menu_y, menu_w, menu_h, 0x00_00_08_1C);

                // Outer border — phosphor blue
                draw_rect_border(s, sw, menu_x, menu_y, menu_w, menu_h, 0x00_00_55_AA);

                // Header: "coolOS" branding strip
                s_fill(s, sw, menu_x + 1, menu_y + 1, menu_w - 2, 36, 0x00_00_10_2C);
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 12,
                    menu_y + (36 - 8) / 2,
                    "coolOS",
                    0x00_88_CC_FF,
                    0x00_00_10_2C,
                    menu_x + menu_w - 4,
                );
                // Small accent dot next to brand (6 chars × 8px = 48px → dot at +62)
                s_fill(s, sw, menu_x + 62, menu_y + 14, 4, 4, ACCENT);

                // Search bar
                let srch_y = menu_y + 40;
                s_fill(s, sw, menu_x + 8, srch_y, menu_w - 16, 28, 0x00_00_0C_22);
                draw_rect_border(s, sw, menu_x + 8, srch_y, menu_w - 16, 28, 0x00_00_44_88);
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 16,
                    srch_y + (28 - 8) / 2,
                    "Search",
                    0x00_00_55_99,
                    0x00_00_0C_22,
                    menu_x + menu_w - 12,
                );

                // "Pinned" section header
                s_draw_str_small(
                    s,
                    sw,
                    menu_x + 12,
                    menu_y + 74,
                    "Pinned",
                    0x00_00_77_BB,
                    0x00_00_08_1C,
                    menu_x + menu_w - 4,
                );

                // App entries
                let apps: [(&str, u32, &str); 4] = [
                    ("Terminal", ICON_TERM_ACC, "T>"),
                    ("System Mon", ICON_MON_ACC, "M#"),
                    ("Text View", ICON_TXT_ACC, "E "),
                    ("Color Pick", ICON_COL_ACC, "CP"),
                ];

                let mut hover_idx: Option<usize> = None;
                let items_y = menu_y + 88;
                if mx_i >= menu_x + 1
                    && mx_i < menu_x + menu_w - 1
                    && my_i >= items_y
                    && my_i < items_y + 40 * 4
                {
                    hover_idx = Some(((my_i - items_y) / 40) as usize);
                }

                for (i, (name, acc, glyph)) in apps.iter().enumerate() {
                    let iy = items_y + i as i32 * 40;
                    let is_hov = hover_idx == Some(i);

                    let row_bg = if is_hov { 0x00_00_18_38 } else { 0x00_00_08_1C };
                    s_fill(s, sw, menu_x + 1, iy, menu_w - 2, 38, row_bg);

                    // Coloured app icon square: dim accent fill + bright accent top band + glyph
                    let icon_sq_bg = blend_color(0x00_00_0C_22, *acc, 55); // ~22% accent tint
                    s_fill(s, sw, menu_x + 10, iy + 7, 24, 24, icon_sq_bg);
                    s_fill(s, sw, menu_x + 10, iy + 7, 24, 3, *acc); // accent top band
                    s_draw_str_small(
                        s,
                        sw,
                        menu_x + 12,
                        iy + 14,
                        glyph,
                        *acc,
                        icon_sq_bg,
                        menu_x + 38,
                    );

                    // App name
                    s_draw_str_small(
                        s,
                        sw,
                        menu_x + 40,
                        iy + (38 - 8) / 2,
                        name,
                        if is_hov { WHITE } else { 0x00_88_CC_FF },
                        row_bg,
                        menu_x + menu_w - 8,
                    );

                    // Hover accent bar on left edge
                    if is_hov {
                        s_fill(s, sw, menu_x + 1, iy + 4, 2, 30, ACCENT);
                    }

                    // Row separator
                    s_fill(s, sw, menu_x + 8, iy + 38, menu_w - 16, 1, 0x00_00_22_44);
                }
            }

            // ── Taskbar window tabs — slim underline style ───────────────────────
            let taskbar_btn_x0 = sep_x + 8;
            let hovered_btn = if mx_i >= taskbar_btn_x0
                && mx_i < sw as i32 - TASKBAR_CLOCK_W - 6
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H
            {
                Some(((mx_i - taskbar_btn_x0) / (BUTTON_W + 6)) as usize)
            } else {
                None
            };

            // App accent colours for taskbar icons (matches desktop tiles)
            const BTN_ACCENTS: [u32; 4] = [ICON_TERM_ACC, ICON_MON_ACC, ICON_TXT_ACC, ICON_COL_ACC];

            for i in 0..self.windows.len() {
                let bx = taskbar_btn_x0 + i as i32 * (BUTTON_W + 6);
                if bx + BUTTON_W > sw as i32 - TASKBAR_CLOCK_W - 4 {
                    break;
                }

                let focused = self.focused == Some(i);
                let minimized = self.windows[i].is_minimized();
                let hovered = hovered_btn == Some(i);
                let accent = BTN_ACCENTS[i % BTN_ACCENTS.len()];

                let bh = TASKBAR_H - 4;
                let by = taskbar_y + 2;

                // Tab background — subtle tint only, no heavy fill
                let bg = if focused {
                    0x00_00_22_4A // slightly brighter navy for focused
                } else if hovered {
                    0x00_00_14_30 // hover tint
                } else {
                    0x00_00_00_00 // transparent — glass shows through
                };

                if focused || hovered {
                    s_fill(s, sw, bx, by, BUTTON_W, bh, bg);
                }

                // Focused: full-width 2px bottom underline in app accent colour
                if focused {
                    s_fill(s, sw, bx, taskbar_y + TASKBAR_H - 2, BUTTON_W, 2, accent);
                    // Also a very dim 1px top line for depth
                    s_fill(s, sw, bx, taskbar_y + 2, BUTTON_W, 1, 0x00_00_55_99);
                } else if hovered {
                    // Hovered: dim 1px bottom line
                    s_fill(
                        s,
                        sw,
                        bx,
                        taskbar_y + TASKBAR_H - 2,
                        BUTTON_W,
                        1,
                        0x00_00_44_88,
                    );
                }

                // Minimised: small centre dot above underline position
                if minimized {
                    s_fill(
                        s,
                        sw,
                        bx + BUTTON_W / 2 - 3,
                        taskbar_y + TASKBAR_H - 5,
                        6,
                        2,
                        0x00_00_55_99,
                    );
                }

                // Title text — centred, no accent strip offset
                let title = self.windows[i].window().title;
                let trunc = if title.len() > 10 {
                    &title[..10]
                } else {
                    title
                };
                let tcol = if focused {
                    0x00_CC_EE_FF
                } else if hovered {
                    0x00_66_BB_EE
                } else {
                    0x00_00_66_99
                };
                let text_w = trunc.len() as i32 * 8;
                let text_x = bx + (BUTTON_W - text_w).max(0) / 2;
                s_draw_str_small(
                    s,
                    sw,
                    text_x,
                    by + (bh - 8) / 2,
                    trunc,
                    tcol,
                    bg,
                    bx + BUTTON_W - 4,
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

            // Clock hover tint
            let clk_hot = mx_i >= clk_x
                && mx_i < clk_x + clk_w
                && my_i >= taskbar_y + 2
                && my_i < taskbar_y + TASKBAR_H;
            let clk_bg = 0x00_00_00_00; // fully transparent — glass shows through
            if clk_hot {
                s_fill(
                    s,
                    sw,
                    clk_x,
                    taskbar_y + 2,
                    clk_w,
                    TASKBAR_H - 2,
                    0x00_00_14_30,
                );
                // Thin border when hovered
                draw_rect_border(
                    s,
                    sw,
                    clk_x,
                    taskbar_y + 2,
                    clk_w,
                    TASKBAR_H - 2,
                    0x00_00_44_88,
                );
            }

            // Two-line clock: uptime HH:MM on top, brand below.
            {
                let secs = current_tick / 60;
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
                    let time_x = clk_x + (clk_w - time_w) / 2;
                    s_draw_str_small(
                        s,
                        sw,
                        time_x,
                        taskbar_y + 8,
                        time_str,
                        0x00_00_EE_FF, // phosphor cyan clock digits
                        clk_bg,
                        clk_x + clk_w,
                    );
                }
            }
            {
                let brand_w = 6 * 8;
                let brand_x = clk_x + (clk_w - brand_w) / 2;
                s_draw_str_small(
                    s,
                    sw,
                    brand_x,
                    taskbar_y + 22,
                    "coolOS",
                    0x00_00_66_99,
                    clk_bg,
                    clk_x + clk_w,
                );
            }

            // ── Context menu — CRT phosphor blue ─────────────────────────────────
            if let Some(ref cm) = self.context_menu {
                let menu_h = CTX_ITEM_H * CTX_ITEMS.len() as i32 + 8;
                let pad = 4i32;

                // Shadow
                s_fill(
                    s,
                    sw,
                    cm.x + 3,
                    cm.y + 3,
                    CTX_W + 2,
                    menu_h + 2,
                    0x00_00_02_06,
                );

                // Background
                s_fill(s, sw, cm.x, cm.y, CTX_W, menu_h, 0x00_00_08_1C);

                // Border — phosphor blue
                draw_rect_border(s, sw, cm.x, cm.y, CTX_W, menu_h, 0x00_00_55_AA);

                for (i, &label) in CTX_ITEMS.iter().enumerate() {
                    let item_y = cm.y + pad + i as i32 * CTX_ITEM_H;
                    let hot = mx_i >= cm.x
                        && mx_i < cm.x + CTX_W
                        && my_i >= item_y
                        && my_i < item_y + CTX_ITEM_H;

                    if hot {
                        s_fill(s, sw, cm.x + 2, item_y, CTX_W - 4, CTX_ITEM_H, ACCENT);
                        // Clip hover corners
                        s_fill(s, sw, cm.x + 2, item_y, 1, 1, 0x00_00_08_1C);
                        s_fill(s, sw, cm.x + CTX_W - 3, item_y, 1, 1, 0x00_00_08_1C);
                        s_fill(
                            s,
                            sw,
                            cm.x + 2,
                            item_y + CTX_ITEM_H - 1,
                            1,
                            1,
                            0x00_00_08_1C,
                        );
                        s_fill(
                            s,
                            sw,
                            cm.x + CTX_W - 3,
                            item_y + CTX_ITEM_H - 1,
                            1,
                            1,
                            0x00_00_08_1C,
                        );
                    }

                    s_draw_str_small(
                        s,
                        sw,
                        cm.x + 14,
                        item_y + (CTX_ITEM_H - 8) / 2,
                        label,
                        if hot { WHITE } else { 0x00_88_CC_FF },
                        if hot { ACCENT } else { 0x00_00_08_1C },
                        cm.x + CTX_W - 4,
                    );

                    // Separator (not after last item)
                    if i + 1 < CTX_ITEMS.len() && !hot {
                        s_fill(
                            s,
                            sw,
                            cm.x + 8,
                            item_y + CTX_ITEM_H - 1,
                            CTX_W - 16,
                            1,
                            0x00_00_33_66,
                        );
                    }
                }
            }

            // ── Cursor — Windows-style arrow with black outline ───────────────────
            // Draw outline first (black), then fill (white)
            for (row, &mask) in CURSOR_OUTLINE.iter().enumerate() {
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
            for (row, &mask) in CURSOR_SHAPE.iter().enumerate() {
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
                    let mut scratch = alloc::vec![0u8; row_bytes];
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
            if wi < self.windows.len() && self.windows[wi].window().hit(px, py) {
                return Some(z_pos);
            }
        }
        None
    }

    /// Draw a single window — Windows 11 Dark Mode chrome.
    fn draw_window(s: &mut [u32], sw: usize, w: &Window, focused: bool) {
        // ── Drop shadow ───────────────────────────────────────────────────────
        // Four-sided soft shadow: 6px offset, fades with distance
        const SHADOW_R: i32 = 6;
        for d in 1..=SHADOW_R {
            let alpha = ((SHADOW_R - d + 1) as u32 * 6).min(40);
            let shadow_col = alpha << 24; // pure black with varying alpha weight
            let sx = w.x + w.width + d - 1;
            let sy = w.y + w.height + d - 1;
            // Right edge
            s_fill_alpha(s, sw, sx, w.y + d, 1, w.height, shadow_col);
            // Bottom edge
            s_fill_alpha(s, sw, w.x + d, sy, w.width, 1, shadow_col);
        }

        // ── Title bar — CRT phosphor chrome ──────────────────────────────────
        let title_bg = if focused { WIN_BAR_F } else { WIN_BAR_U };
        s_fill(s, sw, w.x, w.y, w.width, TITLE_H, title_bg);

        // Thin top accent stripe on focused window
        if focused {
            s_fill(s, sw, w.x, w.y, w.width, 1, ACCENT);
        }

        // ── Window border ─────────────────────────────────────────────────────
        let bord = if focused { WIN_BDR_F } else { WIN_BDR_U };
        s_fill(s, sw, w.x - 1, w.y - 1, w.width + 2, 1, bord); // top
        s_fill(s, sw, w.x - 1, w.y + w.height, w.width + 2, 1, bord); // bottom
        s_fill(s, sw, w.x - 1, w.y, 1, w.height, bord); // left
        s_fill(s, sw, w.x + w.width, w.y, 1, w.height, bord); // right

        // ── Title text ────────────────────────────────────────────────────────
        let max_title_x = w.x + w.width - WIN_BTN_W * 3 - 10;
        let title_fg = if focused {
            0x00_AA_DD_FF
        } else {
            0x00_00_55_88
        };
        s_draw_str_small(
            s,
            sw,
            w.x + 8,
            w.y + (TITLE_H - 8) / 2,
            w.title,
            title_fg,
            title_bg,
            max_title_x,
        );

        // ── Caption buttons — CRT phosphor style ──────────────────────────────
        let btn_y = w.y + 1;
        let btn_h = TITLE_H - 2;

        // Minimize  ─
        let mx = w.x + w.width - WIN_BTN_W * 3;
        s_fill(s, sw, mx, btn_y, WIN_BTN_W, btn_h, CAP_NORMAL);
        // Horizontal bar glyph centred
        s_fill(
            s,
            sw,
            mx + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 + 1,
            8,
            1,
            0x00_00_99_FF,
        );

        // Maximize  □
        let mx2 = w.x + w.width - WIN_BTN_W * 2;
        s_fill(s, sw, mx2, btn_y, WIN_BTN_W, btn_h, CAP_NORMAL);
        // Hollow square glyph
        s_fill(
            s,
            sw,
            mx2 + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 - 4,
            8,
            1,
            0x00_00_99_FF,
        );
        s_fill(
            s,
            sw,
            mx2 + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 + 3,
            8,
            1,
            0x00_00_99_FF,
        );
        s_fill(
            s,
            sw,
            mx2 + WIN_BTN_W / 2 - 4,
            btn_y + btn_h / 2 - 4,
            1,
            8,
            0x00_00_99_FF,
        );
        s_fill(
            s,
            sw,
            mx2 + WIN_BTN_W / 2 + 3,
            btn_y + btn_h / 2 - 4,
            1,
            8,
            0x00_00_99_FF,
        );

        // Close  ✕ — pixel diagonals (font glyph gets clipped inside WIN_BTN_W)
        let cx = w.x + w.width - WIN_BTN_W;
        let cx_c = cx + WIN_BTN_W / 2;
        let cy_c = btn_y + btn_h / 2;
        let sh_wnd = s.len() / sw;
        s_fill(s, sw, cx, btn_y, WIN_BTN_W, btn_h, CLOSE_REST);
        for i in -3i32..=3 {
            s_put(s, sw, sh_wnd, cx_c + i, cy_c + i, 0x00_FF_44_44);
            s_put(s, sw, sh_wnd, cx_c + i + 1, cy_c + i, 0x00_FF_44_44);
            s_put(s, sw, sh_wnd, cx_c + i, cy_c - i, 0x00_FF_44_44);
            s_put(s, sw, sh_wnd, cx_c + i + 1, cy_c - i, 0x00_FF_44_44);
        }

        // ── Content area ──────────────────────────────────────────────────────
        let content_y = w.y + TITLE_H;
        let content_h = (w.height - TITLE_H).max(0) as usize;
        let cw = w.width as usize;

        for row in 0..content_h {
            for col in 0..cw {
                let px = w.x + col as i32;
                let py = content_y + row as i32;
                if px >= 0 && py >= 0 && (px as usize) < sw && (py as usize) < s.len() / sw {
                    let pixel = w.buf[row * cw + col];
                    s[(py as usize) * sw + (px as usize)] =
                        if pixel == 0 { WIN_CONTENT } else { pixel };
                }
            }
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

fn s_draw_char(s: &mut [u32], sw: usize, x: i32, y: i32, c: char, fg: u32, bg: u32) {
    use font8x8::UnicodeFonts;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    use crate::framebuffer::FONT_SCALE;
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            for sy in 0..FONT_SCALE {
                for sx in 0..FONT_SCALE {
                    let px = x + (bit * FONT_SCALE + sx) as i32;
                    let py = y + (gy * FONT_SCALE + sy) as i32;
                    if px >= 0 && py >= 0 {
                        let (px, py) = (px as usize, py as usize);
                        if px < sw && py < sh {
                            s[py * sw + px] = color;
                        }
                    }
                }
            }
        }
    }
}

fn s_draw_str(s: &mut [u32], sw: usize, x: i32, y: i32, text: &str, fg: u32, bg: u32, max_x: i32) {
    let mut cx = x;
    for c in text.chars() {
        if cx + CHAR_W as i32 > max_x {
            break;
        }
        s_draw_char(s, sw, cx, y, c, fg, bg);
        cx += CHAR_W as i32;
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
