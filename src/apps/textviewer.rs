extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::framebuffer::{DARK_GRAY, LIGHT_GRAY, WHITE, YELLOW};
use crate::wm::window::{Window, TITLE_H};
/// Text Viewer — scrollable read-only document display.
/// Press 'j' to scroll down, 'k' to scroll up.
use font8x8::UnicodeFonts;

pub const VIEWER_W: i32 = 640;
pub const VIEWER_H: i32 = 480;

const CHAR_W: usize = 8;
const CHAR_H: usize = 8;

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
    " uptime  - tick count",
    " clear   - clear term",
    " reboot  - restart OS",
    "",
    " == Controls ==",
    " j / k   scroll dn/up",
    " Drag title bar: move",
    " x button: close win",
    " Right-click: new app",
    " File Manager: browse FAT32 + open text files",
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
}

impl TextViewerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, VIEWER_W, VIEWER_H, "Text Viewer");
        let content_h = (VIEWER_H - TITLE_H) as usize;
        let mut app = TextViewerApp {
            window,
            lines: ABOUT.iter().map(|line| String::from(*line)).collect(),
            scroll: 0,
            rows: content_h / CHAR_H,
            cols: VIEWER_W as usize / CHAR_W,
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
        app.scroll = 0;
        app.render();
        Ok(app)
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

    fn render(&mut self) {
        let stride = VIEWER_W as usize;
        for b in self.window.buf.iter_mut() {
            *b = DARK_GRAY;
        }
        self.window.scroll.content_h = (self.lines.len() * CHAR_H) as i32;
        self.window.scroll.clamp((self.rows * CHAR_H) as i32);

        for screen_row in 0..self.rows {
            let doc_row = self.scroll + screen_row;
            if doc_row >= self.lines.len() {
                break;
            }
            let line = &self.lines[doc_row];
            let py = screen_row * CHAR_H;
            for (ci, c) in line.chars().enumerate() {
                if ci >= self.cols {
                    break;
                }
                let px = ci * CHAR_W;
                let fg = if line.starts_with(" ==") {
                    YELLOW
                } else if line.starts_with("  ") {
                    LIGHT_GRAY
                } else {
                    WHITE
                };
                put_char(&mut self.window.buf, stride, px, py, c, fg);
            }
        }

        // Scroll indicators.
        let top_color = if self.scroll > 0 {
            LIGHT_GRAY
        } else {
            DARK_GRAY
        };
        let bot_color = if self.scroll + self.rows < self.lines.len() {
            LIGHT_GRAY
        } else {
            DARK_GRAY
        };
        let hint_row = (self.rows - 1) * CHAR_H;
        for px in 0..stride {
            if self.window.buf[px] != DARK_GRAY {
                self.window.buf[px] = top_color;
            }
            let idx = hint_row * stride + px;
            if idx < self.window.buf.len() {
                self.window.buf[idx] = bot_color;
            }
        }
    }
}

fn put_char(buf: &mut [u32], stride: usize, px0: usize, py0: usize, c: char, fg: u32) {
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            if byte & (1 << bit) != 0 {
                let px = px0 + bit;
                let py = py0 + gy;
                let idx = py * stride + px;
                if idx < buf.len() {
                    buf[idx] = fg;
                }
            }
        }
    }
}
