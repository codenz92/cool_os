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
const MENU_W: i32 = 154;
const MENU_ROW_H: i32 = 20;
const DIALOG_W: i32 = 320;
const DIALOG_H: i32 = 140;
const EDITOR_W: i32 = 468;
const EDITOR_H: i32 = 286;
const PROPERTIES_W: i32 = 380;
const PROPERTIES_H: i32 = 248;
const CONFIRM_W: i32 = 360;
const CONFIRM_H: i32 = 154;
const CONFLICT_W: i32 = 420;
const CONFLICT_H: i32 = 174;
const TRASH_PATH: &str = "/Trash";
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
    Copy,
    Cut,
    Paste,
    NewFile,
    NewFolder,
    Rename,
    EditText,
    Delete,
    Duplicate,
    Properties,
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
struct ConfirmDialogState {
    title: String,
    message: String,
    confirm_label: String,
    cancel_label: String,
    action: ConfirmAction,
}

#[derive(Clone)]
enum ConfirmAction {
    Trash(Vec<FileTarget>),
    Delete(Vec<FileTarget>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConflictPolicy {
    Replace,
    Skip,
    Rename,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileOperationKind {
    Copy,
    Move,
}

impl FileOperationKind {
    fn past_tense(self) -> &'static str {
        match self {
            FileOperationKind::Copy => "pasted",
            FileOperationKind::Move => "moved",
        }
    }
}

#[derive(Clone)]
enum FileOperationStep {
    CreateDir(String),
    CopyFile { src: String, dst: String },
    Delete { path: String },
}

struct FileOperationState {
    job: u64,
    kind: FileOperationKind,
    steps: Vec<FileOperationStep>,
    step_idx: usize,
    target_count: usize,
    selected_name: Option<String>,
}

#[derive(Clone)]
struct ConflictDialogState {
    clipboard: ClipboardState,
    name: String,
}

#[derive(Clone)]
struct PropertiesState {
    target: Option<FileTarget>,
    path: String,
    name: String,
    kind: String,
    size: u32,
    recursive_size: Option<u64>,
    child_count: Option<usize>,
    note: String,
}

#[derive(Clone)]
enum ModalState {
    Name(NameDialogState),
    TextEditor(TextEditorState),
    Confirm(ConfirmDialogState),
    Conflict(ConflictDialogState),
    Properties(PropertiesState),
}

#[derive(Clone)]
struct FileTarget {
    path: String,
    name: String,
    is_dir: bool,
}

#[derive(Clone)]
struct ClipboardState {
    entries: Vec<FileTarget>,
    cut: bool,
}

pub enum FileManagerOpenRequest {
    File(String),
    Exec(String),
    App(&'static str),
}

pub struct FileManagerApp {
    pub window: Window,
    entries: Vec<DirEntryInfo>,
    entry_kinds: Vec<EntryKind>,
    entry_paths: Vec<String>,
    entry_child_counts: Vec<usize>,
    all_entries: Vec<DirEntryInfo>,
    all_entry_kinds: Vec<EntryKind>,
    all_entry_paths: Vec<String>,
    path: String,
    offset: usize,
    view_h: i32,
    selected: Vec<usize>,
    focused: Option<usize>,
    total_rows: usize,
    pending_open: Option<FileManagerOpenRequest>,
    status_note: Option<String>,
    last_width: i32,
    last_height: i32,
    sort_column: SortColumn,
    sort_desc: bool,
    context_menu: Option<ContextMenuState>,
    modal: Option<ModalState>,
    back_stack: Vec<String>,
    forward_stack: Vec<String>,
    tabs: Vec<String>,
    active_tab: usize,
    split_view: bool,
    search_filter: String,
    search_active: bool,
    clipboard: Option<ClipboardState>,
    active_file_op: Option<FileOperationState>,
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
            entry_paths: Vec::new(),
            entry_child_counts: Vec::new(),
            all_entries: Vec::new(),
            all_entry_kinds: Vec::new(),
            all_entry_paths: Vec::new(),
            path: String::from("/"),
            offset: 0,
            view_h: 0,
            selected: Vec::new(),
            focused: None,
            total_rows: 0,
            pending_open: None,
            status_note: None,
            last_width: FILEMAN_W,
            last_height: FILEMAN_H,
            sort_column: SortColumn::Name,
            sort_desc: false,
            context_menu: None,
            modal: None,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            tabs: alloc::vec![String::from(dir)],
            active_tab: 0,
            split_view: false,
            search_filter: String::new(),
            search_active: false,
            clipboard: None,
            active_file_op: None,
        };
        app.load_dir(dir);
        app
    }

    pub const START_MENU_LINKS: [&'static str; 9] = START_MENU_FOLDERS;

    pub fn shell_link_path(label: &str) -> String {
        let root_names = Self::root_directory_names();
        Self::shell_link_path_with_roots(label, &root_names)
    }

    pub fn current_path(&self) -> &str {
        &self.path
    }

}
