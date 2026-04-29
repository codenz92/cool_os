use font8x8::UnicodeFonts;

use crate::desktop_settings::{self, DesktopSettings, WallpaperPreset};
use crate::framebuffer::WHITE;
use crate::wm::window::{Window, TITLE_H};

pub const PERSONALIZE_W: i32 = 460;
pub const PERSONALIZE_H: i32 = 300;

const BG_A: u32 = 0x00_04_06_15;
const BG_B: u32 = 0x00_02_03_0A;
const PANEL: u32 = 0x00_00_0A_1E;
const PANEL_ALT: u32 = 0x00_00_0F_26;
const BORDER: u32 = 0x00_00_44_88;
const ACCENT: u32 = 0x00_AA_55_FF;
const LABEL: u32 = 0x00_CC_EE_FF;
const MUTED: u32 = 0x00_66_AA_DD;

pub struct PersonalizeApp {
    pub window: Window,
    last_width: i32,
    last_height: i32,
    last_settings: DesktopSettings,
}

impl PersonalizeApp {
    pub fn new(x: i32, y: i32) -> Self {
        let mut app = PersonalizeApp {
            window: Window::new(x, y, PERSONALIZE_W, PERSONALIZE_H, "Personalize"),
            last_width: PERSONALIZE_W,
            last_height: PERSONALIZE_H,
            last_settings: desktop_settings::snapshot(),
        };
        app.render();
        app
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        for (idx, preset) in WallpaperPreset::ALL.iter().enumerate() {
            let top = 58 + idx as i32 * 70;
            if lx >= 16 && lx < self.window.width - 16 && ly >= top && ly < top + 58 {
                desktop_settings::set_wallpaper(*preset);
                crate::wm::request_repaint();
                self.render();
                return;
            }
        }
    }

    pub fn update(&mut self) {
        let settings = desktop_settings::snapshot();
        if self.window.width != self.last_width
            || self.window.height != self.last_height
            || settings != self.last_settings
        {
            self.render();
        }
    }

    fn render(&mut self) {
        let settings = desktop_settings::snapshot();
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.last_settings = settings;

        let stride = self.window.width.max(0) as usize;
        self.fill_background(stride);
        self.window.scroll.content_h = 0;
        self.window.scroll.offset = 0;

        self.fill_rect(stride, 0, 0, stride, 36, PANEL_ALT);
        self.fill_rect(stride, 0, 35, stride, 1, BORDER);
        self.put_str(stride, 18, 12, "PERSONALIZE", LABEL);
        self.put_str(stride, 18, 24, "pick a desktop wallpaper treatment", MUTED);

        for (idx, preset) in WallpaperPreset::ALL.iter().enumerate() {
            let y = 58 + idx * 70;
            self.draw_preset_row(
                stride,
                16,
                y,
                stride.saturating_sub(32),
                58,
                *preset,
                settings.wallpaper == *preset,
            );
        }
        self.window.mark_dirty_all();
    }

    fn draw_preset_row(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        preset: WallpaperPreset,
        selected: bool,
    ) {
        let border = if selected { ACCENT } else { BORDER };
        let text = if selected { WHITE } else { LABEL };
        self.fill_rect(stride, x, y, w, h, PANEL);
        self.draw_rect_border(stride, x, y, w, h, border);
        self.draw_rect_border(
            stride,
            x + 1,
            y + 1,
            w.saturating_sub(2),
            h.saturating_sub(2),
            0x00_00_18_30,
        );

        let preview_x = x + 12;
        let preview_y = y + 10;
        let preview_w = 96usize;
        let preview_h = 36usize;
        self.draw_preview(stride, preview_x, preview_y, preview_w, preview_h, preset);
        self.draw_rect_border(
            stride,
            preview_x,
            preview_y,
            preview_w,
            preview_h,
            if selected { WHITE } else { 0x00_00_55_88 },
        );

        self.put_str(stride, x + 124, y + 14, preset.label(), text);
        self.put_str(stride, x + 124, y + 28, preset.description(), MUTED);
        self.put_str(
            stride,
            x + w.saturating_sub(68),
            y + 20,
            if selected { "ACTIVE" } else { "SET" },
            if selected { WHITE } else { LABEL },
        );
    }

    fn draw_preview(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        preset: WallpaperPreset,
    ) {
        let (top, bottom, glow) = preview_colors(preset);
        for row in 0..h {
            let t = (row as u32).saturating_mul(255) / h.max(1) as u32;
            let row_color = blend_color(top, bottom, t);
            self.fill_rect(stride, x, y + row, w, 1, row_color);
        }
        self.fill_rect(stride, x + w / 3, y + 6, w / 3, h.saturating_sub(12), glow);
        for col in (x + 6..x + w.saturating_sub(6)).step_by(18) {
            self.fill_rect(stride, col, y + 4, 1, h.saturating_sub(8), 0x00_00_22_44);
        }
        for row in (y + 4..y + h.saturating_sub(4)).step_by(9) {
            self.fill_rect(stride, x + 4, row, w.saturating_sub(8), 1, 0x00_00_22_44);
        }
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

    fn put_str(&mut self, stride: usize, x: usize, y: usize, s: &str, color: u32) {
        for (i, ch) in s.chars().enumerate() {
            if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
                for (gy, &byte) in glyph.iter().enumerate() {
                    for gx in 0..8 {
                        if (byte >> gx) & 1 == 1 {
                            let px = x + i * 8 + gx;
                            let py = y + gy;
                            let idx = py * stride + px;
                            if idx < self.window.buf.len() {
                                self.window.buf[idx] = color;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn preview_colors(preset: WallpaperPreset) -> (u32, u32, u32) {
    match preset {
        WallpaperPreset::Phosphor => (0x00_00_03_0C, 0x00_00_01_06, 0x00_00_66_DD),
        WallpaperPreset::Aurora => (0x00_00_09_10, 0x00_00_03_08, 0x00_11_CC_BB),
        WallpaperPreset::Midnight => (0x00_03_02_10, 0x00_01_01_08, 0x00_33_55_CC),
    }
}

fn blend_color(a: u32, b: u32, t: u32) -> u32 {
    let inv = 255u32.saturating_sub(t.min(255));
    let r = (((a >> 16) & 0xFF) * inv + ((b >> 16) & 0xFF) * t.min(255)) / 255;
    let g = (((a >> 8) & 0xFF) * inv + ((b >> 8) & 0xFF) * t.min(255)) / 255;
    let blue = ((a & 0xFF) * inv + (b & 0xFF) * t.min(255)) / 255;
    (r << 16) | (g << 8) | blue
}
