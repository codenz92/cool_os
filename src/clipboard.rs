extern crate alloc;

use alloc::{string::String, vec::Vec};
use spin::Mutex;

#[derive(Clone)]
pub enum ClipboardPayload {
    Text {
        text: String,
        mime: &'static str,
    },
    Paths {
        paths: Vec<String>,
        cut: bool,
        mime: &'static str,
    },
    Image {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
        mime: &'static str,
    },
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
        payload: ClipboardPayload::Text {
            text: String::from(text),
            mime: "text/plain",
        },
    });
    crate::notifications::push("Clipboard", "text copied to shared clipboard");
}

pub fn set_paths(paths: Vec<String>, cut: bool) {
    *CLIPBOARD.lock() = Some(ClipboardState {
        tick: crate::interrupts::ticks(),
        payload: ClipboardPayload::Paths {
            paths,
            cut,
            mime: "application/x-coolos-file-list",
        },
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

pub fn set_image(width: u32, height: u32, bytes: Vec<u8>, mime: &'static str) {
    *CLIPBOARD.lock() = Some(ClipboardState {
        tick: crate::interrupts::ticks(),
        payload: ClipboardPayload::Image {
            width,
            height,
            bytes,
            mime,
        },
    });
    crate::notifications::push("Clipboard", "image copied to shared clipboard");
}

#[allow(dead_code)]
pub fn get() -> Option<ClipboardState> {
    CLIPBOARD.lock().clone()
}

pub fn get_text() -> Option<String> {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Text { text, .. }) => Some(text.clone()),
        _ => None,
    }
}

pub fn get_paths() -> Option<(Vec<String>, bool)> {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Paths { paths, cut, .. }) => Some((paths.clone(), *cut)),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn get_image() -> Option<(u32, u32, Vec<u8>, &'static str)> {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Image {
            width,
            height,
            bytes,
            mime,
        }) => Some((*width, *height, bytes.clone(), mime)),
        _ => None,
    }
}

pub fn mime_type() -> &'static str {
    match CLIPBOARD.lock().as_ref().map(|state| &state.payload) {
        Some(ClipboardPayload::Text { mime, .. }) => mime,
        Some(ClipboardPayload::Paths { mime, .. }) => mime,
        Some(ClipboardPayload::Image { mime, .. }) => mime,
        None => "empty",
    }
}

pub fn mime_lines() -> Vec<String> {
    alloc::vec![
        String::from("text/plain"),
        String::from("application/x-coolos-file-list"),
        String::from("image/rgba"),
        String::from("negotiation: apps query current MIME before paste/drop"),
        summary(),
    ]
}

pub fn summary() -> String {
    match CLIPBOARD.lock().as_ref() {
        Some(ClipboardState {
            payload: ClipboardPayload::Text { text, mime },
            ..
        }) => {
            let mut s = String::from(*mime);
            s.push(' ');
            push_usize(&mut s, text.len());
            s.push_str(" bytes");
            s
        }
        Some(ClipboardState {
            payload: ClipboardPayload::Paths { paths, cut, mime },
            ..
        }) => {
            let mut s = String::from(*mime);
            s.push(' ');
            push_usize(&mut s, paths.len());
            s.push_str(if *cut {
                " cut path(s)"
            } else {
                " copied path(s)"
            });
            s
        }
        Some(ClipboardState {
            payload:
                ClipboardPayload::Image {
                    width,
                    height,
                    bytes,
                    mime,
                },
            ..
        }) => {
            let mut s = String::from(*mime);
            s.push(' ');
            push_usize(&mut s, *width as usize);
            s.push('x');
            push_usize(&mut s, *height as usize);
            s.push(' ');
            push_usize(&mut s, bytes.len());
            s.push_str(" bytes");
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
