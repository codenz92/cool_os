use crate::framebuffer::{
    BLACK, GREEN, LIGHT_CYAN, LIGHT_GRAY, WHITE, YELLOW,
};
use crate::wm::window::Window;
/// System Monitor — shows CPU vendor, heap usage, and uptime.
use font8x8::UnicodeFonts;

pub const SYSMON_W: i32 = 520;
pub const SYSMON_H: i32 = 300;

const CHAR_W_SMALL: usize = 8;
const CHAR_H_SMALL: usize = 8;

pub struct SysMonApp {
    pub window: Window,
}

impl SysMonApp {
    pub fn new(x: i32, y: i32) -> Self {
        let mut app = SysMonApp {
            window: Window::new(x, y, SYSMON_W, SYSMON_H, "System Monitor"),
        };
        app.update();
        app
    }

    pub fn update(&mut self) {
        for b in self.window.buf.iter_mut() {
            *b = BLACK;
        }

        let stride = SYSMON_W as usize;
        let mut row = 0usize;

        self.put_str(stride, row, "CPU vendor:", WHITE);
        row += 1;
        let cpuid = raw_cpuid::CpuId::new();
        if let Some(v) = cpuid.get_vendor_info() {
            self.put_str(stride, row, v.as_str(), GREEN);
        } else {
            self.put_str(stride, row, "unknown", LIGHT_GRAY);
        }
        row += 2;

        self.put_str(stride, row, "Heap used:", WHITE);
        row += 1;
        let used = crate::allocator::heap_used();
        let mut line = NumberLine::new();
        line.push_usize(used);
        line.push_str(" / ");
        line.push_usize(crate::allocator::HEAP_SIZE);
        line.push_str(" B");
        self.put_str(stride, row, line.as_str(), YELLOW);
        row += 2;

        self.put_str(stride, row, "Uptime:", WHITE);
        row += 1;
        let ticks = crate::interrupts::ticks();
        let mut line = NumberLine::new();
        line.push_u64(ticks);
        line.push_str(" ticks (~");
        line.push_u64(ticks / 18);
        line.push_str("s)");
        self.put_str(stride, row, line.as_str(), YELLOW);
        row += 2;

        self.put_str(stride, row, "Scheduler:", WHITE);
        row += 1;
        let counter =
            crate::scheduler::BACKGROUND_COUNTER.load(core::sync::atomic::Ordering::Relaxed);
        let mut line = NumberLine::new();
        line.push_u64(counter);
        self.put_str(stride, row, line.as_str(), LIGHT_CYAN);
        row += 2;

        self.put_str(stride, row, "USB:", WHITE);
        row += 1;
        let usb_lines = crate::usb::status_lines();
        if usb_lines.is_empty() {
            self.put_str(stride, row, "USB: no probe data", LIGHT_GRAY);
        } else {
            let max_rows = SYSMON_H as usize / CHAR_H_SMALL;
            for line in usb_lines {
                if row >= max_rows {
                    break;
                }
                self.put_str(stride, row, &line, LIGHT_CYAN);
                row += 1;
            }
        }
    }

    fn put_str(&mut self, stride: usize, text_row: usize, s: &str, color: u32) {
        let py = text_row * CHAR_H_SMALL;
        for (ci, c) in s.chars().enumerate() {
            let px = ci * CHAR_W_SMALL;
            if px + CHAR_W_SMALL > stride {
                break;
            }
            put_char_buf(&mut self.window.buf, stride, px, py, c, color, BLACK);
        }
    }
}

// ── Number formatting helper ──────────────────────────────────────────────────

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

// ── Shared glyph helper ───────────────────────────────────────────────────────

fn put_char_buf(buf: &mut [u32], stride: usize, px0: usize, py0: usize, c: char, fg: u32, bg: u32) {
    let glyph = font8x8::BASIC_FONTS
        .get(c)
        .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
    for (gy, &byte) in glyph.iter().enumerate() {
        for bit in 0..8usize {
            let color = if byte & (1 << bit) != 0 { fg } else { bg };
            let px = px0 + bit;
            let py = py0 + gy;
            let idx = py * stride + px;
            if idx < buf.len() {
                buf[idx] = color;
            }
        }
    }
}
