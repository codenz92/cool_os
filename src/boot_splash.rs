extern crate alloc;

use core::{
    hint::spin_loop,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use font8x8::UnicodeFonts;

use crate::framebuffer;

static DRAWN: AtomicBool = AtomicBool::new(false);
static LAST_PROGRESS_UNITS: AtomicUsize = AtomicUsize::new(0);

pub const BOOT_PROGRESS_TOTAL: usize = 24;

const PROGRESS_SUBSTEPS: usize = 4;
const FRAME_DELAY_SPINS: usize = 10_000;

const BG_TOP: u32 = 0x00_02_04_10;
const BG_BOTTOM: u32 = 0x00_00_01_06;
const GLOW: u32 = 0x00_00_6E_DD;
const PANEL_BG: u32 = 0x00_02_08_16;
const PANEL_EDGE: u32 = 0x00_00_BB_FF;
const PANEL_EDGE_DIM: u32 = 0x00_00_3E_70;
const TITLE: u32 = 0x00_EE_FB_FF;
const SUBTITLE: u32 = 0x00_77_BB_DD;
const LABEL: u32 = 0x00_58_8A_A8;
const TEXT: u32 = 0x00_C8_F6_FF;
const BAR_BG: u32 = 0x00_01_04_0A;
const BAR_FILL: u32 = 0x00_00_BB_FF;
const BAR_FILL_GLOW: u32 = 0x00_44_D8_FF;

pub fn show(stage: &str, completed: usize, total: usize) {
    if !DRAWN.swap(true, Ordering::Relaxed) {
        draw_static();
    }

    let total_stages = total.max(1);
    let total_units = total_stages * PROGRESS_SUBSTEPS;
    let target_units = completed.min(total_stages) * PROGRESS_SUBSTEPS;
    let previous_units = LAST_PROGRESS_UNITS.swap(target_units, Ordering::Relaxed);

    if target_units > previous_units {
        for units in (previous_units + 1)..=target_units {
            draw_progress(stage, units, total_units, units);
            short_delay();
        }
    } else {
        draw_progress(stage, target_units, total_units, target_units);
    }
}

fn draw_static() {
    let w = framebuffer::width() as i32;
    let h = framebuffer::height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }

    let glow_cx = w / 2;
    let glow_cy = h * 2 / 5;
    let glow_rx = (w * 11 / 40).max(1);
    let glow_ry = (h * 9 / 28).max(1);

    for y in 0..h {
        let base = lerp_color(BG_TOP, BG_BOTTOM, y as usize, (h - 1).max(1) as usize);
        for x in 0..w {
            let dx = (x - glow_cx).abs() as i64;
            let dy = (y - glow_cy).abs() as i64;
            let nx = dx * 255 / glow_rx as i64;
            let ny = dy * 255 / glow_ry as i64;
            let dist = (nx + ny).min(255) as u8;
            let glow = 255u8.saturating_sub(dist);
            let mut color = mix(base, GLOW, glow / 3);
            if y % 3 == 2 {
                color = darken(color, 26);
            }
            framebuffer::put_pixel(x as usize, y as usize, color);
        }
    }

    let panel_w = (w * 9 / 20).max(420);
    let panel_h = 188;
    let panel_x = (w - panel_w) / 2;
    let panel_y = (h - panel_h) / 2 - 22;
    fill_rect(panel_x, panel_y, panel_w, panel_h, PANEL_BG);
    draw_rect(
        panel_x - 2,
        panel_y - 2,
        panel_w + 4,
        panel_h + 4,
        PANEL_EDGE_DIM,
    );
    draw_rect(panel_x, panel_y, panel_w, panel_h, PANEL_EDGE);
    draw_rect(
        panel_x + 1,
        panel_y + 1,
        panel_w - 2,
        panel_h - 2,
        PANEL_EDGE_DIM,
    );

    let icon_s = 84;
    let title_y = panel_y + 34;
    let subtitle_y = panel_y + 82;
    let title_w = text_width_scaled_with_tracking("coolOS", 4, 0);
    let subtitle_w = text_width_scaled("PHOSPHOR DESKTOP", 1);
    let text_w = title_w.max(subtitle_w);
    let lockup_gap = 28;
    let lockup_w = icon_s + lockup_gap + text_w;
    let lockup_x = panel_x + (panel_w - lockup_w) / 2;
    let icon_x = lockup_x;
    let text_x = icon_x + icon_s + lockup_gap;
    let logo_size = 18 * 4;
    let logo_y = title_y + (((subtitle_y + 8) - title_y) - logo_size) / 2;
    draw_logo_icon(icon_x + 6, logo_y);

    draw_str_scaled_with_tracking(text_x, title_y, "coolOS", TITLE, 4, 0);
    draw_str_scaled(
        text_x + (text_w - subtitle_w) / 2,
        subtitle_y,
        "PHOSPHOR DESKTOP",
        SUBTITLE,
        1,
    );
    draw_str_scaled(
        panel_x + 36,
        panel_y + panel_h - 78,
        "boot sequence",
        LABEL,
        1,
    );
}

fn draw_progress(stage: &str, completed_units: usize, total_units: usize, phase: usize) {
    let w = framebuffer::width() as i32;
    let h = framebuffer::height() as i32;
    if w <= 0 || h <= 0 {
        return;
    }

    let panel_w = (w * 9 / 20).max(420);
    let panel_h = 188;
    let panel_x = (w - panel_w) / 2;
    let panel_y = (h - panel_h) / 2 - 22;

    let bar_x = panel_x + 36;
    let bar_y = panel_y + panel_h - 52;
    let bar_w = panel_w - 72;
    let bar_h = 16;
    let stage_y = bar_y - 22;

    fill_rect(bar_x, stage_y - 4, bar_w, 18, PANEL_BG);
    draw_str_scaled(bar_x, stage_y, stage, TEXT, 1);

    fill_rect(bar_x, bar_y, bar_w, bar_h, BAR_BG);
    draw_rect(bar_x, bar_y, bar_w, bar_h, PANEL_EDGE_DIM);

    let fill = if total_units == 0 {
        bar_w - 4
    } else {
        (((bar_w - 4) as i64 * completed_units.min(total_units) as i64) / total_units as i64) as i32
    };
    if fill > 0 {
        fill_rect(bar_x + 2, bar_y + 2, fill, bar_h - 4, BAR_FILL);
        if fill > 6 {
            fill_rect(bar_x + 2, bar_y + 2, fill, 3, BAR_FILL_GLOW);
            let stripe_offset = (phase as i32 * 7) % 18;
            let mut sx = bar_x + 2 - stripe_offset;
            while sx < bar_x + 2 + fill {
                let stripe_x = sx.max(bar_x + 2);
                let stripe_w = (6).min(bar_x + 2 + fill - stripe_x);
                if stripe_w > 0 {
                    fill_rect(
                        stripe_x,
                        bar_y + 5,
                        stripe_w,
                        bar_h - 8,
                        mix(BAR_FILL, framebuffer::WHITE, 46),
                    );
                }
                sx += 18;
            }

            let head_w = 12.min(fill);
            fill_rect(
                bar_x + 2 + fill - head_w,
                bar_y + 2,
                head_w,
                bar_h - 4,
                mix(BAR_FILL_GLOW, framebuffer::WHITE, 70),
            );
        }
    }
}

fn draw_logo_icon(x: i32, y: i32) {
    for rect in crate::branding::SNOWFLAKE_LOGO_RECTS.iter() {
        let color = if rect.highlight {
            BAR_FILL_GLOW
        } else {
            BAR_FILL
        };
        fill_rect(
            x + rect.x * 4,
            y + rect.y * 4,
            rect.w * 4,
            rect.h * 4,
            color,
        );
    }
}

fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: u32) {
    let sw = framebuffer::width() as i32;
    let sh = framebuffer::height() as i32;
    if w <= 0 || h <= 0 || sw <= 0 || sh <= 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + w).min(sw);
    let y1 = (y + h).min(sh);
    for py in y0..y1 {
        for px in x0..x1 {
            framebuffer::put_pixel(px as usize, py as usize, color);
        }
    }
}

fn draw_rect(x: i32, y: i32, w: i32, h: i32, color: u32) {
    if w <= 0 || h <= 0 {
        return;
    }
    fill_rect(x, y, w, 1, color);
    fill_rect(x, y + h - 1, w, 1, color);
    fill_rect(x, y, 1, h, color);
    fill_rect(x + w - 1, y, 1, h, color);
}

fn draw_str_scaled(x: i32, y: i32, text: &str, color: u32, scale: usize) {
    draw_str_scaled_with_tracking(x, y, text, color, scale, scale as i32);
}

fn draw_str_scaled_with_tracking(
    x: i32,
    y: i32,
    text: &str,
    color: u32,
    scale: usize,
    tracking: i32,
) {
    let mut cx = x;
    for ch in text.chars() {
        draw_char_scaled(cx, y, ch, color, scale);
        cx += 8 * scale as i32 + tracking;
    }
}

fn text_width_scaled(text: &str, scale: usize) -> i32 {
    text_width_scaled_with_tracking(text, scale, scale as i32)
}

fn text_width_scaled_with_tracking(text: &str, scale: usize, tracking: i32) -> i32 {
    let chars = text.chars().count() as i32;
    if chars == 0 {
        0
    } else {
        chars * (8 * scale as i32) + (chars - 1) * tracking
    }
}

fn draw_char_scaled(x: i32, y: i32, c: char, color: u32, scale: usize) {
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            if byte & (1 << bit) == 0 {
                continue;
            }
            let px = x + (bit * scale) as i32;
            let py = y + (gy * scale) as i32;
            fill_rect(px, py, scale as i32, scale as i32, color);
        }
    }
}

fn lerp_color(a: u32, b: u32, num: usize, den: usize) -> u32 {
    let den = den.max(1) as u32;
    let num = num.min(den as usize) as u32;
    let inv = den - num;
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    (((ar * inv + br * num) / den) << 16)
        | (((ag * inv + bg * num) / den) << 8)
        | ((ab * inv + bb * num) / den)
}

fn mix(base: u32, accent: u32, alpha: u8) -> u32 {
    let inv = 255u32 - alpha as u32;
    let alpha = alpha as u32;
    let br = (base >> 16) & 0xFF;
    let bg = (base >> 8) & 0xFF;
    let bb = base & 0xFF;
    let ar = (accent >> 16) & 0xFF;
    let ag = (accent >> 8) & 0xFF;
    let ab = accent & 0xFF;
    (((br * inv + ar * alpha) / 255) << 16)
        | (((bg * inv + ag * alpha) / 255) << 8)
        | ((bb * inv + ab * alpha) / 255)
}

fn darken(color: u32, amount: u8) -> u32 {
    let factor = 255u32.saturating_sub(amount as u32);
    let r = ((color >> 16) & 0xFF) * factor / 255;
    let g = ((color >> 8) & 0xFF) * factor / 255;
    let b = (color & 0xFF) * factor / 255;
    (r << 16) | (g << 8) | b
}

fn short_delay() {
    for _ in 0..FRAME_DELAY_SPINS {
        spin_loop();
    }
}
