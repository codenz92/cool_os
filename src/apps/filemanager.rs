use font8x8::UnicodeFonts;

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fat32::DirEntryInfo;
use crate::framebuffer::{BLACK, WHITE};
use crate::wm::window::{Window, TITLE_H};

pub const FILEMAN_W: i32 = 760;
pub const FILEMAN_H: i32 = 500;

const CW: usize = 8;
const COMMAND_H: i32 = 28;
const PATHBAR_H: i32 = 30;
const STATUS_H: i32 = 20;
const SIDEBAR_W: i32 = 176;
const SECTION_HDR_H: i32 = 18;
const TILE_H: i32 = 58;
const TILE_GAP_X: i32 = 12;
const TILE_GAP_Y: i32 = 10;
const DRIVE_H: i32 = 52;
const DRIVE_GAP_Y: i32 = 10;
const LIST_ROW_H: i32 = 18;
const NAV_ROW_H: i32 = 18;
const NAV_BTN_W: i32 = 26;
const ACTION_BTN_W: i32 = 74;

const FM_BG_TOP: u32 = 0x00_06_0C_18;
const FM_BG_BOT: u32 = 0x00_03_07_12;
const FM_SHELL: u32 = 0x00_08_11_20;
const FM_PANEL: u32 = 0x00_0D_18_2A;
const FM_PANEL_ALT: u32 = 0x00_12_1E_33;
const FM_PANEL_SOFT: u32 = 0x00_10_1A_2B;
const FM_BORDER: u32 = 0x00_26_4A_72;
const FM_BORDER_SOFT: u32 = 0x00_1B_33_50;
const FM_ACCENT: u32 = 0x00_44_C8_F5;
const FM_ACCENT_SOFT: u32 = 0x00_1D_73_A2;
const FM_SELECTION: u32 = 0x00_17_3A_58;
const FM_SELECTION_GLOW: u32 = 0x00_27_9B_CB;
const FM_TEXT: u32 = 0x00_E7_F6_FF;
const FM_TEXT_DIM: u32 = 0x00_99_BF_DA;
const FM_TEXT_MUTED: u32 = 0x00_6F_91_AE;
const FM_FOLDER: u32 = 0x00_54_B9_FF;
const FM_FOLDER_SHADE: u32 = 0x00_2D_73_B0;
const FM_FILE: u32 = 0x00_8F_E4_D0;
const FM_DRIVE: u32 = 0x00_C9_EE_FD;
const FM_DRIVE_FILL: u32 = 0x00_3C_DA_F8;
const FM_SEARCH: u32 = 0x00_0B_14_24;

#[derive(Clone, Copy, PartialEq)]
enum EntryType {
    Folder,
    File,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Name,
    Size,
    Type,
}

impl SortColumn {
    fn label(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Size => "size",
            SortColumn::Type => "type",
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    width: i32,
    height: i32,
    sidebar_w: i32,
    main_x: i32,
    main_w: i32,
    status_y: i32,
}

#[derive(Clone, Copy)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Rect {
    fn hit(self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
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
    status_note: Option<String>,
    last_width: i32,
    last_height: i32,
    sort_column: SortColumn,
    sort_desc: bool,
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
            status_note: None,
            last_width: FILEMAN_W,
            last_height: FILEMAN_H,
            sort_column: SortColumn::Name,
            sort_desc: false,
        };
        app.load_dir("/");
        app
    }

    pub fn load_dir(&mut self, dir: &str) {
        self.load_dir_with_state(dir, None, None);
    }

    fn load_dir_with_state(
        &mut self,
        dir: &str,
        selected_name: Option<&str>,
        preferred_offset: Option<usize>,
    ) {
        self.path.clear();
        self.path.push_str(dir);
        let mut new_entries = crate::fat32::list_dir(dir).unwrap_or_default();
        Self::sort_entries(&mut new_entries, self.sort_column, self.sort_desc);
        self.entries = new_entries;
        self.selected = selected_name.and_then(|name| {
            self.entries
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(name))
        });
        if self.selected.is_none() {
            self.selected = self.entries.first().map(|_| 0);
        }
        let visible_rows = self.visible_row_capacity();
        let max_offset = self.entries.len().saturating_sub(visible_rows.max(1));
        self.offset = preferred_offset.unwrap_or(0).min(max_offset);
        self.ensure_selected_visible();
        self.status_note = None;
        self.render();
    }

    pub fn handle_key(&mut self, c: char) {
        match c {
            '\u{0008}' => self.navigate_up(),
            '\u{F700}' => self.move_selection(-1),
            '\u{F701}' => self.move_selection(1),
            'n' | 'N' => self.create_new_file(),
            'd' | 'D' => self.create_new_dir(),
            'h' | 'H' => self.navigate_home(),
            'r' | 'R' => self.refresh_current_dir(),
            's' | 'S' => self.cycle_sort(),
            '\n' => self.open_selected(),
            _ => {}
        }
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        let layout = self.layout();

        if let Some(path) = self.hit_navigation(lx, ly) {
            self.load_dir(&path);
            return;
        }

        if ly < COMMAND_H {
            if (Rect {
                x: 10,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            })
            .hit(lx, ly)
            {
                self.navigate_up();
                return;
            }
            if (Rect {
                x: 42,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            })
            .hit(lx, ly)
            {
                self.navigate_home();
                return;
            }
            if (Rect {
                x: 74,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            })
            .hit(lx, ly)
            {
                self.refresh_current_dir();
                return;
            }

            let new_file_rect = Rect {
                x: layout.width - ACTION_BTN_W * 2 - 24,
                y: 4,
                w: ACTION_BTN_W,
                h: 20,
            };
            let new_dir_rect = Rect {
                x: layout.width - ACTION_BTN_W - 12,
                y: 4,
                w: ACTION_BTN_W,
                h: 20,
            };
            if new_file_rect.hit(lx, ly) {
                self.create_new_file();
                return;
            }
            if new_dir_rect.hit(lx, ly) {
                self.create_new_dir();
                return;
            }
        }

        if ly >= COMMAND_H && ly < COMMAND_H + PATHBAR_H {
            let crumb_rect = self.breadcrumb_rect();
            if crumb_rect.hit(lx, ly) {
                let rel_x = (lx - crumb_rect.x).max(0) as usize;
                let clicked_char = rel_x / CW;
                let path_chars = (crumb_rect.w.max(0) as usize) / CW;
                let path_start = self.path.len().saturating_sub(path_chars);
                if path_start + clicked_char < self.path.len() {
                    self.navigate_to_pos(path_start + clicked_char);
                }
            }
            return;
        }

        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.selected = Some(idx);
            self.render();
        }
    }

    pub fn handle_dbl_click(&mut self, lx: i32, ly: i32) {
        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.selected = Some(idx);
            self.open_selected();
        }
    }

    pub fn take_open_request(&mut self) -> Option<String> {
        self.pending_open.take()
    }

    pub fn handle_scroll(&mut self, delta: i32) {
        if self.path == "/" {
            return;
        }
        let max_offset = self.entries.len().saturating_sub(self.total_rows.max(1));
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

        if self.path != "/" {
            let expected = self.offset as i32 * LIST_ROW_H;
            if self.window.scroll.offset != expected {
                let visible_rows = self.visible_row_capacity();
                let max_row = self.entries.len().saturating_sub(visible_rows.max(1));
                self.offset = ((self.window.scroll.offset / LIST_ROW_H) as usize).min(max_row);
                self.render();
            }
        }
    }

    pub fn refresh_current_dir(&mut self) {
        let path = self.path.clone();
        let selected = self.selected_name();
        let offset = self.offset;
        self.load_dir_with_state(&path, selected.as_deref(), Some(offset));
        self.status_note = Some(String::from("refreshed"));
        self.render();
    }

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
        if self.path == "/" {
            return;
        }
        let sel = match self.selected {
            Some(s) => s,
            None => return,
        };
        let visible_rows = self.visible_row_capacity().max(1);
        if sel < self.offset {
            self.offset = sel;
        } else if sel >= self.offset + visible_rows {
            self.offset = sel.saturating_sub(visible_rows - 1);
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

    fn selected_name(&self) -> Option<String> {
        self.selected
            .and_then(|idx| self.entries.get(idx))
            .map(|entry| entry.name.clone())
    }

    fn visible_row_capacity(&self) -> usize {
        let content_area_h = (self.window.height - TITLE_H).max(0);
        let entries_area_h =
            (content_area_h - COMMAND_H - PATHBAR_H - STATUS_H - SECTION_HDR_H - 52).max(0);
        (entries_area_h / LIST_ROW_H) as usize
    }

    fn sort_entries(entries: &mut [DirEntryInfo], sort_column: SortColumn, sort_desc: bool) {
        entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                return if a.is_dir {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Greater
                };
            }

            let base = match sort_column {
                SortColumn::Name => a
                    .name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase()),
                SortColumn::Size => a.size.cmp(&b.size).then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                }),
                SortColumn::Type => Self::type_label(&a.name, a.is_dir)
                    .cmp(Self::type_label(&b.name, b.is_dir))
                    .then_with(|| {
                        a.name
                            .to_ascii_lowercase()
                            .cmp(&b.name.to_ascii_lowercase())
                    }),
            };

            if sort_desc {
                base.reverse()
            } else {
                base
            }
        });
    }

    fn resort_entries(&mut self, note: &str) {
        let selected = self.selected_name();
        Self::sort_entries(&mut self.entries, self.sort_column, self.sort_desc);
        self.selected = selected.and_then(|name| {
            self.entries
                .iter()
                .position(|entry| entry.name.eq_ignore_ascii_case(&name))
        });
        if self.selected.is_none() {
            self.selected = self.entries.first().map(|_| 0);
        }
        self.ensure_selected_visible();
        self.status_note = Some(String::from(note));
        self.render();
    }

    fn change_sort(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort_column = column;
            self.sort_desc = false;
        }
        let mut note = String::from("sort ");
        note.push_str(column.label());
        note.push(' ');
        note.push_str(if self.sort_desc { "desc" } else { "asc" });
        self.resort_entries(&note);
    }

    fn cycle_sort(&mut self) {
        self.sort_column = match self.sort_column {
            SortColumn::Name => SortColumn::Size,
            SortColumn::Size => SortColumn::Type,
            SortColumn::Type => SortColumn::Name,
        };
        self.sort_desc = false;
        let mut note = String::from("sort ");
        note.push_str(self.sort_column.label());
        self.resort_entries(&note);
    }

    fn navigate_up(&mut self) {
        if self.path.len() > 1 {
            let parent = self.parent_path();
            self.load_dir(&parent);
            self.status_note = Some(String::from("up one folder"));
            self.render();
        }
    }

    fn navigate_home(&mut self) {
        self.load_dir("/");
        self.status_note = Some(String::from("home"));
        self.render();
    }

    fn render(&mut self) {
        let layout = self.layout();
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.view_h = (layout.height - COMMAND_H - PATHBAR_H - STATUS_H).max(0);

        self.fill_background();
        self.draw_command_bar(layout);
        self.draw_path_bar(layout);
        self.draw_sidebar(layout);
        self.draw_main_shell(layout);
        if self.path == "/" {
            self.window.scroll.content_h = 0;
            self.window.scroll.offset = 0;
            self.draw_root_overview(layout);
        } else {
            self.draw_directory_view(layout);
        }
        self.draw_status_bar(layout);
    }

    fn layout(&self) -> Layout {
        let width = self.window.width.max(0);
        let height = (self.window.height - TITLE_H).max(0);
        let sidebar_w = SIDEBAR_W.min(width / 3).max(140);
        let main_x = sidebar_w + 1;
        let main_w = (width - main_x).max(0);
        let status_y = (height - STATUS_H).max(0);
        Layout {
            width,
            height,
            sidebar_w,
            main_x,
            main_w,
            status_y,
        }
    }

    fn draw_command_bar(&mut self, layout: Layout) {
        self.fill_rect(0, 0, layout.width, COMMAND_H, FM_SHELL);
        self.fill_rect(0, COMMAND_H - 1, layout.width, 1, FM_BORDER_SOFT);

        self.draw_command_button(
            Rect {
                x: 10,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            },
            "<",
        );
        self.draw_command_button(
            Rect {
                x: 42,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            },
            "^",
        );
        self.draw_command_button(
            Rect {
                x: 74,
                y: 4,
                w: NAV_BTN_W,
                h: 20,
            },
            "R",
        );

        self.draw_action_button(
            Rect {
                x: layout.width - ACTION_BTN_W * 2 - 24,
                y: 4,
                w: ACTION_BTN_W,
                h: 20,
            },
            "NEW FILE",
        );
        self.draw_action_button(
            Rect {
                x: layout.width - ACTION_BTN_W - 12,
                y: 4,
                w: ACTION_BTN_W,
                h: 20,
            },
            "NEW FOLDER",
        );
    }

    fn draw_path_bar(&mut self, layout: Layout) {
        let crumb = self.breadcrumb_rect();
        let search = Rect {
            x: layout.width - 170,
            y: COMMAND_H + 4,
            w: 156,
            h: 22,
        };

        self.fill_rect(0, COMMAND_H, layout.width, PATHBAR_H, FM_PANEL_ALT);
        self.fill_rect(
            0,
            COMMAND_H + PATHBAR_H - 1,
            layout.width,
            1,
            FM_BORDER_SOFT,
        );

        self.fill_rect(crumb.x, crumb.y, crumb.w, crumb.h, FM_PANEL);
        self.draw_rect_border(crumb.x, crumb.y, crumb.w, crumb.h, FM_BORDER);
        self.put_str(
            (crumb.x + 10) as usize,
            (crumb.y + 7) as usize,
            &Self::clip_text(
                &self.breadcrumb_text(),
                (crumb.w as usize).saturating_sub(20) / CW,
            ),
            FM_TEXT,
        );

        self.fill_rect(search.x, search.y, search.w, search.h, FM_SEARCH);
        self.draw_rect_border(search.x, search.y, search.w, search.h, FM_BORDER_SOFT);
        self.put_str(
            (search.x + 10) as usize,
            (search.y + 7) as usize,
            "search",
            FM_TEXT_MUTED,
        );
    }

    fn breadcrumb_rect(&self) -> Rect {
        let layout = self.layout();
        Rect {
            x: 116,
            y: COMMAND_H + 4,
            w: (layout.width - 294).max(140),
            h: 22,
        }
    }

    fn draw_sidebar(&mut self, layout: Layout) {
        self.fill_rect(
            0,
            COMMAND_H + PATHBAR_H,
            layout.sidebar_w,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_PANEL_SOFT,
        );
        self.fill_rect(
            layout.sidebar_w,
            COMMAND_H + PATHBAR_H,
            1,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_BORDER_SOFT,
        );

        let mut y = COMMAND_H + PATHBAR_H + 14;
        self.put_str(18, y as usize, "QUICK ACCESS", FM_TEXT_MUTED);
        y += 16;

        for (label, path, active) in self.sidebar_items() {
            self.draw_sidebar_item(
                Rect {
                    x: 10,
                    y,
                    w: layout.sidebar_w - 20,
                    h: NAV_ROW_H,
                },
                &label,
                path.is_some(),
                active,
            );
            y += NAV_ROW_H + 4;
        }
    }

    fn sidebar_items(&self) -> Vec<(String, Option<String>, bool)> {
        let mut items = Vec::new();
        items.push((
            String::from("Home"),
            Some(String::from("/")),
            self.path == "/",
        ));
        items.push((
            String::from("This PC"),
            Some(String::from("/")),
            self.path == "/",
        ));

        for name in self.root_directory_names().into_iter().take(7) {
            let mut path = String::from("/");
            path.push_str(&name);
            let active = self.path.eq_ignore_ascii_case(&path);
            items.push((String::new(), None, false));
            items.push((name, Some(path), active));
        }

        items
    }

    fn draw_sidebar_item(&mut self, rect: Rect, label: &str, clickable: bool, active: bool) {
        if !clickable || label.is_empty() {
            return;
        }
        if active {
            self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_SELECTION);
            self.fill_rect(rect.x, rect.y, 3, rect.h, FM_SELECTION_GLOW);
        }
        self.put_str(
            (rect.x + 10) as usize,
            (rect.y + 5) as usize,
            label,
            if active { FM_TEXT } else { FM_TEXT_DIM },
        );
    }

    fn draw_main_shell(&mut self, layout: Layout) {
        self.fill_rect(
            layout.main_x,
            COMMAND_H + PATHBAR_H,
            layout.main_w,
            layout.status_y - COMMAND_H - PATHBAR_H,
            FM_BG_BOT,
        );
    }

    fn draw_root_overview(&mut self, layout: Layout) {
        let top = COMMAND_H + PATHBAR_H + 14;
        self.put_str(
            (layout.main_x + 18) as usize,
            top as usize,
            "This PC",
            FM_TEXT,
        );
        self.put_str(
            (layout.main_x + 18) as usize,
            (top + 14) as usize,
            "coolOS shell view",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        let section_y = top + 34;
        let tiles_h = self.draw_folder_section(layout, section_y, &folders, true);
        let drives_y = section_y + tiles_h + 20;
        self.draw_drive_section(
            layout,
            drives_y,
            if files.is_empty() { &folders } else { &files },
        );
    }

    fn draw_directory_view(&mut self, layout: Layout) {
        let top = COMMAND_H + PATHBAR_H + 14;
        let title = if self.path == "/" {
            "This PC"
        } else {
            self.path.as_str()
        };
        self.put_str(
            (layout.main_x + 18) as usize,
            top as usize,
            &Self::clip_text(title, 34),
            FM_TEXT,
        );
        self.put_str(
            (layout.main_x + 18) as usize,
            (top + 14) as usize,
            "folders first, files below",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        let section_y = top + 34;
        let folders_h = self.draw_folder_section(layout, section_y, &folders, false);
        let files_y = section_y + folders_h + 18;
        self.draw_file_list_section(layout, files_y, &files);
    }

    fn draw_folder_section(
        &mut self,
        layout: Layout,
        y: i32,
        indices: &[usize],
        root_mode: bool,
    ) -> i32 {
        let title = if root_mode { "Folders" } else { "Subfolders" };
        let count = indices.len();
        let mut label = String::from(title);
        label.push(' ');
        label.push('(');
        fmt_push_u(&mut label, count as u64);
        label.push(')');

        self.put_str(
            (layout.main_x + 18) as usize,
            y as usize,
            &label,
            FM_TEXT_DIM,
        );
        self.fill_rect(
            layout.main_x + 18,
            y + SECTION_HDR_H - 2,
            layout.main_w - 36,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (layout.main_x + 28) as usize,
                (y + 24) as usize,
                "(no folders)",
                FM_TEXT_MUTED,
            );
            return 40;
        }

        let tile_y = y + 22;
        let tile_w = ((layout.main_w - 60 - TILE_GAP_X * 2) / 3).max(140);
        let cols = ((layout.main_w - 36) / (tile_w + TILE_GAP_X)).max(1) as usize;

        for (visual_idx, &entry_idx) in indices.iter().take(9).enumerate() {
            let col = (visual_idx % cols).min(2);
            let row = visual_idx / cols;
            let rect = Rect {
                x: layout.main_x + 18 + col as i32 * (tile_w + TILE_GAP_X),
                y: tile_y + row as i32 * (TILE_H + TILE_GAP_Y),
                w: tile_w,
                h: TILE_H,
            };
            self.draw_folder_tile(rect, entry_idx);
        }

        let rows = ((indices.len().min(9) + cols - 1) / cols).max(1) as i32;
        22 + rows * TILE_H + (rows - 1) * TILE_GAP_Y
    }

    fn draw_drive_section(&mut self, layout: Layout, y: i32, indices: &[usize]) {
        self.put_str(
            (layout.main_x + 18) as usize,
            y as usize,
            "Devices and drives",
            FM_TEXT_DIM,
        );
        self.fill_rect(
            layout.main_x + 18,
            y + SECTION_HDR_H - 2,
            layout.main_w - 36,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (layout.main_x + 28) as usize,
                (y + 24) as usize,
                "(no items)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let card_w = ((layout.main_w - 54) / 2).max(180);
        let cols = if layout.main_w > 420 { 2 } else { 1 };
        let max_items = if cols == 2 { 4 } else { 3 };
        for (visual_idx, &entry_idx) in indices.iter().take(max_items).enumerate() {
            let col = (visual_idx % cols as usize) as i32;
            let row = (visual_idx / cols as usize) as i32;
            let rect = Rect {
                x: layout.main_x + 18 + col * (card_w + 12),
                y: y + 22 + row * (DRIVE_H + DRIVE_GAP_Y),
                w: card_w,
                h: DRIVE_H,
            };
            self.draw_drive_card(rect, entry_idx);
        }
    }

    fn draw_file_list_section(&mut self, layout: Layout, y: i32, file_indices: &[usize]) {
        self.put_str(
            (layout.main_x + 18) as usize,
            y as usize,
            "Files",
            FM_TEXT_DIM,
        );
        self.fill_rect(
            layout.main_x + 18,
            y + SECTION_HDR_H - 2,
            layout.main_w - 36,
            1,
            FM_BORDER_SOFT,
        );

        let list_y = y + 22;
        let list_h = (layout.status_y - list_y - 10).max(0);
        self.view_h = list_h;
        self.total_rows = (list_h / LIST_ROW_H).max(0) as usize;
        self.window.scroll.content_h = file_indices.len() as i32 * LIST_ROW_H;
        self.window.scroll.offset = self.offset as i32 * LIST_ROW_H;
        self.window.scroll.clamp(list_h);

        if file_indices.is_empty() {
            self.put_str(
                (layout.main_x + 28) as usize,
                (list_y + 8) as usize,
                "(no files)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let visible = self.total_rows.max(1);
        let max_offset = file_indices.len().saturating_sub(visible);
        self.offset = self.offset.min(max_offset);
        let name_w = (layout.main_w - 250).max(120) as usize / CW;

        for visual_row in 0..visible {
            let idx_in_files = self.offset + visual_row;
            if idx_in_files >= file_indices.len() {
                break;
            }
            let entry_idx = file_indices[idx_in_files];
            let (name, size) = match self.entries.get(entry_idx) {
                Some(entry) => (entry.name.clone(), entry.size),
                None => continue,
            };
            let row_y = list_y + visual_row as i32 * LIST_ROW_H;
            let selected = self.selected == Some(entry_idx);
            self.fill_rect(
                layout.main_x + 18,
                row_y,
                layout.main_w - 36,
                LIST_ROW_H,
                if selected {
                    FM_SELECTION
                } else if visual_row % 2 == 0 {
                    FM_PANEL_SOFT
                } else {
                    FM_BG_BOT
                },
            );
            if selected {
                self.fill_rect(layout.main_x + 18, row_y, 3, LIST_ROW_H, FM_SELECTION_GLOW);
            }
            self.draw_file_icon((layout.main_x + 28) as usize, (row_y + 4) as usize);

            let name = Self::clip_text(&name, name_w);
            self.put_str(
                (layout.main_x + 46) as usize,
                (row_y + 5) as usize,
                &name,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.put_str(
                (layout.main_x + layout.main_w - 180) as usize,
                (row_y + 5) as usize,
                Self::type_label(&name, false),
                FM_TEXT_MUTED,
            );
            let size = Self::format_size(size);
            self.put_str(
                (layout.main_x + layout.main_w - 84) as usize,
                (row_y + 5) as usize,
                &size,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.fill_rect(
                layout.main_x + 18,
                row_y + LIST_ROW_H - 1,
                layout.main_w - 36,
                1,
                FM_BORDER_SOFT,
            );
        }
    }

    fn draw_status_bar(&mut self, layout: Layout) {
        self.fill_rect(0, layout.status_y, layout.width, STATUS_H, FM_SHELL);
        self.fill_rect(0, layout.status_y, layout.width, 1, FM_BORDER_SOFT);

        let folders = self.entries.iter().filter(|e| e.is_dir).count();
        let files = self.entries.len().saturating_sub(folders);
        let mut left = String::new();
        fmt_push_u(&mut left, folders as u64);
        left.push_str(" folders  ");
        fmt_push_u(&mut left, files as u64);
        left.push_str(" files");
        self.put_str(10, (layout.status_y + 6) as usize, &left, FM_TEXT_MUTED);

        let hint = self.status_note.clone().unwrap_or_else(|| {
            String::from("Enter open  Backspace up  H home  R refresh  S sort  N file  D folder")
        });
        let hint_x = ((layout.width as usize).saturating_sub(hint.len() * CW)) / 2;
        self.put_str(hint_x, (layout.status_y + 6) as usize, &hint, FM_TEXT_MUTED);

        if let Some(idx) = self.selected {
            if let Some(entry) = self.entries.get(idx) {
                let entry_name = entry.name.clone();
                let entry_is_dir = entry.is_dir;
                let entry_size = entry.size;
                let mut right = String::from(&entry_name);
                right.push_str("  ");
                right.push_str(Self::type_label(&entry_name, entry_is_dir));
                if !entry_is_dir {
                    right.push_str("  ");
                    right.push_str(&Self::format_size(entry_size));
                }
                let right_x = (layout.width as usize)
                    .saturating_sub(right.len() * CW)
                    .saturating_sub(10);
                self.put_str(right_x, (layout.status_y + 6) as usize, &right, FM_TEXT_DIM);
            }
        }
    }

    fn fill_background(&mut self) {
        let stride = self.window.width.max(0) as usize;
        let height = (self.window.height - TITLE_H).max(0) as usize;
        for row in 0..height {
            let t = (row as u32).saturating_mul(255) / height.max(1) as u32;
            let row_color = blend(FM_BG_TOP, FM_BG_BOT, t);
            let base = row * stride;
            for col in 0..stride {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = row_color;
                }
            }
        }
    }

    fn folder_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| if entry.is_dir { Some(idx) } else { None })
            .collect()
    }

    fn file_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| if entry.is_dir { None } else { Some(idx) })
            .collect()
    }

    fn root_directory_names(&self) -> Vec<String> {
        crate::fat32::list_dir("/")
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.name)
            .collect()
    }

    fn breadcrumb_text(&self) -> String {
        if self.path == "/" {
            return String::from("This PC");
        }

        let mut out = String::from("This PC");
        for component in self.path.split('/').filter(|s| !s.is_empty()) {
            out.push_str(" > ");
            out.push_str(component);
        }
        out
    }

    fn draw_command_button(&mut self, rect: Rect, label: &str) {
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER_SOFT);
        let label_x = rect.x + (rect.w - (label.len() as i32 * CW as i32)) / 2;
        self.put_str(
            label_x.max(0) as usize,
            (rect.y + 6) as usize,
            label,
            FM_TEXT,
        );
    }

    fn draw_action_button(&mut self, rect: Rect, label: &str) {
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_ACCENT_SOFT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_SELECTION_GLOW);
        let label_x = rect.x + (rect.w - (label.len() as i32 * CW as i32)) / 2;
        self.put_str(
            label_x.max(0) as usize,
            (rect.y + 6) as usize,
            label,
            FM_TEXT,
        );
    }

    fn draw_folder_tile(&mut self, rect: Rect, entry_idx: usize) {
        let selected = self.selected == Some(entry_idx);
        self.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected { FM_SELECTION } else { FM_PANEL },
        );
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER
            },
        );
        self.fill_rect(rect.x + 10, rect.y + 12, 18, 12, FM_FOLDER);
        self.fill_rect(rect.x + 8, rect.y + 16, 28, 18, FM_FOLDER);
        self.fill_rect(rect.x + 10, rect.y + 20, 24, 10, FM_FOLDER_SHADE);

        if let Some(entry) = self.entries.get(entry_idx) {
            let entry_name = entry.name.clone();
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 14) as usize,
                &Self::clip_text(&entry_name, ((rect.w - 56).max(8) as usize) / CW),
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 28) as usize,
                "File folder",
                FM_TEXT_MUTED,
            );
        }
    }

    fn draw_drive_card(&mut self, rect: Rect, entry_idx: usize) {
        let selected = self.selected == Some(entry_idx);
        self.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected { FM_SELECTION } else { FM_PANEL },
        );
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if selected {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER
            },
        );

        self.draw_drive_icon((rect.x + 10) as usize, (rect.y + 12) as usize);
        if let Some(entry) = self.entries.get(entry_idx) {
            let entry_name = entry.name.clone();
            let entry_is_dir = entry.is_dir;
            let entry_size = entry.size;
            let label = Self::clip_text(&entry_name, ((rect.w - 62).max(8) as usize) / CW);
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 10) as usize,
                &label,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            let detail = if entry_is_dir {
                "Folder root"
            } else {
                Self::type_label(&entry_name, false)
            };
            self.put_str(
                (rect.x + 46) as usize,
                (rect.y + 22) as usize,
                detail,
                FM_TEXT_MUTED,
            );

            let usage = Self::usage_ratio(&DirEntryInfo {
                name: entry_name.clone(),
                is_dir: entry_is_dir,
                size: entry_size,
            });
            self.fill_rect(rect.x + 46, rect.y + 35, rect.w - 58, 8, FM_SEARCH);
            self.draw_rect_border(rect.x + 46, rect.y + 35, rect.w - 58, 8, FM_BORDER_SOFT);
            self.fill_rect(
                rect.x + 47,
                rect.y + 36,
                ((rect.w - 60) * usage / 100).max(6),
                6,
                FM_DRIVE_FILL,
            );

            let size = if entry_is_dir {
                String::from("shell view")
            } else {
                Self::format_size(entry_size)
            };
            self.put_str(
                (rect.x + rect.w - 10 - size.len() as i32 * CW as i32).max(rect.x + 46) as usize,
                (rect.y + 10) as usize,
                &size,
                FM_TEXT,
            );
        }
    }

    fn draw_drive_icon(&mut self, x: usize, y: usize) {
        self.fill_rect(x as i32, y as i32 + 8, 22, 8, FM_DRIVE);
        self.fill_rect(
            x as i32 + 2,
            y as i32 + 4,
            18,
            6,
            blend(FM_DRIVE, WHITE, 90),
        );
        self.fill_rect(x as i32 + 5, y as i32 + 11, 12, 2, FM_ACCENT);
        self.draw_rect_border(x as i32, y as i32 + 8, 22, 8, blend(FM_DRIVE, BLACK, 120));
    }

    fn draw_file_icon(&mut self, x: usize, y: usize) {
        self.fill_rect(x as i32 + 1, y as i32, 10, 12, FM_FILE);
        self.draw_rect_border(x as i32 + 1, y as i32, 10, 12, blend(FM_FILE, BLACK, 120));
        self.fill_rect(x as i32 + 6, y as i32, 4, 3, WHITE);
    }

    fn usage_ratio(entry: &DirEntryInfo) -> i32 {
        if entry.is_dir {
            ((entry.name.len() as i32 * 11) % 62) + 20
        } else if entry.size == 0 {
            8
        } else {
            ((entry.size % 100) as i32).clamp(12, 92)
        }
    }

    fn hit_navigation(&self, lx: i32, ly: i32) -> Option<String> {
        let layout = self.layout();
        if lx >= layout.sidebar_w || ly < COMMAND_H + PATHBAR_H {
            return None;
        }
        let mut y = COMMAND_H + PATHBAR_H + 30;
        for (_label, path, _active) in self.sidebar_items() {
            let rect = Rect {
                x: 10,
                y,
                w: layout.sidebar_w - 20,
                h: NAV_ROW_H,
            };
            if let Some(path) = path {
                if rect.hit(lx, ly) {
                    return Some(path);
                }
                y += NAV_ROW_H + 4;
            } else {
                y += NAV_ROW_H + 4;
            }
        }
        None
    }

    fn hit_main_entry(&self, lx: i32, ly: i32) -> Option<usize> {
        let layout = self.layout();
        if lx < layout.main_x || ly < COMMAND_H + PATHBAR_H {
            return None;
        }

        if self.path == "/" {
            self.hit_root_entry(lx, ly)
        } else {
            self.hit_directory_entry(lx, ly)
        }
    }

    fn hit_root_entry(&self, lx: i32, ly: i32) -> Option<usize> {
        let layout = self.layout();
        let folders = self.folder_indices();
        let files = self.file_indices();
        let top = COMMAND_H + PATHBAR_H + 14 + 34;
        if let Some(idx) = self.hit_folder_grid(layout, top, &folders, true, lx, ly) {
            return Some(idx);
        }
        let tiles_h = self.folder_section_height(layout, &folders);
        let drives_y = top + tiles_h + 20;
        self.hit_drive_grid(
            layout,
            drives_y,
            if files.is_empty() { &folders } else { &files },
            lx,
            ly,
        )
    }

    fn hit_directory_entry(&self, lx: i32, ly: i32) -> Option<usize> {
        let layout = self.layout();
        let folders = self.folder_indices();
        let files = self.file_indices();
        let top = COMMAND_H + PATHBAR_H + 14 + 34;
        if let Some(idx) = self.hit_folder_grid(layout, top, &folders, false, lx, ly) {
            return Some(idx);
        }

        let files_y = top + self.folder_section_height(layout, &folders) + 18;
        let list_y = files_y + 22;
        if ly < list_y || ly >= layout.status_y - 10 {
            return None;
        }
        let visible = self.total_rows.max(1);
        let idx_in_files = self.offset + ((ly - list_y) / LIST_ROW_H) as usize;
        if idx_in_files >= files.len() || idx_in_files >= self.offset + visible {
            return None;
        }
        Some(files[idx_in_files])
    }

    fn hit_folder_grid(
        &self,
        layout: Layout,
        top: i32,
        indices: &[usize],
        root_mode: bool,
        lx: i32,
        ly: i32,
    ) -> Option<usize> {
        let tile_y = top + 22;
        let tile_w = ((layout.main_w - 60 - TILE_GAP_X * 2) / 3).max(140);
        let cols = ((layout.main_w - 36) / (tile_w + TILE_GAP_X)).max(1) as usize;
        let max = if root_mode { 9 } else { indices.len().min(9) };
        for (visual_idx, &entry_idx) in indices.iter().take(max).enumerate() {
            let col = (visual_idx % cols).min(2);
            let row = visual_idx / cols;
            let rect = Rect {
                x: layout.main_x + 18 + col as i32 * (tile_w + TILE_GAP_X),
                y: tile_y + row as i32 * (TILE_H + TILE_GAP_Y),
                w: tile_w,
                h: TILE_H,
            };
            if rect.hit(lx, ly) {
                return Some(entry_idx);
            }
        }
        None
    }

    fn hit_drive_grid(
        &self,
        layout: Layout,
        y: i32,
        indices: &[usize],
        lx: i32,
        ly: i32,
    ) -> Option<usize> {
        let card_w = ((layout.main_w - 54) / 2).max(180);
        let cols = if layout.main_w > 420 { 2 } else { 1 };
        let max_items = if cols == 2 { 4 } else { 3 };
        for (visual_idx, &entry_idx) in indices.iter().take(max_items).enumerate() {
            let col = (visual_idx % cols as usize) as i32;
            let row = (visual_idx / cols as usize) as i32;
            let rect = Rect {
                x: layout.main_x + 18 + col * (card_w + 12),
                y: y + 22 + row * (DRIVE_H + DRIVE_GAP_Y),
                w: card_w,
                h: DRIVE_H,
            };
            if rect.hit(lx, ly) {
                return Some(entry_idx);
            }
        }
        None
    }

    fn folder_section_height(&self, layout: Layout, indices: &[usize]) -> i32 {
        if indices.is_empty() {
            return 40;
        }
        let tile_w = ((layout.main_w - 60 - TILE_GAP_X * 2) / 3).max(140);
        let cols = ((layout.main_w - 36) / (tile_w + TILE_GAP_X)).max(1) as usize;
        let rows = ((indices.len().min(9) + cols - 1) / cols).max(1) as i32;
        22 + rows * TILE_H + (rows - 1) * TILE_GAP_Y
    }

    fn create_new_file(&mut self) {
        let name = self.next_available_name("FILE", Some("TXT"));
        let path = self.join_child_path(&name);
        match crate::fat32::create_file(&path) {
            Ok(()) => self.reload_after_create(&name, "created"),
            Err(err) => {
                self.set_status_error("file", err.as_str());
                self.render();
            }
        }
    }

    fn create_new_dir(&mut self) {
        let name = self.next_available_name("DIR", None);
        let path = self.join_child_path(&name);
        match crate::fat32::create_dir(&path) {
            Ok(()) => self.reload_after_create(&name, "created"),
            Err(err) => {
                self.set_status_error("folder", err.as_str());
                self.render();
            }
        }
    }

    fn reload_after_create(&mut self, name: &str, verb: &str) {
        let current = self.path.clone();
        self.load_dir_with_state(&current, Some(name), Some(self.offset));
        let mut note = String::from(verb);
        note.push(' ');
        note.push_str(name);
        self.status_note = Some(note);
        self.render();
    }

    fn set_status_error(&mut self, subject: &str, detail: &str) {
        let mut note = String::from(subject);
        note.push_str(" create failed: ");
        note.push_str(detail);
        self.status_note = Some(note);
    }

    fn next_available_name(&self, prefix: &str, ext: Option<&str>) -> String {
        for n in 1..10_000usize {
            let candidate = Self::numbered_name(prefix, n, ext);
            if !self
                .entries
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
        }

        let mut fallback = String::from(prefix);
        if let Some(ext) = ext {
            fallback.push('.');
            fallback.push_str(ext);
        }
        fallback
    }

    fn numbered_name(prefix: &str, n: usize, ext: Option<&str>) -> String {
        let mut s = String::from(prefix);
        fmt_push_u(&mut s, n as u64);
        if let Some(ext) = ext {
            s.push('.');
            s.push_str(ext);
        }
        s
    }

    fn join_child_path(&self, name: &str) -> String {
        let mut path = self.path.clone();
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(name);
        path
    }

    fn clip_text(text: &str, max_chars: usize) -> String {
        if max_chars == 0 {
            return String::new();
        }
        if text.len() <= max_chars {
            return String::from(text);
        }
        if max_chars == 1 {
            return String::from("~");
        }
        let mut clipped = String::new();
        for ch in text.chars().take(max_chars - 1) {
            clipped.push(ch);
        }
        clipped.push('~');
        clipped
    }

    fn put_str(&mut self, px: usize, py: usize, s: &str, color: u32) {
        let stride = self.window.width.max(0) as usize;
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

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let stride = self.window.width.max(0) as usize;
        let max_h = if stride > 0 {
            self.window.buf.len() / stride
        } else {
            0
        };
        let start_x = x.max(0) as usize;
        let start_y = y.max(0) as usize;
        let end_x = (x + w).max(0) as usize;
        let end_y = (y + h).max(0) as usize;
        for row in start_y..end_y.min(max_h) {
            let base = row * stride;
            for col in start_x..end_x.min(stride) {
                let idx = base + col;
                if idx < self.window.buf.len() {
                    self.window.buf[idx] = color;
                }
            }
        }
    }

    fn draw_rect_border(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }
}

fn blend(a: u32, b: u32, t: u32) -> u32 {
    let clamped = t.min(255);
    let inv = 255 - clamped;
    let r = (((a >> 16) & 0xFF) * inv + ((b >> 16) & 0xFF) * clamped) / 255;
    let g = (((a >> 8) & 0xFF) * inv + ((b >> 8) & 0xFF) * clamped) / 255;
    let bl = ((a & 0xFF) * inv + (b & 0xFF) * clamped) / 255;
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
