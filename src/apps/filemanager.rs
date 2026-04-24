use font8x8::UnicodeFonts;

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String as String2;

use crate::fat32::DirEntryInfo;
use crate::framebuffer::{BLACK, DARK_GRAY, GRAY, LIGHT_CYAN, LIGHT_GRAY, SELECTED_BG, WHITE};
use crate::wm::window::{Window, TITLE_H};

pub const FILEMAN_W: i32 = 640;
pub const FILEMAN_H: i32 = 440;

const CW: usize = 8;
const TOOLBAR_H: i32 = 24;
const COL_HDR_H: i32 = 16;
const NAME_COL_W: i32 = 260;
const SIZE_COL_W: i32 = 80;
const ROW_H: i32 = 16;

const COL_SIZE: usize = NAME_COL_W as usize;
const COL_TYPE: usize = NAME_COL_W as usize + SIZE_COL_W as usize;

#[derive(Clone, Copy, PartialEq)]
enum EntryType {
    Folder,
    File,
    Unknown,
}

pub struct FileManagerApp {
    pub window: Window,
    entries: Vec<DirEntryInfo>,
    path: String2,
    offset: usize,
    view_h: i32,
    selected: Option<usize>,
    total_rows: usize,
    pending_open: Option<String2>,
}

impl FileManagerApp {
    pub fn new(x: i32, y: i32) -> Self {
        let mut app = FileManagerApp {
            window: Window::new(x, y, FILEMAN_W, FILEMAN_H, "File Manager"),
            entries: Vec::new(),
            path: String2::from("/"),
            offset: 0,
            view_h: 0,
            selected: None,
            total_rows: 0,
            pending_open: None,
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
            _ => {}
        }
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        let toolbar_bottom = TOOLBAR_H + COL_HDR_H;
        if ly < toolbar_bottom {
            if ly < TOOLBAR_H {
                if lx >= 4 && lx < 4 + 16 && ly >= 4 && ly < 4 + 16 {
                    if self.path.len() > 1 {
                        let parent = self.parent_path();
                        self.load_dir(&parent);
                    }
                    return;
                }
                if lx >= 20 && lx < 20 + (FILEMAN_W - 24) {
                    let rel_x = (lx - 20) as usize;
                    let path_chars = (FILEMAN_W as usize - 24) / CW;
                    let clicked_char = rel_x / CW;
                    let path_start = self.path.len().saturating_sub(path_chars);
                    let clicked = if path_start + clicked_char < self.path.len() {
                        Some(path_start + clicked_char)
                    } else {
                        None
                    };
                    if let Some(pos) = clicked {
                        self.navigate_to_pos(pos);
                    }
                }
            }
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
        if ly <= toolbar_bottom {
            return;
        }
        let content_y = ly - toolbar_bottom;
        let clicked_row = content_y as usize / ROW_H as usize;
        let entry_idx = self.offset + clicked_row;
        if entry_idx < self.entries.len() {
            let abs = self.make_abs(entry_idx);
            if self.is_dir_idx(entry_idx) {
                self.load_dir(&abs);
            } else {
                self.pending_open = Some(abs);
            }
        }
    }

    pub fn take_open_request(&mut self) -> Option<String2> {
        self.pending_open.take()
    }

    pub fn handle_scroll(&mut self, delta: i32) {
        let max_offset = self.entries.len().saturating_sub(self.total_rows);
        let new_offset = (self.offset as i32 + delta.signum() * 3).clamp(0, max_offset as i32) as usize;
        if new_offset != self.offset {
            self.offset = new_offset;
            self.render();
        }
    }

    pub fn update(&mut self) {
        let expected = self.offset as i32 * ROW_H;
        if self.window.scroll.offset != expected {
            let content_area_h = (self.window.height - TITLE_H) as i32;
            let entries_area_h = (content_area_h - TOOLBAR_H - COL_HDR_H).max(0);
            let visible_rows = (entries_area_h / ROW_H) as usize;
            let max_row = self.entries.len().saturating_sub(visible_rows);
            self.offset = ((self.window.scroll.offset / ROW_H) as usize).min(max_row);
            self.render();
        }
    }

    fn parent_path(&self) -> String2 {
        if self.path == "/" {
            return String2::from("/");
        }
        let mut components: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();
        if !components.is_empty() {
            components.pop();
        }
        if components.is_empty() {
            String2::from("/")
        } else {
            String2::from("/") + &components.join("/")
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
            let mut normalized = String2::from(target);
            while normalized.len() > 1 && normalized.ends_with('/') {
                normalized.pop();
            }
            self.load_dir(&normalized);
        }
    }

    fn make_abs(&self, idx: usize) -> String2 {
        let mut s = String2::from(&self.path);
        if !s.ends_with('/') && self.path != "/" {
            s.push('/');
        }
        s.push_str(&self.entries[idx].name);
        s
    }

    fn is_dir_idx(&self, idx: usize) -> bool {
        self.entries.get(idx).map(|e| e.is_dir).unwrap_or(false)
    }

    fn entry_type(idx: usize, entries: &[DirEntryInfo]) -> EntryType {
        match entries.get(idx) {
            Some(e) if e.is_dir => EntryType::Folder,
            Some(_) => EntryType::File,
            None => EntryType::Unknown,
        }
    }

    fn format_size(size: u32) -> alloc::string::String {
        if size == 0 {
            return alloc::string::String::from("0 B");
        }
        let kb = size as u64 / 1024;
        if kb == 0 {
            let mut s = alloc::string::String::new();
            let mut n = size as u64;
            if n == 0 {
                s.push('0');
            } else {
                let mut digits = [0u8; 12];
                let mut len = 0usize;
                while n > 0 {
                    digits[len] = b'0' + (n % 10) as u8;
                    n /= 10;
                    len += 1;
                }
                for i in (0..len).rev() {
                    s.push(digits[i] as char);
                }
            }
            s.push_str(" B");
            s
        } else {
            let mut s = alloc::string::String::new();
            let mut n = kb;
            if n == 0 {
                s.push('0');
            } else {
                let mut digits = [0u8; 12];
                let mut len = 0usize;
                while n > 0 {
                    digits[len] = b'0' + (n % 10) as u8;
                    n /= 10;
                    len += 1;
                }
                for i in (0..len).rev() {
                    s.push(digits[i] as char);
                }
            }
            s.push_str(" KB");
            s
        }
    }

    fn render(&mut self) {
        let w = FILEMAN_W as usize;
        let h = (FILEMAN_H - TITLE_H) as usize;

        for p in self.window.buf.iter_mut() {
            *p = BLACK;
        }

        let stride = w;
        self.view_h = h as i32 - TOOLBAR_H - COL_HDR_H;
        self.total_rows = (self.view_h as usize) / ROW_H as usize;

        // Sync scroll state so the compositor draws the scrollbar in the right position.
        self.window.scroll.content_h = self.entries.len() as i32 * ROW_H + TOOLBAR_H + COL_HDR_H;
        self.window.scroll.offset = self.offset as i32 * ROW_H;
        self.window.scroll.clamp(FILEMAN_H - TITLE_H);

        self.draw_toolbar(stride);
        self.draw_column_header(stride);
        self.draw_entries(stride);
    }

    fn draw_toolbar(&mut self, stride: usize) {
        let bg = 0x00_00_00_40u32;
        for y in 0..TOOLBAR_H as usize {
            for px in 0..FILEMAN_W as usize {
                let idx = y * stride + px;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = bg;
                }
            }
        }

        self.draw_up_button(4, 4);

        let path_str = self.path.clone();
        self.draw_address_bar(&path_str, 20, 0, stride);
    }

    fn draw_up_button(&mut self, px: usize, py: usize) {
        for gy in 0..8 {
            for gx in 0..8 {
                let arrow_y = py + gy;
                let arrow_x = px + gx;
                let on_arrow = if arrow_y == py {
                    gx == 3 || gx == 4
                } else if arrow_y == py + 1 {
                    gx == 2 || gx == 3 || gx == 4 || gx == 5
                } else if arrow_y == py + 2 {
                    gx == 1 || gx == 2 || gx == 3 || gx == 4 || gx == 5 || gx == 6
                } else if arrow_y == py + 3 {
                    gx <= 7
                } else {
                    false
                };
                let color = if on_arrow { LIGHT_GRAY } else { BLACK };
                let idx = arrow_y * (FILEMAN_W as usize) + arrow_x;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = color;
                }
            }
        }
    }

    fn draw_address_bar(&mut self, path_str: &str, x0: usize, y0: usize, _stride: usize) {
        let avail = (FILEMAN_W as usize - 24) - x0;
        let display = if path_str.len() * CW > avail {
            let start = path_str.len() - avail / CW;
            &path_str[start..]
        } else {
            path_str
        };
        self.put_str(x0, y0 + 4, display, WHITE);
    }

    fn draw_column_header(&mut self, stride: usize) {
        let y = TOOLBAR_H as usize;
        for px in 0..FILEMAN_W as usize {
            let idx = y * stride + px;
            if idx < self.window.buf.len() {
                self.window.buf[idx] = DARK_GRAY;
            }
        }
        self.put_str(4, y + 3, "Name", GRAY);
        self.put_str(COL_SIZE, y + 3, "Size", GRAY);
        self.put_str(COL_TYPE, y + 3, "Type", GRAY);
    }

    fn draw_entries(&mut self, stride: usize) {
        let y0 = (TOOLBAR_H + COL_HDR_H) as usize;
        let entries_copy: Vec<DirEntryInfo> = self.entries.clone();

        for (row, entry) in entries_copy.iter().enumerate() {
            if row < self.offset {
                continue;
            }
            let visual_row = row - self.offset;
            if visual_row >= self.total_rows {
                break;
            }
            let py = y0 + visual_row * ROW_H as usize;
            let entry_idx_val = row;

            let is_selected = self.selected == Some(entry_idx_val);
            if is_selected {
                for px in 0..FILEMAN_W as usize {
                    let idx = py * stride + px;
                    if idx < self.window.buf.len() {
                        self.window.buf[idx] = SELECTED_BG;
                    }
                }
            }

            let et = Self::entry_type(entry_idx_val, &entries_copy);
            match et {
                EntryType::Folder => {
                    self.put_str(4, py + 3, &entry.name, LIGHT_CYAN);
                    self.put_str(COL_SIZE, py + 3, "", GRAY);
                    self.put_str(COL_TYPE, py + 3, "Folder", GRAY);
                }
                EntryType::File => {
                    self.put_str(4, py + 3, &entry.name, LIGHT_GRAY);
                    let size_str = Self::format_size(entry.size);
                    self.put_str(COL_SIZE, py + 3, &size_str, GRAY);
                    let ext = Self::file_ext(&entry.name);
                    self.put_str(COL_TYPE, py + 3, &ext, GRAY);
                }
                EntryType::Unknown => {}
            }
        }

        let separator_y = y0 + self.total_rows * ROW_H as usize;
        for px in 0..FILEMAN_W as usize {
            let idx = separator_y * stride + px;
            if idx < self.window.buf.len() {
                self.window.buf[idx] = DARK_GRAY;
            }
        }
    }

    fn file_ext(name: &str) -> alloc::string::String {
        match name.rfind('.') {
            Some(pos) if pos < name.len() - 1 => {
                let ext = &name[pos + 1..];
                alloc::string::String::from(ext)
            }
            _ => alloc::string::String::from("File"),
        }
    }

    fn put_str(&mut self, px: usize, py: usize, s: &str, color: u32) {
        let stride = FILEMAN_W as usize;
        let max_chars = (FILEMAN_W as usize - px) / CW;
        for (ci, ch) in s.chars().take(max_chars).enumerate() {
            let glyph = font8x8::BASIC_FONTS
                .get(ch)
                .unwrap_or_else(|| font8x8::BASIC_FONTS.get(' ').unwrap());
            for (gi, &byte) in glyph.iter().enumerate() {
                for bit in 0..8 {
                    let color = if byte & (1 << bit) != 0 { color } else { BLACK };
                    let px = px + ci * CW + bit;
                    let py = py + gi;
                    let idx = py * stride + px;
                    if idx < self.window.buf.len() {
                        self.window.buf[idx] = color;
                    }
                }
            }
        }
    }
}
