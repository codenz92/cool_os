extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub struct Package {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub permissions: &'static str,
    pub installed: bool,
}

static INSTALLED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn init() {
    let _ = crate::fat32::create_dir("/APPS");
    let mut installed = Vec::new();
    for app in crate::app_metadata::APPS {
        installed.push(String::from(app.id));
        let dir = app_dir(app.command);
        let _ = crate::fat32::create_dir(&dir);
        let manifest = manifest_for(app);
        let path = app_manifest_path(app.command);
        let _ = crate::fat32::create_file(&path);
        let _ = crate::fat32::write_file(&path, manifest.as_bytes());
    }
    *INSTALLED.lock() = installed;
    crate::event_bus::emit("packages", "init", "built-in package manifests ready");
}

pub fn list() -> Vec<Package> {
    let installed = INSTALLED.lock();
    crate::app_metadata::APPS
        .iter()
        .map(|app| Package {
            id: app.id,
            name: app.name,
            version: "builtin",
            permissions: app.permission,
            installed: installed.iter().any(|id| id == app.id),
        })
        .collect()
}

pub fn lines() -> Vec<String> {
    list()
        .iter()
        .map(|pkg| {
            format!(
                "{} {} version={} perms={} {}",
                pkg.id,
                pkg.name,
                pkg.version,
                pkg.permissions,
                if pkg.installed {
                    "installed"
                } else {
                    "removed"
                }
            )
        })
        .collect()
}

pub fn install(id_or_command: &str) -> Result<(), &'static str> {
    if id_or_command.to_ascii_uppercase().ends_with(".PKG") || id_or_command.starts_with('/') {
        return install_archive(id_or_command);
    }
    let app = find_app(id_or_command).ok_or("unknown package")?;
    let mut installed = INSTALLED.lock();
    if !installed.iter().any(|id| id == app.id) {
        installed.push(String::from(app.id));
    }
    let dir = app_dir(app.command);
    let _ = crate::fat32::create_dir(&dir);
    let path = app_manifest_path(app.command);
    let _ = crate::fat32::create_file(&path);
    let _ = crate::fat32::write_file(&path, manifest_for(app).as_bytes());
    crate::event_bus::emit("packages", "install", app.id);
    Ok(())
}

pub fn install_archive(path: &str) -> Result<(), &'static str> {
    let bytes = crate::fat32::read_file(path).ok_or("package file not found")?;
    let text = core::str::from_utf8(&bytes).map_err(|_| "package is not UTF-8 manifest")?;
    let id = manifest_value(text, "id").ok_or("package missing id")?;
    let name = manifest_value(text, "name").unwrap_or(id);
    let command = manifest_value(text, "command").ok_or("package missing command")?;
    let icon = manifest_value(text, "icon").unwrap_or("PK");
    let permission = manifest_value(text, "permission").unwrap_or("user");
    if !id.starts_with("app.") || command.contains('/') || command.contains("..") {
        return Err("invalid package manifest");
    }
    let dir = app_dir(command);
    let _ = crate::fat32::create_dir(&dir);
    let manifest_path = app_manifest_path(command);
    let mut manifest = String::new();
    manifest.push_str("id=");
    manifest.push_str(id);
    manifest.push_str("\nname=");
    manifest.push_str(name);
    manifest.push_str("\ncommand=");
    manifest.push_str(command);
    manifest.push_str("\nicon=");
    manifest.push_str(icon);
    manifest.push_str("\npermission=");
    manifest.push_str(permission);
    manifest.push('\n');
    let _ = crate::fat32::create_file(&manifest_path);
    crate::fat32::safe_write_file(&manifest_path, manifest.as_bytes())
        .map_err(|_| "install write failed")?;
    let mut installed = INSTALLED.lock();
    if !installed.iter().any(|existing| existing == id) {
        installed.push(String::from(id));
    }
    crate::event_bus::emit("packages", "install-pkg", id);
    Ok(())
}

pub fn uninstall(id_or_command: &str) -> Result<(), &'static str> {
    let app = find_app(id_or_command).ok_or("unknown package")?;
    INSTALLED.lock().retain(|id| id != app.id);
    crate::event_bus::emit("packages", "remove", app.id);
    Ok(())
}

fn find_app(id_or_command: &str) -> Option<&'static crate::app_metadata::AppMetadata> {
    crate::app_metadata::APPS.iter().find(|app| {
        app.id.eq_ignore_ascii_case(id_or_command)
            || app.command.eq_ignore_ascii_case(id_or_command)
            || app.name.eq_ignore_ascii_case(id_or_command)
    })
}

fn app_dir(command: &str) -> String {
    let mut path = String::from("/APPS/");
    path.push_str(command);
    path
}

fn app_manifest_path(command: &str) -> String {
    let mut path = app_dir(command);
    path.push_str("/APP.CFG");
    path
}

fn manifest_for(app: &crate::app_metadata::AppMetadata) -> String {
    format!(
        "id={}\nname={}\ncommand={}\nicon={}\npermission={}\n",
        app.id, app.name, app.command, app.glyph, app.permission
    )
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
