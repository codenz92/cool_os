/// Terminal app — renders a shell into a window's pixel back-buffer.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

use crate::wm::window::{Window, TITLE_H};

pub const TERM_W: i32 = 640;
pub const TERM_H: i32 = 440;

const CHAR_W_SMALL: usize = 8;
const CHAR_H_SMALL: usize = 8;

const TERM_BG_A: u32 = 0x00_03_09_06;
const TERM_BG_B: u32 = 0x00_01_04_02;
const TERM_BG_C: u32 = 0x00_06_0F_09;
const FG_OUTPUT: u32 = 0x00_B8_F3_CE;
const FG_PROMPT: u32 = 0x00_00_FF_88;
const FG_INPUT: u32 = 0x00_E4_FF_F1;
const FG_ACCENT: u32 = 0x00_55_FF_FF;
const FG_DIM: u32 = 0x00_58_8A_70;
const FG_ERROR: u32 = 0x00_FF_72_72;

pub struct TerminalApp {
    pub window: Window,
    cmd_buf:    String,
    pending_key_sink_fd: Option<usize>,
    col:        usize,
    row:        usize,
    cols:       usize,
    rows:       usize,
    fg:         u32,
}

impl TerminalApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, TERM_W, TERM_H, "Terminal");
        let cols = TERM_W as usize / CHAR_W_SMALL;
        let content_h = (TERM_H - TITLE_H) as usize;
        let rows = content_h / CHAR_H_SMALL;

        let mut t = TerminalApp {
            window,
            cmd_buf: String::new(),
            pending_key_sink_fd: None,
            col: 0,
            row: 0,
            cols,
            rows,
            fg: FG_OUTPUT,
        };
        t.fill_background();
        t.set_fg(FG_ACCENT);
        t.print_str("coolOS phosphor shell\n");
        t.set_fg(FG_DIM);
        t.print_str("type help, usb, exec /bin/hello\n\n");
        t.print_prompt();
        t
    }

    pub fn handle_key(&mut self, c: char) {
        match c {
            '\n' => {
                self.print_char('\n');
                let cmd = core::mem::take(&mut self.cmd_buf);
                self.run_command(&cmd);
            }
            '\u{0008}' => {
                if self.cmd_buf.pop().is_some() && self.col > 0 {
                    self.col -= 1;
                    self.draw_char_at(self.col, self.row, ' ');
                }
            }
            c => {
                self.cmd_buf.push(c);
                self.print_char(c);
            }
        }
    }

    pub fn take_pending_key_sink(&mut self) -> Option<usize> {
        self.pending_key_sink_fd.take()
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn run_command(&mut self, input: &str) {
        let mut words = input.split_whitespace();
        self.set_fg(FG_OUTPUT);
        match words.next() {
            Some("help") => {
                self.set_fg(FG_ACCENT);
                self.print_str("Commands");
                self.set_fg(FG_OUTPUT);
                self.print_str(": help clear reboot echo exec ipc keydemo term info uptime usb\n");
            }
            Some("clear") => {
                self.fill_background();
                self.col = 0;
                self.row = 0;
            }
            Some("reboot") => crate::interrupts::reboot(),
            Some("echo") => {
                for word in words { self.print_str(word); self.print_char(' '); }
                self.print_char('\n');
            }
            Some("exec") => {
                match words.next() {
                    Some(path) => {
                        let args: Vec<&str> = words.collect();
                        match crate::elf::spawn_elf_process_with_args(path, &args) {
                            Ok(()) => {
                                self.set_fg(FG_ACCENT);
                                self.print_str("Spawned ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(path);
                                if !args.is_empty() {
                                    self.print_str(" with args");
                                }
                                self.print_char('\n');
                            }
                            Err(err) => {
                                self.set_fg(FG_ERROR);
                                self.print_str("exec failed: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("usage: ");
                        self.set_fg(FG_OUTPUT);
                        self.print_str("exec /bin/hello [args...]\n");
                    }
                }
            }
            Some("ipc") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        let reader = crate::elf::spawn_elf_process_with_fds(
                            "/bin/piperd",
                            &[],
                            &[(read_fd, 3)],
                        );
                        let writer = crate::elf::spawn_elf_process_with_fds(
                            "/bin/pipewr",
                            &[],
                            &[(write_fd, 3)],
                        );
                        crate::vfs::vfs_close(read_fd);
                        crate::vfs::vfs_close(write_fd);

                        match (reader, writer) {
                            (Ok(()), Ok(())) => {
                                self.set_fg(FG_ACCENT);
                                self.print_str("Spawned shared pipe demo\n");
                            }
                            (Err(err), _) => {
                                self.set_fg(FG_ERROR);
                                self.print_str("ipc failed: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                            (_, Err(err)) => {
                                self.set_fg(FG_ERROR);
                                self.print_str("ipc failed: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("ipc unavailable: ");
                        self.set_fg(FG_OUTPUT);
                        self.print_str("no pipe slots\n");
                    }
                }
            }
            Some("keydemo") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        match crate::elf::spawn_elf_process_with_fds(
                            "/bin/keyecho",
                            &[],
                            &[(read_fd, 3)],
                        ) {
                            Ok(()) => {
                                crate::vfs::vfs_close(read_fd);
                                self.pending_key_sink_fd = Some(write_fd);
                                self.set_fg(FG_ACCENT);
                                self.print_str("keydemo active; type into the terminal, `~` ends input\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.set_fg(FG_ERROR);
                                self.print_str("keydemo failed: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("keydemo unavailable: ");
                        self.set_fg(FG_OUTPUT);
                        self.print_str("no pipe slots\n");
                    }
                }
            }
            Some("term") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        match crate::elf::spawn_elf_process_with_stdin(
                            "/bin/terminal",
                            &[],
                            read_fd,
                        ) {
                            Ok(()) => {
                                self.pending_key_sink_fd = Some(write_fd);
                                self.set_fg(FG_ACCENT);
                                self.print_str("userspace terminal started; type commands, Ctrl+D ends\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.set_fg(FG_ERROR);
                                self.print_str("term failed: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("term unavailable: ");
                        self.set_fg(FG_OUTPUT);
                        self.print_str("no pipe slots\n");
                    }
                }
            }
            Some("uptime") => {
                self.set_fg(FG_ACCENT);
                self.print_str("Uptime: ");
                self.set_fg(FG_OUTPUT);
                self.print_u64(crate::interrupts::ticks());
                self.print_str(" ticks\n");
            }
            Some("info") => {
                self.set_fg(FG_ACCENT);
                self.print_str("Heap: ");
                self.set_fg(FG_OUTPUT);
                self.print_u64(crate::allocator::heap_used() as u64);
                self.print_str(" bytes\n");
                let cpuid = raw_cpuid::CpuId::new();
                if let Some(v) = cpuid.get_vendor_info() {
                    self.set_fg(FG_ACCENT);
                    self.print_str("CPU: ");
                    self.set_fg(FG_OUTPUT);
                    self.print_str(v.as_str());
                    self.print_char('\n');
                }
            }
            Some("usb") => {
                let lines = crate::usb::status_lines();
                if lines.is_empty() {
                    self.set_fg(FG_ERROR);
                    self.print_str("USB: ");
                    self.set_fg(FG_OUTPUT);
                    self.print_str("no probe data\n");
                } else {
                    self.set_fg(FG_ACCENT);
                    self.print_str("USB STATUS\n");
                    for line in lines {
                        self.set_fg(FG_OUTPUT);
                        self.print_str(&line);
                        self.print_char('\n');
                    }
                }
            }
            Some(unknown) => {
                self.set_fg(FG_ERROR);
                self.print_str("Unknown: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(unknown);
                self.print_char('\n');
            }
            None => {}
        }
        self.print_prompt();
    }

    pub fn print_char(&mut self, c: char) {
        if c == '\n' { self.col = 0; self.advance_row(); return; }
        if self.col >= self.cols { self.col = 0; self.advance_row(); }
        self.draw_char_at(self.col, self.row, c);
        self.col += 1;
    }

    pub fn print_str(&mut self, s: &str) { for c in s.chars() { self.print_char(c); } }

    fn print_u64(&mut self, mut n: u64) {
        if n == 0 { self.print_char('0'); return; }
        let mut buf = [0u8; 20];
        let mut i = 20usize;
        while n > 0 { i -= 1; buf[i] = b'0' + (n % 10) as u8; n /= 10; }
        for &b in &buf[i..] { self.print_char(b as char); }
    }

    fn advance_row(&mut self) {
        self.row += 1;
        if self.row >= self.rows { self.scroll_up(); self.row = self.rows - 1; }
    }

    fn scroll_up(&mut self) {
        let stride = self.window.width as usize;
        let row_pixels = stride * CHAR_H_SMALL;
        let total = self.window.buf.len();
        self.window.buf.copy_within(row_pixels..total, 0);
        let last = total - row_pixels;
        for idx in last..total {
            let py = idx / stride;
            self.window.buf[idx] = Self::bg_at(py);
        }
    }

    fn draw_char_at(&mut self, col: usize, row: usize, c: char) {
        let glyph = font8x8::BASIC_FONTS
            .get(c)
            .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
        let px0 = col * CHAR_W_SMALL;
        let py0 = row * CHAR_H_SMALL;
        let stride = self.window.width as usize;
        for (gy, &byte) in glyph.iter().enumerate() {
            for bit in 0..8usize {
                let px = px0 + bit;
                let py = py0 + gy;
                let idx = py * stride + px;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = if byte & (1 << bit) != 0 {
                        self.fg
                    } else {
                        Self::bg_at(py)
                    };
                }
            }
        }
    }

    fn set_fg(&mut self, color: u32) {
        self.fg = color;
    }

    fn print_prompt(&mut self) {
        self.set_fg(FG_PROMPT);
        self.print_str("cool");
        self.set_fg(FG_ACCENT);
        self.print_str("> ");
        self.set_fg(FG_INPUT);
    }

    fn fill_background(&mut self) {
        let stride = self.window.width as usize;
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = Self::bg_at(py);
        }
    }

    fn bg_at(py: usize) -> u32 {
        match py % 6 {
            0 => TERM_BG_C,
            1 | 2 => TERM_BG_A,
            _ => TERM_BG_B,
        }
    }
}
