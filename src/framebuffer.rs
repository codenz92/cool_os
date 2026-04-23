// Suppress dead_code for palette constants and put_pixel — they're part of the
// public API and will be used by future phases.
#![allow(dead_code)]

/// Linear 32bpp framebuffer — high-resolution, bootloader-provided.
///
/// The bootloader delivers a base address, width, height, stride (pixels per
/// row), and pixel format (RGB or BGR) at boot time.  We store these in a
/// global and expose the public helpers used by `vga_buffer` (panic output)
/// and the compositor.

use spin::Mutex;

// ── Font constants ────────────────────────────────────────────────────────────

/// Render each 8×8 font glyph at 2× scale → 16×16 effective pixels.
pub const FONT_SCALE: usize = 2;
pub const CHAR_W:     usize = 8 * FONT_SCALE; // 16
pub const CHAR_H:     usize = 8 * FONT_SCALE; // 16

// ── Pixel format ──────────────────────────────────────────────────────────────

/// How bytes are ordered in a 4-byte framebuffer pixel on this machine.
/// Our internal color representation is always `0x00_RR_GG_BB`.
/// For BGR hardware: write the u32 as-is (on LE the bytes land B, G, R, 0).
/// For RGB hardware: swap R and B before writing.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum PixFmt { Bgr, Rgb }

// ── Global state ──────────────────────────────────────────────────────────────

struct FbState {
    base:   u64,   // pointer to first pixel (as integer for Send-safety)
    width:  usize,
    height: usize,
    stride: usize, // pixels per row (≥ width, may include right-hand padding)
    bpp:    usize, // bytes per pixel (3 or 4)
    fmt:    PixFmt,
}

static FB: Mutex<Option<FbState>> = Mutex::new(None);

/// Called once in `kernel_main` after the bootloader provides framebuffer info.
pub fn init(base: u64, width: usize, height: usize, stride: usize, bpp: usize, fmt: PixFmt) {
    *FB.lock() = Some(FbState { base, width, height, stride, bpp, fmt });
}

/// Screen width in pixels (0 until `init` is called).
pub fn width()  -> usize { FB.lock().as_ref().map_or(0, |s| s.width)  }
/// Screen height in pixels (0 until `init` is called).
pub fn height() -> usize { FB.lock().as_ref().map_or(0, |s| s.height) }
/// Pixel stride — pixels per scanline (≥ width).
pub fn stride() -> usize { FB.lock().as_ref().map_or(0, |s| s.stride) }
/// Bytes per pixel (3 or 4).
pub fn bpp()    -> usize { FB.lock().as_ref().map_or(4, |s| s.bpp)    }
/// Base pointer as u64.
pub fn base()   -> u64   { FB.lock().as_ref().map_or(0, |s| s.base)   }
/// Pixel format.
pub fn fmt()    -> PixFmt { FB.lock().as_ref().map_or(PixFmt::Bgr, |s| s.fmt) }

/// Character columns available on screen.
pub fn cols() -> usize { width()  / CHAR_W }
/// Character rows available on screen.
pub fn rows() -> usize { height() / CHAR_H }

// ── Color constants (0x00_RR_GG_BB) ──────────────────────────────────────────

// All colour constants are part of the public palette API; suppress dead_code
// warnings for the ones not yet used by the current set of apps.
// All colour constants are part of the public palette API.
pub const BLACK:       u32 = 0x00_00_00_00;
pub const BLUE:        u32 = 0x00_00_00_AA;
pub const GREEN:       u32 = 0x00_00_AA_00;
pub const CYAN:        u32 = 0x00_00_AA_AA;
pub const RED:         u32 = 0x00_AA_00_00;
pub const MAGENTA:     u32 = 0x00_AA_00_AA;
pub const BROWN:       u32 = 0x00_AA_55_00;
pub const LIGHT_GRAY:  u32 = 0x00_AA_AA_AA;
pub const DARK_GRAY:   u32 = 0x00_55_55_55;
pub const DARK_BLUE:   u32 = 0x00_00_00_40;
pub const GRAY:       u32 = 0x00_77_77_77;
pub const SELECTED_BG: u32 = 0x00_00_00_80;
pub const LIGHT_BLUE:  u32 = 0x00_55_55_FF;
pub const LIGHT_GREEN: u32 = 0x00_55_FF_55;
pub const LIGHT_CYAN:  u32 = 0x00_55_FF_FF;
pub const LIGHT_RED:   u32 = 0x00_FF_55_55;
pub const PINK:        u32 = 0x00_FF_55_FF;
pub const YELLOW:      u32 = 0x00_FF_FF_55;
pub const WHITE:       u32 = 0x00_FF_FF_FF;

// ── Pixel-format conversion ───────────────────────────────────────────────────

/// Swap the R and B channels of a `0x00_RR_GG_BB` color to produce
/// `0x00_BB_GG_RR`, needed when writing to an RGB framebuffer.
#[inline(always)]
fn to_hw(color: u32, fmt: PixFmt) -> u32 {
    match fmt {
        PixFmt::Bgr => color,  // 0x00RRGGBB bytes = [BB GG RR 00] = BGR ✓
        PixFmt::Rgb => {       // need bytes [RR GG BB 00]
            let r = (color >> 16) & 0xFF;
            let g = (color >>  8) & 0xFF;
            let b =  color        & 0xFF;
            (b << 16) | (g << 8) | r
        }
    }
}

// ── Low-level pixel writer (handles 3bpp and 4bpp) ───────────────────────────

/// Write one pixel at `offset` (in pixels from the framebuffer base).
/// `hw_color` must already be in hardware byte order (`to_hw` applied).
#[inline(always)]
unsafe fn write_hw_pixel(base: u64, offset: usize, bpp: usize, hw_color: u32) {
    match bpp {
        4 => (base as *mut u32).add(offset).write_volatile(hw_color),
        3 => {
            let p = (base as *mut u8).add(offset * 3);
            p.write_volatile((hw_color       & 0xFF) as u8);
            p.add(1).write_volatile(((hw_color >>  8) & 0xFF) as u8);
            p.add(2).write_volatile(((hw_color >> 16) & 0xFF) as u8);
        }
        _ => {}
    }
}

// ── Direct-to-hardware pixel write (used by vga_buffer panic output) ──────────

pub fn put_pixel(x: usize, y: usize, color: u32) {
    let guard = FB.lock();
    if let Some(ref s) = *guard {
        if x < s.width && y < s.height {
            unsafe {
                write_hw_pixel(s.base, y * s.stride + x, s.bpp, to_hw(color, s.fmt));
            }
        }
    }
}

/// Draw a single character directly onto the hardware framebuffer at
/// character-grid cell `(col, row)`, scaled by `FONT_SCALE`.
pub fn draw_char(col: usize, row: usize, c: char, fg: u32, bg: u32) {
    use font8x8::UnicodeFonts;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());

    let x0 = col * CHAR_W;
    let y0 = row * CHAR_H;

    let guard = FB.lock();
    if let Some(ref s) = *guard {
        let hw_fg = to_hw(fg, s.fmt);
        let hw_bg = to_hw(bg, s.fmt);

        for (gy, byte) in glyph.iter().enumerate() {
            for bit in 0..8usize {
                let hw_color = if byte & (1 << bit) != 0 { hw_fg } else { hw_bg };
                for sy in 0..FONT_SCALE {
                    for sx in 0..FONT_SCALE {
                        let px = x0 + bit * FONT_SCALE + sx;
                        let py = y0 + gy  * FONT_SCALE + sy;
                        if px < s.width && py < s.height {
                            unsafe {
                                write_hw_pixel(s.base, py * s.stride + px, s.bpp, hw_color);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Draw a single character at 1× scale (8×8 pixels) to a buffer.
pub fn draw_char_small(buf: &mut [u32], stride: usize, x: i32, y: i32, c: char, fg: u32, bg: u32) {
    use font8x8::UnicodeFonts;
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    let sh = if stride > 0 { buf.len() / stride } else { 0 };
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            let px = x + bit as i32;
            let py = y + gy as i32;
            if px >= 0 && py >= 0 {
                let (px, py) = (px as usize, py as usize);
                if px < stride && py < sh {
                    buf[py * stride + px] = color;
                }
            }
        }
    }
}

/// Draw a string at 1× scale (8×8 pixels) to a buffer.
pub fn draw_str_small(buf: &mut [u32], stride: usize, x: i32, y: i32, text: &str, fg: u32, bg: u32, max_x: i32) {
    let mut cx = x;
    for c in text.chars() {
        if cx + 8 > max_x {
            break;
        }
        draw_char_small(buf, stride, cx, y, c, fg, bg);
        cx += 8;
    }
}

/// Scroll the hardware framebuffer up by one character row, filling the
/// vacated bottom row with `bg`.  Used by the panic-mode VGA writer.
pub fn scroll_up(bg: u32) {
    let guard = FB.lock();
    if let Some(ref s) = *guard {
        let rows_to_keep = s.height.saturating_sub(CHAR_H);
        let hw_bg = to_hw(bg, s.fmt);
        let bpp = s.bpp;
        let base = s.base;
        let stride = s.stride;
        unsafe {
            // Shift all pixel rows up by CHAR_H rows (byte-accurate).
            let byte_stride = stride * bpp;
            let src = (base as *mut u8).add(CHAR_H * byte_stride);
            let dst = base as *mut u8;
            core::ptr::copy(src, dst, rows_to_keep * byte_stride);
            // Clear the newly revealed bottom rows.
            let bottom_base = base + (rows_to_keep * byte_stride) as u64;
            for i in 0..CHAR_H * stride {
                write_hw_pixel(bottom_base, i, bpp, hw_bg);
            }
        }
    }
}
