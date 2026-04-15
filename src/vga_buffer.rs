/// Text-rendering layer backed by the Mode 13h pixel framebuffer.
///
/// Public API is identical to the old VGA-text implementation so that
/// `main.rs`, `interrupts.rs`, and the `print!`/`println!` macros need
/// no changes.

use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::framebuffer::{self, COLS, ROWS};

// ── Colour ────────────────────────────────────────────────────────────────────

/// Logical colours — values map directly to VGA Mode 13h palette indices 0-15.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black     = 0,
    Blue      = 1,
    Green     = 2,
    Cyan      = 3,
    Red       = 4,
    Magenta   = 5,
    Brown     = 6,
    LightGray = 7,
    DarkGray  = 8,
    LightBlue = 9,
    LightGreen  = 10,
    LightCyan   = 11,
    LightRed    = 12,
    Pink        = 13,
    Yellow      = 14,
    White       = 15,
}

// ── Writer ────────────────────────────────────────────────────────────────────

pub struct Writer {
    col: usize,
    row: usize,
    fg:  u8,
    bg:  u8,
}

impl Writer {
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.col >= COLS {
                    self.new_line();
                }
                framebuffer::draw_char(self.col, self.row, byte as char, self.fg, self.bg);
                self.col += 1;
            }
        }
    }

    pub fn backspace(&mut self) {
        // Guard: don't erase past the "> " prompt (2 chars).
        if self.col > 2 {
            self.col -= 1;
            framebuffer::draw_char(self.col, self.row, ' ', self.fg, self.bg);
        }
    }

    fn new_line(&mut self) {
        self.col = 0;
        if self.row + 1 < ROWS {
            self.row += 1;
        } else {
            framebuffer::scroll_up(self.bg);
            // row stays at ROWS - 1
        }
    }

    pub fn clear_screen(&mut self) {
        framebuffer::clear(self.bg);
        self.col = 0;
        self.row = 0;
    }

    pub fn set_color(&mut self, fore: Color, back: Color) {
        self.fg = fore as u8;
        self.bg = back as u8;
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

// ── Global instance ───────────────────────────────────────────────────────────

lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        col: 0,
        row: 0,
        fg:  Color::Yellow as u8,
        bg:  Color::Black  as u8,
    });
}

// ── Public helpers ────────────────────────────────────────────────────────────

pub fn clear_screen() {
    WRITER.lock().clear_screen();
}

pub fn backspace() {
    WRITER.lock().backspace();
}

pub fn set_color(foreground: Color, background: Color) {
    WRITER.lock().set_color(foreground, background);
}

// ── Macros ────────────────────────────────────────────────────────────────────

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
