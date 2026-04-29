extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use font8x8::UnicodeFonts;

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
        String::from("PSF loader hook: ready for /FONTS/*.PSF once packaged"),
    ]
}
