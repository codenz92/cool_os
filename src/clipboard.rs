extern crate alloc;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub enum ClipboardPayload {
    Text(String),
    Paths { paths: Vec<String>, cut: bool },
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct ClipboardState {
    pub tick: u64,
    pub payload: ClipboardPayload,
}

static CLIPBOARD: Mutex<Option<ClipboardState>> = Mutex::new(None);

pub fn set_text(text: &str) {
    *CLIPBOARD.lock() = Some(ClipboardState {
        tick: crate::interrupts::ticks(),
        payload: ClipboardPayload::Text(String::from(text)),
    });
    crate::notifications::push("Clipboard", "text copied to shared clipboard");
}

pub fn set_paths(paths: Vec<String>, cut: bool) {
    *CLIPBOARD.lock() = Some(ClipboardState {
        tick: crate::interrupts::ticks(),
        payload: ClipboardPayload::Paths { paths, cut },
    });
    crate::notifications::push(
        "Clipboard",
        if cut {
            "file paths cut to shared clipboard"
        } else {
            "file paths copied to shared clipboard"
        },
    );
}

#[allow(dead_code)]
pub fn get() -> Option<ClipboardState> {
    CLIPBOARD.lock().clone()
}

pub fn get_text() -> Option<String> {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Text(text)) => Some(text.clone()),
        _ => None,
    }
}

pub fn get_paths() -> Option<(Vec<String>, bool)> {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Paths { paths, cut }) => Some((paths.clone(), *cut)),
        _ => None,
    }
}

pub fn summary() -> String {
    match CLIPBOARD.lock().as_ref() {
        Some(ClipboardState {
            payload: ClipboardPayload::Text(text),
            ..
        }) => {
            let mut s = String::from("text ");
            push_usize(&mut s, text.len());
            s.push_str(" bytes");
            s
        }
        Some(ClipboardState {
            payload: ClipboardPayload::Paths { paths, cut },
            ..
        }) => {
            let mut s = String::new();
            push_usize(&mut s, paths.len());
            s.push_str(if *cut {
                " cut path(s)"
            } else {
                " copied path(s)"
            });
            s
        }
        None => String::from("empty"),
    }
}

fn push_usize(out: &mut String, mut value: usize) {
    if value == 0 {
        out.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        value /= 10;
        len += 1;
    }
    for idx in (0..len).rev() {
        out.push(digits[idx] as char);
    }
}
