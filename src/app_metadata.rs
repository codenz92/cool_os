#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct AppMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub glyph: &'static str,
    pub command: &'static str,
    pub permission: &'static str,
    pub associations: &'static [&'static str],
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

pub const APPS: &[AppMetadata] = &[
    AppMetadata {
        id: "app.terminal",
        name: "Terminal",
        glyph: "T>",
        command: "terminal",
        permission: "shell",
        associations: &["CMD"],
    },
    AppMetadata {
        id: "app.sysmon",
        name: "System Monitor",
        glyph: "M#",
        command: "sysmon",
        permission: "diagnostics",
        associations: &[],
    },
    AppMetadata {
        id: "app.files",
        name: "File Manager",
        glyph: "FM",
        command: "files",
        permission: "filesystem",
        associations: &["DIR"],
    },
    AppMetadata {
        id: "app.viewer",
        name: "Text Viewer",
        glyph: "Tx",
        command: "viewer",
        permission: "read-files",
        associations: &["TXT", "MD", "LOG", "CFG", "RS"],
    },
    AppMetadata {
        id: "app.colors",
        name: "Color Picker",
        glyph: "CP",
        command: "colors",
        permission: "desktop",
        associations: &[],
    },
    AppMetadata {
        id: "app.display",
        name: "Display Settings",
        glyph: "DS",
        command: "display",
        permission: "settings",
        associations: &[],
    },
    AppMetadata {
        id: "app.personalize",
        name: "Personalize",
        glyph: "P*",
        command: "personalize",
        permission: "settings",
        associations: &[],
    },
    AppMetadata {
        id: "app.crash",
        name: "Crash Viewer",
        glyph: "CV",
        command: "crash",
        permission: "diagnostics",
        associations: &["DMP"],
    },
    AppMetadata {
        id: "app.logs",
        name: "Log Viewer",
        glyph: "LV",
        command: "logs",
        permission: "diagnostics",
        associations: &["LOG"],
    },
    AppMetadata {
        id: "app.profiler",
        name: "Boot Profiler",
        glyph: "BP",
        command: "profiler",
        permission: "diagnostics",
        associations: &[],
    },
    AppMetadata {
        id: "app.welcome",
        name: "Welcome",
        glyph: "W?",
        command: "welcome",
        permission: "desktop",
        associations: &[],
    },
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
