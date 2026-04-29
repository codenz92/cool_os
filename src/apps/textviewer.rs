extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::framebuffer::{LIGHT_GRAY, WHITE, YELLOW};
use crate::wm::window::{Window, TITLE_H};
use font8x8::UnicodeFonts;

pub const VIEWER_W: i32 = 640;
pub const VIEWER_H: i32 = 480;

const CHAR_W: usize = 8;
const LINE_H: usize = 12;
const HEADER_H: usize = 42;
const FOOTER_H: usize = 18;
const PAD_X: usize = 18;
const GLYPH_Y_INSET: usize = 1;

const BG_A: u32 = 0x00_03_08_15;
const BG_B: u32 = 0x00_01_03_0A;
const PANEL: u32 = 0x00_00_0A_1C;
const PANEL_ALT: u32 = 0x00_00_0E_24;
const PANEL_BORDER: u32 = 0x00_00_44_88;
const ACCENT: u32 = 0x00_00_DD_FF;
const SUBTLE: u32 = 0x00_66_AA_DD;
const MUTED: u32 = 0x00_55_7A_92;

const ABOUT: &[&str] = &[
    " coolOS v1.16",
    " Bare-metal OS in Rust",
    "",
    " == Phases ==",
    " 1. Pixel framebuffer",
    " 2. PS/2 mouse driver",
    " 3. Window manager",
    " 4. Desktop shell",
    " 5. Applications",
    " 6. High-res framebuffer",
    " 7-13. Scheduler, userspace, VMM, FS, ELF, IPC",
    " 16. Desktop shell in progress",
    "",
    " == Commands ==",
    " help    - list commands",
    " echo    - print text",
    " info    - CPU + heap",
    " touch   - create file",
    " mkdir   - create folder",
    " uptime  - tick count",
    " clear   - clear term",
    " reboot  - restart OS",
    "",
    " == Controls ==",
    " j / k   scroll dn/up",
    " Drag title bar: move",
    " x button: close win",
    " Right-click: new app",
    " File Manager: browse FAT32 + create files/folders",
    "",
    " github.com/codenz92",
    "   /cool-os",
];

pub struct TextViewerApp {
    pub window: Window,
    lines: Vec<String>,
    scroll: usize,
    rows: usize,
    cols: usize,
    heading: String,
    subheading: String,
    last_width: i32,
    last_height: i32,
}

impl TextViewerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, VIEWER_W, VIEWER_H, "Text Viewer");
        let mut app = TextViewerApp {
            window,
            lines: ABOUT.iter().map(|line| String::from(*line)).collect(),
            scroll: 0,
            rows: 0,
            cols: 0,
            heading: String::from("About coolOS"),
            subheading: String::from("system notes, controls, and current milestone"),
            last_width: VIEWER_W,
            last_height: VIEWER_H,
        };
        app.render();
        app
    }

    pub fn open_file(x: i32, y: i32, path: &str) -> Result<Self, &'static str> {
        let bytes = crate::fat32::read_file(path).ok_or("file not found")?;
        let text = core::str::from_utf8(&bytes).map_err(|_| "file is not UTF-8 text")?;
        let mut app = Self::new(x, y);
        app.lines = if text.is_empty() {
            alloc::vec![String::from("(empty file)")]
        } else {
            text.lines().map(String::from).collect()
        };
        app.heading = String::from("Text document");
        app.subheading = String::from(path);
        app.scroll = 0;
        app.render();
        Ok(app)
    }

    pub fn crash_viewer(x: i32, y: i32) -> Self {
        let mut app = Self::new(x, y);
        let lines = crate::crashdump::lines();
        app.lines = if lines.is_empty() {
            alloc::vec![String::from("no crash dumps recorded")]
        } else {
            lines
        };
        app.heading = String::from("Crash dump viewer");
        app.subheading = String::from("/LOGS/CRASH.TXT and per-task reports");
        app.scroll = 0;
        app.render();
        app
    }

    pub fn handle_key(&mut self, c: char) {
        match c {
            'j' | 'J' => {
                if self.scroll + self.rows < self.lines.len() {
                    self.scroll += 1;
                    self.render();
                }
            }
            'k' | 'K' => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                    self.render();
                }
            }
            _ => {}
        }
    }

    pub fn handle_scroll(&mut self, delta: i32) {
        let max = self.lines.len().saturating_sub(self.rows);
        let new = (self.scroll as i32 + delta.signum() * 3).clamp(0, max as i32) as usize;
        if new != self.scroll {
            self.scroll = new;
            self.render();
        }
    }

    pub fn update(&mut self) {
        if self.window.width != self.last_width || self.window.height != self.last_height {
            self.last_width = self.window.width;
            self.last_height = self.window.height;
            self.render();
            return;
        }

        let expected = self.scroll as i32 * LINE_H as i32;
        if self.window.scroll.offset != expected {
            let max = self.lines.len().saturating_sub(self.rows);
            self.scroll = ((self.window.scroll.offset / LINE_H as i32) as usize).min(max);
            self.render();
        }
    }

    fn render(&mut self) {
        let width = self.window.width.max(0) as usize;
        let content_h = (self.window.height - TITLE_H).max(0) as usize;
        let stride = width;
        self.fill_background(stride);

        let text_y0 = HEADER_H + 8;
        let text_h = content_h.saturating_sub(HEADER_H + FOOTER_H + 10);
        self.rows = text_h / LINE_H;
        self.cols = width.saturating_sub(PAD_X * 2 + 8) / CHAR_W;
        self.window.scroll.content_h = (self.lines.len() * LINE_H) as i32;
        self.window.scroll.offset = self.scroll as i32 * LINE_H as i32;
        self.window.scroll.clamp((self.rows * LINE_H) as i32);

        self.fill_rect(stride, 0, 0, width, HEADER_H, PANEL_ALT);
        self.fill_rect(stride, 0, HEADER_H - 1, width, 1, PANEL_BORDER);
        self.fill_rect(
            stride,
            0,
            content_h.saturating_sub(FOOTER_H),
            width,
            FOOTER_H,
            PANEL,
        );
        self.fill_rect(
            stride,
            0,
            content_h.saturating_sub(FOOTER_H),
            width,
            1,
            PANEL_BORDER,
        );
        self.fill_rect(stride, PAD_X - 10, text_y0 - 2, 2, text_h, ACCENT);

        let heading = self.heading.clone();
        let subheading = self.subheading.clone();
        self.put_str(stride, PAD_X, 12, &heading, WHITE);
        self.put_str(stride, PAD_X, 24, &subheading, SUBTLE);

        for screen_row in 0..self.rows {
            let doc_row = self.scroll + screen_row;
            if doc_row >= self.lines.len() {
                break;
            }
            let line = self.lines[doc_row].clone();
            let py = text_y0 + screen_row * LINE_H + GLYPH_Y_INSET;
            let fg = if line.starts_with(" ==") {
                YELLOW
            } else if line.starts_with("  ") {
                LIGHT_GRAY
            } else {
                WHITE
            };
            if line.starts_with(" ==") {
                self.fill_rect(stride, PAD_X - 6, py + 1, 3, 6, YELLOW);
            }
            for (ci, c) in line.chars().enumerate() {
                if ci >= self.cols {
                    break;
                }
                let px = PAD_X + ci * CHAR_W;
                put_char_transparent(&mut self.window.buf, stride, px, py, c, fg);
            }
        }

        let mut footer = String::from("j/k scroll");
        footer.push_str("   line ");
        let current_line = self.scroll + 1;
        push_number(&mut footer, current_line);
        footer.push('/');
        push_number(&mut footer, self.lines.len().max(1));
        self.put_str(
            stride,
            PAD_X,
            content_h.saturating_sub(FOOTER_H).saturating_add(5),
            &footer,
            MUTED,
        );

        let progress = if self.lines.is_empty() {
            0
        } else {
            ((self.scroll + self.rows).min(self.lines.len()) * 100) / self.lines.len()
        };
        self.draw_bar(
            stride,
            width.saturating_sub(170),
            content_h.saturating_sub(FOOTER_H).saturating_add(6),
            140,
            6,
            progress,
            0x00_11_22_33,
            ACCENT,
        );
    }

    fn fill_background(&mut self, stride: usize) {
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = if py % 12 < 6 { BG_A } else { BG_B };
        }
    }

    fn fill_rect(&mut self, stride: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let content_h = (self.window.height - TITLE_H).max(0) as usize;
        let width = self.window.width.max(0) as usize;
        for row in y..(y + h).min(content_h) {
            let base = row * stride;
            for col in x..(x + w).min(width) {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = color;
                }
            }
        }
    }

    fn draw_bar(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        percent: usize,
        track: u32,
        fill: u32,
    ) {
        self.fill_rect(stride, x, y, w, h, track);
        let fill_w = (w.saturating_sub(2) * percent.min(100)) / 100;
        if fill_w > 0 {
            self.fill_rect(stride, x + 1, y + 1, fill_w, h.saturating_sub(2), fill);
        }
    }

    fn put_str(&mut self, stride: usize, px: usize, py: usize, s: &str, color: u32) {
        for (ci, c) in s.chars().enumerate() {
            let gx = px + ci * CHAR_W;
            if gx + CHAR_W > stride {
                break;
            }
            put_char_transparent(&mut self.window.buf, stride, gx, py, c, color);
        }
    }
}

fn push_number(out: &mut String, mut n: usize) {
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
    for i in (0..len).rev() {
        out.push(digits[i] as char);
    }
}

fn put_char_transparent(buf: &mut [u32], stride: usize, px0: usize, py0: usize, c: char, fg: u32) {
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            if byte & (1 << bit) == 0 {
                continue;
            }
            let px = px0 + bit;
            let py = py0 + gy;
            let idx = py * stride + px;
            if idx < buf.len() {
                buf[idx] = fg;
            }
        }
    }
}
