extern crate alloc;

use alloc::{string::String, vec::Vec};

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct AppMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub glyph: &'static str,
    pub command: &'static str,
    pub category: AppCategory,
    pub permission: &'static str,
    pub aliases: &'static [&'static str],
    pub associations: &'static [&'static str],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppCategory {
    System,
    Files,
    Network,
    Tools,
    Games,
    Settings,
    Development,
}

impl AppCategory {
    pub const fn label(self) -> &'static str {
        match self {
            AppCategory::System => "System",
            AppCategory::Files => "Files",
            AppCategory::Network => "Network",
            AppCategory::Tools => "Tools",
            AppCategory::Games => "Games",
            AppCategory::Settings => "Settings",
            AppCategory::Development => "Development",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Association {
    Directory,
    Executable,
    Text,
    AppShortcut(&'static str),
    Unknown,
}

#[derive(Clone, Copy)]
pub enum LauncherKind {
    App(&'static str),
    Path(&'static str),
    Command(&'static str),
}

#[derive(Clone, Copy)]
pub struct LauncherEntry {
    pub label: &'static str,
    pub detail: &'static str,
    pub kind: LauncherKind,
}

#[derive(Clone)]
pub struct AppManifest {
    pub id: String,
    pub name: String,
    pub command: String,
    pub icon: String,
    pub category: String,
    pub permission: String,
    pub associations: Vec<String>,
}

pub const APPS: &[AppMetadata] = &[
    AppMetadata {
        id: "app.terminal",
        name: "Terminal",
        glyph: "T>",
        command: "terminal",
        category: AppCategory::System,
        permission: "shell",
        aliases: &["shell", "console", "cmd", "command"],
        associations: &["CMD"],
    },
    AppMetadata {
        id: "app.sysmon",
        name: "System Monitor",
        glyph: "M#",
        command: "sysmon",
        category: AppCategory::System,
        permission: "diagnostics",
        aliases: &["monitor", "tasks", "processes", "performance"],
        associations: &[],
    },
    AppMetadata {
        id: "app.files",
        name: "File Manager",
        glyph: "FM",
        command: "files",
        category: AppCategory::Files,
        permission: "filesystem",
        aliases: &["files", "folders", "explorer", "documents"],
        associations: &["DIR"],
    },
    AppMetadata {
        id: "app.viewer",
        name: "Text Viewer",
        glyph: "Tx",
        command: "viewer",
        category: AppCategory::Files,
        permission: "read-files",
        aliases: &["text", "notes", "readme", "viewer"],
        associations: &["TXT", "MD", "LOG", "CFG", "RS"],
    },
    AppMetadata {
        id: "app.colors",
        name: "Color Picker",
        glyph: "CP",
        command: "colors",
        category: AppCategory::Tools,
        permission: "desktop",
        aliases: &["colors", "palette", "theme"],
        associations: &[],
    },
    AppMetadata {
        id: "app.display",
        name: "Display Settings",
        glyph: "DS",
        command: "display",
        category: AppCategory::Settings,
        permission: "settings",
        aliases: &[
            "settings",
            "display",
            "accessibility",
            "network",
            "storage",
            "power",
        ],
        associations: &[],
    },
    AppMetadata {
        id: "app.personalize",
        name: "Personalize",
        glyph: "P*",
        command: "personalize",
        category: AppCategory::Settings,
        permission: "settings",
        aliases: &["wallpaper", "theme", "desktop"],
        associations: &[],
    },
    AppMetadata {
        id: "app.crash",
        name: "Crash Viewer",
        glyph: "CV",
        command: "crash",
        category: AppCategory::System,
        permission: "diagnostics",
        aliases: &["crash", "dump", "fault", "panic"],
        associations: &["DMP"],
    },
    AppMetadata {
        id: "app.logs",
        name: "Log Viewer",
        glyph: "LV",
        command: "logs",
        category: AppCategory::System,
        permission: "diagnostics",
        aliases: &["logs", "kernel", "services", "events"],
        associations: &["LOG"],
    },
    AppMetadata {
        id: "app.profiler",
        name: "Boot Profiler",
        glyph: "BP",
        command: "profiler",
        category: AppCategory::System,
        permission: "diagnostics",
        aliases: &["boot", "profiler", "startup", "timing"],
        associations: &[],
    },
    AppMetadata {
        id: "app.welcome",
        name: "Welcome",
        glyph: "W?",
        command: "welcome",
        category: AppCategory::System,
        permission: "desktop",
        aliases: &["help", "cheatsheet", "shortcuts"],
        associations: &[],
    },
];

pub const APP_CATEGORIES: &[AppCategory] = &[
    AppCategory::System,
    AppCategory::Files,
    AppCategory::Network,
    AppCategory::Tools,
    AppCategory::Games,
    AppCategory::Settings,
    AppCategory::Development,
];

pub const LAUNCHER_ENTRIES: &[LauncherEntry] = &[
    LauncherEntry {
        label: "Terminal",
        detail: "open shell",
        kind: LauncherKind::App("Terminal"),
    },
    LauncherEntry {
        label: "Files",
        detail: "open File Manager",
        kind: LauncherKind::App("File Manager"),
    },
    LauncherEntry {
        label: "System Monitor",
        detail: "runtime dashboard",
        kind: LauncherKind::App("System Monitor"),
    },
    LauncherEntry {
        label: "Display Settings",
        detail: "desktop settings",
        kind: LauncherKind::App("Display Settings"),
    },
    LauncherEntry {
        label: "Personalize",
        detail: "wallpaper presets",
        kind: LauncherKind::App("Personalize"),
    },
    LauncherEntry {
        label: "Text Viewer",
        detail: "open text viewer",
        kind: LauncherKind::App("Text Viewer"),
    },
    LauncherEntry {
        label: "Color Picker",
        detail: "open palette",
        kind: LauncherKind::App("Color Picker"),
    },
    LauncherEntry {
        label: "Crash Viewer",
        detail: "open crash reports",
        kind: LauncherKind::App("Crash Viewer"),
    },
    LauncherEntry {
        label: "Log Viewer",
        detail: "kernel/service/filesystem logs",
        kind: LauncherKind::App("Log Viewer"),
    },
    LauncherEntry {
        label: "Boot Profiler",
        detail: "boot phases and service timing",
        kind: LauncherKind::App("Boot Profiler"),
    },
    LauncherEntry {
        label: "Welcome",
        detail: "shortcut cheatsheet",
        kind: LauncherKind::App("Welcome"),
    },
    LauncherEntry {
        label: "hello.txt",
        detail: "/bin/hello.txt",
        kind: LauncherKind::Path("/bin/hello.txt"),
    },
    LauncherEntry {
        label: "Documents",
        detail: "/Documents",
        kind: LauncherKind::Path("/Documents"),
    },
    LauncherEntry {
        label: "Desktop",
        detail: "/Desktop",
        kind: LauncherKind::Path("/Desktop"),
    },
    LauncherEntry {
        label: "Trash",
        detail: "/Trash",
        kind: LauncherKind::Path("/Trash"),
    },
    LauncherEntry {
        label: "Run ps",
        detail: "terminal command",
        kind: LauncherKind::Command("ps"),
    },
    LauncherEntry {
        label: "Run devices",
        detail: "terminal command",
        kind: LauncherKind::Command("devices"),
    },
    LauncherEntry {
        label: "Run net",
        detail: "terminal command",
        kind: LauncherKind::Command("net"),
    },
    LauncherEntry {
        label: "Run fsck",
        detail: "terminal command",
        kind: LauncherKind::Command("fsck"),
    },
    LauncherEntry {
        label: "Run log",
        detail: "terminal command",
        kind: LauncherKind::Command("log"),
    },
];

#[allow(dead_code)]
pub fn app_by_command(command: &str) -> Option<&'static AppMetadata> {
    APPS.iter()
        .find(|app| app.command.eq_ignore_ascii_case(command))
}

pub fn app_by_name(name: &str) -> Option<&'static AppMetadata> {
    APPS.iter().find(|app| app.name.eq_ignore_ascii_case(name))
}

pub fn app_by_id_or_command(value: &str) -> Option<&'static AppMetadata> {
    APPS.iter().find(|app| {
        app.id.eq_ignore_ascii_case(value)
            || app.command.eq_ignore_ascii_case(value)
            || app.name.eq_ignore_ascii_case(value)
    })
}

pub fn installed_app_manifests() -> Vec<AppManifest> {
    let Some(dirs) = crate::fat32::list_dir("/APPS") else {
        return Vec::new();
    };
    let mut manifests = Vec::new();
    for dir in dirs.iter().filter(|entry| entry.is_dir).take(32) {
        let mut path = String::from("/APPS/");
        path.push_str(&dir.name);
        path.push_str("/APP.CFG");
        let Some(bytes) = crate::fat32::read_file(&path) else {
            continue;
        };
        let Ok(text) = core::str::from_utf8(&bytes) else {
            continue;
        };
        let id = manifest_value(text, "id").unwrap_or("app.unknown");
        let name = manifest_value(text, "name").unwrap_or(&dir.name);
        let command = manifest_value(text, "command").unwrap_or(&dir.name);
        let icon = manifest_value(text, "icon").unwrap_or("[]");
        let category = manifest_value(text, "category").unwrap_or("Tools");
        let permission = manifest_value(text, "permission").unwrap_or("user");
        let associations = manifest_value(text, "associations")
            .map(parse_manifest_list)
            .unwrap_or_default();
        manifests.push(AppManifest {
            id: String::from(id),
            name: String::from(name),
            command: String::from(command),
            icon: String::from(icon),
            category: String::from(category),
            permission: String::from(permission),
            associations,
        });
    }
    manifests
}

pub fn category_lines() -> Vec<String> {
    let mut lines = Vec::new();
    for category in APP_CATEGORIES {
        let mut line = String::from(category.label());
        line.push_str(": ");
        let mut count = 0usize;
        for app in APPS.iter().filter(|app| app.category == *category) {
            if count > 0 {
                line.push_str(", ");
            }
            line.push_str(app.name);
            count += 1;
        }
        if count == 0 {
            line.push_str("(empty)");
        }
        lines.push(line);
    }
    lines
}

pub fn association_for(path: &str, is_dir: bool) -> Association {
    if is_dir {
        return Association::Directory;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    for app in APPS {
        if name.eq_ignore_ascii_case(app.name) || name.eq_ignore_ascii_case(app.command) {
            return Association::AppShortcut(app.name);
        }
    }
    let ext = file_ext(name);
    if ext.eq_ignore_ascii_case("ELF") {
        return Association::Executable;
    }
    if is_text_extension(ext) {
        return Association::Text;
    }
    for app in APPS {
        if matches_ignore_ascii(ext, app.associations) {
            return Association::AppShortcut(app.name);
        }
    }
    Association::Unknown
}

pub fn is_text_extension(ext: &str) -> bool {
    matches_ignore_ascii(ext, &["TXT", "MD", "LOG", "CFG", "RS"])
}

fn file_ext(name: &str) -> &str {
    name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn matches_ignore_ascii(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}

fn manifest_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(key) {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn parse_manifest_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}
