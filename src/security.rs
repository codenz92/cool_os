extern crate alloc;

use alloc::{format, string::String, vec::Vec};

pub struct User {
    pub name: &'static str,
    pub role: &'static str,
    pub uid: u32,
    pub gid: u32,
}

#[allow(dead_code)]
pub struct Group {
    pub name: &'static str,
    pub gid: u32,
}

pub fn init() {
    crate::event_bus::emit("security", "init", "single-user admin profile active");
}

pub fn current_user() -> User {
    User {
        name: "user",
        role: "admin",
        uid: 1000,
        gid: 1000,
    }
}

#[allow(dead_code)]
pub fn groups() -> &'static [Group] {
    &[
        Group {
            name: "users",
            gid: 1000,
        },
        Group {
            name: "wheel",
            gid: 10,
        },
    ]
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

pub fn app_permission_for(name: &str) -> Option<&'static str> {
    crate::app_metadata::app_by_name(name).map(|app| app.permission)
}

pub fn lines() -> Vec<String> {
    let user = current_user();
    let mut lines = alloc::vec![
        format!(
            "current user={} uid={} gid={} role={}",
            user.name, user.uid, user.gid, user.role
        ),
        String::from("groups: users(1000), wheel(10)"),
        String::from("protected paths: /CONFIG /LOGS /DEV /APPS enforced in VFS write wrappers"),
        String::from(
            "capabilities: app metadata surfaced before launch and enforced by shell policy"
        ),
    ];
    lines.extend(app_permission_lines());
    lines
}
