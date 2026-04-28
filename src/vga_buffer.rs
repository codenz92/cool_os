/// Text-rendering layer backed by the 32bpp linear framebuffer.
///
/// Used by the `print!`/`println!` macros and the panic handler.
/// Writes directly to the hardware framebuffer (not through the WM shadow).
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

use crate::framebuffer;

static FRAMEBUFFER_OUTPUT_ENABLED: AtomicBool = AtomicBool::new(true);

pub struct Writer {
    col: usize,
    row: usize,
    fg: u32,
    bg: u32,
}

/// Write a byte to QEMU's debug console (I/O port 0xE9).
/// Requires `-debugcon stdio` in the QEMU command.
#[inline(always)]
fn debug_byte(byte: u8) {
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(0xE9).write(byte);
    }
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        debug_byte(byte); // mirror every byte to QEMU debug console
        match byte {
            b'\n' => self.new_line(),
            byte => {
                let cols = framebuffer::cols();
                if cols == 0 {
                    return;
                } // framebuffer not yet initialised
                if self.col >= cols {
                    self.new_line();
                }
                if framebuffer_output_enabled() {
                    framebuffer::draw_char(self.col, self.row, byte as char, self.fg, self.bg);
                }
                self.col += 1;
            }
        }
    }

    fn new_line(&mut self) {
        self.col = 0;
        let rows = framebuffer::rows();
        if rows == 0 {
            return;
        }
        if !framebuffer_output_enabled() {
            self.row = (self.row + 1).min(rows.saturating_sub(1));
            return;
        }
        if self.row + 1 < rows {
            self.row += 1;
        } else {
            framebuffer::scroll_up(self.bg);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(b'?'),
            }
        }
        Ok(())
    }
}

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        col: 0,
        row: 0,
        fg: framebuffer::YELLOW,
        bg: framebuffer::BLACK,
    });
}

#[inline]
fn framebuffer_output_enabled() -> bool {
    FRAMEBUFFER_OUTPUT_ENABLED.load(Ordering::Relaxed)
}

pub fn set_framebuffer_output(enabled: bool) {
    FRAMEBUFFER_OUTPUT_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn reset_cursor() {
    let mut writer = WRITER.lock();
    writer.col = 0;
    writer.row = 0;
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    WRITER.lock().write_fmt(args).unwrap();
}
