use font8x8::UnicodeFonts;

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fat32::DirEntryInfo;
use crate::framebuffer::{BLACK, DARK_GRAY, LIGHT_CYAN, LIGHT_GRAY, WHITE};
use crate::wm::window::{Window, TITLE_H};

pub const FILEMAN_W: i32 = 640;
pub const FILEMAN_H: i32 = 440;

const CW: usize = 8;
const TOOLBAR_H: i32 = 24;
const COL_HDR_H: i32 = 16;
const STATUS_H: i32 = 18;
const NAME_COL_W: i32 = 288;
const SIZE_COL_W: i32 = 72;
const ROW_H: i32 = 16;

const FM_BG: u32 = 0x00_02_07_12;
const FM_PANEL: u32 = 0x00_00_0A_1C;
const FM_PANEL_ALT: u32 = 0x00_00_0E_24;
const FM_BORDER: u32 = 0x00_00_44_88;
const FM_ACCENT: u32 = 0x00_00_99_FF;
const FM_ROW_ALT: u32 = 0x00_00_0B_18;
const FM_ROW_SEL: u32 = 0x00_00_24_46;
const FM_STATUS: u32 = 0x00_00_08_14;
const FOLDER_ICON: u32 = 0x00_55_DD_FF;
const FILE_ICON: u32 = 0x00_AA_FF_CC;
const TEXT_MUTED: u32 = 0x00_66_AA_DD;
const COL_SEP: u32 = 0x00_00_22_44;

const COL_SIZE: usize = NAME_COL_W as usize + 4;
const COL_TYPE: usize = NAME_COL_W as usize + SIZE_COL_W as usize + 4;

#[derive(Clone, Copy, PartialEq)]
enum EntryType {
    Folder,
    File,
    Unknown,
}

pub struct FileManagerApp {
    pub window: Window,
    entries: Vec<DirEntryInfo>,
    path: String,
    offset: usize,
    view_h: i32,
    selected: Option<usize>,
    total_rows: usize,
    pending_open: Option<String>,
    last_width: i32,
    last_height: i32,
}

impl FileManagerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let mut app = FileManagerApp {
            window: Window::new(x, y, FILEMAN_W, FILEMAN_H, "File Manager"),
            entries: Vec::new(),
            path: String::from("/"),
            offset: 0,
            view_h: 0,
            selected: None,
            total_rows: 0,
            pending_open: None,
            last_width: FILEMAN_W,
            last_height: FILEMAN_H,
        };
        app.load_dir("/");
        app
    }

    pub fn load_dir(&mut self, dir: &str) {
        self.path.clear();
        self.path.push_str(dir);
        let mut new_entries = crate::fat32::list_dir(dir).unwrap_or_default();
        new_entries.sort_by(|a, b| {
            if a.is_dir == b.is_dir {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            } else if a.is_dir {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Greater
            }
        });
        self.entries = new_entries;
        self.offset = 0;
        self.selected = self.entries.first().map(|_| 0);
        self.render();
    }

    pub fn handle_key(&mut self, c: char) {
        match c {
            '\u{0008}' => {
                if self.path.len() > 1 {
                    let parent = self.parent_path();
                    self.load_dir(&parent);
                }
            }
            '\u{F700}' => self.move_selection(-1), // up arrow
            '\u{F701}' => self.move_selection(1),  // down arrow
            '\n' => self.open_selected(),
            _ => {}
        }
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        let toolbar_bottom = TOOLBAR_H + COL_HDR_H;
        let content_bottom = toolbar_bottom + self.view_h;
        if ly < toolbar_bottom {
            if ly < TOOLBAR_H {
                if lx >= 6 && lx < 6 + 18 && ly >= 3 && ly < 3 + 18 {
                    if self.path.len() > 1 {
                        let parent = self.parent_path();
                        self.load_dir(&parent);
                    }
                    return;
                }
                let window_w = self.window.width.max(0);
                if lx >= 32 && lx < 32 + (window_w - 42).max(0) {
                    let rel_x = (lx - 32) as usize;
                    let path_chars = (window_w as usize).saturating_sub(44) / CW;
                    let clicked_char = rel_x / CW;
                    let path_start = self.path.len().saturating_sub(path_chars);
                    if path_start + clicked_char < self.path.len() {
                        self.navigate_to_pos(path_start + clicked_char);
                    }
                }
            }
            return;
        }
        if ly >= content_bottom {
            return;
        }

        let content_y = ly - toolbar_bottom;
        if content_y < 0 {
            return;
        }
        let clicked_row = content_y as usize / ROW_H as usize;
        let entry_idx = self.offset + clicked_row;
        if entry_idx < self.entries.len() {
            self.selected = Some(entry_idx);
            self.render();
        }
    }

    pub fn handle_dbl_click(&mut self, _lx: i32, ly: i32) {
        let toolbar_bottom = TOOLBAR_H + COL_HDR_H;
        if ly <= toolbar_bottom || ly >= toolbar_bottom + self.view_h {
            return;
        }
        let content_y = ly - toolbar_bottom;
        let clicked_row = content_y as usize / ROW_H as usize;
        let entry_idx = self.offset + clicked_row;
        if entry_idx < self.entries.len() {
            self.selected = Some(entry_idx);
            self.open_selected();
        }
    }

    pub fn take_open_request(&mut self) -> Option<String> {
        self.pending_open.take()
    }

    pub fn handle_scroll(&mut self, delta: i32) {
        let max_offset = self.entries.len().saturating_sub(self.total_rows);
        let new_offset =
            (self.offset as i32 + delta.signum() * 3).clamp(0, max_offset as i32) as usize;
        if new_offset != self.offset {
            self.offset = new_offset;
            self.render();
        }
    }

    pub fn update(&mut self) {
        if self.window.width != self.last_width || self.window.height != self.last_height {
            self.last_width = self.window.width;
            self.last_height = self.window.height;
            self.render();
            return;
        }

        let expected = self.offset as i32 * ROW_H;
        if self.window.scroll.offset != expected {
            let content_area_h = (self.window.height - TITLE_H).max(0);
            let entries_area_h = (content_area_h - TOOLBAR_H - COL_HDR_H - STATUS_H).max(0);
            let visible_rows = (entries_area_h / ROW_H) as usize;
            let max_row = self.entries.len().saturating_sub(visible_rows);
            self.offset = ((self.window.scroll.offset / ROW_H) as usize).min(max_row);
            self.render();
        }
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn move_selection(&mut self, delta: i32) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let new_sel = match self.selected {
            Some(s) => (s as i32 + delta).clamp(0, len as i32 - 1) as usize,
            None => {
                if delta > 0 {
                    0
                } else {
                    len - 1
                }
            }
        };
        self.selected = Some(new_sel);
        self.ensure_selected_visible();
        self.render();
    }

    fn open_selected(&mut self) {
        let sel = match self.selected {
            Some(s) => s,
            None => return,
        };
        if sel >= self.entries.len() {
            return;
        }
        let abs = self.make_abs(sel);
        if self.is_dir_idx(sel) {
            self.load_dir(&abs);
        } else {
            self.pending_open = Some(abs);
        }
    }

    fn ensure_selected_visible(&mut self) {
        let sel = match self.selected {
            Some(s) => s,
            None => return,
        };
        if sel < self.offset {
            self.offset = sel;
        } else if self.total_rows > 0 && sel >= self.offset + self.total_rows {
            self.offset = sel.saturating_sub(self.total_rows - 1);
        }
    }

    fn parent_path(&self) -> String {
        if self.path == "/" {
            return String::from("/");
        }
        let mut components: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();
        if !components.is_empty() {
            components.pop();
        }
        if components.is_empty() {
            String::from("/")
        } else {
            let mut s = String::from("/");
            for (i, c) in components.iter().enumerate() {
                if i > 0 {
                    s.push('/');
                }
                s.push_str(c);
            }
            s
        }
    }

    fn navigate_to_pos(&mut self, pos: usize) {
        if self.path == "/" {
            return;
        }
        let bytes = self.path.as_bytes();
        if pos >= bytes.len() {
            return;
        }
        if pos == 0 {
            self.load_dir("/");
            return;
        }
        let mut end = bytes.len();
        for (idx, &b) in bytes.iter().enumerate().skip(pos) {
            if b == b'/' {
                end = idx;
                break;
            }
        }
        if end == 0 {
            self.load_dir("/");
            return;
        }
        let target = &self.path[..end];
        if !target.is_empty() {
            let mut normalized = String::from(target);
            while normalized.len() > 1 && normalized.ends_with('/') {
                normalized.pop();
            }
            self.load_dir(&normalized);
        }
    }

    fn make_abs(&self, idx: usize) -> String {
        let mut s = String::from(&self.path);
        if !s.ends_with('/') {
            s.push('/');
        }
        s.push_str(&self.entries[idx].name);
        s
    }

    fn is_dir_idx(&self, idx: usize) -> bool {
        self.entries.get(idx).map(|e| e.is_dir).unwrap_or(false)
    }

    fn entry_type(entries: &[DirEntryInfo], idx: usize) -> EntryType {
        match entries.get(idx) {
            Some(e) if e.is_dir => EntryType::Folder,
            Some(_) => EntryType::File,
            None => EntryType::Unknown,
        }
    }

    fn format_size(size: u32) -> String {
        fn fmt_u32(n: u32) -> String {
            if n == 0 {
                return String::from("0");
            }
            let mut digits = [0u8; 12];
            let mut len = 0usize;
            let mut v = n;
            while v > 0 {
                digits[len] = b'0' + (v % 10) as u8;
                v /= 10;
                len += 1;
            }
            let mut s = String::new();
            for i in (0..len).rev() {
                s.push(digits[i] as char);
            }
            s
        }
        if size >= 1024 * 1024 {
            let mut s = fmt_u32(size / (1024 * 1024));
            s.push_str(" MB");
            s
        } else if size >= 1024 {
            let mut s = fmt_u32(size / 1024);
            s.push_str(" KB");
            s
        } else {
            let mut s = fmt_u32(size);
            s.push_str(" B");
            s
        }
    }

    fn file_ext(name: &str) -> &str {
        match name.rfind('.') {
            Some(pos) if pos < name.len() - 1 => &name[pos + 1..],
            _ => "",
        }
    }

    fn type_label(name: &str, is_dir: bool) -> &'static str {
        if is_dir {
            return "Folder";
        }
        match Self::file_ext(name) {
            "TXT" | "MD" | "LOG" | "RST" | "CSV" => "Text",
            "RS" => "Rust",
            "C" | "H" => "C Source",
            "CPP" | "HPP" | "CC" => "C++",
            "ELF" | "BIN" => "Binary",
            "JSON" | "TOML" | "YAML" | "YML" => "Config",
            "SH" | "BASH" => "Script",
            "PY" => "Python",
            "JS" | "TS" => "JavaScript",
            "ASM" | "S" => "Assembly",
            "" => "File",
            _ => "File",
        }
    }

    // ── Render ────────────────────────────────────────────────────────────────

    fn render(&mut self) {
        let w = self.window.width as usize;
        let h = (self.window.height - TITLE_H).max(0) as usize;
        let stride = w;

        for p in self.window.buf.iter_mut() {
            *p = FM_BG;
        }

        self.view_h = (h as i32 - TOOLBAR_H - COL_HDR_H - STATUS_H).max(0);
        self.total_rows = (self.view_h as usize) / ROW_H as usize;

        self.window.scroll.content_h = self.entries.len() as i32 * ROW_H;
        self.window.scroll.offset = self.offset as i32 * ROW_H;
        self.window.scroll.clamp(self.view_h);

        self.draw_toolbar(stride);
        self.draw_column_header(stride);
        self.draw_entries(stride);
        self.draw_status_bar(stride);
    }

    fn draw_toolbar(&mut self, stride: usize) {
        let window_w = self.window.width.max(0) as usize;
        let address_w = window_w.saturating_sub(38);
        self.fill_rect(stride, 0, 0, window_w, TOOLBAR_H as usize, FM_PANEL_ALT);
        self.fill_rect(stride, 0, 0, window_w, 2, FM_ACCENT);
        self.fill_rect(stride, 30, 3, address_w, 18, FM_PANEL);
        self.draw_rect_border(stride, 30, 3, address_w, 18, FM_BORDER);
        self.draw_up_button(stride, 6, 3);
        let path_str = self.path.clone();
        self.draw_address_bar(&path_str, 38, 0, stride);
    }

    fn draw_up_button(&mut self, stride: usize, px: usize, py: usize) {
        self.fill_rect(stride, px, py, 18, 18, FM_PANEL);
        self.draw_rect_border(stride, px, py, 18, 18, FM_BORDER);
        // Up arrow glyph
        let arrow: [(usize, usize); 14] = [
            (5, 5),
            (6, 5),
            (4, 6),
            (5, 6),
            (6, 6),
            (7, 6),
            (3, 7),
            (4, 7),
            (5, 7),
            (6, 7),
            (7, 7),
            (8, 7),
            (5, 8),
            (6, 8),
        ];
        for (gx, gy) in arrow {
            let idx = (py + gy) * stride + (px + gx);
            if idx < self.window.buf.len() {
                self.window.buf[idx] = WHITE;
            }
        }
        // Stem
        for gy in 9..13 {
            for gx in 5..7 {
                let idx = (py + gy) * stride + (px + gx);
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = WHITE;
                }
            }
        }
    }

    fn draw_address_bar(&mut self, path_str: &str, x0: usize, y0: usize, _stride: usize) {
        let avail = (self.window.width.max(0) as usize)
            .saturating_sub(44)
            .saturating_sub(x0);
        let display = if path_str.len() * CW > avail {
            let start = path_str.len() - avail / CW;
            &path_str[start..]
        } else {
            path_str
        };
        self.put_str(x0, y0 + 6, display, WHITE);
    }

    fn draw_column_header(&mut self, stride: usize) {
        let window_w = self.window.width.max(0) as usize;
        let y = TOOLBAR_H as usize;
        self.fill_rect(stride, 0, y, window_w, COL_HDR_H as usize, FM_PANEL);
        self.fill_rect(
            stride,
            0,
            y + COL_HDR_H as usize - 1,
            window_w,
            1,
            FM_BORDER,
        );

        self.put_str(8, y + 4, "Name", TEXT_MUTED);
        self.put_str(COL_SIZE, y + 4, "Size", TEXT_MUTED);
        self.put_str(COL_TYPE, y + 4, "Type", TEXT_MUTED);

        // Column separators
        self.fill_rect(
            stride,
            NAME_COL_W as usize,
            y,
            1,
            COL_HDR_H as usize,
            FM_BORDER,
        );
        self.fill_rect(
            stride,
            (NAME_COL_W + SIZE_COL_W) as usize,
            y,
            1,
            COL_HDR_H as usize,
            FM_BORDER,
        );
    }

    fn draw_entries(&mut self, stride: usize) {
        let y0 = (TOOLBAR_H + COL_HDR_H) as usize;
        let window_w = self.window.width.max(0) as usize;
        let entries_copy: Vec<DirEntryInfo> = self.entries.clone();
        let dir_count = entries_copy.iter().filter(|e| e.is_dir).count();
        let _ = dir_count; // used in status bar only

        if entries_copy.is_empty() {
            let msg_y = y0 + self.view_h as usize / 2 - 4;
            self.put_str(
                window_w.saturating_sub(64) / 2,
                msg_y,
                "(empty folder)",
                TEXT_MUTED,
            );
        }

        for (row, entry) in entries_copy.iter().enumerate() {
            if row < self.offset {
                continue;
            }
            let visual_row = row - self.offset;
            if visual_row >= self.total_rows {
                break;
            }

            let py = y0 + visual_row * ROW_H as usize;
            let is_sel = self.selected == Some(row);
            let row_bg = if is_sel {
                FM_ROW_SEL
            } else if visual_row % 2 == 0 {
                FM_BG
            } else {
                FM_ROW_ALT
            };
            self.fill_rect(stride, 0, py, window_w, ROW_H as usize, row_bg);
            if is_sel {
                self.fill_rect(stride, 0, py, 3, ROW_H as usize, FM_ACCENT);
            }

            let et = Self::entry_type(&entries_copy, row);
            self.draw_entry_icon(stride, 8, py + 3, et);

            // Column separator lines
            self.fill_rect(stride, NAME_COL_W as usize, py, 1, ROW_H as usize, COL_SEP);
            self.fill_rect(
                stride,
                (NAME_COL_W + SIZE_COL_W) as usize,
                py,
                1,
                ROW_H as usize,
                COL_SEP,
            );

            match et {
                EntryType::Folder => {
                    self.put_str(
                        24,
                        py + 4,
                        &entry.name,
                        if is_sel { WHITE } else { LIGHT_CYAN },
                    );
                    self.put_str(
                        COL_TYPE,
                        py + 4,
                        "Folder",
                        if is_sel { WHITE } else { TEXT_MUTED },
                    );
                }
                EntryType::File => {
                    self.put_str(
                        24,
                        py + 4,
                        &entry.name,
                        if is_sel { WHITE } else { LIGHT_GRAY },
                    );
                    let size_str = Self::format_size(entry.size);
                    self.put_str(
                        COL_SIZE,
                        py + 4,
                        &size_str,
                        if is_sel { WHITE } else { TEXT_MUTED },
                    );
                    let label = Self::type_label(&entry.name, false);
                    self.put_str(
                        COL_TYPE,
                        py + 4,
                        label,
                        if is_sel { WHITE } else { TEXT_MUTED },
                    );
                }
                EntryType::Unknown => {}
            }

            // Row divider
            self.fill_rect(
                stride,
                8,
                py + ROW_H as usize - 1,
                window_w.saturating_sub(16),
                1,
                DARK_GRAY,
            );
        }
    }

    fn draw_status_bar(&mut self, stride: usize) {
        let window_w = self.window.width.max(0) as usize;
        let y = (TOOLBAR_H + COL_HDR_H + self.view_h) as usize;
        self.fill_rect(stride, 0, y, window_w, STATUS_H as usize, FM_STATUS);
        self.fill_rect(stride, 0, y, window_w, 1, FM_BORDER);

        // Left: item counts
        let n_dirs = self.entries.iter().filter(|e| e.is_dir).count();
        let n_files = self.entries.len() - n_dirs;
        let mut left = String::new();
        fmt_push_u(&mut left, n_dirs as u64);
        left.push_str(" folders  ");
        fmt_push_u(&mut left, n_files as u64);
        left.push_str(" files");
        self.put_str(8, y + 5, &left, TEXT_MUTED);

        // Right: selected item info
        if let Some(idx) = self.selected {
            if let Some(e) = self.entries.get(idx) {
                let mut right = String::new();
                right.push_str(&e.name);
                if !e.is_dir {
                    right.push_str("  ");
                    let sz = Self::format_size(e.size);
                    right.push_str(&sz);
                }
                let right_x = window_w.saturating_sub(right.len() * CW + 10);
                self.put_str(right_x, y + 5, &right, LIGHT_GRAY);
            }
        }

        // Keyboard hints (centre)
        let hint = "^/v select  Enter open  BS up";
        let hint_x = window_w.saturating_sub(hint.len() * CW) / 2;
        self.put_str(hint_x, y + 5, hint, FM_BORDER);
    }

    fn draw_entry_icon(&mut self, stride: usize, x: usize, y: usize, et: EntryType) {
        match et {
            EntryType::Folder => {
                self.fill_rect(stride, x + 1, y, 6, 2, FOLDER_ICON);
                self.fill_rect(stride, x, y + 2, 10, 6, FOLDER_ICON);
                self.fill_rect(stride, x + 1, y + 3, 8, 4, blend(FOLDER_ICON, BLACK, 140));
            }
            EntryType::File => {
                self.fill_rect(stride, x + 1, y, 8, 10, FILE_ICON);
                self.draw_rect_border(stride, x + 1, y, 8, 10, blend(FILE_ICON, BLACK, 120));
                self.fill_rect(stride, x + 5, y, 4, 3, WHITE);
            }
            EntryType::Unknown => {}
        }
    }

    fn put_str(&mut self, px: usize, py: usize, s: &str, color: u32) {
        let stride = self.window.width as usize;
        let max_chars = stride.saturating_sub(px) / CW;
        for (ci, ch) in s.chars().take(max_chars).enumerate() {
            let glyph = font8x8::BASIC_FONTS
                .get(ch)
                .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
            for (gi, &byte) in glyph.iter().enumerate() {
                for bit in 0..8 {
                    if byte & (1 << bit) == 0 {
                        continue;
                    }
                    let x = px + ci * CW + bit;
                    let y = py + gi;
                    let idx = y * stride + x;
                    if idx < self.window.buf.len() {
                        self.window.buf[idx] = color;
                    }
                }
            }
        }
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
}

fn blend(a: u32, b: u32, t: u32) -> u32 {
    let lerp = |ca: u32, cb: u32| -> u32 {
        if cb >= ca {
            (ca + (cb - ca) * t / 255).min(255)
        } else {
            ca - (ca - cb) * t / 255
        }
    };
    let r = lerp((a >> 16) & 0xFF, (b >> 16) & 0xFF);
    let g = lerp((a >> 8) & 0xFF, (b >> 8) & 0xFF);
    let bl = lerp(a & 0xFF, b & 0xFF);
    (r << 16) | (g << 8) | bl
}

fn fmt_push_u(s: &mut String, mut n: u64) {
    if n == 0 {
        s.push('0');
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
        s.push(b as char);
    }
}
