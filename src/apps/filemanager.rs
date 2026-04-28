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
const COMMAND_H: i32 = 0;
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
const SUMMARY_CARD_H: i32 = 42;
const SUMMARY_CARD_GAP: i32 = 10;
const FILE_HEADER_H: i32 = 20;
const DETAIL_W: i32 = 186;
const DETAIL_GAP: i32 = 14;
const BACK_BTN_W: i32 = 24;
const BACK_BTN_GAP: i32 = 8;
const BREAD_SEG_PAD: i32 = 10;
const BREAD_SEG_GAP: i32 = 6;
const MENU_W: i32 = 138;
const MENU_ROW_H: i32 = 20;
const DIALOG_W: i32 = 320;
const DIALOG_H: i32 = 140;
const EDITOR_W: i32 = 468;
const EDITOR_H: i32 = 286;
const QUICK_ACCESS_FOLDERS: [&str; 6] = [
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Videos",
    "Desktop",
];
const START_MENU_FOLDERS: [&str; 9] = [
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Videos",
    "Desktop",
    "Home",
    "Shared",
    "Trash",
];
const DESKTOP_APP_LINKS: [(&str, &str); 5] = [
    ("Terminal", "Terminal"),
    ("Monitor", "System Monitor"),
    ("Files", "File Manager"),
    ("Viewer", "Text Viewer"),
    ("Colors", "Color Picker"),
];

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

#[derive(Clone, Copy)]
struct FileColumns {
    row_x: i32,
    row_w: i32,
    name_x: i32,
    name_w: i32,
    type_x: i32,
    size_x: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarItemKind {
    Section,
    Link,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarIcon {
    Computer,
    Folder,
}

struct SidebarItem {
    label: String,
    path: Option<String>,
    active: bool,
    kind: SidebarItemKind,
    indent: i32,
    icon: SidebarIcon,
}

#[derive(Clone, Copy)]
enum EntryKind {
    Fs,
    DesktopApp(&'static str),
}

#[derive(Clone, Copy)]
enum ContextAction {
    Open,
    NewFile,
    NewFolder,
    Rename,
    EditText,
    Refresh,
}

struct ContextMenuState {
    x: i32,
    y: i32,
    target: Option<usize>,
}

#[derive(Clone)]
enum NameDialogMode {
    NewFile,
    NewFolder,
    Rename(usize),
}

#[derive(Clone)]
struct NameDialogState {
    mode: NameDialogMode,
    input: String,
    cursor: usize,
    error: Option<String>,
}

#[derive(Clone)]
struct TextEditorState {
    entry_idx: usize,
    path: String,
    text: String,
    cursor: usize,
    scroll_line: usize,
    error: Option<String>,
}

#[derive(Clone)]
enum ModalState {
    Name(NameDialogState),
    TextEditor(TextEditorState),
}

pub enum FileManagerOpenRequest {
    File(String),
    App(&'static str),
}

pub struct FileManagerApp {
    pub window: Window,
    entries: Vec<DirEntryInfo>,
    entry_kinds: Vec<EntryKind>,
    path: String,
    offset: usize,
    view_h: i32,
    selected: Option<usize>,
    total_rows: usize,
    pending_open: Option<FileManagerOpenRequest>,
    status_note: Option<String>,
    last_width: i32,
    last_height: i32,
    sort_column: SortColumn,
    sort_desc: bool,
    context_menu: Option<ContextMenuState>,
    modal: Option<ModalState>,
}

impl FileManagerApp {
    pub fn new(x: i32, y: i32) -> Self {
        Self::new_at_path(x, y, "/")
    }

    pub fn new_at_path(x: i32, y: i32, dir: &str) -> Self {
        let mut app = FileManagerApp {
            window: Window::new(x, y, FILEMAN_W, FILEMAN_H, "File Manager"),
            entries: Vec::new(),
            entry_kinds: Vec::new(),
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
            context_menu: None,
            modal: None,
        };
        app.load_dir(dir);
        app
    }

    pub const START_MENU_LINKS: [&'static str; 9] = START_MENU_FOLDERS;

    pub fn shell_link_path(label: &str) -> String {
        let root_names = Self::root_directory_names();
        Self::shell_link_path_with_roots(label, &root_names)
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
        let (mut new_entries, mut entry_kinds) = self.load_entries_for_path(dir);
        Self::sort_entries(
            &mut new_entries,
            &mut entry_kinds,
            self.sort_column,
            self.sort_desc,
        );
        self.entries = new_entries;
        self.entry_kinds = entry_kinds;
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
        self.context_menu = None;
        self.status_note = None;
        self.render();
    }

    pub fn handle_key(&mut self, c: char) {
        if self.handle_modal_key(c) {
            self.render();
        }
    }

    pub fn handle_click(&mut self, lx: i32, ly: i32) {
        if self.handle_modal_click(lx, ly) {
            self.render();
            return;
        }

        if self.handle_context_menu_click(lx, ly) {
            self.render();
            return;
        }

        if let Some(path) = self.hit_navigation(lx, ly) {
            self.context_menu = None;
            self.load_dir(&path);
            return;
        }

        if ly >= COMMAND_H && ly < COMMAND_H + PATHBAR_H {
            self.context_menu = None;
            if self.back_button_rect().hit(lx, ly) {
                if self.path != "/" {
                    let parent = self.parent_path();
                    self.load_dir(&parent);
                }
                return;
            }
            if let Some(path) = self.hit_breadcrumb(lx, ly) {
                self.load_dir(&path);
            }
            return;
        }

        if let Some(column) = self.hit_file_header_column(lx, ly) {
            self.context_menu = None;
            self.change_sort(column);
            return;
        }

        self.context_menu = None;
        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.selected = Some(idx);
            self.render();
        }
    }

    pub fn handle_secondary_click(&mut self, lx: i32, ly: i32) {
        if self.modal.is_some() {
            return;
        }
        let layout = self.layout();
        if lx < layout.main_x || ly < COMMAND_H + PATHBAR_H || ly >= layout.status_y {
            self.context_menu = None;
            self.render();
            return;
        }
        let target = self.hit_main_entry(lx, ly);
        if let Some(idx) = target {
            self.selected = Some(idx);
        }
        self.context_menu = Some(self.clamp_context_menu(lx, ly, target));
        self.render();
    }

    pub fn handle_dbl_click(&mut self, lx: i32, ly: i32) {
        if self.modal.is_some() || self.context_menu.is_some() {
            return;
        }
        if let Some(idx) = self.hit_main_entry(lx, ly) {
            self.selected = Some(idx);
            self.open_selected();
        }
    }

    pub fn take_open_request(&mut self) -> Option<FileManagerOpenRequest> {
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

    fn open_selected(&mut self) {
        let sel = match self.selected {
            Some(s) => s,
            None => return,
        };
        if sel >= self.entries.len() {
            return;
        }
        if let Some(app) = self.desktop_app_for_idx(sel) {
            self.pending_open = Some(FileManagerOpenRequest::App(app));
            return;
        }
        let abs = self.make_abs(sel);
        if self.is_dir_idx(sel) {
            self.load_dir(&abs);
        } else {
            self.pending_open = Some(FileManagerOpenRequest::File(abs));
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

    fn type_label_for_kind(entry: &DirEntryInfo, kind: EntryKind) -> &'static str {
        match kind {
            EntryKind::Fs => Self::type_label(&entry.name, entry.is_dir),
            EntryKind::DesktopApp(_) => "Application",
        }
    }

    fn is_editable_text_name(name: &str) -> bool {
        matches!(Self::file_ext(name), "TXT" | "MD" | "LOG" | "RST" | "CSV")
    }

    fn selected_name(&self) -> Option<String> {
        self.selected
            .and_then(|idx| self.entries.get(idx))
            .map(|entry| entry.name.clone())
    }

    fn visible_row_capacity(&self) -> usize {
        if self.path == "/" {
            return 0;
        }
        let layout = self.layout();
        let rows_y = self.file_rows_y(layout);
        let list_h = (layout.status_y - rows_y - 10).max(0);
        (list_h / LIST_ROW_H) as usize
    }

    fn sort_entries(
        entries: &mut [DirEntryInfo],
        entry_kinds: &mut [EntryKind],
        sort_column: SortColumn,
        sort_desc: bool,
    ) {
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by(|&a_idx, &b_idx| {
            let a = &entries[a_idx];
            let b = &entries[b_idx];
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
                SortColumn::Type => Self::type_label_for_kind(a, entry_kinds[a_idx])
                    .cmp(Self::type_label_for_kind(b, entry_kinds[b_idx]))
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

        let old_entries = entries.to_vec();
        let old_kinds = entry_kinds.to_vec();
        for (dst, src) in order.into_iter().enumerate() {
            entries[dst] = old_entries[src].clone();
            entry_kinds[dst] = old_kinds[src];
        }
    }

    fn resort_entries(&mut self, note: &str) {
        let selected = self.selected_name();
        Self::sort_entries(
            &mut self.entries,
            &mut self.entry_kinds,
            self.sort_column,
            self.sort_desc,
        );
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

    fn load_entries_for_path(&self, dir: &str) -> (Vec<DirEntryInfo>, Vec<EntryKind>) {
        let mut entries = crate::fat32::list_dir(dir).unwrap_or_default();
        let mut entry_kinds = alloc::vec![EntryKind::Fs; entries.len()];

        if dir.eq_ignore_ascii_case("/Desktop") {
            for (label, app) in DESKTOP_APP_LINKS {
                if entries
                    .iter()
                    .any(|entry| entry.name.eq_ignore_ascii_case(label))
                {
                    continue;
                }
                entries.push(DirEntryInfo {
                    name: String::from(label),
                    is_dir: false,
                    size: 0,
                });
                entry_kinds.push(EntryKind::DesktopApp(app));
            }
        }

        (entries, entry_kinds)
    }

    fn desktop_app_for_idx(&self, idx: usize) -> Option<&'static str> {
        match self.entry_kinds.get(idx).copied() {
            Some(EntryKind::DesktopApp(app)) => Some(app),
            _ => None,
        }
    }

    fn entry_kind(&self, idx: usize) -> EntryKind {
        self.entry_kinds.get(idx).copied().unwrap_or(EntryKind::Fs)
    }

    fn entry_can_rename(&self, idx: usize) -> bool {
        matches!(self.entry_kind(idx), EntryKind::Fs)
    }

    fn entry_can_edit_text(&self, idx: usize) -> bool {
        matches!(self.entry_kind(idx), EntryKind::Fs)
            && self
                .entries
                .get(idx)
                .map(|entry| !entry.is_dir && Self::is_editable_text_name(&entry.name))
                .unwrap_or(false)
    }

    fn ensure_current_dir_exists(&self) -> Result<(), &'static str> {
        if self.path == "/" || crate::fat32::list_dir(&self.path).is_some() {
            return Ok(());
        }
        crate::fat32::create_dir(&self.path).map_err(|err| err.as_str())
    }

    fn render(&mut self) {
        let layout = self.layout();
        self.last_width = self.window.width;
        self.last_height = self.window.height;
        self.view_h = (layout.height - COMMAND_H - PATHBAR_H - STATUS_H).max(0);

        self.fill_background();
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
        self.draw_detail_panel(layout);
        self.draw_status_bar(layout);
        self.draw_context_menu();
        self.draw_modal();
    }

    fn context_menu_items(&self, target: Option<usize>) -> Vec<(&'static str, ContextAction)> {
        let mut items = Vec::new();
        if let Some(idx) = target {
            items.push(("Open", ContextAction::Open));
            if self.entry_can_edit_text(idx) {
                items.push(("Edit Text", ContextAction::EditText));
            }
            if self.entry_can_rename(idx) {
                items.push(("Rename", ContextAction::Rename));
            }
        }
        items.push(("New File", ContextAction::NewFile));
        items.push(("New Folder", ContextAction::NewFolder));
        items.push(("Refresh", ContextAction::Refresh));
        items
    }

    fn context_menu_rect(&self, menu: &ContextMenuState) -> Rect {
        let items = self.context_menu_items(menu.target);
        Rect {
            x: menu.x,
            y: menu.y,
            w: MENU_W,
            h: items.len() as i32 * MENU_ROW_H + 6,
        }
    }

    fn clamp_context_menu(&self, lx: i32, ly: i32, target: Option<usize>) -> ContextMenuState {
        let layout = self.layout();
        let temp = ContextMenuState {
            x: lx,
            y: ly,
            target,
        };
        let rect = self.context_menu_rect(&temp);
        ContextMenuState {
            x: lx.clamp(
                layout.main_x + 4,
                (layout.width - rect.w - 4).max(layout.main_x + 4),
            ),
            y: ly.clamp(
                COMMAND_H + PATHBAR_H + 4,
                (layout.status_y - rect.h - 4).max(COMMAND_H + PATHBAR_H + 4),
            ),
            target,
        }
    }

    fn handle_context_menu_click(&mut self, lx: i32, ly: i32) -> bool {
        let Some(menu) = self.context_menu.as_ref() else {
            return false;
        };
        let items = self.context_menu_items(menu.target);
        let rect = self.context_menu_rect(menu);
        if !rect.hit(lx, ly) {
            self.context_menu = None;
            return true;
        }

        let rel_y = ly - rect.y - 3;
        if rel_y < 0 {
            return true;
        }
        let idx = (rel_y / MENU_ROW_H) as usize;
        if let Some((_, action)) = items.get(idx).copied() {
            let target = menu.target;
            self.context_menu = None;
            self.run_context_action(action, target);
        }
        true
    }

    fn run_context_action(&mut self, action: ContextAction, target: Option<usize>) {
        match action {
            ContextAction::Open => {
                if let Some(idx) = target {
                    self.selected = Some(idx);
                    self.open_selected();
                }
            }
            ContextAction::NewFile => {
                self.modal = Some(ModalState::Name(NameDialogState {
                    mode: NameDialogMode::NewFile,
                    input: String::from("NEWFILE.TXT"),
                    cursor: "NEWFILE.TXT".len(),
                    error: None,
                }));
            }
            ContextAction::NewFolder => {
                self.modal = Some(ModalState::Name(NameDialogState {
                    mode: NameDialogMode::NewFolder,
                    input: String::from("NEWDIR"),
                    cursor: "NEWDIR".len(),
                    error: None,
                }));
            }
            ContextAction::Rename => {
                if let Some(idx) = target {
                    if let Some(entry) = self.entries.get(idx) {
                        let name = entry.name.clone();
                        self.modal = Some(ModalState::Name(NameDialogState {
                            mode: NameDialogMode::Rename(idx),
                            input: name.clone(),
                            cursor: name.len(),
                            error: None,
                        }));
                    }
                }
            }
            ContextAction::EditText => {
                if let Some(idx) = target {
                    self.open_text_editor(idx);
                }
            }
            ContextAction::Refresh => self.refresh_current_dir(),
        }
    }

    fn handle_modal_key(&mut self, c: char) -> bool {
        let changed = match self.modal.as_mut() {
            Some(ModalState::Name(state)) => Self::handle_name_dialog_key(state, c),
            Some(ModalState::TextEditor(state)) => Self::handle_text_editor_key(state, c),
            None => false,
        };
        if changed {
            self.sync_modal_state();
        }
        changed
    }

    fn handle_modal_click(&mut self, lx: i32, ly: i32) -> bool {
        let handled = match self.modal.clone() {
            Some(ModalState::Name(_)) => self.handle_name_dialog_click(lx, ly),
            Some(ModalState::TextEditor(_)) => self.handle_text_editor_click(lx, ly),
            None => false,
        };
        if handled {
            self.sync_modal_state();
        }
        handled
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

    fn draw_path_bar(&mut self, layout: Layout) {
        let back = self.back_button_rect();
        let crumb = self.breadcrumb_rect();
        let search = self.search_rect(layout);

        self.fill_rect(0, COMMAND_H, layout.width, PATHBAR_H, FM_PANEL_ALT);
        self.fill_rect(
            0,
            COMMAND_H + PATHBAR_H - 1,
            layout.width,
            1,
            FM_BORDER_SOFT,
        );

        self.draw_back_button(back);
        self.fill_rect(crumb.x, crumb.y, crumb.w, crumb.h, FM_PANEL);
        self.draw_rect_border(crumb.x, crumb.y, crumb.w, crumb.h, FM_BORDER);
        self.draw_breadcrumbs(crumb);

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
        let search = self.search_rect(layout);
        let back = self.back_button_rect();
        Rect {
            x: back.x + back.w + BACK_BTN_GAP,
            y: COMMAND_H + 4,
            w: (search.x - (back.x + back.w + BACK_BTN_GAP) - 12).max(104),
            h: 22,
        }
    }

    fn back_button_rect(&self) -> Rect {
        Rect {
            x: 12,
            y: COMMAND_H + 4,
            w: BACK_BTN_W,
            h: 22,
        }
    }

    fn search_rect(&self, layout: Layout) -> Rect {
        Rect {
            x: layout.width - 170,
            y: COMMAND_H + 4,
            w: 156,
            h: 22,
        }
    }

    fn draw_context_menu(&mut self) {
        if self.modal.is_some() {
            return;
        }
        let Some(menu) = self.context_menu.as_ref() else {
            return;
        };
        let items = self.context_menu_items(menu.target);
        let rect = self.context_menu_rect(menu);
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        for (idx, (label, _)) in items.iter().enumerate() {
            let row_y = rect.y + 3 + idx as i32 * MENU_ROW_H;
            self.fill_rect(rect.x + 1, row_y, rect.w - 2, MENU_ROW_H, FM_PANEL);
            self.put_str((rect.x + 10) as usize, (row_y + 6) as usize, label, FM_TEXT);
            if idx + 1 < items.len() {
                self.fill_rect(
                    rect.x + 8,
                    row_y + MENU_ROW_H - 1,
                    rect.w - 16,
                    1,
                    FM_BORDER_SOFT,
                );
            }
        }
    }

    fn draw_modal(&mut self) {
        let Some(modal) = self.modal.clone() else {
            return;
        };
        match modal {
            ModalState::Name(state) => self.draw_name_dialog(&state),
            ModalState::TextEditor(state) => self.draw_text_editor(&state),
        }
    }

    fn draw_name_dialog(&mut self, state: &NameDialogState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, DIALOG_W, DIALOG_H);
        let input = self.name_dialog_input_rect(rect);
        let save = self.dialog_save_button_rect(rect);
        let cancel = self.dialog_cancel_button_rect(rect);
        let title = match state.mode {
            NameDialogMode::NewFile => "Create New File",
            NameDialogMode::NewFolder => "Create New Folder",
            NameDialogMode::Rename(_) => "Rename Item",
        };

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 12) as usize,
            title,
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 28) as usize,
            "8.3 names only",
            FM_TEXT_MUTED,
        );

        self.fill_rect(input.x, input.y, input.w, input.h, FM_SEARCH);
        self.draw_rect_border(input.x, input.y, input.w, input.h, FM_BORDER_SOFT);
        let max_chars = ((input.w - 16).max(8) as usize) / CW;
        self.put_str(
            (input.x + 8) as usize,
            (input.y + 8) as usize,
            &Self::clip_text(&state.input, max_chars),
            FM_TEXT,
        );

        let cursor_char = state.cursor.min(state.input.chars().count());
        let cursor_x = input.x + 8 + (cursor_char as i32 * CW as i32).min(input.w - 14);
        self.fill_rect(cursor_x, input.y + 6, 2, input.h - 12, FM_SELECTION_GLOW);

        if let Some(error) = state.error.as_ref() {
            self.put_str(
                (rect.x + 14) as usize,
                (rect.y + 76) as usize,
                &Self::clip_text(error, ((rect.w - 28).max(0) as usize) / CW),
                blend(FM_ACCENT, WHITE, 72),
            );
        }

        self.draw_dialog_button(save, "Save");
        self.draw_dialog_button(cancel, "Cancel");
    }

    fn draw_text_editor(&mut self, state: &TextEditorState) {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let text_rect = self.editor_text_rect(rect);
        let save = self.editor_save_button_rect(rect);
        let cancel = self.editor_cancel_button_rect(rect);
        let visible_lines = ((text_rect.h - 12).max(12) as usize) / 12;
        let (cursor_line, cursor_col) = Self::text_cursor_line_col(&state.text, state.cursor);
        let lines = Self::text_lines(&state.text);

        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL_ALT);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x, rect.y, rect.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 12) as usize,
            "Edit Text File",
            FM_TEXT,
        );
        self.put_str(
            (rect.x + 14) as usize,
            (rect.y + 28) as usize,
            &Self::clip_text(&state.path, ((rect.w - 28).max(0) as usize) / CW),
            FM_TEXT_MUTED,
        );

        self.fill_rect(
            text_rect.x,
            text_rect.y,
            text_rect.w,
            text_rect.h,
            FM_SEARCH,
        );
        self.draw_rect_border(
            text_rect.x,
            text_rect.y,
            text_rect.w,
            text_rect.h,
            FM_BORDER_SOFT,
        );
        let max_chars = ((text_rect.w - 18).max(8) as usize) / CW;
        for screen_line in 0..visible_lines {
            let doc_line = state.scroll_line + screen_line;
            let Some(line) = lines.get(doc_line) else {
                break;
            };
            self.put_str(
                (text_rect.x + 8) as usize,
                (text_rect.y + 8 + screen_line as i32 * 12) as usize,
                &Self::clip_text(line, max_chars),
                FM_TEXT,
            );
        }
        if cursor_line >= state.scroll_line && cursor_line < state.scroll_line + visible_lines {
            let cursor_x = text_rect.x + 8 + (cursor_col.min(max_chars) as i32 * CW as i32);
            let cursor_y = text_rect.y + 8 + ((cursor_line - state.scroll_line) as i32 * 12);
            self.fill_rect(cursor_x, cursor_y, 2, 10, FM_SELECTION_GLOW);
        }

        if let Some(error) = state.error.as_ref() {
            self.put_str(
                (rect.x + 14) as usize,
                (rect.y + rect.h - 52) as usize,
                &Self::clip_text(error, ((rect.w - 28).max(0) as usize) / CW),
                blend(FM_ACCENT, WHITE, 72),
            );
        }

        self.draw_dialog_button(save, "Save");
        self.draw_dialog_button(cancel, "Cancel");
    }

    fn draw_dialog_button(&mut self, rect: Rect, label: &str) {
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        let label_x = rect.x + ((rect.w - label.len() as i32 * CW as i32) / 2).max(6);
        self.put_str(label_x as usize, (rect.y + 7) as usize, label, FM_TEXT);
    }

    fn centered_rect(layout: Layout, w: i32, h: i32) -> Rect {
        Rect {
            x: ((layout.width - w) / 2).max(14),
            y: ((layout.status_y - h) / 2).max(PATHBAR_H + 14),
            w,
            h,
        }
    }

    fn name_dialog_input_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + 14,
            y: rect.y + 46,
            w: rect.w - 28,
            h: 26,
        }
    }

    fn dialog_save_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 156,
            y: rect.y + rect.h - 36,
            w: 64,
            h: 24,
        }
    }

    fn dialog_cancel_button_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + rect.w - 82,
            y: rect.y + rect.h - 36,
            w: 68,
            h: 24,
        }
    }

    fn editor_text_rect(&self, rect: Rect) -> Rect {
        Rect {
            x: rect.x + 14,
            y: rect.y + 46,
            w: rect.w - 28,
            h: rect.h - 94,
        }
    }

    fn editor_save_button_rect(&self, rect: Rect) -> Rect {
        self.dialog_save_button_rect(rect)
    }

    fn editor_cancel_button_rect(&self, rect: Rect) -> Rect {
        self.dialog_cancel_button_rect(rect)
    }

    fn handle_name_dialog_key(state: &mut NameDialogState, c: char) -> bool {
        match c {
            '\u{0008}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    let byte = Self::char_to_byte_index(&state.input, state.cursor);
                    let next = Self::char_to_byte_index(&state.input, state.cursor + 1);
                    state.input.replace_range(byte..next, "");
                    state.error = None;
                    return true;
                }
            }
            '\u{F702}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    return true;
                }
            }
            '\u{F703}' => {
                let len = state.input.chars().count();
                if state.cursor < len {
                    state.cursor += 1;
                    return true;
                }
            }
            _ if !c.is_control() => {
                let byte = Self::char_to_byte_index(&state.input, state.cursor);
                state.input.insert(byte, c);
                state.cursor += 1;
                state.error = None;
                return true;
            }
            _ => {}
        }
        false
    }

    fn handle_text_editor_key(state: &mut TextEditorState, c: char) -> bool {
        match c {
            '\u{0008}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    let byte = Self::char_to_byte_index(&state.text, state.cursor);
                    let next = Self::char_to_byte_index(&state.text, state.cursor + 1);
                    state.text.replace_range(byte..next, "");
                    state.error = None;
                    return true;
                }
            }
            '\n' => {
                let byte = Self::char_to_byte_index(&state.text, state.cursor);
                state.text.insert(byte, '\n');
                state.cursor += 1;
                state.error = None;
                return true;
            }
            '\t' => {
                for _ in 0..4 {
                    let byte = Self::char_to_byte_index(&state.text, state.cursor);
                    state.text.insert(byte, ' ');
                    state.cursor += 1;
                }
                state.error = None;
                return true;
            }
            '\u{F702}' => {
                if state.cursor > 0 {
                    state.cursor -= 1;
                    return true;
                }
            }
            '\u{F703}' => {
                let len = state.text.chars().count();
                if state.cursor < len {
                    state.cursor += 1;
                    return true;
                }
            }
            '\u{F700}' => {
                let (line, col) = Self::text_cursor_line_col(&state.text, state.cursor);
                if line > 0 {
                    state.cursor = Self::text_cursor_from_line_col(&state.text, line - 1, col);
                    return true;
                }
            }
            '\u{F701}' => {
                let (line, col) = Self::text_cursor_line_col(&state.text, state.cursor);
                state.cursor = Self::text_cursor_from_line_col(&state.text, line + 1, col);
                return true;
            }
            _ if !c.is_control() => {
                let byte = Self::char_to_byte_index(&state.text, state.cursor);
                state.text.insert(byte, c);
                state.cursor += 1;
                state.error = None;
                return true;
            }
            _ => {}
        }
        false
    }

    fn handle_name_dialog_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, DIALOG_W, DIALOG_H);
        let save = self.dialog_save_button_rect(rect);
        let cancel = self.dialog_cancel_button_rect(rect);
        let input = self.name_dialog_input_rect(rect);
        if save.hit(lx, ly) {
            self.save_name_dialog();
            return true;
        }
        if cancel.hit(lx, ly) {
            self.modal = None;
            return true;
        }
        if input.hit(lx, ly) {
            if let Some(ModalState::Name(state)) = self.modal.as_mut() {
                state.cursor = state.input.chars().count();
                state.error = None;
            }
            return true;
        }
        true
    }

    fn handle_text_editor_click(&mut self, lx: i32, ly: i32) -> bool {
        let layout = self.layout();
        let rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let save = self.editor_save_button_rect(rect);
        let cancel = self.editor_cancel_button_rect(rect);
        let text_rect = self.editor_text_rect(rect);
        if save.hit(lx, ly) {
            self.save_text_editor();
            return true;
        }
        if cancel.hit(lx, ly) {
            self.modal = None;
            return true;
        }
        if text_rect.hit(lx, ly) {
            if let Some(ModalState::TextEditor(state)) = self.modal.as_mut() {
                let max_chars = ((text_rect.w - 18).max(8) as usize) / CW;
                let line = state.scroll_line + ((ly - text_rect.y - 8).max(0) / 12) as usize;
                let col = ((lx - text_rect.x - 8).max(0) as usize / CW).min(max_chars);
                state.cursor = Self::text_cursor_from_line_col(&state.text, line, col);
                state.error = None;
            }
            return true;
        }
        true
    }

    fn save_name_dialog(&mut self) {
        let Some(ModalState::Name(mut state)) = self.modal.take() else {
            return;
        };
        let trimmed = state.input.trim();
        if trimmed.is_empty() {
            state.error = Some(String::from("name required"));
            self.modal = Some(ModalState::Name(state));
            return;
        }

        let result = match state.mode {
            NameDialogMode::NewFile => self.ensure_current_dir_exists().and_then(|_| {
                crate::fat32::create_file(&self.join_child_path(trimmed))
                    .map_err(|err| err.as_str())
            }),
            NameDialogMode::NewFolder => self.ensure_current_dir_exists().and_then(|_| {
                crate::fat32::create_dir(&self.join_child_path(trimmed)).map_err(|err| err.as_str())
            }),
            NameDialogMode::Rename(idx) => {
                crate::fat32::rename(&self.make_abs(idx), trimmed).map_err(|err| err.as_str())
            }
        };

        match result {
            Ok(()) => {
                let current = self.path.clone();
                self.modal = None;
                self.load_dir_with_state(&current, Some(trimmed), Some(self.offset));
                self.status_note = Some(String::from("saved changes"));
            }
            Err(err) => {
                state.error = Some(String::from(err));
                self.modal = Some(ModalState::Name(state));
            }
        }
    }

    fn open_text_editor(&mut self, idx: usize) {
        let path = self.make_abs(idx);
        match crate::fat32::read_file(&path) {
            Some(bytes) => match core::str::from_utf8(&bytes) {
                Ok(text) => {
                    self.modal = Some(ModalState::TextEditor(TextEditorState {
                        entry_idx: idx,
                        path,
                        text: String::from(text),
                        cursor: text.chars().count(),
                        scroll_line: 0,
                        error: None,
                    }));
                }
                Err(_) => {
                    self.status_note = Some(String::from("file is not UTF-8 text"));
                }
            },
            None => {
                self.status_note = Some(String::from("file not found"));
            }
        }
    }

    fn save_text_editor(&mut self) {
        let Some(ModalState::TextEditor(mut state)) = self.modal.take() else {
            return;
        };
        match crate::fat32::write_file(&state.path, state.text.as_bytes()) {
            Ok(()) => {
                let current = self.path.clone();
                let selected_name = self
                    .entries
                    .get(state.entry_idx)
                    .map(|entry| entry.name.clone());
                self.modal = None;
                self.load_dir_with_state(&current, selected_name.as_deref(), Some(self.offset));
                self.status_note = Some(String::from("text saved"));
            }
            Err(err) => {
                state.error = Some(String::from(err.as_str()));
                self.modal = Some(ModalState::TextEditor(state));
            }
        }
    }

    fn sync_modal_state(&mut self) {
        let layout = self.layout();
        let editor_rect = Self::centered_rect(layout, EDITOR_W, EDITOR_H);
        let text_rect = self.editor_text_rect(editor_rect);
        let visible_lines = ((text_rect.h - 12).max(12) as usize) / 12;
        if let Some(ModalState::TextEditor(state)) = self.modal.as_mut() {
            let (cursor_line, _) = Self::text_cursor_line_col(&state.text, state.cursor);
            if cursor_line < state.scroll_line {
                state.scroll_line = cursor_line;
            } else if cursor_line >= state.scroll_line + visible_lines {
                state.scroll_line = cursor_line.saturating_sub(visible_lines.saturating_sub(1));
            }
        }
    }

    fn text_lines(text: &str) -> Vec<String> {
        if text.is_empty() {
            return alloc::vec![String::new()];
        }
        text.split('\n').map(String::from).collect()
    }

    fn text_cursor_line_col(text: &str, cursor: usize) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (idx, ch) in text.chars().enumerate() {
            if idx >= cursor {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    fn text_cursor_from_line_col(text: &str, target_line: usize, target_col: usize) -> usize {
        let mut line = 0usize;
        let mut col = 0usize;
        let mut cursor = 0usize;
        for ch in text.chars() {
            if line == target_line && col == target_col {
                return cursor;
            }
            if ch == '\n' {
                if line == target_line {
                    return cursor;
                }
                line += 1;
                col = 0;
            } else if line == target_line {
                col += 1;
            }
            cursor += 1;
        }
        cursor
    }

    fn char_to_byte_index(text: &str, char_idx: usize) -> usize {
        if char_idx == 0 {
            return 0;
        }
        text.char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(text.len())
    }

    fn join_child_path(&self, name: &str) -> String {
        let mut path = self.path.clone();
        if !path.ends_with('/') {
            path.push('/');
        }
        path.push_str(name);
        path
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
        for item in self.sidebar_items() {
            match item.kind {
                SidebarItemKind::Section => {
                    self.put_str(18, y as usize, &item.label, FM_TEXT_MUTED);
                    y += 16;
                }
                SidebarItemKind::Link => {
                    self.draw_sidebar_item(
                        Rect {
                            x: 10,
                            y,
                            w: layout.sidebar_w - 20,
                            h: NAV_ROW_H,
                        },
                        &item,
                    );
                    y += NAV_ROW_H + 4;
                }
            }
        }
    }

    fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();
        let root_names = Self::root_directory_names();
        items.push(SidebarItem {
            label: String::from("This PC"),
            path: Some(String::from("/")),
            active: self.path == "/",
            kind: SidebarItemKind::Link,
            indent: 0,
            icon: SidebarIcon::Computer,
        });
        for label in QUICK_ACCESS_FOLDERS {
            let path = Self::shell_link_path_with_roots(label, &root_names);
            items.push(SidebarItem {
                label: String::from(label),
                active: self.path_matches_or_contains(&path),
                path: Some(path),
                kind: SidebarItemKind::Link,
                indent: 0,
                icon: SidebarIcon::Folder,
            });
        }

        let path_links = self.current_path_links();
        if !path_links.is_empty() {
            items.push(SidebarItem {
                label: String::from("CURRENT PATH"),
                path: None,
                active: false,
                kind: SidebarItemKind::Section,
                indent: 0,
                icon: SidebarIcon::Folder,
            });
            for (depth, label, path) in path_links {
                items.push(SidebarItem {
                    label,
                    active: self.path.eq_ignore_ascii_case(&path),
                    path: Some(path),
                    kind: SidebarItemKind::Link,
                    indent: 10 + depth as i32 * 12,
                    icon: SidebarIcon::Folder,
                });
            }
        }

        items
    }

    fn draw_sidebar_item(&mut self, rect: Rect, item: &SidebarItem) {
        if item.active {
            self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_SELECTION);
            self.fill_rect(rect.x, rect.y, 3, rect.h, FM_SELECTION_GLOW);
        }
        let icon_x = rect.x + 8 + item.indent;
        let icon_y = rect.y + 4;
        self.draw_sidebar_icon(icon_x, icon_y, item.icon, item.active);
        self.put_str(
            (icon_x + 16) as usize,
            (rect.y + 5) as usize,
            &item.label,
            if item.active { FM_TEXT } else { FM_TEXT_DIM },
        );
    }

    fn draw_sidebar_icon(&mut self, x: i32, y: i32, icon: SidebarIcon, active: bool) {
        match icon {
            SidebarIcon::Computer => {
                self.fill_rect(x, y + 4, 10, 6, if active { FM_TEXT } else { FM_DRIVE });
                self.fill_rect(
                    x + 1,
                    y + 2,
                    8,
                    3,
                    if active {
                        FM_SELECTION_GLOW
                    } else {
                        FM_ACCENT_SOFT
                    },
                );
                self.fill_rect(x + 3, y + 11, 4, 1, FM_TEXT_MUTED);
            }
            SidebarIcon::Folder => {
                self.fill_rect(x + 1, y, 6, 2, if active { FM_TEXT } else { FM_FOLDER });
                self.fill_rect(x, y + 2, 10, 6, if active { FM_TEXT } else { FM_FOLDER });
                self.fill_rect(
                    x + 1,
                    y + 4,
                    8,
                    3,
                    if active {
                        FM_SELECTION_GLOW
                    } else {
                        FM_FOLDER_SHADE
                    },
                );
            }
        }
    }

    fn draw_back_button(&mut self, rect: Rect) {
        let enabled = self.path != "/";
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if enabled { FM_BORDER } else { FM_BORDER_SOFT },
        );
        self.fill_rect(
            rect.x + 1,
            rect.y + 1,
            rect.w - 2,
            3,
            if enabled {
                FM_SELECTION_GLOW
            } else {
                FM_BORDER_SOFT
            },
        );
        self.put_str(
            (rect.x + 8) as usize,
            (rect.y + 7) as usize,
            "<",
            if enabled { FM_TEXT } else { FM_TEXT_MUTED },
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
        if let Some(detail) = self.detail_rect(layout) {
            self.fill_rect(detail.x - 7, detail.y, 1, detail.h, FM_BORDER_SOFT);
        }
    }

    fn title_y(&self) -> i32 {
        COMMAND_H + PATHBAR_H + 14
    }

    fn summary_cards_y(&self) -> i32 {
        self.title_y() + 34
    }

    fn section_start_y(&self) -> i32 {
        self.summary_cards_y() + self.summary_cards_height(self.layout()) + 18
    }

    fn detail_rect(&self, layout: Layout) -> Option<Rect> {
        if layout.main_w < 520 {
            return None;
        }
        let h = layout.status_y - COMMAND_H - PATHBAR_H - 20;
        Some(Rect {
            x: layout.main_x + layout.main_w - DETAIL_W - 14,
            y: COMMAND_H + PATHBAR_H + 10,
            w: DETAIL_W,
            h,
        })
    }

    fn content_left(&self, layout: Layout) -> i32 {
        layout.main_x + 18
    }

    fn content_right(&self, layout: Layout) -> i32 {
        if let Some(detail) = self.detail_rect(layout) {
            detail.x - DETAIL_GAP
        } else {
            layout.main_x + layout.main_w - 18
        }
    }

    fn content_width(&self, layout: Layout) -> i32 {
        (self.content_right(layout) - self.content_left(layout)).max(120)
    }

    fn summary_card_cols(&self, layout: Layout) -> i32 {
        let available_w = self.content_width(layout);
        if available_w >= 3 * 104 + SUMMARY_CARD_GAP * 2 {
            3
        } else if available_w >= 2 * 104 + SUMMARY_CARD_GAP {
            2
        } else {
            1
        }
    }

    fn summary_cards_height(&self, layout: Layout) -> i32 {
        let cols = self.summary_card_cols(layout).max(1);
        let rows = ((3 + cols - 1) / cols).max(1);
        rows * SUMMARY_CARD_H + (rows - 1) * SUMMARY_CARD_GAP
    }

    fn draw_summary_cards(
        &mut self,
        layout: Layout,
        y: i32,
        folder_count: usize,
        file_count: usize,
    ) {
        let row_x = self.content_left(layout);
        let available_w = self.content_width(layout);
        let cols = self.summary_card_cols(layout).max(1);
        let card_w = ((available_w - SUMMARY_CARD_GAP * (cols - 1)) / cols).max(72);

        let mut folder_value = String::new();
        fmt_push_u(&mut folder_value, folder_count as u64);
        let mut file_value = String::new();
        fmt_push_u(&mut file_value, file_count as u64);
        let mut sort_value = String::new();
        sort_value.push_str(self.sort_column.label());
        sort_value.push(' ');
        sort_value.push(if self.sort_desc { 'v' } else { '^' });
        let cards = [
            ("Folders", folder_value, FM_FOLDER),
            ("Files", file_value, FM_FILE),
            ("Sort", sort_value, FM_ACCENT),
        ];
        for (idx, (label, value, accent)) in cards.iter().enumerate() {
            let col = idx as i32 % cols;
            let row = idx as i32 / cols;
            self.draw_summary_card(
                Rect {
                    x: row_x + col * (card_w + SUMMARY_CARD_GAP),
                    y: y + row * (SUMMARY_CARD_H + SUMMARY_CARD_GAP),
                    w: card_w,
                    h: SUMMARY_CARD_H,
                },
                label,
                value,
                *accent,
            );
        }
    }

    fn draw_summary_card(&mut self, rect: Rect, label: &str, value: &str, accent: u32) {
        let max_chars = ((rect.w - 20).max(0) as usize) / CW;
        self.fill_rect(rect.x, rect.y, rect.w, rect.h, FM_PANEL);
        self.draw_rect_border(rect.x, rect.y, rect.w, rect.h, FM_BORDER);
        self.fill_rect(rect.x + 1, rect.y + 1, rect.w - 2, 3, accent);
        self.put_str(
            (rect.x + 10) as usize,
            (rect.y + 10) as usize,
            &Self::clip_text(label, max_chars),
            FM_TEXT_MUTED,
        );
        self.put_str(
            (rect.x + 10) as usize,
            (rect.y + 22) as usize,
            &Self::clip_text(value, max_chars),
            FM_TEXT,
        );
    }

    fn draw_detail_panel(&mut self, layout: Layout) {
        let detail = match self.detail_rect(layout) {
            Some(detail) => detail,
            None => return,
        };
        self.fill_rect(detail.x, detail.y, detail.w, detail.h, FM_PANEL);
        self.draw_rect_border(detail.x, detail.y, detail.w, detail.h, FM_BORDER);
        self.fill_rect(detail.x, detail.y, detail.w, 3, FM_SELECTION_GLOW);
        self.put_str(
            (detail.x + 12) as usize,
            (detail.y + 10) as usize,
            "DETAILS",
            FM_TEXT_MUTED,
        );

        if let Some(idx) = self.selected {
            if let Some((name, is_dir, size)) = self
                .entries
                .get(idx)
                .map(|entry| (entry.name.clone(), entry.is_dir, entry.size))
            {
                let full_path = self.make_abs(idx);
                let detail_type = self
                    .entries
                    .get(idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(idx)))
                    .unwrap_or("File");
                let size_text = if let Some(app) = self.desktop_app_for_idx(idx) {
                    let mut s = String::from("Launch ");
                    s.push_str(app);
                    s
                } else if is_dir {
                    String::from("Open container")
                } else {
                    Self::format_size(size)
                };

                self.draw_large_entry_icon(detail.x + 14, detail.y + 30, is_dir);
                self.put_str(
                    (detail.x + 56) as usize,
                    (detail.y + 34) as usize,
                    &Self::clip_text(&name, 14),
                    FM_TEXT,
                );
                self.put_str(
                    (detail.x + 56) as usize,
                    (detail.y + 48) as usize,
                    detail_type,
                    FM_TEXT_MUTED,
                );

                self.draw_detail_row(detail.x + 12, detail.y + 76, "Location", &full_path);
                self.draw_detail_row(detail.x + 12, detail.y + 108, "Size", &size_text);
                self.draw_detail_row(
                    detail.x + 12,
                    detail.y + 140,
                    "Action",
                    if self.desktop_app_for_idx(idx).is_some() {
                        "Launch application"
                    } else if is_dir {
                        "Open folder"
                    } else {
                        "Open or edit file"
                    },
                );

                let note = if self.desktop_app_for_idx(idx).is_some() {
                    "Desktop launchers mirror the shell icons."
                } else if is_dir {
                    "Folders stay in the shell view."
                } else {
                    "Files open with the default app."
                };
                self.put_str(
                    (detail.x + 12) as usize,
                    (detail.y + detail.h - 26) as usize,
                    &Self::clip_text(note, ((detail.w - 24).max(0) as usize) / CW),
                    FM_TEXT_MUTED,
                );
            }
        } else {
            let folders = self.folder_indices().len();
            let files = self.file_indices().len();
            let mut folder_text = String::new();
            fmt_push_u(&mut folder_text, folders as u64);
            folder_text.push_str(" folders");
            let mut file_text = String::new();
            fmt_push_u(&mut file_text, files as u64);
            file_text.push_str(" files");
            self.draw_large_entry_icon(detail.x + 14, detail.y + 30, true);
            self.put_str(
                (detail.x + 56) as usize,
                (detail.y + 34) as usize,
                if self.path == "/" {
                    "This PC"
                } else {
                    "Folder"
                },
                FM_TEXT,
            );
            self.put_str(
                (detail.x + 56) as usize,
                (detail.y + 48) as usize,
                &Self::clip_text(&self.path, 14),
                FM_TEXT_MUTED,
            );
            self.draw_detail_row(detail.x + 12, detail.y + 82, "Folders", &folder_text);
            self.draw_detail_row(detail.x + 12, detail.y + 114, "Files", &file_text);
            self.draw_detail_row(
                detail.x + 12,
                detail.y + 146,
                "Sort",
                self.sort_column.label(),
            );
        }
    }

    fn draw_detail_row(&mut self, x: i32, y: i32, label: &str, value: &str) {
        self.put_str(x as usize, y as usize, label, FM_TEXT_MUTED);
        self.put_str(
            x as usize,
            (y + 13) as usize,
            &Self::clip_text(value, 18),
            FM_TEXT_DIM,
        );
        self.fill_rect(x, y + 24, 150, 1, FM_BORDER_SOFT);
    }

    fn draw_large_entry_icon(&mut self, x: i32, y: i32, is_dir: bool) {
        if is_dir {
            self.fill_rect(x + 4, y, 14, 6, FM_FOLDER);
            self.fill_rect(x, y + 6, 26, 18, FM_FOLDER);
            self.fill_rect(x + 2, y + 11, 22, 9, FM_FOLDER_SHADE);
        } else {
            self.fill_rect(x + 2, y, 18, 24, FM_FILE);
            self.draw_rect_border(x + 2, y, 18, 24, blend(FM_FILE, BLACK, 120));
            self.fill_rect(x + 12, y, 8, 6, WHITE);
            self.fill_rect(x + 5, y + 10, 10, 2, blend(FM_FILE, WHITE, 110));
            self.fill_rect(x + 5, y + 14, 10, 2, blend(FM_FILE, WHITE, 110));
        }
    }

    fn draw_file_header(&mut self, layout: Layout, y: i32) {
        let columns = self.file_columns(layout);
        self.fill_rect(columns.row_x, y, columns.row_w, FILE_HEADER_H, FM_PANEL);
        self.draw_rect_border(
            columns.row_x,
            y,
            columns.row_w,
            FILE_HEADER_H,
            FM_BORDER_SOFT,
        );
        self.draw_sort_header_label(columns.name_x, y + 6, "Name", SortColumn::Name);
        self.draw_sort_header_label(columns.type_x, y + 6, "Type", SortColumn::Type);
        self.draw_sort_header_label(columns.size_x, y + 6, "Size", SortColumn::Size);
    }

    fn draw_sort_header_label(&mut self, x: i32, y: i32, label: &str, column: SortColumn) {
        let mut text = String::from(label);
        if self.sort_column == column {
            text.push(' ');
            text.push(if self.sort_desc { 'v' } else { '^' });
        }
        self.put_str(
            x as usize,
            y as usize,
            &text,
            if self.sort_column == column {
                FM_TEXT
            } else {
                FM_TEXT_MUTED
            },
        );
    }

    fn draw_root_overview(&mut self, layout: Layout) {
        let top = self.title_y();
        let content_left = self.content_left(layout);
        self.put_str(content_left as usize, top as usize, "This PC", FM_TEXT);
        self.put_str(
            content_left as usize,
            (top + 14) as usize,
            "coolOS shell view",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        self.draw_summary_cards(layout, self.summary_cards_y(), folders.len(), files.len());
        let section_y = self.section_start_y();
        let tiles_h = self.draw_folder_section(layout, section_y, &folders, true);
        let drives_y = section_y + tiles_h + 20;
        self.draw_drive_section(
            layout,
            drives_y,
            if files.is_empty() { &folders } else { &files },
        );
    }

    fn draw_directory_view(&mut self, layout: Layout) {
        let top = self.title_y();
        let content_left = self.content_left(layout);
        let title_chars = (self.content_width(layout).max(0) as usize) / CW;
        let title = if self.path == "/" {
            "This PC"
        } else {
            self.path.as_str()
        };
        self.put_str(
            content_left as usize,
            top as usize,
            &Self::clip_text(title, title_chars.max(8)),
            FM_TEXT,
        );
        self.put_str(
            content_left as usize,
            (top + 14) as usize,
            "folders first, files below",
            FM_TEXT_MUTED,
        );

        let folders = self.folder_indices();
        let files = self.file_indices();
        self.draw_summary_cards(layout, self.summary_cards_y(), folders.len(), files.len());
        let section_y = self.section_start_y();
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
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        let mut label = String::from(title);
        label.push(' ');
        label.push('(');
        fmt_push_u(&mut label, count as u64);
        label.push(')');

        self.put_str(content_left as usize, y as usize, &label, FM_TEXT_DIM);
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (y + 24) as usize,
                "(no folders)",
                FM_TEXT_MUTED,
            );
            return 40;
        }

        let tile_y = y + 22;
        let tile_w = ((content_w - 24 - TILE_GAP_X * 2) / 3).max(140);
        let cols = (content_w / (tile_w + TILE_GAP_X)).max(1) as usize;

        for (visual_idx, &entry_idx) in indices.iter().take(9).enumerate() {
            let col = (visual_idx % cols).min(2);
            let row = visual_idx / cols;
            let rect = Rect {
                x: content_left + col as i32 * (tile_w + TILE_GAP_X),
                y: tile_y + row as i32 * (TILE_H + TILE_GAP_Y),
                w: tile_w,
                h: TILE_H,
            };
            self.draw_folder_tile(rect, entry_idx);
        }

        if indices.len() > 9 {
            let mut more = String::from("+");
            fmt_push_u(&mut more, (indices.len() - 9) as u64);
            more.push_str(" more");
            self.put_str(
                (content_left + content_w - (more.len() as i32 * CW as i32) - 4).max(content_left)
                    as usize,
                y as usize,
                &more,
                FM_TEXT_MUTED,
            );
        }

        let rows = ((indices.len().min(9) + cols - 1) / cols).max(1) as i32;
        22 + rows * TILE_H + (rows - 1) * TILE_GAP_Y
    }

    fn draw_drive_section(&mut self, layout: Layout, y: i32, indices: &[usize]) {
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        self.put_str(
            content_left as usize,
            y as usize,
            "Devices and drives",
            FM_TEXT_DIM,
        );
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        if indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (y + 24) as usize,
                "(no items)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let card_w = ((content_w - 12) / 2).max(180);
        let cols = if content_w > 420 { 2 } else { 1 };
        let max_items = if cols == 2 { 4 } else { 3 };
        for (visual_idx, &entry_idx) in indices.iter().take(max_items).enumerate() {
            let col = (visual_idx % cols as usize) as i32;
            let row = (visual_idx / cols as usize) as i32;
            let rect = Rect {
                x: content_left + col * (card_w + 12),
                y: y + 22 + row * (DRIVE_H + DRIVE_GAP_Y),
                w: card_w,
                h: DRIVE_H,
            };
            self.draw_drive_card(rect, entry_idx);
        }

        if indices.len() > max_items {
            let mut more = String::from("+");
            fmt_push_u(&mut more, (indices.len() - max_items) as u64);
            more.push_str(" more");
            self.put_str(
                (content_left + content_w - (more.len() as i32 * CW as i32) - 4).max(content_left)
                    as usize,
                y as usize,
                &more,
                FM_TEXT_MUTED,
            );
        }
    }

    fn draw_file_list_section(&mut self, layout: Layout, y: i32, file_indices: &[usize]) {
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        self.put_str(content_left as usize, y as usize, "Files", FM_TEXT_DIM);
        self.fill_rect(
            content_left,
            y + SECTION_HDR_H - 2,
            content_w,
            1,
            FM_BORDER_SOFT,
        );

        let header_y = y + 22;
        self.draw_file_header(layout, header_y);

        let list_y = header_y + FILE_HEADER_H;
        let list_h = (layout.status_y - list_y - 10).max(0);
        self.view_h = list_h;
        self.total_rows = (list_h / LIST_ROW_H).max(0) as usize;
        self.window.scroll.content_h = file_indices.len() as i32 * LIST_ROW_H;
        self.window.scroll.offset = self.offset as i32 * LIST_ROW_H;
        self.window.scroll.clamp(list_h);

        if file_indices.is_empty() {
            self.put_str(
                (content_left + 10) as usize,
                (list_y + 8) as usize,
                "(no files)",
                FM_TEXT_MUTED,
            );
            return;
        }

        let visible = self.total_rows.max(1);
        let max_offset = file_indices.len().saturating_sub(visible);
        self.offset = self.offset.min(max_offset);
        let columns = self.file_columns(layout);
        let name_w = (columns.name_w.max(0) as usize) / CW;

        for visual_row in 0..visible {
            let idx_in_files = self.offset + visual_row;
            if idx_in_files >= file_indices.len() {
                break;
            }
            let entry_idx = file_indices[idx_in_files];
            let (full_name, size) = match self.entries.get(entry_idx) {
                Some(entry) => (entry.name.clone(), entry.size),
                None => continue,
            };
            let row_y = list_y + visual_row as i32 * LIST_ROW_H;
            let selected = self.selected == Some(entry_idx);
            self.fill_rect(
                columns.row_x,
                row_y,
                columns.row_w,
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
                self.fill_rect(columns.row_x, row_y, 3, LIST_ROW_H, FM_SELECTION_GLOW);
            }
            self.draw_file_icon((columns.row_x + 10) as usize, (row_y + 4) as usize);

            let name = Self::clip_text(&full_name, name_w);
            self.put_str(
                columns.name_x as usize,
                (row_y + 5) as usize,
                &name,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.put_str(
                columns.type_x as usize,
                (row_y + 5) as usize,
                self.entries
                    .get(entry_idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(entry_idx)))
                    .unwrap_or("File"),
                FM_TEXT_MUTED,
            );
            let size = Self::format_size(size);
            self.put_str(
                columns.size_x as usize,
                (row_y + 5) as usize,
                &size,
                if selected { FM_TEXT } else { FM_TEXT_DIM },
            );
            self.fill_rect(
                columns.row_x,
                row_y + LIST_ROW_H - 1,
                columns.row_w,
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

        let hint = self
            .status_note
            .clone()
            .unwrap_or_else(|| String::from("Mouse navigation only"));
        let hint_x = ((layout.width as usize).saturating_sub(hint.len() * CW)) / 2;
        self.put_str(hint_x, (layout.status_y + 6) as usize, &hint, FM_TEXT_MUTED);

        if let Some(idx) = self.selected {
            if let Some(entry) = self.entries.get(idx) {
                let entry_name = entry.name.clone();
                let entry_is_dir = entry.is_dir;
                let entry_size = entry.size;
                let mut right = String::from(&entry_name);
                right.push_str("  ");
                right.push_str(Self::type_label_for_kind(entry, self.entry_kind(idx)));
                if !entry_is_dir && self.desktop_app_for_idx(idx).is_none() {
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

    fn file_columns(&self, layout: Layout) -> FileColumns {
        let row_x = self.content_left(layout);
        let row_w = self.content_width(layout);
        let size_x = row_x + row_w - 84;
        let type_x = row_x + row_w - 180;
        let name_x = row_x + 28;
        let name_w = (type_x - name_x - 12).max(96);
        FileColumns {
            row_x,
            row_w,
            name_x,
            name_w,
            type_x,
            size_x,
        }
    }

    fn file_rows_y(&self, layout: Layout) -> i32 {
        let folders = self.folder_indices();
        let files_y = self.section_start_y() + self.folder_section_height(layout, &folders) + 18;
        files_y + 22 + FILE_HEADER_H
    }

    fn root_directory_names() -> Vec<String> {
        let mut names: Vec<String> = crate::fat32::list_dir("/")
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| entry.is_dir)
            .map(|entry| entry.name)
            .collect();
        names.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()));
        names
    }

    fn shell_link_path_with_roots(label: &str, root_names: &[String]) -> String {
        if let Some(name) = root_names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(label))
        {
            let mut path = String::from("/");
            path.push_str(name);
            return path;
        }

        if label.eq_ignore_ascii_case("Home") {
            return String::from("/");
        }

        let mut path = String::from("/");
        path.push_str(label);
        path
    }

    fn current_path_links(&self) -> Vec<(usize, String, String)> {
        let mut items = Vec::new();
        if self.path == "/" {
            return items;
        }

        let components: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();
        if components.len() <= 1 {
            return items;
        }

        let mut path = String::new();
        for (depth, component) in components.iter().enumerate() {
            path.push('/');
            path.push_str(component);
            if depth == 0 {
                continue;
            }
            items.push((depth - 1, String::from(*component), path.clone()));
        }
        items
    }

    fn path_matches_or_contains(&self, path: &str) -> bool {
        self.path.eq_ignore_ascii_case(path)
            || self
                .path
                .strip_prefix(path)
                .map(|suffix| suffix.starts_with('/'))
                .unwrap_or(false)
    }

    fn breadcrumb_segments(&self) -> Vec<(String, String)> {
        let mut segments = Vec::new();
        segments.push((String::from("This PC"), String::from("/")));
        if self.path == "/" {
            return segments;
        }

        let mut built = String::new();
        for component in self.path.split('/').filter(|s| !s.is_empty()) {
            built.push('/');
            built.push_str(component);
            segments.push((String::from(component), built.clone()));
        }
        segments
    }

    fn breadcrumb_segment_rects(&self, rect: Rect) -> Vec<(Rect, String, String)> {
        let mut out = Vec::new();
        let mut x = rect.x + 8;
        let y = rect.y + 3;
        let right = rect.x + rect.w - 8;
        let segments = self.breadcrumb_segments();
        let segment_len = segments.len();
        for (idx, (label, path)) in segments.into_iter().enumerate() {
            let seg_w = (label.len() as i32 * CW as i32 + BREAD_SEG_PAD * 2).max(44);
            if x + seg_w > right {
                break;
            }
            out.push((
                Rect {
                    x,
                    y,
                    w: seg_w,
                    h: 16,
                },
                label,
                path,
            ));
            x += seg_w;
            if idx + 1 < segment_len {
                x += BREAD_SEG_GAP + CW as i32;
            }
        }
        out
    }

    fn draw_breadcrumbs(&mut self, rect: Rect) {
        let segments = self.breadcrumb_segment_rects(rect);
        for (idx, (seg, label, _path)) in segments.iter().enumerate() {
            let active = idx + 1 == segments.len();
            self.fill_rect(
                seg.x,
                seg.y,
                seg.w,
                seg.h,
                if active { FM_SELECTION } else { FM_PANEL_ALT },
            );
            self.draw_rect_border(
                seg.x,
                seg.y,
                seg.w,
                seg.h,
                if active {
                    FM_SELECTION_GLOW
                } else {
                    FM_BORDER_SOFT
                },
            );
            self.put_str(
                (seg.x + BREAD_SEG_PAD) as usize,
                (seg.y + 4) as usize,
                &Self::clip_text(label, ((seg.w - BREAD_SEG_PAD * 2).max(0) as usize) / CW),
                if active { FM_TEXT } else { FM_TEXT_DIM },
            );
            if idx + 1 < segments.len() {
                self.put_str(
                    (seg.x + seg.w + 2) as usize,
                    (seg.y + 4) as usize,
                    ">",
                    FM_TEXT_MUTED,
                );
            }
        }
    }

    fn hit_breadcrumb(&self, lx: i32, ly: i32) -> Option<String> {
        let crumb = self.breadcrumb_rect();
        if !crumb.hit(lx, ly) {
            return None;
        }
        for (seg, _label, path) in self.breadcrumb_segment_rects(crumb) {
            if seg.hit(lx, ly) {
                return Some(path);
            }
        }
        None
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
                self.entries
                    .get(entry_idx)
                    .map(|entry| Self::type_label_for_kind(entry, self.entry_kind(entry_idx)))
                    .unwrap_or("File")
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
        let mut y = COMMAND_H + PATHBAR_H + 14;
        for item in self.sidebar_items() {
            match item.kind {
                SidebarItemKind::Section => y += 16,
                SidebarItemKind::Link => {
                    let rect = Rect {
                        x: 10,
                        y,
                        w: layout.sidebar_w - 20,
                        h: NAV_ROW_H,
                    };
                    if let Some(path) = item.path {
                        if rect.hit(lx, ly) {
                            return Some(path);
                        }
                    }
                    y += NAV_ROW_H + 4;
                }
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
        let section_y = self.section_start_y();
        if let Some(idx) = self.hit_folder_grid(layout, section_y, &folders, true, lx, ly) {
            return Some(idx);
        }
        let tiles_h = self.folder_section_height(layout, &folders);
        let drives_y = section_y + tiles_h + 20;
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
        let section_y = self.section_start_y();
        if let Some(idx) = self.hit_folder_grid(layout, section_y, &folders, false, lx, ly) {
            return Some(idx);
        }

        let files_y = section_y + self.folder_section_height(layout, &folders) + 18;
        let list_y = files_y + 22 + FILE_HEADER_H;
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

    fn hit_file_header_column(&self, lx: i32, ly: i32) -> Option<SortColumn> {
        if self.path == "/" {
            return None;
        }
        let layout = self.layout();
        let header_y = self.file_rows_y(layout) - FILE_HEADER_H;
        if ly < header_y || ly >= header_y + FILE_HEADER_H {
            return None;
        }
        let columns = self.file_columns(layout);
        if lx >= columns.name_x && lx < columns.type_x - 8 {
            Some(SortColumn::Name)
        } else if lx >= columns.type_x && lx < columns.size_x - 8 {
            Some(SortColumn::Type)
        } else if lx >= columns.size_x && lx < columns.row_x + columns.row_w {
            Some(SortColumn::Size)
        } else {
            None
        }
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
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        let tile_y = top + 22;
        let tile_w = ((content_w - 24 - TILE_GAP_X * 2) / 3).max(140);
        let cols = (content_w / (tile_w + TILE_GAP_X)).max(1) as usize;
        let max = if root_mode { 9 } else { indices.len().min(9) };
        for (visual_idx, &entry_idx) in indices.iter().take(max).enumerate() {
            let col = (visual_idx % cols).min(2);
            let row = visual_idx / cols;
            let rect = Rect {
                x: content_left + col as i32 * (tile_w + TILE_GAP_X),
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
        let content_left = self.content_left(layout);
        let content_w = self.content_width(layout);
        let card_w = ((content_w - 12) / 2).max(180);
        let cols = if content_w > 420 { 2 } else { 1 };
        let max_items = if cols == 2 { 4 } else { 3 };
        for (visual_idx, &entry_idx) in indices.iter().take(max_items).enumerate() {
            let col = (visual_idx % cols as usize) as i32;
            let row = (visual_idx / cols as usize) as i32;
            let rect = Rect {
                x: content_left + col * (card_w + 12),
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
        let content_w = self.content_width(layout);
        let tile_w = ((content_w - 24 - TILE_GAP_X * 2) / 3).max(140);
        let cols = (content_w / (tile_w + TILE_GAP_X)).max(1) as usize;
        let rows = ((indices.len().min(9) + cols - 1) / cols).max(1) as i32;
        22 + rows * TILE_H + (rows - 1) * TILE_GAP_Y
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
