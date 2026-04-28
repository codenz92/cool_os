use crate::framebuffer::{GREEN, LIGHT_CYAN, LIGHT_GRAY, WHITE, YELLOW};
use crate::wm::window::Window;
use font8x8::UnicodeFonts;

pub const SYSMON_W: i32 = 520;
pub const SYSMON_H: i32 = 300;

const CHAR_W_SMALL: usize = 8;
const CHAR_H_SMALL: usize = 8;

const BG: u32 = 0x00_03_08_16;
const BG_ALT: u32 = 0x00_01_04_0B;
const PANEL_BG: u32 = 0x00_00_0A_1C;
const PANEL_ALT: u32 = 0x00_00_0E_24;
const PANEL_BORDER: u32 = 0x00_00_44_88;
const PANEL_INNER: u32 = 0x00_00_18_30;
const LABEL: u32 = 0x00_66_AA_DD;
const MUTED: u32 = 0x00_55_7A_92;
const USB_GOOD: u32 = 0x00_00_DD_99;
const USB_WARN: u32 = 0x00_DD_AA_44;

pub struct SysMonApp {
    pub window: Window,
    last_redraw_tick: u64,
    last_width: i32,
    last_height: i32,
}

impl SysMonApp {
    pub fn new(x: i32, y: i32) -> Self {
        let mut app = SysMonApp {
            window: Window::new(x, y, SYSMON_W, SYSMON_H, "System Monitor"),
            last_redraw_tick: 0,
            last_width: SYSMON_W,
            last_height: SYSMON_H,
        };
        app.update();
        app
    }

    pub fn update(&mut self) {
        let ticks = crate::interrupts::ticks();
        let resized =
            self.window.width != self.last_width || self.window.height != self.last_height;
        let redraw_interval = (crate::interrupts::TIMER_HZ / 12).max(1) as u64;
        if !resized && ticks.wrapping_sub(self.last_redraw_tick) < redraw_interval {
            return;
        }
        self.last_redraw_tick = ticks;
        self.last_width = self.window.width;
        self.last_height = self.window.height;

        let stride = self.window.width as usize;
        self.fill_background(stride);

        let cpuid = raw_cpuid::CpuId::new();
        let vendor_info = cpuid.get_vendor_info();
        let vendor = vendor_info
            .as_ref()
            .map(|v| v.as_str())
            .unwrap_or("unknown");

        let used = crate::allocator::heap_used();
        let heap_total = crate::allocator::HEAP_SIZE;
        let heap_ratio = if heap_total > 0 {
            (used.saturating_mul(100) / heap_total).min(100)
        } else {
            0
        };

        let secs = crate::interrupts::uptime_secs();
        let hrs = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let secs_only = secs % 60;

        let counter =
            crate::scheduler::BACKGROUND_COUNTER.load(core::sync::atomic::Ordering::Relaxed);
        let usb_lines = crate::usb::status_lines();
        let (usb_keyboard, usb_mouse) = crate::usb::input_presence();
        let usb_present = !usb_lines.is_empty();
        let usb_active = usb_lines
            .iter()
            .any(|line| line.contains("active init ready"));

        self.put_str_px(stride, 18, 14, "SYSTEM DASHBOARD", LABEL);
        self.put_str_px(
            stride,
            18,
            26,
            "runtime view for scheduler, memory, and USB",
            MUTED,
        );

        let card_w = 236usize;
        let card_h = 58usize;
        self.draw_card_frame(stride, 16, 44, card_w, card_h, 0x00_00_EE_FF);
        self.draw_card_frame(stride, 268, 44, card_w, card_h, 0x00_FF_DD_55);
        self.draw_card_frame(stride, 16, 114, card_w, card_h, 0x00_55_FF_BB);
        self.draw_card_frame(stride, 268, 114, card_w, card_h, 0x00_66_BB_FF);
        self.draw_card_frame(stride, 16, 184, 488, 98, 0x00_00_CC_FF);

        self.put_str_px(stride, 28, 58, "CPU VENDOR", LABEL);
        self.put_str_px(stride, 28, 76, vendor, GREEN);

        self.put_str_px(stride, 280, 58, "HEAP LOAD", LABEL);
        let mut heap_line = NumberLine::new();
        heap_line.push_usize(used);
        heap_line.push_str(" / ");
        heap_line.push_usize(heap_total);
        heap_line.push_str(" B");
        self.put_str_px(stride, 280, 76, heap_line.as_str(), YELLOW);
        self.draw_bar(
            stride,
            280,
            90,
            200,
            6,
            heap_ratio,
            0x00_11_22_33,
            0x00_FF_DD_55,
        );

        self.put_str_px(stride, 28, 128, "UPTIME", LABEL);
        let time = [
            b'0' + (hrs / 10) as u8,
            b'0' + (hrs % 10) as u8,
            b':',
            b'0' + (mins / 10) as u8,
            b'0' + (mins % 10) as u8,
            b':',
            b'0' + (secs_only / 10) as u8,
            b'0' + (secs_only % 10) as u8,
        ];
        if let Ok(time_str) = core::str::from_utf8(&time) {
            self.put_str_px(stride, 28, 146, time_str, LIGHT_CYAN);
        }
        let mut tick_line = NumberLine::new();
        tick_line.push_u64(ticks);
        tick_line.push_str(" ticks");
        self.put_str_px(stride, 116, 146, tick_line.as_str(), MUTED);

        self.put_str_px(stride, 280, 128, "SCHEDULER", LABEL);
        let mut counter_line = NumberLine::new();
        counter_line.push_u64(counter);
        self.put_str_px(stride, 280, 146, counter_line.as_str(), LIGHT_CYAN);
        let pulse = ((counter as usize / 64) % 100).max(8);
        self.draw_bar(
            stride,
            280,
            160,
            200,
            6,
            pulse,
            0x00_11_22_33,
            0x00_66_BB_FF,
        );

        self.put_str_px(stride, 28, 198, "USB RUNTIME", LABEL);
        self.draw_status_pill(
            stride,
            28,
            214,
            "CTRL",
            usb_present,
            if usb_present { LIGHT_CYAN } else { MUTED },
        );
        self.draw_status_pill(
            stride,
            90,
            214,
            "ACTIVE",
            usb_active,
            if usb_active { USB_GOOD } else { USB_WARN },
        );
        self.draw_status_pill(
            stride,
            164,
            214,
            "KBD",
            usb_keyboard,
            if usb_keyboard { USB_GOOD } else { MUTED },
        );
        self.draw_status_pill(
            stride,
            220,
            214,
            "MOUSE",
            usb_mouse,
            if usb_mouse { USB_GOOD } else { MUTED },
        );

        let mut row = 234usize;
        if usb_lines.is_empty() {
            self.put_str_px(stride, 28, row, "USB: no probe data", LIGHT_GRAY);
        } else {
            for line in usb_lines {
                if row + CHAR_H_SMALL > 278 {
                    break;
                }
                self.put_str_px(stride, 28, row, &line, LIGHT_CYAN);
                row += 10;
            }
        }
    }

    fn fill_background(&mut self, stride: usize) {
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = if py % 12 < 6 { BG } else { BG_ALT };
        }
    }

    fn draw_card_frame(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        accent: u32,
    ) {
        self.fill_rect(stride, x, y, w, h, PANEL_BG);
        self.fill_rect(stride, x, y, w, 3, accent);
        self.fill_rect(stride, x + 1, y + 1, w - 2, h - 2, PANEL_ALT);
        self.draw_rect_border(stride, x, y, w, h, PANEL_BORDER);
        self.draw_rect_border(stride, x + 1, y + 1, w - 2, h - 2, PANEL_INNER);
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
        self.draw_rect_border(stride, x, y, w, h, PANEL_INNER);
        let fill_w = (w.saturating_sub(2) * percent.min(100)) / 100;
        if fill_w > 0 {
            self.fill_rect(stride, x + 1, y + 1, fill_w, h.saturating_sub(2), fill);
        }
    }

    fn draw_status_pill(
        &mut self,
        stride: usize,
        x: usize,
        y: usize,
        label: &str,
        active: bool,
        accent: u32,
    ) {
        let w = label.len() * CHAR_W_SMALL + 18;
        let bg = if active { PANEL_ALT } else { PANEL_BG };
        self.fill_rect(stride, x, y, w, 14, bg);
        self.draw_rect_border(
            stride,
            x,
            y,
            w,
            14,
            if active { accent } else { PANEL_INNER },
        );
        self.fill_rect(
            stride,
            x + 4,
            y + 4,
            4,
            4,
            if active { accent } else { 0x00_33_44_55 },
        );
        self.put_str_px(
            stride,
            x + 12,
            y + 3,
            label,
            if active { WHITE } else { MUTED },
        );
    }

    fn fill_rect(&mut self, stride: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let max_h = if stride > 0 {
            self.window.buf.len() / stride
        } else {
            0
        };
        for row in y..(y + h).min(max_h) {
            let base = row * stride;
            for col in x..(x + w).min(stride) {
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

    fn put_str_px(&mut self, stride: usize, px: usize, py: usize, s: &str, color: u32) {
        for (ci, c) in s.chars().enumerate() {
            let gx = px + ci * CHAR_W_SMALL;
            if gx + CHAR_W_SMALL > stride {
                break;
            }
            put_char_buf_transparent(&mut self.window.buf, stride, gx, py, c, color);
        }
    }
}

struct NumberLine {
    buf: [u8; 64],
    len: usize,
}

impl NumberLine {
    fn new() -> Self {
        NumberLine {
            buf: [b' '; 64],
            len: 0,
        }
    }

    fn push_str(&mut self, s: &str) {
        for b in s.bytes() {
            if self.len < 64 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn push_u64(&mut self, mut n: u64) {
        if n == 0 {
            self.push_str("0");
            return;
        }
        let mut tmp = [0u8; 20];
        let mut i = 20usize;
        while n > 0 {
            i -= 1;
            tmp[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        for &b in &tmp[i..] {
            if self.len < 64 {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn push_usize(&mut self, n: usize) {
        self.push_u64(n as u64);
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

fn put_char_buf_transparent(
    buf: &mut [u32],
    stride: usize,
    px0: usize,
    py0: usize,
    c: char,
    fg: u32,
) {
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
