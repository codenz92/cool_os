/// Terminal app — renders a shell into a window's pixel back-buffer.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use font8x8::UnicodeFonts;

use crate::wm::window::{Window, TITLE_H};

pub const TERM_W: i32 = 640;
pub const TERM_H: i32 = 440;

const CHAR_W: usize = 8;
const CHAR_H: usize = 8;

const TERM_BG_A: u32 = 0x00_03_09_06;
const TERM_BG_B: u32 = 0x00_01_04_02;
const TERM_BG_C: u32 = 0x00_06_0F_09;
const FG_OUTPUT: u32 = 0x00_B8_F3_CE;
const FG_PROMPT: u32 = 0x00_00_FF_88;
const FG_INPUT:  u32 = 0x00_E4_FF_F1;
const FG_ACCENT: u32 = 0x00_55_FF_FF;
const FG_DIM:    u32 = 0x00_58_8A_70;
const FG_ERROR:  u32 = 0x00_FF_72_72;
const FG_DIR:    u32 = 0x00_55_DD_FF;
const FG_WARN:   u32 = 0x00_FF_CC_44;

const HISTORY_MAX: usize = 32;

pub struct TerminalApp {
    pub window: Window,
    cmd_buf:          String,
    pending_key_sink_fd: Option<usize>,
    col:              usize,
    row:              usize,
    cols:             usize,
    rows:             usize,
    fg:               u32,
    cwd:              String,
    cmd_history:      Vec<String>,
    history_pos:      usize,
    saved_input:      String,
    input_start_col:  usize,
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
            fg: FG_OUTPUT,
            cwd: String::from("/"),
            cmd_history: Vec::new(),
            history_pos: 0,
            saved_input: String::new(),
            input_start_col: 0,
        };
        t.fill_background();
        t.set_fg(FG_ACCENT);
        t.print_str("coolOS phosphor shell\n");
        t.set_fg(FG_DIM);
        t.print_str("type help for commands\n\n");
        t.print_prompt();
        t
    }

    pub fn handle_key(&mut self, c: char) {
        match c {
            // Arrow keys (private-use Unicode set by keyboard drivers)
            '\u{F700}' => self.history_up(),
            '\u{F701}' => self.history_down(),
            '\u{F702}' | '\u{F703}' => {} // left/right: ignore (no cursor movement)

            '\n' => {
                self.print_char('\n');
                let cmd = core::mem::take(&mut self.cmd_buf);
                self.push_history(&cmd);
                self.run_command(&cmd);
            }
            '\u{0008}' => {
                if self.cmd_buf.pop().is_some() && self.col > self.input_start_col {
                    self.col -= 1;
                    self.draw_char_at(self.col, self.row, ' ');
                }
            }
            c if !c.is_control() => {
                let max = self.cols.saturating_sub(self.input_start_col + 1);
                if self.cmd_buf.len() < max {
                    self.cmd_buf.push(c);
                    self.print_char(c);
                }
            }
            _ => {}
        }
    }

    pub fn take_pending_key_sink(&mut self) -> Option<usize> {
        self.pending_key_sink_fd.take()
    }

    // ── History ───────────────────────────────────────────────────────────────

    fn push_history(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() { return; }
        if self.cmd_history.last().map(|s| s.as_str()) == Some(cmd) { return; }
        if self.cmd_history.len() >= HISTORY_MAX {
            self.cmd_history.remove(0);
        }
        self.cmd_history.push(String::from(cmd));
        self.history_pos = 0;
        self.saved_input.clear();
    }

    fn history_up(&mut self) {
        if self.cmd_history.is_empty() { return; }
        let new_pos = (self.history_pos + 1).min(self.cmd_history.len());
        if new_pos == self.history_pos { return; }
        if self.history_pos == 0 {
            self.saved_input = self.cmd_buf.clone();
        }
        self.history_pos = new_pos;
        let entry = self.cmd_history[self.cmd_history.len() - self.history_pos].clone();
        self.erase_input();
        self.cmd_buf = entry.clone();
        self.set_fg(FG_INPUT);
        self.print_str(&entry);
    }

    fn history_down(&mut self) {
        if self.history_pos == 0 { return; }
        self.history_pos -= 1;
        self.erase_input();
        if self.history_pos == 0 {
            let saved = self.saved_input.clone();
            self.cmd_buf = saved.clone();
            self.set_fg(FG_INPUT);
            self.print_str(&saved);
        } else {
            let entry = self.cmd_history[self.cmd_history.len() - self.history_pos].clone();
            self.cmd_buf = entry.clone();
            self.set_fg(FG_INPUT);
            self.print_str(&entry);
        }
    }

    fn erase_input(&mut self) {
        while self.col > self.input_start_col {
            self.col -= 1;
            self.draw_char_at(self.col, self.row, ' ');
        }
        self.cmd_buf.clear();
    }

    // ── Command dispatch ──────────────────────────────────────────────────────

    fn run_command(&mut self, input: &str) {
        let mut words = input.split_whitespace();
        self.set_fg(FG_OUTPUT);
        match words.next() {

            Some("help") => self.cmd_help(),

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

            Some("pwd") => {
                self.set_fg(FG_DIR);
                let cwd = self.cwd.clone();
                self.print_str(&cwd);
                self.print_char('\n');
            }

            Some("cd") => {
                let target = match words.next() {
                    Some(p) => resolve_path(&self.cwd, p),
                    None    => String::from("/"),
                };
                if crate::fat32::list_dir(&target).is_some() {
                    self.cwd = target;
                } else {
                    self.set_fg(FG_ERROR);
                    self.print_str("cd: no such directory\n");
                }
            }

            Some("ls") => {
                let path_arg = words.next();
                let path = match path_arg {
                    Some(p) => resolve_path(&self.cwd, p),
                    None    => self.cwd.clone(),
                };
                self.cmd_ls(&path);
            }

            Some("cat") => {
                match words.next() {
                    Some(p) => {
                        let path = resolve_path(&self.cwd, p);
                        self.cmd_cat(&path);
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("usage: cat <path>\n");
                    }
                }
            }

            Some("ps") => self.cmd_ps(),

            Some("info") => self.cmd_info(),

            Some("uptime") => self.cmd_uptime(),

            Some("exec") => {
                match words.next() {
                    Some(path) => {
                        let args: Vec<&str> = words.collect();
                        let abs = resolve_path(&self.cwd, path);
                        match crate::elf::spawn_elf_process_with_args(&abs, &args) {
                            Ok(()) => {
                                self.set_fg(FG_ACCENT);
                                self.print_str("spawned ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(&abs);
                                self.print_char('\n');
                            }
                            Err(err) => {
                                self.set_fg(FG_ERROR);
                                self.print_str("exec: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("usage: exec <path> [args...]\n");
                    }
                }
            }

            Some("ipc") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        let r = crate::elf::spawn_elf_process_with_fds("/bin/piperd", &[], &[(read_fd, 3)]);
                        let w = crate::elf::spawn_elf_process_with_fds("/bin/pipewr", &[], &[(write_fd, 3)]);
                        crate::vfs::vfs_close(read_fd);
                        crate::vfs::vfs_close(write_fd);
                        match (r, w) {
                            (Ok(()), Ok(())) => {
                                self.set_fg(FG_ACCENT);
                                self.print_str("pipe demo spawned\n");
                            }
                            _ => {
                                self.set_fg(FG_ERROR);
                                self.print_str("ipc: spawn failed\n");
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("ipc: no pipe slots\n");
                    }
                }
            }

            Some("keydemo") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        match crate::elf::spawn_elf_process_with_fds("/bin/keyecho", &[], &[(read_fd, 3)]) {
                            Ok(()) => {
                                crate::vfs::vfs_close(read_fd);
                                self.pending_key_sink_fd = Some(write_fd);
                                self.set_fg(FG_ACCENT);
                                self.print_str("keydemo active — ~ ends\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.set_fg(FG_ERROR);
                                self.print_str("keydemo: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("keydemo: no pipe slots\n");
                    }
                }
            }

            Some("term") => {
                match crate::vfs::vfs_pipe() {
                    Some((read_fd, write_fd)) => {
                        match crate::elf::spawn_elf_process_with_stdin("/bin/terminal", &[], read_fd) {
                            Ok(()) => {
                                self.pending_key_sink_fd = Some(write_fd);
                                self.set_fg(FG_ACCENT);
                                self.print_str("userspace terminal — Ctrl+D ends\n");
                            }
                            Err(err) => {
                                crate::vfs::vfs_close(read_fd);
                                crate::vfs::vfs_close(write_fd);
                                self.set_fg(FG_ERROR);
                                self.print_str("term: ");
                                self.set_fg(FG_OUTPUT);
                                self.print_str(err.as_str());
                                self.print_char('\n');
                            }
                        }
                    }
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("term: no pipe slots\n");
                    }
                }
            }

            Some("usb") => {
                let lines = crate::usb::status_lines();
                if lines.is_empty() {
                    self.set_fg(FG_WARN);
                    self.print_str("USB: no probe data\n");
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
                self.print_str(unknown);
                self.set_fg(FG_DIM);
                self.print_str(": not found. ");
                self.set_fg(FG_OUTPUT);
                self.print_str("type help\n");
            }

            None => {}
        }
        self.print_prompt();
    }

    fn cmd_help(&mut self) {
        let cmds: &[(&str, &str)] = &[
            ("help",          "list commands"),
            ("clear",         "clear terminal"),
            ("reboot",        "restart OS"),
            ("pwd",           "print working directory"),
            ("cd <dir>",      "change directory"),
            ("ls [path]",     "list directory contents"),
            ("cat <path>",    "print file to terminal"),
            ("ps",            "list running processes"),
            ("exec <path>",   "run ELF binary"),
            ("info",          "CPU, memory, and system info"),
            ("uptime",        "time since boot"),
            ("usb",           "USB controller status"),
            ("echo <text>",   "print text"),
            ("ipc",           "pipe demo"),
            ("keydemo",       "keyboard event stream"),
            ("term",          "userspace terminal"),
        ];
        self.set_fg(FG_ACCENT);
        self.print_str("Commands:\n");
        for &(name, desc) in cmds {
            self.set_fg(FG_PROMPT);
            self.print_str("  ");
            self.print_str(name);
            // pad to column 18
            let name_len = name.len() + 2;
            for _ in name_len..20 {
                self.print_char(' ');
            }
            self.set_fg(FG_DIM);
            self.print_str(desc);
            self.print_char('\n');
        }
    }

    fn cmd_ls(&mut self, path: &str) {
        match crate::fat32::list_dir(path) {
            Some(mut entries) => {
                entries.sort_by(|a, b| {
                    if a.is_dir == b.is_dir { a.name.cmp(&b.name) }
                    else if a.is_dir { core::cmp::Ordering::Less }
                    else { core::cmp::Ordering::Greater }
                });
                if entries.is_empty() {
                    self.set_fg(FG_DIM);
                    self.print_str("(empty)\n");
                } else {
                    for e in &entries {
                        if e.is_dir {
                            self.set_fg(FG_DIR);
                            self.print_str(&e.name);
                            self.print_char('/');
                        } else {
                            self.set_fg(FG_OUTPUT);
                            self.print_str(&e.name);
                        }
                        self.print_char('\n');
                    }
                }
            }
            None => {
                self.set_fg(FG_ERROR);
                self.print_str("ls: no such directory\n");
            }
        }
    }

    fn cmd_cat(&mut self, path: &str) {
        match crate::fat32::read_file(path) {
            Some(bytes) => {
                match core::str::from_utf8(&bytes) {
                    Ok(text) => {
                        self.set_fg(FG_OUTPUT);
                        self.print_str(text);
                        if !text.ends_with('\n') { self.print_char('\n'); }
                    }
                    Err(_) => {
                        self.set_fg(FG_WARN);
                        self.print_str("cat: binary file (");
                        self.print_u64(bytes.len() as u64);
                        self.print_str(" bytes)\n");
                    }
                }
            }
            None => {
                self.set_fg(FG_ERROR);
                self.print_str("cat: file not found\n");
            }
        }
    }

    fn cmd_ps(&mut self) {
        // Copy task info while holding the lock, then drop it before printing.
        let tasks: Vec<(usize, &'static str, crate::scheduler::TaskStatus, bool)> = {
            let sched = crate::scheduler::SCHEDULER.lock();
            let cur = sched.current;
            sched.tasks.iter().enumerate().map(|(i, t)| {
                (i, t.name, t.status, i == cur)
            }).collect()
        };

        self.set_fg(FG_ACCENT);
        self.print_str("PID  RING  STATUS   NAME\n");
        self.set_fg(FG_DIM);
        self.print_str("---  ----  -------  ----\n");
        for (id, name, status, is_cur) in tasks {
            self.set_fg(if is_cur { FG_PROMPT } else { FG_OUTPUT });
            self.print_u64(id as u64);
            self.print_str("    ");
            self.set_fg(FG_DIM);
            let ring = if id == 0 { "k" } else { "u" };
            self.print_str(ring);
            self.print_str("     ");
            self.set_fg(FG_OUTPUT);
            let status_str = match status {
                crate::scheduler::TaskStatus::Running => "running",
                crate::scheduler::TaskStatus::Ready   => "ready  ",
                crate::scheduler::TaskStatus::Blocked => "blocked",
            };
            self.print_str(status_str);
            self.print_str("  ");
            if is_cur {
                self.set_fg(FG_PROMPT);
            }
            self.print_str(name);
            self.print_char('\n');
        }
    }

    fn cmd_info(&mut self) {
        let heap_used  = crate::allocator::heap_used();
        let heap_total = crate::allocator::HEAP_SIZE;
        let task_count = crate::scheduler::SCHEDULER.lock().tasks.len();

        self.set_fg(FG_ACCENT);
        self.print_str("Heap  : ");
        self.set_fg(FG_OUTPUT);
        self.print_size(heap_used);
        self.set_fg(FG_DIM);
        self.print_str(" / ");
        self.set_fg(FG_OUTPUT);
        self.print_size(heap_total);
        self.print_char('\n');

        self.set_fg(FG_ACCENT);
        self.print_str("Tasks : ");
        self.set_fg(FG_OUTPUT);
        self.print_u64(task_count as u64);
        self.print_char('\n');

        let cpuid = raw_cpuid::CpuId::new();
        if let Some(v) = cpuid.get_vendor_info() {
            self.set_fg(FG_ACCENT);
            self.print_str("CPU   : ");
            self.set_fg(FG_OUTPUT);
            self.print_str(v.as_str());
            self.print_char('\n');
        }
        if let Some(b) = cpuid.get_processor_brand_string() {
            self.set_fg(FG_ACCENT);
            self.print_str("Brand : ");
            self.set_fg(FG_OUTPUT);
            self.print_str(b.as_str().trim());
            self.print_char('\n');
        }

        self.set_fg(FG_ACCENT);
        self.print_str("CWD   : ");
        self.set_fg(FG_DIR);
        let cwd = self.cwd.clone();
        self.print_str(&cwd);
        self.print_char('\n');
    }

    fn cmd_uptime(&mut self) {
        let ticks = crate::interrupts::ticks();
        let secs  = ticks / 100;
        let mins  = secs / 60;
        let hours = mins / 60;
        let s = secs % 60;
        let m = mins % 60;

        self.set_fg(FG_ACCENT);
        self.print_str("Up: ");
        self.set_fg(FG_OUTPUT);
        self.print_u64(hours);
        self.print_char(':');
        if m < 10 { self.print_char('0'); }
        self.print_u64(m);
        self.print_char(':');
        if s < 10 { self.print_char('0'); }
        self.print_u64(s);
        self.set_fg(FG_DIM);
        self.print_str("  (");
        self.print_u64(ticks);
        self.print_str(" ticks)\n");
    }

    // ── Rendering helpers ─────────────────────────────────────────────────────

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

    fn print_size(&mut self, bytes: usize) {
        if bytes >= 1024 * 1024 {
            self.print_u64((bytes / (1024 * 1024)) as u64);
            self.print_str(" MB");
        } else if bytes >= 1024 {
            self.print_u64((bytes / 1024) as u64);
            self.print_str(" KB");
        } else {
            self.print_u64(bytes as u64);
            self.print_str(" B");
        }
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
        for idx in last..total {
            let py = idx / stride;
            self.window.buf[idx] = Self::bg_at(py);
        }
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

    fn set_fg(&mut self, color: u32) { self.fg = color; }

    fn print_prompt(&mut self) {
        self.set_fg(FG_DIM);
        let cwd = self.cwd.clone();
        self.print_str(&cwd);
        self.print_char(' ');
        self.set_fg(FG_PROMPT);
        self.print_str("cool");
        self.set_fg(FG_ACCENT);
        self.print_str("> ");
        self.set_fg(FG_INPUT);
        self.input_start_col = self.col;
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

// ── Path utilities ────────────────────────────────────────────────────────────

fn resolve_path(cwd: &str, input: &str) -> String {
    if input.starts_with('/') {
        normalize_path(input)
    } else {
        let mut base = String::from(cwd);
        if !base.ends_with('/') { base.push('/'); }
        base.push_str(input);
        normalize_path(&base)
    }
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/').filter(|s| !s.is_empty()) {
        match component {
            ".." => { parts.pop(); }
            "."  => {}
            seg  => parts.push(seg),
        }
    }
    if parts.is_empty() {
        return String::from("/");
    }
    let mut result = String::from("/");
    for (i, &part) in parts.iter().enumerate() {
        if i > 0 { result.push('/'); }
        result.push_str(part);
    }
    result
}
