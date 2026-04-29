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
const LINE_H: usize = 12;
const GLYPH_Y_INSET: usize = 1;
const TERM_PAD_X: usize = 14;
const TERM_PAD_Y: usize = 10;

const TERM_BG_A: u32 = 0x00_03_09_06;
const TERM_BG_B: u32 = 0x00_01_04_02;
const TERM_BG_C: u32 = 0x00_06_0F_09;
const FG_OUTPUT: u32 = 0x00_B8_F3_CE;
const FG_PROMPT: u32 = 0x00_00_FF_88;
const FG_INPUT: u32 = 0x00_E4_FF_F1;
const FG_ACCENT: u32 = 0x00_55_FF_FF;
const FG_DIM: u32 = 0x00_58_8A_70;
const FG_ERROR: u32 = 0x00_FF_72_72;
const FG_DIR: u32 = 0x00_55_DD_FF;
const FG_WARN: u32 = 0x00_FF_CC_44;

const HISTORY_MAX: usize = 32;

pub struct TerminalApp {
    pub window: Window,
    cmd_buf: String,
    pending_key_sink_fd: Option<usize>,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
    fg: u32,
    cwd: String,
    cmd_history: Vec<String>,
    history_pos: usize,
    saved_input: String,
    input_start_col: usize,
    last_width: i32,
    last_height: i32,
}

impl TerminalApp {
    pub fn new(x: i32, y: i32) -> Self {
        let window = Window::new(x, y, TERM_W, TERM_H, "Terminal");
        let cols = text_cols(TERM_W as usize);
        let rows = text_rows((TERM_H - TITLE_H) as usize);

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
            last_width: TERM_W,
            last_height: TERM_H,
        };
        t.fill_background();
        t.set_fg(FG_ACCENT);
        t.print_str("coolOS phosphor shell\n");
        t.set_fg(FG_DIM);
        t.print_str("type help for commands\n\n");
        t.print_prompt();
        t
    }

    pub fn update(&mut self) {
        if self.window.width == self.last_width && self.window.height == self.last_height {
            return;
        }

        let old_width = self.last_width.max(0) as usize;
        let old_content_h = (self.last_height - TITLE_H).max(0) as usize;
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.refresh_layout();
        self.paint_exposed_background(old_width, old_content_h);
    }

    pub fn handle_key(&mut self, c: char) {
        self.refresh_layout();
        match c {
            // Arrow keys (private-use Unicode set by keyboard drivers)
            '\u{F700}' => self.history_up(),
            '\u{F701}' => self.history_down(),
            '\u{F702}' | '\u{F703}' => {} // left/right: ignore (no cursor movement)

            '\n' => {
                self.print_char('\n');
                let cmd = core::mem::take(&mut self.cmd_buf);
                self.push_history(&cmd);
                crate::app_lifecycle::record_command(&cmd);
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
        if cmd.is_empty() {
            return;
        }
        if self.cmd_history.last().map(|s| s.as_str()) == Some(cmd) {
            return;
        }
        if self.cmd_history.len() >= HISTORY_MAX {
            self.cmd_history.remove(0);
        }
        self.cmd_history.push(String::from(cmd));
        self.history_pos = 0;
        self.saved_input.clear();
    }

    fn history_up(&mut self) {
        if self.cmd_history.is_empty() {
            return;
        }
        let new_pos = (self.history_pos + 1).min(self.cmd_history.len());
        if new_pos == self.history_pos {
            return;
        }
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
        if self.history_pos == 0 {
            return;
        }
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
                for word in words {
                    self.print_str(word);
                    self.print_char(' ');
                }
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
                    None => String::from("/"),
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
                    None => self.cwd.clone(),
                };
                self.cmd_ls(&path);
            }

            Some("touch") => match words.next() {
                Some(p) => {
                    let path = resolve_path(&self.cwd, p);
                    self.cmd_touch(&path);
                }
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("usage: touch <path>\n");
                }
            },

            Some("mkdir") => match words.next() {
                Some(p) => {
                    let path = resolve_path(&self.cwd, p);
                    self.cmd_mkdir(&path);
                }
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("usage: mkdir <path>\n");
                }
            },

            Some("cat") => match words.next() {
                Some(p) => {
                    let path = resolve_path(&self.cwd, p);
                    self.cmd_cat(&path);
                }
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("usage: cat <path>\n");
                }
            },

            Some("ps") => self.cmd_ps(),

            Some("kill") => match words.next().and_then(parse_usize) {
                Some(pid) => self.cmd_kill(pid),
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("usage: kill <pid>\n");
                }
            },

            Some("wait") => match words.next().and_then(parse_usize) {
                Some(pid) => self.cmd_wait(pid),
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("usage: wait <pid>\n");
                }
            },

            Some("reap") => self.cmd_reap(),

            Some("info") => self.cmd_info(),

            Some("uptime") => self.cmd_uptime(),

            Some("devices") => self.cmd_devices(),

            Some("net") => self.cmd_lines("NETWORK", crate::net::status_lines()),

            Some("netproto") => self.cmd_lines("NETWORK PROTOCOLS", crate::net::protocol_lines()),

            Some("http") => {
                let host = words.next();
                let path = words.next().unwrap_or("/");
                match host {
                    Some(host) => self.cmd_http(host, path),
                    None => {
                        self.set_fg(FG_ERROR);
                        self.print_str("usage: http <host> [path]\n");
                    }
                }
            }

            Some("power") => self.cmd_power(words.next()),

            Some("log") => self.cmd_log(),

            Some("fsck") => self.cmd_fsck(),

            Some("fsrepair") => self.cmd_lines("FS REPAIR", crate::fs_hardening::repair()),

            Some("mounts") => self.cmd_lines("MOUNTS", crate::fs_hardening::status_lines()),

            Some("journal") => self.cmd_lines("FS JOURNAL", crate::fs_hardening::journal_lines()),

            Some("df") => self.cmd_df(),

            Some("shortcuts") => self.cmd_lines("SHORTCUTS", crate::shortcuts::summary_lines()),

            Some("access") => self.cmd_access(words.next(), words.next()),

            Some("apps") => self.cmd_lines("APP LIFECYCLE", crate::app_lifecycle::lines()),

            Some("pinned") => self.cmd_pinned(words.collect()),

            Some("recent") => self.cmd_recent(),

            Some("startup") => self.cmd_startup(words.collect()),

            Some("search") => {
                let query = collect_words(words);
                if query.is_empty() {
                    self.cmd_lines("SEARCH INDEX", crate::search_index::lines(None));
                } else {
                    self.cmd_lines("SEARCH", crate::search_index::lines(Some(&query)));
                }
            }

            Some("index") => {
                crate::search_index::refresh();
                self.cmd_lines("SEARCH INDEX", crate::search_index::lines(None));
            }

            Some("drivers") => {
                crate::drivers::refresh();
                self.cmd_lines("DRIVERS", crate::drivers::lines());
            }

            Some("users") => self.cmd_lines("USERS", crate::security::lines()),

            Some("security") => self.cmd_lines("SECURITY", crate::security::lines()),

            Some("pkg") => self.cmd_pkg(words.next(), words.next()),

            Some("proc") => self.cmd_lines("PROCESS MODEL", crate::process_model::status_lines()),

            Some("zombies") => {
                self.cmd_lines("ZOMBIE POLICY", crate::process_model::zombie_policy_lines())
            }

            Some("signal") => self.cmd_signal(words.next(), words.next()),

            Some("pgroup") => self.cmd_pgroup(words.next(), words.next()),

            Some("events") => self.cmd_lines("EVENTS", crate::event_bus::lines(12)),

            Some("services") => self.cmd_services(words.next(), words.next()),

            Some("crash") => self.cmd_lines("CRASH DUMP", crate::crashdump::lines()),

            Some("notify") => self.cmd_notify(words.next(), words.next()),

            Some("clip") => {
                let mut text = String::new();
                for word in words {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(word);
                }
                if text.is_empty() {
                    self.set_fg(FG_ACCENT);
                    self.print_str("clipboard: ");
                    self.set_fg(FG_OUTPUT);
                    self.print_str(&crate::clipboard::summary());
                    self.print_char('\n');
                } else {
                    crate::clipboard::set_text(&text);
                    self.set_fg(FG_ACCENT);
                    self.print_str("copied text\n");
                }
            }

            Some("paste") => match crate::clipboard::get_text() {
                Some(text) => {
                    self.set_fg(FG_OUTPUT);
                    self.print_str(&text);
                    self.print_char('\n');
                }
                None => {
                    self.set_fg(FG_ERROR);
                    self.print_str("paste: clipboard has no text\n");
                }
            },

            Some("exec") => match words.next() {
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
            },

            Some("ipc") => match crate::vfs::vfs_pipe() {
                Some((read_fd, write_fd)) => {
                    let r =
                        crate::elf::spawn_elf_process_with_fds("/bin/piperd", &[], &[(read_fd, 3)]);
                    let w = crate::elf::spawn_elf_process_with_fds(
                        "/bin/pipewr",
                        &[],
                        &[(write_fd, 3)],
                    );
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
            },

            Some("keydemo") => match crate::vfs::vfs_pipe() {
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
            },

            Some("term") => match crate::vfs::vfs_pipe() {
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
            },

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
            ("help", "list commands"),
            ("clear", "clear terminal"),
            ("reboot", "restart OS"),
            ("pwd", "print working directory"),
            ("cd <dir>", "change directory"),
            ("ls [path]", "list directory contents"),
            ("touch <path>", "create empty file"),
            ("mkdir <path>", "create folder"),
            ("cat <path>", "print file to terminal"),
            ("ps", "list running processes"),
            ("kill <pid>", "terminate a task"),
            ("wait <pid>", "reap an exited child"),
            ("reap", "reap all exited tasks"),
            ("exec <path>", "run ELF binary"),
            ("info", "CPU, memory, and system info"),
            ("uptime", "time since boot"),
            ("usb", "USB controller status"),
            ("devices", "PCI/USB/device registry"),
            ("drivers", "driver binding + /DEV nodes"),
            ("net", "network stack status"),
            ("netproto", "ARP/IP/UDP/DNS/HTTP status"),
            ("http <host> [path]", "build basic HTTP request"),
            ("power <op>", "ACPI power status"),
            ("log", "kernel log tail"),
            ("fsck", "filesystem check summary"),
            ("fsrepair", "repair standard FS dirs"),
            ("mounts", "mount/cache/journal status"),
            ("journal", "filesystem journal tail"),
            ("df", "filesystem free space"),
            ("shortcuts", "configured shortcut keys"),
            ("access [key on/off]", "accessibility settings"),
            ("apps", "app lifecycle metadata"),
            ("pinned [apps...]", "view/set pinned apps"),
            ("recent", "recent files and commands"),
            ("startup [apps...]", "view/set startup apps"),
            ("search <query>", "search indexed files"),
            ("index", "rebuild desktop search index"),
            ("users", "user/security status"),
            ("pkg <op>", "package list/install/remove"),
            ("proc", "process groups and signals"),
            ("zombies", "zombie cleanup policy"),
            ("signal <pid> <sig>", "queue signal to task"),
            ("pgroup <pid> <grp>", "set process group"),
            ("events", "event bus tail"),
            ("services <op>", "service supervisor"),
            ("crash", "crash dump summary"),
            ("notify <op>", "notification history/actions"),
            ("clip [text]", "shared clipboard"),
            ("paste", "paste shared clipboard text"),
            ("echo <text>", "print text"),
            ("ipc", "pipe demo"),
            ("keydemo", "keyboard event stream"),
            ("term", "userspace terminal"),
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
                    if a.is_dir == b.is_dir {
                        a.name.cmp(&b.name)
                    } else if a.is_dir {
                        core::cmp::Ordering::Less
                    } else {
                        core::cmp::Ordering::Greater
                    }
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
            Some(bytes) => match core::str::from_utf8(&bytes) {
                Ok(text) => {
                    self.set_fg(FG_OUTPUT);
                    self.print_str(text);
                    if !text.ends_with('\n') {
                        self.print_char('\n');
                    }
                }
                Err(_) => {
                    self.set_fg(FG_WARN);
                    self.print_str("cat: binary file (");
                    self.print_u64(bytes.len() as u64);
                    self.print_str(" bytes)\n");
                }
            },
            None => {
                self.set_fg(FG_ERROR);
                self.print_str("cat: file not found\n");
            }
        }
    }

    fn cmd_touch(&mut self, path: &str) {
        if !crate::security::can_write_path(path) {
            self.set_fg(FG_ERROR);
            self.print_str("touch: permission denied\n");
            return;
        }
        match crate::fat32::create_file(path) {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("created ");
                self.set_fg(FG_OUTPUT);
                self.print_str(path);
                self.print_char('\n');
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("touch: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_mkdir(&mut self, path: &str) {
        if !crate::security::can_write_path(path) {
            self.set_fg(FG_ERROR);
            self.print_str("mkdir: permission denied\n");
            return;
        }
        match crate::fat32::create_dir(path) {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("created ");
                self.set_fg(FG_DIR);
                self.print_str(path);
                self.print_char('\n');
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("mkdir: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_ps(&mut self) {
        // Copy task info while holding the lock, then drop it before printing.
        let tasks: Vec<(
            usize,
            &'static str,
            crate::scheduler::TaskStatus,
            bool,
            bool,
            Option<u64>,
            Option<usize>,
        )> = {
            let sched = crate::scheduler::SCHEDULER.lock();
            let cur = sched.current;
            sched
                .tasks
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    (
                        i,
                        t.name,
                        t.status,
                        i == cur,
                        t.pml4.is_some(),
                        t.exit_code,
                        t.parent,
                    )
                })
                .collect()
        };

        self.set_fg(FG_ACCENT);
        self.print_str("PID  PPID  RING  STATUS   EXIT  NAME\n");
        self.set_fg(FG_DIM);
        self.print_str("---  ----  ----  -------  ----  ----\n");
        for (id, name, status, is_cur, is_user, exit_code, parent) in tasks {
            self.set_fg(if is_cur { FG_PROMPT } else { FG_OUTPUT });
            self.print_u64(id as u64);
            self.print_str("    ");
            self.set_fg(FG_DIM);
            if let Some(parent) = parent {
                self.print_u64(parent as u64);
            } else {
                self.print_char('-');
            }
            self.print_str("     ");
            self.set_fg(FG_DIM);
            let ring = if is_user { "u" } else { "k" };
            self.print_str(ring);
            self.print_str("     ");
            self.set_fg(FG_OUTPUT);
            let status_str = match status {
                crate::scheduler::TaskStatus::Running => "running",
                crate::scheduler::TaskStatus::Ready => "ready  ",
                crate::scheduler::TaskStatus::Blocked => "blocked",
                crate::scheduler::TaskStatus::Exited => "exited ",
                crate::scheduler::TaskStatus::Reaped => "reaped ",
            };
            self.print_str(status_str);
            self.print_str("  ");
            self.set_fg(FG_DIM);
            if let Some(code) = exit_code {
                self.print_u64(code);
            } else {
                self.print_char('-');
            }
            self.print_str("     ");
            if is_cur {
                self.set_fg(FG_PROMPT);
            } else if status == crate::scheduler::TaskStatus::Exited {
                self.set_fg(FG_DIM);
            } else {
                self.set_fg(FG_OUTPUT);
            }
            self.print_str(name);
            self.print_char('\n');
        }
    }

    fn cmd_kill(&mut self, pid: usize) {
        match crate::scheduler::kill_task(pid, 130) {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("killed task ");
                self.set_fg(FG_OUTPUT);
                self.print_u64(pid as u64);
                self.print_char('\n');
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("kill: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_wait(&mut self, pid: usize) {
        match crate::scheduler::waitpid(0, pid) {
            Ok(code) => {
                self.set_fg(FG_ACCENT);
                self.print_str("reaped ");
                self.set_fg(FG_OUTPUT);
                self.print_u64(pid as u64);
                self.print_str(" exit ");
                self.print_u64(code);
                self.print_char('\n');
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("wait: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_reap(&mut self) {
        let count = crate::scheduler::reap_all_exited(0);
        self.set_fg(FG_ACCENT);
        self.print_str("reaped ");
        self.set_fg(FG_OUTPUT);
        self.print_u64(count as u64);
        self.print_str(" task(s)\n");
    }

    fn cmd_devices(&mut self) {
        crate::device_registry::refresh_pci();
        self.cmd_lines("DEVICES", crate::device_registry::lines());
    }

    fn cmd_lines(&mut self, title: &str, lines: Vec<String>) {
        self.set_fg(FG_ACCENT);
        self.print_str(title);
        self.print_char('\n');
        if lines.is_empty() {
            self.set_fg(FG_DIM);
            self.print_str("(none)\n");
            return;
        }
        for line in lines {
            self.set_fg(FG_OUTPUT);
            self.print_str(&line);
            self.print_char('\n');
        }
    }

    fn cmd_http(&mut self, host: &str, path: &str) {
        match crate::net::http_get(host, path) {
            Ok(request) => {
                self.set_fg(FG_ACCENT);
                self.print_str("HTTP REQUEST\n");
                self.set_fg(FG_OUTPUT);
                self.print_str(&request);
                self.print_char('\n');
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("http: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err);
                self.print_char('\n');
            }
        }
    }

    fn cmd_access(&mut self, key: Option<&str>, value: Option<&str>) {
        match (key, value.and_then(parse_bool_word)) {
            (Some(key), Some(value)) => {
                if crate::accessibility::set(key, value) {
                    self.set_fg(FG_ACCENT);
                    self.print_str("updated accessibility setting\n");
                } else {
                    self.set_fg(FG_ERROR);
                    self.print_str("access: unknown key\n");
                }
            }
            (None, _) => self.cmd_lines("ACCESSIBILITY", crate::accessibility::lines()),
            _ => {
                self.set_fg(FG_ERROR);
                self.print_str(
                    "usage: access <keyboard_nav|focus_rings|large_text|reduced_motion> <on|off>\n",
                );
            }
        }
    }

    fn cmd_recent(&mut self) {
        let mut lines = Vec::new();
        lines.push(String::from("files:"));
        lines.extend(crate::app_lifecycle::recent_files());
        lines.push(String::from("commands:"));
        lines.extend(crate::app_lifecycle::recent_commands());
        self.cmd_lines("RECENT", lines);
    }

    fn cmd_pinned(&mut self, apps: Vec<&str>) {
        if apps.is_empty() {
            self.cmd_lines("PINNED APPS", crate::app_lifecycle::pinned_apps());
            return;
        }
        crate::app_lifecycle::set_pinned(apps.iter().map(|app| String::from(*app)).collect());
        self.set_fg(FG_ACCENT);
        self.print_str("pinned apps updated\n");
    }

    fn cmd_startup(&mut self, apps: Vec<&str>) {
        if apps.is_empty() {
            self.cmd_lines("STARTUP APPS", crate::app_lifecycle::startup_apps());
            return;
        }
        crate::app_lifecycle::set_startup(apps.iter().map(|app| String::from(*app)).collect());
        self.set_fg(FG_ACCENT);
        self.print_str("startup apps updated\n");
    }

    fn cmd_pkg(&mut self, op: Option<&str>, arg: Option<&str>) {
        match (op, arg) {
            (None, _) | (Some("list"), _) => self.cmd_lines("PACKAGES", crate::packages::lines()),
            (Some("install"), Some(id)) => self.print_result("pkg", crate::packages::install(id)),
            (Some("remove"), Some(id)) | (Some("uninstall"), Some(id)) => {
                self.print_result("pkg", crate::packages::uninstall(id))
            }
            _ => {
                self.set_fg(FG_ERROR);
                self.print_str("usage: pkg [list|install <id>|remove <id>]\n");
            }
        }
    }

    fn cmd_signal(&mut self, pid: Option<&str>, signal: Option<&str>) {
        let Some(pid) = pid.and_then(parse_usize) else {
            self.set_fg(FG_ERROR);
            self.print_str("usage: signal <pid> <term|int|usr1>\n");
            return;
        };
        let Some(signal) = signal.and_then(crate::process_model::Signal::parse) else {
            self.set_fg(FG_ERROR);
            self.print_str("usage: signal <pid> <term|int|usr1>\n");
            return;
        };
        match crate::scheduler::send_signal(pid, signal) {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("signal queued\n");
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("signal: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_pgroup(&mut self, pid: Option<&str>, group: Option<&str>) {
        let Some(pid) = pid.and_then(parse_usize) else {
            self.set_fg(FG_ERROR);
            self.print_str("usage: pgroup <pid> <group>\n");
            return;
        };
        let Some(group) = group.and_then(parse_usize) else {
            self.set_fg(FG_ERROR);
            self.print_str("usage: pgroup <pid> <group>\n");
            return;
        };
        match crate::scheduler::set_process_group(pid, group) {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("process group updated\n");
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("pgroup: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err.as_str());
                self.print_char('\n');
            }
        }
    }

    fn cmd_services(&mut self, op: Option<&str>, name: Option<&str>) {
        match (op, name) {
            (None, _) | (Some("list"), _) => self.cmd_lines("SERVICES", crate::services::lines()),
            (Some("start"), Some(name)) => self.print_bool("service", crate::services::start(name)),
            (Some("stop"), Some(name)) => self.print_bool("service", crate::services::stop(name)),
            (Some("fail"), Some(name)) => self.print_bool("service", crate::services::fail(name)),
            _ => {
                self.set_fg(FG_ERROR);
                self.print_str("usage: services [list|start <name>|stop <name>|fail <name>]\n");
            }
        }
    }

    fn cmd_notify(&mut self, op: Option<&str>, arg: Option<&str>) {
        match (op, arg) {
            (None, _) | (Some("history"), _) => self.cmd_lines(
                "NOTIFICATION HISTORY",
                crate::notifications::history_lines(),
            ),
            (Some("dismiss"), Some(id)) => {
                let ok = parse_u64(id)
                    .map(crate::notifications::dismiss)
                    .unwrap_or(false);
                self.print_bool("notify", ok);
            }
            (Some("group"), Some(title)) => {
                let count = crate::notifications::dismiss_group(title);
                self.set_fg(FG_ACCENT);
                self.print_str("dismissed ");
                self.print_u64(count as u64);
                self.print_str(" notification(s)\n");
            }
            (Some("clear"), _) => {
                crate::notifications::clear();
                self.set_fg(FG_ACCENT);
                self.print_str("notifications cleared\n");
            }
            _ => {
                self.set_fg(FG_ERROR);
                self.print_str("usage: notify [history|dismiss <id>|group <title>|clear]\n");
            }
        }
    }

    fn print_result(&mut self, prefix: &str, result: Result<(), &'static str>) {
        match result {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str(prefix);
                self.print_str(": ok\n");
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str(prefix);
                self.print_str(": ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err);
                self.print_char('\n');
            }
        }
    }

    fn print_bool(&mut self, prefix: &str, ok: bool) {
        if ok {
            self.set_fg(FG_ACCENT);
            self.print_str(prefix);
            self.print_str(": ok\n");
        } else {
            self.set_fg(FG_ERROR);
            self.print_str(prefix);
            self.print_str(": not found\n");
        }
    }

    fn cmd_power(&mut self, op: Option<&str>) {
        match op {
            Some("reboot") => crate::interrupts::reboot(),
            Some("shutdown") => self.print_power_result(crate::acpi::shutdown()),
            Some("sleep") => self.print_power_result(crate::acpi::sleep()),
            _ => self.cmd_lines("POWER", crate::acpi::status_lines()),
        }
    }

    fn print_power_result(&mut self, result: Result<(), &'static str>) {
        match result {
            Ok(()) => {
                self.set_fg(FG_ACCENT);
                self.print_str("power operation requested\n");
            }
            Err(err) => {
                self.set_fg(FG_ERROR);
                self.print_str("power: ");
                self.set_fg(FG_OUTPUT);
                self.print_str(err);
                self.print_char('\n');
            }
        }
    }

    fn cmd_log(&mut self) {
        let _ = crate::klog::flush_to_disk();
        self.cmd_lines("KERNEL LOG", crate::klog::lines());
    }

    fn cmd_fsck(&mut self) {
        match crate::fat32::check() {
            Some(report) => {
                self.set_fg(if report.ok { FG_ACCENT } else { FG_WARN });
                self.print_str(if report.ok {
                    "filesystem ok\n"
                } else {
                    "filesystem warning\n"
                });
                self.set_fg(FG_OUTPUT);
                self.print_str("root entries ");
                self.print_u64(report.root_entries as u64);
                self.print_str("  clusters ");
                self.print_u64(report.stats.used_clusters as u64);
                self.print_char('/');
                self.print_u64(report.stats.total_clusters as u64);
                self.print_char('\n');
            }
            None => {
                self.set_fg(FG_ERROR);
                self.print_str("fsck: unable to read filesystem\n");
            }
        }
    }

    fn cmd_df(&mut self) {
        match crate::fat32::stats() {
            Some(stats) => {
                let free = stats.free_clusters as usize * stats.bytes_per_cluster as usize;
                let used = stats.used_clusters as usize * stats.bytes_per_cluster as usize;
                let total = stats.total_clusters as usize * stats.bytes_per_cluster as usize;
                self.set_fg(FG_ACCENT);
                self.print_str("Filesystem  Used  Free  Total\n");
                self.set_fg(FG_OUTPUT);
                self.print_str("fat32       ");
                self.print_size(used);
                self.print_str("  ");
                self.print_size(free);
                self.print_str("  ");
                self.print_size(total);
                self.print_char('\n');
            }
            None => {
                self.set_fg(FG_ERROR);
                self.print_str("df: unable to read filesystem\n");
            }
        }
    }

    fn cmd_info(&mut self) {
        let heap_used = crate::allocator::heap_used();
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
        let secs = crate::interrupts::uptime_secs();
        let mins = secs / 60;
        let hours = mins / 60;
        let s = secs % 60;
        let m = mins % 60;

        self.set_fg(FG_ACCENT);
        self.print_str("Up: ");
        self.set_fg(FG_OUTPUT);
        self.print_u64(hours);
        self.print_char(':');
        if m < 10 {
            self.print_char('0');
        }
        self.print_u64(m);
        self.print_char(':');
        if s < 10 {
            self.print_char('0');
        }
        self.print_u64(s);
        self.set_fg(FG_DIM);
        self.print_str("  (");
        self.print_u64(ticks);
        self.print_str(" ticks)\n");
    }

    // ── Rendering helpers ─────────────────────────────────────────────────────

    pub fn print_char(&mut self, c: char) {
        self.refresh_layout();
        if c == '\n' {
            self.col = 0;
            self.advance_row();
            return;
        }
        if self.col >= self.cols {
            self.col = 0;
            self.advance_row();
        }
        self.draw_char_at(self.col, self.row, c);
        self.col += 1;
    }

    pub fn print_str(&mut self, s: &str) {
        for c in s.chars() {
            self.print_char(c);
        }
    }

    fn print_u64(&mut self, mut n: u64) {
        if n == 0 {
            self.print_char('0');
            return;
        }
        let mut buf = [0u8; 20];
        let mut i = 20usize;
        while n > 0 {
            i -= 1;
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        for &b in &buf[i..] {
            self.print_char(b as char);
        }
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
        if self.row >= self.rows {
            self.scroll_up();
            self.row = self.rows - 1;
        }
    }

    fn scroll_up(&mut self) {
        let stride = self.window.width as usize;
        let text_x = TERM_PAD_X;
        let text_y = TERM_PAD_Y;
        let text_w = self.cols * CHAR_W;
        let text_h = self.rows * LINE_H;

        if text_w == 0 || text_h <= LINE_H {
            return;
        }

        for y in 0..(text_h - LINE_H) {
            let dst_row = text_y + y;
            let src_row = dst_row + LINE_H;
            let dst = dst_row * stride + text_x;
            let src = src_row * stride + text_x;
            self.window.buf.copy_within(src..src + text_w, dst);
        }

        for y in (text_h - LINE_H)..text_h {
            let py = text_y + y;
            let row_start = py * stride + text_x;
            for x in 0..text_w {
                self.window.buf[row_start + x] = Self::bg_at(py);
            }
        }
    }

    fn draw_char_at(&mut self, col: usize, row: usize, c: char) {
        let glyph = font8x8::BASIC_FONTS
            .get(c)
            .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
        let px0 = TERM_PAD_X + col * CHAR_W;
        let py0 = TERM_PAD_Y + row * LINE_H + GLYPH_Y_INSET;
        let stride = self.window.width as usize;
        for (gy, &byte) in glyph.iter().take(CHAR_H).enumerate() {
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
        self.input_start_col = self.col;
    }

    fn fill_background(&mut self) {
        let stride = self.window.width as usize;
        for (idx, pixel) in self.window.buf.iter_mut().enumerate() {
            let py = idx / stride;
            *pixel = Self::bg_at(py);
        }
    }

    fn paint_exposed_background(&mut self, old_width: usize, old_content_h: usize) {
        let new_width = self.window.width.max(0) as usize;
        let new_content_h = (self.window.height - TITLE_H).max(0) as usize;
        let shared_h = old_content_h.min(new_content_h);

        if new_width > old_width {
            let fill_w = new_width - old_width;
            for py in 0..shared_h {
                let row_start = py * new_width + old_width;
                let row_end = row_start + fill_w;
                for idx in row_start..row_end {
                    self.window.buf[idx] = Self::bg_at(py);
                }
            }
        }

        if new_content_h > old_content_h {
            for py in old_content_h..new_content_h {
                let row_start = py * new_width;
                for idx in row_start..row_start + new_width {
                    self.window.buf[idx] = Self::bg_at(py);
                }
            }
        }
    }

    fn refresh_layout(&mut self) {
        self.cols = text_cols(self.window.width as usize);
        self.rows = text_rows((self.window.height - TITLE_H).max(0) as usize);
        self.col = self.col.min(self.cols.saturating_sub(1));
        self.row = self.row.min(self.rows.saturating_sub(1));
        self.input_start_col = self.input_start_col.min(self.cols.saturating_sub(1));
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

fn text_cols(width: usize) -> usize {
    (width.saturating_sub(TERM_PAD_X * 2) / CHAR_W).max(1)
}

fn text_rows(content_h: usize) -> usize {
    (content_h.saturating_sub(TERM_PAD_Y * 2) / LINE_H).max(1)
}

fn resolve_path(cwd: &str, input: &str) -> String {
    if input.starts_with('/') {
        normalize_path(input)
    } else {
        let mut base = String::from(cwd);
        if !base.ends_with('/') {
            base.push('/');
        }
        base.push_str(input);
        normalize_path(&base)
    }
}

fn parse_usize(input: &str) -> Option<usize> {
    if input.is_empty() {
        return None;
    }
    let mut out = 0usize;
    for b in input.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        out = out.checked_mul(10)?.checked_add((b - b'0') as usize)?;
    }
    Some(out)
}

fn parse_u64(input: &str) -> Option<u64> {
    if input.is_empty() {
        return None;
    }
    let mut out = 0u64;
    for b in input.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        out = out.checked_mul(10)?.checked_add((b - b'0') as u64)?;
    }
    Some(out)
}

fn parse_bool_word(input: &str) -> Option<bool> {
    match input {
        "on" | "1" | "true" | "yes" => Some(true),
        "off" | "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn collect_words<'a, I>(words: I) -> String
where
    I: Iterator<Item = &'a str>,
{
    let mut out = String::new();
    for word in words {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/').filter(|s| !s.is_empty()) {
        match component {
            ".." => {
                parts.pop();
            }
            "." => {}
            seg => parts.push(seg),
        }
    }
    if parts.is_empty() {
        return String::from("/");
    }
    let mut result = String::from("/");
    for (i, &part) in parts.iter().enumerate() {
        if i > 0 {
            result.push('/');
        }
        result.push_str(part);
    }
    result
}
