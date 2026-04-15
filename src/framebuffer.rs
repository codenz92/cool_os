/// VGA Mode 13h pixel framebuffer — 320×200, 256-colour palette.
///
/// The bootloader's `vga_320x200` feature sets this mode before handing
/// control to the kernel, so we only need to write pixels.  The VGA
/// frame-buffer lives at physical 0xA0000, which is identity-mapped in
/// the kernel's page tables just like the text buffer at 0xB8000.

use font8x8::UnicodeFonts;

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 200;
pub const CHAR_W: usize = 8;
pub const CHAR_H: usize = 8;
pub const COLS: usize = WIDTH / CHAR_W;  // 40
pub const ROWS: usize = HEIGHT / CHAR_H; // 25

const FB: *mut u8 = 0xa0000 as *mut u8;

// Standard VGA Mode 13h palette indices (colours 0-15 match EGA/CGA).
pub const BLACK: u8 = 0;
pub const BLUE: u8 = 1;
pub const GREEN: u8 = 2;
pub const CYAN: u8 = 3;
pub const RED: u8 = 4;
pub const MAGENTA: u8 = 5;
pub const BROWN: u8 = 6;
pub const LIGHT_GRAY: u8 = 7;
pub const DARK_GRAY: u8 = 8;
pub const LIGHT_BLUE: u8 = 9;
pub const LIGHT_GREEN: u8 = 10;
pub const LIGHT_CYAN: u8 = 11;
pub const LIGHT_RED: u8 = 12;
pub const PINK: u8 = 13;
pub const YELLOW: u8 = 14;
pub const WHITE: u8 = 15;

#[inline]
pub fn put_pixel(x: usize, y: usize, color: u8) {
    if x < WIDTH && y < HEIGHT {
        unsafe { FB.add(y * WIDTH + x).write_volatile(color); }
    }
}

pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, color: u8) {
    let x_end = (x + w).min(WIDTH);
    let y_end = (y + h).min(HEIGHT);
    for row in y..y_end {
        for col in x..x_end {
            unsafe { FB.add(row * WIDTH + col).write_volatile(color); }
        }
    }
}

pub fn clear(color: u8) {
    for i in 0..(WIDTH * HEIGHT) {
        unsafe { FB.add(i).write_volatile(color); }
    }
}

/// Scroll the entire screen up by one character row and fill the vacated
/// bottom row with `bg`.
pub fn scroll_up(bg: u8) {
    unsafe {
        // Shift everything up CHAR_H pixel rows.
        core::ptr::copy(
            FB.add(CHAR_H * WIDTH),
            FB,
            (ROWS - 1) * CHAR_H * WIDTH,
        );
        // Clear the exposed bottom row.
        let bottom = (ROWS - 1) * CHAR_H * WIDTH;
        for i in bottom..(bottom + CHAR_H * WIDTH) {
            FB.add(i).write_volatile(bg);
        }
    }
}

/// Draw a single ASCII/Unicode character at character-grid position
/// `(col, row)` using `fg` and `bg` palette indices.
pub fn draw_char(col: usize, row: usize, c: char, fg: u8, bg: u8) {
    let x = col * CHAR_W;
    let y = row * CHAR_H;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            put_pixel(x + bit, y + gy, color);
        }
    }
}
