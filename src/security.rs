extern crate alloc;

use alloc::{format, string::String, vec::Vec};

pub struct User {
    pub name: &'static str,
    pub role: &'static str,
}

pub fn init() {
    crate::event_bus::emit("security", "init", "single-user admin profile active");
}

pub fn current_user() -> User {
    User {
        name: "user",
        role: "admin",
    }
}

pub fn is_protected_path(path: &str) -> bool {
    let upper = path.to_ascii_uppercase();
    upper == "/"
        || upper == "/CONFIG"
        || upper == "/LOGS"
        || upper == "/DEV"
        || upper == "/APPS"
        || upper.starts_with("/CONFIG/")
        || upper.starts_with("/LOGS/")
        || upper.starts_with("/DEV/")
}

pub fn can_write_path(path: &str) -> bool {
    !is_protected_path(path)
}

pub fn can_read_path(_path: &str) -> bool {
    true
}

pub fn app_permission_lines() -> Vec<String> {
    crate::app_metadata::APPS
        .iter()
        .map(|app| {
            format!(
                "{} id={} permission={} command={}",
                app.name, app.id, app.permission, app.command
            )
        })
        .collect()
}

pub fn lines() -> Vec<String> {
    let user = current_user();
    let mut lines = alloc::vec![
        format!("current user={} role={}", user.name, user.role),
        String::from("protected paths: /CONFIG /LOGS /DEV /APPS"),
        String::from("capabilities: app metadata enforced by shell policy"),
    ];
    lines.extend(app_permission_lines());
    lines
}
