extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use font8x8::UnicodeFonts;

static PSF_LOADED: AtomicBool = AtomicBool::new(false);
static PSF_GLYPHS: AtomicUsize = AtomicUsize::new(0);
static PSF_WIDTH: AtomicUsize = AtomicUsize::new(8);
static PSF_HEIGHT: AtomicUsize = AtomicUsize::new(8);
static DPI_SCALE: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy)]
pub struct BitmapFont {
    pub name: &'static str,
    pub cell_w: usize,
    pub cell_h: usize,
    pub scale: usize,
}

pub const UI_FONT: BitmapFont = BitmapFont {
    name: "font8x8-scalable",
    cell_w: 8,
    cell_h: 8,
    scale: 1,
};

pub const LARGE_UI_FONT: BitmapFont = BitmapFont {
    name: "font8x8-scalable",
    cell_w: 8,
    cell_h: 8,
    scale: 2,
};

pub fn load_from_disk() {
    let scale = if crate::accessibility::snapshot().large_text {
        2
    } else {
        1
    };
    DPI_SCALE.store(scale, Ordering::Relaxed);
    let Some(bytes) = crate::fat32::read_file("/FONTS/DEFAULT.PSF") else {
        return;
    };
    if parse_psf2(&bytes) || parse_psf1(&bytes) {
        crate::klog::log("font: loaded /FONTS/DEFAULT.PSF metadata");
    } else {
        crate::klog::log("font: /FONTS/DEFAULT.PSF has unsupported header");
    }
}

#[allow(dead_code)]
pub fn ui_font_for_app(_app: &str) -> BitmapFont {
    if DPI_SCALE.load(Ordering::Relaxed) > 1 {
        LARGE_UI_FONT
    } else {
        UI_FONT
    }
}

pub fn text_width(text: &str, font: BitmapFont) -> usize {
    text.chars().count() * font.cell_w * font.scale
}

pub fn draw_char(
    buf: &mut [u32],
    stride: usize,
    x: usize,
    y: usize,
    ch: char,
    fg: u32,
    bg: Option<u32>,
    font: BitmapFont,
) {
    let Some(glyph) = font8x8::BASIC_FONTS
        .get(ch)
        .or_else(|| font8x8::BASIC_FONTS.get(' '))
    else {
        return;
    };
    let scale = font.scale.max(1);
    let height = if stride > 0 { buf.len() / stride } else { 0 };
    for (gy, &byte) in glyph.iter().enumerate().take(font.cell_h) {
        for bit in 0..font.cell_w {
            let ink = byte & (1 << bit) != 0;
            let Some(color) = (if ink { Some(fg) } else { bg }) else {
                continue;
            };
            for sy in 0..scale {
                for sx in 0..scale {
                    let px = x + bit * scale + sx;
                    let py = y + gy * scale + sy;
                    if px < stride && py < height {
                        buf[py * stride + px] = color;
                    }
                }
            }
        }
    }
}

pub fn draw_str(
    buf: &mut [u32],
    stride: usize,
    x: usize,
    y: usize,
    text: &str,
    fg: u32,
    bg: Option<u32>,
    font: BitmapFont,
) {
    let mut cx = x;
    for ch in text.chars() {
        draw_char(buf, stride, cx, y, ch, fg, bg, font);
        cx += font.cell_w * font.scale;
    }
}

pub fn lines() -> Vec<String> {
    alloc::vec![
        format!(
            "{}: {}x{} scale={} width(sample)={}",
            UI_FONT.name,
            UI_FONT.cell_w,
            UI_FONT.cell_h,
            UI_FONT.scale,
            text_width("coolOS", UI_FONT)
        ),
        format!(
            "{}: {}x{} scale={} width(sample)={}",
            LARGE_UI_FONT.name,
            LARGE_UI_FONT.cell_w,
            LARGE_UI_FONT.cell_h,
            LARGE_UI_FONT.scale,
            text_width("coolOS", LARGE_UI_FONT)
        ),
        format!(
            "active PSF: loaded={} glyphs={} cell={}x{} dpi_scale={}",
            PSF_LOADED.load(Ordering::Relaxed),
            PSF_GLYPHS.load(Ordering::Relaxed),
            PSF_WIDTH.load(Ordering::Relaxed),
            PSF_HEIGHT.load(Ordering::Relaxed),
            DPI_SCALE.load(Ordering::Relaxed)
        ),
        String::from("fallback: font8x8 Unicode BASIC_FONTS for missing glyphs"),
    ]
}

fn parse_psf1(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || bytes[0] != 0x36 || bytes[1] != 0x04 {
        return false;
    }
    let mode = bytes[2];
    let charsize = bytes[3] as usize;
    let glyphs = if mode & 0x01 != 0 { 512 } else { 256 };
    if bytes.len() < 4 + glyphs * charsize {
        return false;
    }
    PSF_LOADED.store(true, Ordering::Relaxed);
    PSF_GLYPHS.store(glyphs, Ordering::Relaxed);
    PSF_WIDTH.store(8, Ordering::Relaxed);
    PSF_HEIGHT.store(charsize, Ordering::Relaxed);
    true
}

fn parse_psf2(bytes: &[u8]) -> bool {
    if bytes.len() < 32 || bytes[0..4] != [0x72, 0xb5, 0x4a, 0x86] {
        return false;
    }
    let header_size = le_u32(bytes, 8) as usize;
    let glyphs = le_u32(bytes, 16) as usize;
    let charsize = le_u32(bytes, 20) as usize;
    let height = le_u32(bytes, 24) as usize;
    let width = le_u32(bytes, 28) as usize;
    if header_size == 0 || glyphs == 0 || bytes.len() < header_size + glyphs * charsize {
        return false;
    }
    PSF_LOADED.store(true, Ordering::Relaxed);
    PSF_GLYPHS.store(glyphs, Ordering::Relaxed);
    PSF_WIDTH.store(width.max(1), Ordering::Relaxed);
    PSF_HEIGHT.store(height.max(1), Ordering::Relaxed);
    true
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
