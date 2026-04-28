use crate::framebuffer::WHITE;
use crate::wm::window::{Window, TITLE_H};
use font8x8::UnicodeFonts;

pub const PICKER_W: i32 = 480;
pub const PICKER_H: i32 = 320;

const GRID_COLS: usize = 4;
const GRID_ROWS: usize = 4;
const SWATCH: usize = 40;
const GAP: usize = 10;
const GRID_X: usize = 20;
const GRID_Y: usize = 52;
const PREVIEW_X: usize = 224;
const PREVIEW_Y: usize = 52;
const PREVIEW_W: usize = 232;
const PREVIEW_H: usize = 228;
const CHAR_W: usize = 8;

const BG_A: u32 = 0x00_05_07_15;
const BG_B: u32 = 0x00_02_03_09;
const PANEL: u32 = 0x00_00_0B_1C;
const PANEL_ALT: u32 = 0x00_00_0F_24;
const BORDER: u32 = 0x00_00_44_88;
const ACCENT: u32 = 0x00_AA_55_FF;
const LABEL: u32 = 0x00_CC_EE_FF;
const MUTED: u32 = 0x00_66_AA_DD;

/// True RGB colors matching the classic EGA/VGA 16-colour palette.
const COLORS: [(&str, u32); 16] = [
    ("Black", 0x00_00_00_00),
    ("Blue", 0x00_00_00_AA),
    ("Green", 0x00_00_AA_00),
    ("Cyan", 0x00_00_AA_AA),
    ("Red", 0x00_AA_00_00),
    ("Magenta", 0x00_AA_00_AA),
    ("Brown", 0x00_AA_55_00),
    ("Lt Gray", 0x00_AA_AA_AA),
    ("Dk Gray", 0x00_55_55_55),
    ("Lt Blue", 0x00_55_55_FF),
    ("Lt Green", 0x00_55_FF_55),
    ("Lt Cyan", 0x00_55_FF_FF),
    ("Lt Red", 0x00_FF_55_55),
    ("Pink", 0x00_FF_55_FF),
    ("Yellow", 0x00_FF_FF_55),
    ("White", 0x00_FF_FF_FF),
];

pub struct ColorPickerApp {
    pub window: Window,
    selected: Option<usize>,
    last_width: i32,
    last_height: i32,
}

impl ColorPickerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, PICKER_W, PICKER_H, "Color Picker");
        let mut app = ColorPickerApp {
            window,
            selected: Some(11),
            last_width: PICKER_W,
            last_height: PICKER_H,
        };
        app.render();
        app
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        if lx < GRID_X as i32 || ly < GRID_Y as i32 {
            return;
        }
        let step = (SWATCH + GAP) as i32;
        let col = (lx - GRID_X as i32) / step;
        let row = (ly - GRID_Y as i32) / step;
        if col >= 0 && col < GRID_COLS as i32 && row >= 0 && row < GRID_ROWS as i32 {
            let idx = (row as usize) * GRID_COLS + col as usize;
            if idx < COLORS.len() {
                self.selected = Some(idx);
                self.render();
            }
        }
    }

    pub fn update(&mut self) {
        if self.window.width != self.last_width || self.window.height != self.last_height {
            self.last_width = self.window.width;
            self.last_height = self.window.height;
            self.render();
        }
    }

    fn render(&mut self) {
        let stride = self.window.width.max(0) as usize;
        let content_h = (self.window.height - TITLE_H).max(0) as usize;
        self.fill_background(stride);

        self.fill_rect(stride, 0, 0, stride, 34, PANEL_ALT);
        self.fill_rect(stride, 0, 0, stride, 3, ACCENT);
        self.fill_rect(stride, 0, 33, stride, 1, BORDER);
        self.fill_rect(stride, PREVIEW_X, PREVIEW_Y, PREVIEW_W, PREVIEW_H, PANEL);
        self.draw_rect_border(stride, PREVIEW_X, PREVIEW_Y, PREVIEW_W, PREVIEW_H, BORDER);
        self.draw_rect_border(
            stride,
            PREVIEW_X + 1,
            PREVIEW_Y + 1,
            PREVIEW_W - 2,
            PREVIEW_H - 2,
            0x00_00_18_30,
        );

        self.put_str(stride, 18, 12, "PALETTE LAB", LABEL);
        self.put_str(
            stride,
            18,
            24,
            "pick a swatch to inspect rgb and hex values",
            MUTED,
        );

        for i in 0..COLORS.len() {
            let col = i % GRID_COLS;
            let row = i / GRID_COLS;
            let x = GRID_X + col * (SWATCH + GAP);
            let y = GRID_Y + row * (SWATCH + GAP);
            let color = COLORS[i].1;
            let selected = self.selected == Some(i);

            self.fill_rect(stride, x, y, SWATCH, SWATCH, blend_color(color, BG_A, 20));
            self.draw_rect_border(stride, x, y, SWATCH, SWATCH, blend_color(color, WHITE, 60));
            self.fill_rect(stride, x + 4, y + 4, SWATCH - 8, SWATCH - 8, color);
            if selected {
                self.draw_rect_border(stride, x - 3, y - 3, SWATCH + 6, SWATCH + 6, ACCENT);
                self.draw_rect_border(stride, x - 2, y - 2, SWATCH + 4, SWATCH + 4, WHITE);
            }
        }

        if let Some(idx) = self.selected {
            let (name, rgb) = COLORS[idx];
            let r = ((rgb >> 16) & 0xFF) as usize;
            let g = ((rgb >> 8) & 0xFF) as usize;
            let b = (rgb & 0xFF) as usize;

            self.put_str(
                stride,
                PREVIEW_X + 16,
                PREVIEW_Y + 14,
                "CURRENT SWATCH",
                LABEL,
            );
            self.put_str(stride, PREVIEW_X + 16, PREVIEW_Y + 30, name, WHITE);

            self.fill_rect(stride, PREVIEW_X + 16, PREVIEW_Y + 50, 92, 92, rgb);
            self.draw_rect_border(stride, PREVIEW_X + 16, PREVIEW_Y + 50, 92, 92, WHITE);
            self.draw_rect_border(
                stride,
                PREVIEW_X + 112,
                PREVIEW_Y + 50,
                PREVIEW_W - 128,
                92,
                BORDER,
            );
            self.put_str(stride, PREVIEW_X + 126, PREVIEW_Y + 62, "Hex", MUTED);
            let mut hex = ['#', '0', '0', '0', '0', '0', '0'];
            write_hex(&mut hex[1..], r as u32, g as u32, b as u32);
            let hex_string: alloc::string::String = hex.iter().collect();
            self.put_str(stride, PREVIEW_X + 126, PREVIEW_Y + 78, &hex_string, WHITE);

            self.put_str(stride, PREVIEW_X + 126, PREVIEW_Y + 100, "RGB", MUTED);
            let mut rgb_line = alloc::string::String::from("R ");
            push_number(&mut rgb_line, r);
            rgb_line.push_str("  G ");
            push_number(&mut rgb_line, g);
            rgb_line.push_str("  B ");
            push_number(&mut rgb_line, b);
            self.put_str(stride, PREVIEW_X + 126, PREVIEW_Y + 116, &rgb_line, WHITE);

            self.put_str(stride, PREVIEW_X + 16, PREVIEW_Y + 160, "CHANNELS", LABEL);
            self.put_str(stride, PREVIEW_X + 16, PREVIEW_Y + 178, "RED", MUTED);
            self.put_str(stride, PREVIEW_X + 16, PREVIEW_Y + 202, "GREEN", MUTED);
            self.put_str(stride, PREVIEW_X + 16, PREVIEW_Y + 226, "BLUE", MUTED);
            self.draw_bar(
                stride,
                PREVIEW_X + 70,
                PREVIEW_Y + 178,
                140,
                8,
                r,
                0x00_AA_00_00,
            );
            self.draw_bar(
                stride,
                PREVIEW_X + 70,
                PREVIEW_Y + 202,
                140,
                8,
                g,
                0x00_00_AA_00,
            );
            self.draw_bar(
                stride,
                PREVIEW_X + 70,
                PREVIEW_Y + 226,
                140,
                8,
                b,
                0x00_00_AA_AA,
            );

            self.put_str(
                stride,
                PREVIEW_X + 16,
                PREVIEW_Y + 254,
                "classic 16-colour ega palette",
                MUTED,
            );
        }

        self.put_str(
            stride,
            GRID_X,
            PREVIEW_Y + PREVIEW_H + 8,
            "click any tile to sample",
            MUTED,
        );
        let _ = content_h;
    }

    fn fill_background(&mut self, stride: usize) {
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = if py % 10 < 5 { BG_A } else { BG_B };
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

    fn draw_rect_border(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        color: u32,
    ) {
        if w == 0 || h == 0 {
            return;
        }
        self.fill_rect(stride, x, y, w, 1, color);
        self.fill_rect(stride, x, y + h - 1, w, 1, color);
        self.fill_rect(stride, x, y, 1, h, color);
        self.fill_rect(stride, x + w - 1, y, 1, h, color);
    }

    fn draw_bar(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        value: usize,
        fill: u32,
    ) {
        self.fill_rect(stride, x, y, w, h, 0x00_11_22_33);
        self.draw_rect_border(stride, x, y, w, h, 0x00_00_18_30);
        let fill_w = (w.saturating_sub(2) * value.min(255)) / 255;
        if fill_w > 0 {
            self.fill_rect(stride, x + 1, y + 1, fill_w, h.saturating_sub(2), fill);
        }
    }

    fn put_str(&mut self, stride: usize, px: usize, py: usize, s: &str, color: u32) {
        for (ci, ch) in s.chars().enumerate() {
            let gx = px + ci * CHAR_W;
            if gx + CHAR_W > stride {
                break;
            }
            put_char_transparent(&mut self.window.buf, stride, gx, py, ch, color);
        }
    }
}

fn write_hex(dst: &mut [char], r: u32, g: u32, b: u32) {
    let bytes = [r, g, b];
    let mut pos = 0usize;
    for byte in bytes {
        for nibble in [byte >> 4, byte & 0xF] {
            dst[pos] = if nibble < 10 {
                (b'0' + nibble as u8) as char
            } else {
                (b'A' + nibble as u8 - 10) as char
            };
            pos += 1;
        }
    }
}

fn push_number(out: &mut alloc::string::String, mut n: usize) {
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
