/// Color Picker — shows 16 EGA-style colours as clickable swatches.
/// Click a swatch to select it; the status bar shows its name and hex value.

use font8x8::UnicodeFonts;
use crate::framebuffer::{CHAR_W, FONT_SCALE, WHITE, LIGHT_GRAY, DARK_GRAY};
use crate::wm::window::{Window, TITLE_H};

pub const PICKER_W: i32 = 480;
pub const PICKER_H: i32 = 320;

const SWATCH: i32 = 32;   // pixels per swatch
const GAP:    i32 = 6;
const STEP:   i32 = SWATCH + GAP;
const GRID_X: i32 = 20;
const GRID_Y: i32 = 16;
const COLS:   i32 = 8;

/// True RGB colors matching the classic EGA/VGA 16-colour palette.
const COLORS: [(&str, u32); 16] = [
    ("Black",    0x00_00_00_00),
    ("Blue",     0x00_00_00_AA),
    ("Green",    0x00_00_AA_00),
    ("Cyan",     0x00_00_AA_AA),
    ("Red",      0x00_AA_00_00),
    ("Magenta",  0x00_AA_00_AA),
    ("Brown",    0x00_AA_55_00),
    ("Lt Gray",  0x00_AA_AA_AA),
    ("Dk Gray",  0x00_55_55_55),
    ("Lt Blue",  0x00_55_55_FF),
    ("Lt Green", 0x00_55_FF_55),
    ("Lt Cyan",  0x00_55_FF_FF),
    ("Lt Red",   0x00_FF_55_55),
    ("Pink",     0x00_FF_55_FF),
    ("Yellow",   0x00_FF_FF_55),
    ("White",    0x00_FF_FF_FF),
];

pub struct ColorPickerApp {
    pub window: Window,
    selected:   Option<usize>,
}

impl ColorPickerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, PICKER_W, PICKER_H, "Color Picker");
        let mut app = ColorPickerApp { window, selected: None };
        app.render();
        app
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        let col = (lx - GRID_X) / STEP;
        let row = (ly - GRID_Y) / STEP;
        if col >= 0 && col < COLS && row >= 0 && row < 2 {
            let idx = (row * COLS + col) as usize;
            if idx < 16 { self.selected = Some(idx); self.render(); }
        }
    }

    fn render(&mut self) {
        let stride = PICKER_W as usize;
        let content_h = (PICKER_H - TITLE_H) as usize;
        for b in self.window.buf.iter_mut() { *b = DARK_GRAY; }

        for i in 0..16usize {
            let col = (i % COLS as usize) as i32;
            let row = (i / COLS as usize) as i32;
            let x   = GRID_X + col * STEP;
            let y   = GRID_Y + row * STEP;
            let color    = COLORS[i].1;
            let selected = self.selected == Some(i);

            if selected {
                fill_buf(&mut self.window.buf, stride, content_h,
                         x - 2, y - 2, SWATCH + 4, SWATCH + 4, WHITE);
            }
            fill_buf(&mut self.window.buf, stride, content_h, x, y, SWATCH, SWATCH, color);
        }

        // Status bar.
        let status_py = (GRID_Y + 2 * STEP + GAP) as usize;
        if let Some(idx) = self.selected {
            let (name, rgb) = COLORS[idx];
            let r = (rgb >> 16) & 0xFF;
            let g = (rgb >>  8) & 0xFF;
            let b =  rgb        & 0xFF;

            // Build "Name #RRGGBB"
            let mut line = [b' '; 24];
            let mut pos = 0usize;
            for byte in name.bytes() { if pos < 24 { line[pos] = byte; pos += 1; } }
            if pos < 24 { line[pos] = b' '; pos += 1; }
            if pos < 24 { line[pos] = b'#'; pos += 1; }
            for nibble in [r >> 4, r & 0xF, g >> 4, g & 0xF, b >> 4, b & 0xF] {
                if pos < 24 {
                    line[pos] = if nibble < 10 { b'0' + nibble as u8 }
                                else           { b'A' + nibble as u8 - 10 };
                    pos += 1;
                }
            }

            for (ci, &byte) in line[..pos].iter().enumerate() {
                let px = ci * CHAR_W;
                if px + CHAR_W > stride { break; }
                put_char_buf(&mut self.window.buf, stride, px, status_py,
                             byte as char, WHITE, DARK_GRAY);
            }
        } else {
            let hint = "Click a colour";
            for (ci, c) in hint.chars().enumerate() {
                let px = ci * CHAR_W;
                if px + CHAR_W > stride { break; }
                put_char_buf(&mut self.window.buf, stride, px, status_py,
                             c, LIGHT_GRAY, DARK_GRAY);
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn fill_buf(buf: &mut [u32], stride: usize, content_h: usize,
            x: i32, y: i32, w: i32, h: i32, color: u32)
{
    let x0 = (x.max(0) as usize).min(stride);
    let y0 = (y.max(0) as usize).min(content_h);
    let x1 = ((x + w).max(0) as usize).min(stride);
    let y1 = ((y + h).max(0) as usize).min(content_h);
    if x0 >= x1 || y0 >= y1 { return; }
    for row in y0..y1 {
        let base = row * stride;
        buf[base + x0..base + x1].fill(color);
    }
}

fn put_char_buf(buf: &mut [u32], stride: usize,
                px0: usize, py0: usize, c: char, fg: u32, bg: u32)
{
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            for sy in 0..FONT_SCALE {
                for sx in 0..FONT_SCALE {
                    let px = px0 + bit * FONT_SCALE + sx;
                    let py = py0 + gy  * FONT_SCALE + sy;
                    let idx = py * stride + px;
                    if idx < buf.len() { buf[idx] = color; }
                }
            }
        }
    }
}
