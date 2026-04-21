/// Terminal app — renders a shell into a window's pixel back-buffer.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

use crate::framebuffer::{CHAR_W, CHAR_H, FONT_SCALE, BLACK, LIGHT_GRAY};
use crate::wm::window::{Window, TITLE_H};

pub const TERM_W: i32 = 640;
pub const TERM_H: i32 = 440;

const FG: u32 = LIGHT_GRAY;
const BG: u32 = BLACK;
pub struct TerminalApp {
    pub window: Window,
    cmd_buf:    String,
    pending_key_sink_fd: Option<usize>,
    col:        usize,
    row:        usize,
    cols:       usize,
    rows:       usize,
}

impl TerminalApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, TERM_W, TERM_H, "Terminal");
        let cols = TERM_W as usize / CHAR_W;
        let content_h = (TERM_H - TITLE_H) as usize;
        let rows = content_h / CHAR_H;

        let mut t = TerminalApp {
            window,
            cmd_buf: String::new(),
            pending_key_sink_fd: None,
            col: 0,
            row: 0,
            cols,
            rows,
        };
        for b in t.window.buf.iter_mut() { *b = BG; }
        t.print_str("> ");
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
        match words.next() {
            Some("help") => {
                self.print_str("Commands: help clear reboot echo exec ipc keydemo term info uptime\n");
            }
            Some("clear") => {
                for b in self.window.buf.iter_mut() { *b = BG; }
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
                                self.print_str("Spawned ");
                                self.print_str(path);
                                if !args.is_empty() {
                                    self.print_str(" with args");
                                }
                                self.print_char('\n');
                            }
                            Err(err) => {
                                self.print_str("exec failed: ");
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => self.print_str("usage: exec /bin/hello [args...]\n"),
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
                                self.print_str("Spawned shared pipe demo\n");
                            }
                            (Err(err), _) => {
                                self.print_str("ipc failed: ");
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                            (_, Err(err)) => {
                                self.print_str("ipc failed: ");
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => self.print_str("ipc unavailable: no pipe slots\n"),
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
                                self.print_str("keydemo active; type into the terminal, `~` ends input\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.print_str("keydemo failed: ");
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => self.print_str("keydemo unavailable: no pipe slots\n"),
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
                                self.print_str("userspace terminal started; type commands, Ctrl+D ends\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.print_str("term failed: ");
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => self.print_str("term unavailable: no pipe slots\n"),
                }
            }
            Some("uptime") => {
                self.print_str("Uptime: ");
                self.print_u64(crate::interrupts::ticks());
                self.print_str(" ticks\n");
            }
            Some("info") => {
                self.print_str("Heap: ");
                self.print_u64(crate::allocator::heap_used() as u64);
                self.print_str(" bytes\n");
                let cpuid = raw_cpuid::CpuId::new();
                if let Some(v) = cpuid.get_vendor_info() {
                    self.print_str("CPU: ");
                    self.print_str(v.as_str());
                    self.print_char('\n');
                }
            }
            Some(unknown) => {
                self.print_str("Unknown: ");
                self.print_str(unknown);
                self.print_char('\n');
            }
            None => {}
        }
        self.print_str("> ");
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
        let row_pixels = stride * CHAR_H;
        let total = self.window.buf.len();
        self.window.buf.copy_within(row_pixels..total, 0);
        let last = total - row_pixels;
        for b in self.window.buf[last..].iter_mut() { *b = BG; }
    }

    fn draw_char_at(&mut self, col: usize, row: usize, c: char) {
        let glyph = font8x8::BASIC_FONTS
            .get(c)
            .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
        let px0 = col * CHAR_W;
        let py0 = row * CHAR_H;
        let stride = self.window.width as usize;
        for (gy, &byte) in glyph.iter().enumerate() {
            for bit in 0..8usize {
                let color = if byte & (1 << bit) != 0 { FG } else { BG };
                for sy in 0..FONT_SCALE {
                    for sx in 0..FONT_SCALE {
                        let px = px0 + bit * FONT_SCALE + sx;
                        let py = py0 + gy  * FONT_SCALE + sy;
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
