/// Window compositor — desktop, windows, taskbar, cursor, context menu.
/// All rendering targets a `Vec<u32>` shadow buffer; one blit per frame.

extern crate alloc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::apps::{TerminalApp, SysMonApp, TextViewerApp, ColorPickerApp};
use crate::framebuffer::{
    CHAR_W, WHITE, BLACK, LIGHT_GRAY, DARK_GRAY, BLUE, RED,
};
use crate::wm::window::{Window, TITLE_H, CLOSE_W};

// ── Layout constants (scaled 4× from the original 320×200 design) ────────────

const TASKBAR_H: i32 = 24;
// TASKBAR_Y is computed at runtime from screen height.
const BUTTON_W:  i32 = 160;
const EVENT_PACKET_SIZE: usize = 8;
const EVENT_KIND_KEY_CHAR: u8 = 1;
const EVENT_KIND_MOUSE_DOWN: u8 = 2;

// ── Cursor ────────────────────────────────────────────────────────────────────

const CURSOR_H: usize = 8;
const CURSOR_SHAPE: [u8; CURSOR_H] = [
    0b11111110,
    0b11111100,
    0b11111000,
    0b11110000,
    0b11111000,
    0b11001100,
    0b10000110,
    0b00000011,
];

// ── Context menu ──────────────────────────────────────────────────────────────

const CTX_W:      i32 = 200;
const CTX_ITEM_H: i32 = 24;
const CTX_ITEMS:  &[&str] = &["Terminal", "System Mon", "Text Viewer", "Color Pick"];

struct ContextMenu { x: i32, y: i32 }

// ── Drag state ────────────────────────────────────────────────────────────────

struct DragState { window: usize, off_x: i32, off_y: i32 }

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
            AppWindow::Terminal(t)    => &t.window,
            AppWindow::SysMon(s)      => &s.window,
            AppWindow::TextViewer(v)  => &v.window,
            AppWindow::ColorPicker(c) => &c.window,
        }
    }
    pub fn window_mut(&mut self) -> &mut Window {
        match self {
            AppWindow::Terminal(t)    => &mut t.window,
            AppWindow::SysMon(s)      => &mut s.window,
            AppWindow::TextViewer(v)  => &mut v.window,
            AppWindow::ColorPicker(c) => &mut c.window,
        }
    }
    pub fn handle_key(&mut self, c: char) {
        match self {
            AppWindow::Terminal(t)   => t.handle_key(c),
            AppWindow::TextViewer(v) => v.handle_key(c),
            _ => {}
        }
    }
    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        if let AppWindow::ColorPicker(cp) = self { cp.handle_click(lx, ly); }
    }
    pub fn update(&mut self) {
        if let AppWindow::SysMon(s) = self { s.update(); }
    }
}

// ── Window manager ────────────────────────────────────────────────────────────

pub struct WindowManager {
    pub windows:   Vec<AppWindow>,
    z_order:       Vec<usize>,
    focused:       Option<usize>,
    key_sink_fd:   Option<usize>,
    key_sink_window: Option<usize>,
    drag:          Option<DragState>,
    prev_left:     bool,
    prev_right:    bool,
    context_menu:  Option<ContextMenu>,
    /// Shadow buffer — screen_width × screen_height u32 pixels (no row padding).
    shadow:        Vec<u32>,
    shadow_width:  usize,
    shadow_height: usize,
}

impl WindowManager {
    pub fn new() -> Self {
        let w = crate::framebuffer::width();
        let h = crate::framebuffer::height();
        WindowManager {
            windows:       Vec::new(),
            z_order:       Vec::new(),
            focused:       None,
            key_sink_fd:   None,
            key_sink_window: None,
            drag:          None,
            prev_left:     false,
            prev_right:    false,
            context_menu:  None,
            shadow:        alloc::vec![0u32; w * h],
            shadow_width:  w,
            shadow_height: h,
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
        // Uses the same lock-free pattern as the keyboard buffer to avoid
        // deadlocking if the timer preempts compose() mid-frame.
        while let Some(b) = crate::syscall::pop_output_byte() {
            for w in self.windows.iter_mut() {
                if let AppWindow::Terminal(ref mut t) = w {
                    t.print_char(b as char);
                    break;
                }
            }
        }

        // Consume deferred terminal requests to install a compositor-owned key
        // sink without recursively locking the WM from inside command handling.
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

        let (mx, my)   = crate::mouse::pos();
        let (left, right) = crate::mouse::buttons();
        let mx_i = mx as i32;
        let my_i = my as i32;

        let left_pressed  = left  && !self.prev_left;
        let left_released = !left &&  self.prev_left;
        let right_pressed = right && !self.prev_right;

        // ── Input ─────────────────────────────────────────────────────────────

        if right_pressed && self.front_to_back_hit(mx_i, my_i).is_none() {
            let cx = mx_i.min(sw as i32 - CTX_W);
            let cy = my_i.min(taskbar_y - CTX_ITEM_H * CTX_ITEMS.len() as i32);
            self.context_menu = Some(ContextMenu { x: cx, y: cy });
        }

        if left_pressed {
            if self.context_menu.is_some() {
                let clicked: Option<&str> = {
                    let cm = self.context_menu.as_ref().unwrap();
                    CTX_ITEMS.iter().find_map(|&label| {
                        let i = CTX_ITEMS.iter().position(|&l| l == label).unwrap();
                        let item_y = cm.y + i as i32 * CTX_ITEM_H;
                        if mx_i >= cm.x && mx_i < cm.x + CTX_W
                            && my_i >= item_y && my_i < item_y + CTX_ITEM_H
                        { Some(label) } else { None }
                    })
                };
                self.context_menu = None;
                let off = self.windows.len() as i32 * 16;
                let wx = (10 + off).min(sw as i32 - 200);
                let wy = (10 + off).min(taskbar_y - 80);
                match clicked {
                    Some("Terminal")    => self.add_window(AppWindow::Terminal(TerminalApp::new(wx, wy))),
                    Some("System Mon")  => self.add_window(AppWindow::SysMon(SysMonApp::new(wx, wy))),
                    Some("Text Viewer") => self.add_window(AppWindow::TextViewer(TextViewerApp::new(wx, wy))),
                    Some("Color Pick")  => self.add_window(AppWindow::ColorPicker(ColorPickerApp::new(wx, wy))),
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
                            if *z > win_idx { *z -= 1; }
                        }
                        self.focused = self.z_order.last().copied();
                        self.drag = None;
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
                            if let Some(z_pos) = self.z_order.iter().position(|&i| i == btn_x) {
                                self.z_order.remove(z_pos);
                                self.z_order.push(btn_x);
                                self.focused = Some(btn_x);
                            }
                        }
                    }
                }
            }
        }

        if left_released { self.drag = None; }

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

        self.prev_left  = left;
        self.prev_right = right;

        // ── Render ────────────────────────────────────────────────────────────
        // Scope the mutable shadow borrow so it ends before the blit section,
        // which needs to access self.shadow immutably alongside a scratch buffer.
        {
        let s = &mut self.shadow;

        // Desktop.
        s_fill(s, sw, 0, 0, sw as i32, taskbar_y, DARK_GRAY);

        for w in self.windows.iter_mut() { w.update(); }

        let z: Vec<usize> = self.z_order.clone();
        for &wi in &z {
            if wi < self.windows.len() {
                let focused = self.focused == Some(wi);
                Self::draw_window(s, sw, self.windows[wi].window(), focused);
            }
        }

        // Taskbar.
        s_fill(s, sw, 0, taskbar_y, sw as i32, TASKBAR_H, BLACK);
        s_fill(s, sw, 0, taskbar_y, sw as i32, 1, LIGHT_GRAY);
        for i in 0..self.windows.len() {
            let bx = 2 + i as i32 * (BUTTON_W + 2);
            if bx + BUTTON_W > sw as i32 { break; }
            let focused    = self.focused == Some(i);
            let btn_col    = if focused { BLUE } else { DARK_GRAY };
            s_fill(s, sw, bx, taskbar_y + 2, BUTTON_W, TASKBAR_H - 3, btn_col);
            let title = self.windows[i].window().title;
            s_draw_str(s, sw, bx + 4, taskbar_y + 4, title, WHITE, btn_col,
                       bx + BUTTON_W - 1);
        }

        // Context menu.
        if let Some(ref cm) = self.context_menu {
            let menu_h = CTX_ITEM_H * CTX_ITEMS.len() as i32;
            s_fill(s, sw, cm.x - 1, cm.y - 1, CTX_W + 2, menu_h + 2, LIGHT_GRAY);
            s_fill(s, sw, cm.x, cm.y, CTX_W, menu_h, DARK_GRAY);
            for (i, &label) in CTX_ITEMS.iter().enumerate() {
                let item_y = cm.y + i as i32 * CTX_ITEM_H;
                s_draw_str(s, sw, cm.x + 4, item_y + 4, label, WHITE, DARK_GRAY,
                           cm.x + CTX_W - 1);
            }
        }

        // Cursor (2× scaled for visibility at high-res).
        for (row, &byte) in CURSOR_SHAPE.iter().enumerate() {
            for bit in 0..8usize {
                if byte & (0x80 >> bit) != 0 {
                    for sy in 0..2usize {
                        for sx in 0..2usize {
                            s_put(s, sw, sh,
                                  mx as i32 + (bit * 2 + sx) as i32,
                                  my as i32 + (row * 2 + sy) as i32,
                                  WHITE);
                        }
                    }
                }
            }
        }

        } // end shadow borrow — rendering done

        // ── Blit shadow → hardware framebuffer ────────────────────────────────
        //
        // 3bpp path: convert each row of u32 shadow pixels into a small stack
        // scratch buffer (fast RAM→RAM), then flush that row with ONE
        // copy_nonoverlapping.  This trades many volatile MMIO writes for a
        // single bulk transfer per row, which is far faster.
        //
        // scratch[5120] fits up to 1706 pixels @ 3 bpp; more than enough for
        // 1280 wide.  Allocated once in compose()'s stack frame, reused every row.
        let hw_base   = crate::framebuffer::base();
        let hw_stride = crate::framebuffer::stride();
        let hw_bpp    = crate::framebuffer::bpp();
        let hw_fmt    = crate::framebuffer::fmt();
        let is_rgb    = hw_fmt == crate::framebuffer::PixFmt::Rgb;
        if hw_base != 0 {
            match hw_bpp {
                4 => {
                    for row in 0..sh {
                        let src      = &self.shadow[row * sw..row * sw + sw];
                        let row_base = hw_base + (row * hw_stride * 4) as u64;
                        let dst      = row_base as *mut u32;
                        if !is_rgb {
                            unsafe {
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, sw);
                            }
                        } else {
                            for col in 0..sw {
                                let c = src[col];
                                let hw = ((c & 0xFF) << 16) | (c & 0x00FF00) | (c >> 16 & 0xFF);
                                unsafe { dst.add(col).write_volatile(hw); }
                            }
                        }
                    }
                }
                3 => {
                    let row_bytes = sw * 3;
                    let mut scratch = [0u8; 5120];
                    for row in 0..sh {
                        let src      = &self.shadow[row * sw..row * sw + sw];
                        let row_base = hw_base + (row * hw_stride * 3) as u64;
                        if !is_rgb {
                            for col in 0..sw {
                                let c = src[col];
                                scratch[col * 3    ] = c as u8;
                                scratch[col * 3 + 1] = (c >> 8) as u8;
                                scratch[col * 3 + 2] = (c >> 16) as u8;
                            }
                        } else {
                            for col in 0..sw {
                                let c = src[col];
                                scratch[col * 3    ] = (c >> 16) as u8;
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

    fn draw_window(s: &mut [u32], sw: usize, w: &Window, focused: bool) {
        let title_color  = if focused { BLUE       } else { DARK_GRAY };
        let border_color = if focused { LIGHT_GRAY } else { DARK_GRAY };

        s_fill(s, sw, w.x - 1, w.y - 1, w.width + 2, w.height + 2, border_color);
        s_fill(s, sw, w.x,     w.y,     w.width,      TITLE_H,      title_color);

        let max_title_x = w.x + w.width - CLOSE_W - 1;
        s_draw_str(s, sw, w.x + 4, w.y + 2, w.title, WHITE, title_color, max_title_x);

        let cx = w.x + w.width - CLOSE_W;
        s_fill(s, sw, cx, w.y, CLOSE_W, TITLE_H, RED);
        s_draw_str(s, sw, cx + 4, w.y + 2, "x", WHITE, RED, cx + CLOSE_W);

        let content_y = w.y + TITLE_H;
        let content_h = (w.height - TITLE_H).max(0) as usize;
        let cw = w.width as usize;

        for row in 0..content_h {
            for col in 0..cw {
                s_put(s, sw, usize::MAX, w.x + col as i32, content_y + row as i32,
                      w.buf[row * cw + col]);
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

fn s_fill(s: &mut [u32], sw: usize,
          x: i32, y: i32, w: i32, h: i32, color: u32)
{
    let sh = if sw > 0 { s.len() / sw } else { 0 };
    let x0 = (x.max(0) as usize).min(sw);
    let y0 = y.max(0) as usize;
    let x1 = ((x + w).max(0) as usize).min(sw);
    let y1 = ((y + h).max(0) as usize).min(sh);
    if x0 >= x1 || y0 >= y1 { return; }
    for row in y0..y1 {
        let base = row * sw;
        s[base + x0..base + x1].fill(color);
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
                    let py = y + (gy  * FONT_SCALE + sy) as i32;
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

fn s_draw_str(s: &mut [u32], sw: usize,
              x: i32, y: i32, text: &str, fg: u32, bg: u32, max_x: i32)
{
    let mut cx = x;
    for c in text.chars() {
        if cx + CHAR_W as i32 > max_x { break; }
        s_draw_char(s, sw, cx, y, c, fg, bg);
        cx += CHAR_W as i32;
    }
}
