/// Window compositor — desktop, windows, taskbar, cursor, context menu.
/// All rendering targets a shadow buffer; one blit to VGA per frame.

extern crate alloc;
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::apps::TerminalApp;
use crate::framebuffer::{
    HEIGHT, WIDTH, CHAR_W,
    WHITE, BLACK, LIGHT_GRAY, DARK_GRAY, BLUE, RED, GREEN,
};
use crate::wm::window::{Window, TITLE_H, CLOSE_W};

const FB: *mut u8 = 0xa0000 as *mut u8;

// ── Layout constants ──────────────────────────────────────────────────────────

const TASKBAR_H: i32 = 12;
const TASKBAR_Y: i32 = HEIGHT as i32 - TASKBAR_H;
const BUTTON_W:  i32 = 52;

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

// ── AppWindow ─────────────────────────────────────────────────────────────────

pub enum AppWindow {
    Plain(Window),
    Terminal(TerminalApp),
}

impl AppWindow {
    pub fn window(&self) -> &Window {
        match self {
            AppWindow::Plain(w)    => w,
            AppWindow::Terminal(t) => &t.window,
        }
    }

    pub fn window_mut(&mut self) -> &mut Window {
        match self {
            AppWindow::Plain(w)    => w,
            AppWindow::Terminal(t) => &mut t.window,
        }
    }

    pub fn handle_key(&mut self, c: char) {
        if let AppWindow::Terminal(t) = self {
            t.handle_key(c);
        }
    }
}

// ── Context menu ──────────────────────────────────────────────────────────────

const CTX_W:      i32 = 80;
const CTX_ITEM_H: i32 = 12;
const CTX_ITEMS:  &[&str] = &["Terminal"];

struct ContextMenu {
    x: i32,
    y: i32,
}

// ── Drag state ────────────────────────────────────────────────────────────────

struct DragState {
    window: usize,
    off_x:  i32,
    off_y:  i32,
}

// ── Window manager ────────────────────────────────────────────────────────────

pub struct WindowManager {
    pub windows:    Vec<AppWindow>,
    z_order:        Vec<usize>,
    focused:        Option<usize>,
    drag:           Option<DragState>,
    prev_left:      bool,
    prev_right:     bool,
    context_menu:   Option<ContextMenu>,
    shadow:         Vec<u8>,
}

impl WindowManager {
    pub fn new() -> Self {
        WindowManager {
            windows:      Vec::new(),
            z_order:      Vec::new(),
            focused:      None,
            drag:         None,
            prev_left:    false,
            prev_right:   false,
            context_menu: None,
            shadow:       alloc::vec![0u8; WIDTH * HEIGHT],
        }
    }

    pub fn add_window(&mut self, w: AppWindow) {
        let idx = self.windows.len();
        self.windows.push(w);
        self.z_order.push(idx);
        self.focused = Some(idx);
    }

    pub fn handle_key(&mut self, c: char) {
        if let Some(idx) = self.focused {
            if idx < self.windows.len() {
                self.windows[idx].handle_key(c);
                crate::wm::request_repaint();
            }
        }
    }

    /// Full composite frame into shadow buffer, then blit to VGA.
    pub fn compose(&mut self) {
        let (mx, my) = crate::mouse::pos();
        let (left, right) = crate::mouse::buttons();
        let mx_i = mx as i32;
        let my_i = my as i32;

        let left_pressed  = left  && !self.prev_left;
        let left_released = !left &&  self.prev_left;
        let right_pressed = right && !self.prev_right;

        // ── Input: context menu ───────────────────────────────────────────────

        if right_pressed && self.front_to_back_hit(mx_i, my_i).is_none() {
            // Clamp so menu stays on screen.
            let cx = (mx_i).min(WIDTH as i32 - CTX_W);
            let cy = (my_i).min(TASKBAR_Y - CTX_ITEM_H * CTX_ITEMS.len() as i32);
            self.context_menu = Some(ContextMenu { x: cx, y: cy });
        }

        if left_pressed {
            if self.context_menu.is_some() {
                // Determine which item (if any) was clicked, then drop the borrow.
                let spawn_terminal = {
                    let cm = self.context_menu.as_ref().unwrap();
                    CTX_ITEMS.iter().enumerate().any(|(i, &label)| {
                        let item_y = cm.y + i as i32 * CTX_ITEM_H;
                        label == "Terminal"
                            && mx_i >= cm.x && mx_i < cm.x + CTX_W
                            && my_i >= item_y && my_i < item_y + CTX_ITEM_H
                    })
                };
                self.context_menu = None;
                if spawn_terminal {
                    let off = self.windows.len() as i32 * 8;
                    let t = TerminalApp::new(
                        (10 + off).min(WIDTH as i32 - 80),
                        (10 + off).min(TASKBAR_Y - 30),
                    );
                    self.add_window(AppWindow::Terminal(t));
                }
            } else {
                // Normal window hit-test.
                if let Some(z_pos) = self.front_to_back_hit(mx_i, my_i) {
                    let win_idx = self.z_order[z_pos];

                    // Raise to front and focus.
                    self.z_order.remove(z_pos);
                    self.z_order.push(win_idx);
                    self.focused = Some(win_idx);

                    let w = self.windows[win_idx].window();

                    if w.hit_close(mx_i, my_i) {
                        // Close window and remap z_order indices.
                        self.windows.remove(win_idx);
                        self.z_order.retain(|&i| i != win_idx);
                        for z in self.z_order.iter_mut() {
                            if *z > win_idx { *z -= 1; }
                        }
                        self.focused = self.z_order.last().copied();
                        self.drag    = None;
                    } else if self.windows[win_idx].window().hit_title(mx_i, my_i) {
                        self.drag = Some(DragState {
                            window: win_idx,
                            off_x:  mx_i - self.windows[win_idx].window().x,
                            off_y:  my_i - self.windows[win_idx].window().y,
                        });
                    }
                }

                // Taskbar click — raise the clicked window.
                if my_i >= TASKBAR_Y {
                    let btn_x = (mx_i - 2) / (BUTTON_W + 2);
                    if btn_x >= 0 {
                        let btn_x = btn_x as usize;
                        if btn_x < self.z_order.len() {
                            // Find the window at taskbar position btn_x.
                            if btn_x < self.windows.len() {
                                let win_idx = btn_x; // taskbar order = creation order
                                if let Some(z_pos) = self.z_order.iter().position(|&i| i == win_idx) {
                                    self.z_order.remove(z_pos);
                                    self.z_order.push(win_idx);
                                    self.focused = Some(win_idx);
                                }
                            }
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

        self.prev_left  = left;
        self.prev_right = right;

        // ── Render ────────────────────────────────────────────────────────────

        let s = &mut self.shadow;

        // Desktop.
        s_fill(s, 0, 0, WIDTH as i32, TASKBAR_Y, DARK_GRAY);

        // Windows back-to-front (snapshot z_order to avoid borrow issues).
        let z: Vec<usize> = self.z_order.clone();
        for &wi in &z {
            if wi < self.windows.len() {
                let focused = self.focused == Some(wi);
                Self::draw_window(s, self.windows[wi].window(), focused);
            }
        }

        // Taskbar.
        s_fill(s, 0, TASKBAR_Y, WIDTH as i32, TASKBAR_H, BLACK);
        // Thin highlight line along top of taskbar.
        s_fill(s, 0, TASKBAR_Y, WIDTH as i32, 1, LIGHT_GRAY);
        for i in 0..self.windows.len() {
            let bx = 2 + i as i32 * (BUTTON_W + 2);
            if bx + BUTTON_W > WIDTH as i32 { break; }
            let focused = self.focused == Some(i);
            let btn_col = if focused { BLUE } else { DARK_GRAY };
            s_fill(s, bx, TASKBAR_Y + 2, BUTTON_W, TASKBAR_H - 3, btn_col);
            let title = self.windows[i].window().title;
            s_draw_str(s, bx + 2, TASKBAR_Y + 3, title, WHITE, btn_col,
                       bx + BUTTON_W - 1);
        }

        // Context menu.
        if let Some(ref cm) = self.context_menu {
            let menu_h = CTX_ITEM_H * CTX_ITEMS.len() as i32;
            // Border.
            s_fill(s, cm.x - 1, cm.y - 1, CTX_W + 2, menu_h + 2, LIGHT_GRAY);
            s_fill(s, cm.x, cm.y, CTX_W, menu_h, DARK_GRAY);
            for (i, &label) in CTX_ITEMS.iter().enumerate() {
                let item_y = cm.y + i as i32 * CTX_ITEM_H;
                s_draw_str(s, cm.x + 2, item_y + 2, label, WHITE, DARK_GRAY,
                           cm.x + CTX_W - 1);
            }
        }

        // Cursor.
        for (row, &byte) in CURSOR_SHAPE.iter().enumerate() {
            for bit in 0..8usize {
                if byte & (0x80 >> bit) != 0 {
                    s_put(s, mx as i32 + bit as i32, my as i32 + row as i32, WHITE);
                }
            }
        }

        // Blit shadow → VGA.
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), FB, WIDTH * HEIGHT);
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

    fn draw_window(s: &mut [u8], w: &Window, focused: bool) {
        let title_color  = if focused { BLUE       } else { DARK_GRAY };
        let border_color = if focused { LIGHT_GRAY } else { DARK_GRAY };

        // Border.
        s_fill(s, w.x - 1, w.y - 1, w.width + 2, w.height + 2, border_color);

        // Title bar.
        s_fill(s, w.x, w.y, w.width, TITLE_H, title_color);

        // Title text.
        let max_title_x = w.x + w.width - CLOSE_W - 1;
        s_draw_str(s, w.x + 2, w.y + 1, w.title, WHITE, title_color, max_title_x);

        // Close button.
        let cx = w.x + w.width - CLOSE_W;
        s_fill(s, cx, w.y, CLOSE_W, TITLE_H, RED);
        s_draw_str(s, cx + 2, w.y + 1, "x", WHITE, RED, cx + CLOSE_W);

        // Content area — blit window back-buffer.
        let content_y = w.y + TITLE_H;
        let content_h = (w.height - TITLE_H).max(0) as usize;
        let cw = w.width as usize;

        for row in 0..content_h {
            for col in 0..cw {
                let color = w.buf[row * cw + col];
                s_put(s, w.x + col as i32, content_y + row as i32, color);
            }
        }
    }
}

lazy_static! {
    pub static ref WM: Mutex<WindowManager> = Mutex::new(WindowManager::new());
}

// ── Shadow-buffer helpers ─────────────────────────────────────────────────────

#[inline(always)]
fn s_put(s: &mut [u8], x: i32, y: i32, color: u8) {
    if x >= 0 && y >= 0 {
        let (x, y) = (x as usize, y as usize);
        if x < WIDTH && y < HEIGHT {
            s[y * WIDTH + x] = color;
        }
    }
}

fn s_fill(s: &mut [u8], x: i32, y: i32, w: i32, h: i32, color: u8) {
    let x0 = (x.max(0) as usize).min(WIDTH);
    let y0 = y.max(0) as usize;
    let x1 = ((x + w).max(0) as usize).min(WIDTH);
    let y1 = ((y + h).max(0) as usize).min(HEIGHT);
    if x0 >= x1 || y0 >= y1 { return; }
    for row in y0..y1 {
        let base = row * WIDTH;
        s[base + x0..base + x1].fill(color);
    }
}

fn s_draw_char(s: &mut [u8], x: i32, y: i32, c: char, fg: u8, bg: u8) {
    use font8x8::UnicodeFonts;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..CHAR_W {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            s_put(s, x + bit as i32, y + gy as i32, color);
        }
    }
}

fn s_draw_str(s: &mut [u8], x: i32, y: i32, text: &str, fg: u8, bg: u8, max_x: i32) {
    let mut cx = x;
    for c in text.chars() {
        if cx + CHAR_W as i32 > max_x { break; }
        s_draw_char(s, cx, y, c, fg, bg);
        cx += CHAR_W as i32;
    }
}
