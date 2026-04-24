//! Lock-free keyboard ring buffer.
//!
//! The PS/2 keyboard IRQ handler and the USB HID runtime both push decoded
//! chars here without touching the WM lock. The compositor drains the buffer
//! at the start of each frame while it already holds the WM lock. This
//! prevents the classic interrupt-context deadlock: IRQ fires while
//! compose() holds WM.lock(), IRQ tries to acquire WM.lock(), single-core
//! deadlock.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, KeyCode, KeyEvent, KeyState, Keyboard, ScancodeSet1};
use spin::Mutex;
use x86_64::instructions::port::Port;

const QUEUE_SIZE: usize = 64;
const USB_DEBUG_LOGS: bool = option_env!("COOLOS_XHCI_ACTIVE_INIT").is_some();

const ZERO: AtomicU32 = AtomicU32::new(0);
static QUEUE: [AtomicU32; QUEUE_SIZE] = [ZERO; QUEUE_SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0); // written by IRQ handler
static TAIL: AtomicUsize = AtomicUsize::new(0); // read  by main loop
static USB_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);

lazy_static! {
    static ref USB_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
        Mutex::new(Keyboard::<layouts::Us104Key, ScancodeSet1>::new(
            HandleControl::MapLettersToUnicode
        ));
    static ref USB_PREV_REPORT: Mutex<[u8; 8]> = Mutex::new([0; 8]);
}

/// Push a character from interrupt context. Silently drops if the buffer is full.
pub fn push(c: char) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE_SIZE;
    if next == TAIL.load(Ordering::Acquire) {
        return; // full — drop
    }
    QUEUE[head].store(c as u32, Ordering::Relaxed);
    HEAD.store(next, Ordering::Release);
}

/// Pop a character from the main loop. Returns `None` when empty.
pub fn pop() -> Option<char> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let v = QUEUE[tail].load(Ordering::Relaxed);
    TAIL.store((tail + 1) % QUEUE_SIZE, Ordering::Release);
    char::from_u32(v)
}

/// Enable the PS/2 keyboard IRQ as a fallback when no USB keyboard is active.
pub fn enable_ps2_fallback() {
    unsafe {
        let mut pic1_mask: Port<u8> = Port::new(0x21);
        let mask = pic1_mask.read();
        pic1_mask.write(mask & !(1 << 1));
    }
}

pub fn handle_usb_boot_report(report: &[u8; 8]) {
    // HID boot keyboards report rollover with usages 0x01..=0x03.
    if report[2..].iter().any(|usage| (1..=3).contains(usage)) {
        return;
    }

    let mut prev = USB_PREV_REPORT.lock();

    for bit in 0..8 {
        let mask = 1u8 << bit;
        let was_down = prev[0] & mask != 0;
        let is_down = report[0] & mask != 0;
        if was_down != is_down {
            if let Some(code) = usb_modifier_keycode(bit) {
                handle_usb_key_event(code, if is_down { KeyState::Down } else { KeyState::Up });
            }
        }
    }

    for &usage in report[2..].iter() {
        if usage == 0 || prev[2..].contains(&usage) {
            continue;
        }
        if let Some(code) = usb_usage_to_keycode(usage) {
            handle_usb_key_event(code, KeyState::Down);
        }
    }

    for &usage in prev[2..].iter() {
        if usage == 0 || report[2..].contains(&usage) {
            continue;
        }
        if let Some(code) = usb_usage_to_keycode(usage) {
            handle_usb_key_event(code, KeyState::Up);
        }
    }

    *prev = *report;
}

fn handle_usb_key_event(code: KeyCode, state: KeyState) {
    let mut keyboard = USB_KEYBOARD.lock();
    if let Some(decoded) = keyboard.process_keyevent(KeyEvent::new(code, state)) {
        if let DecodedKey::Unicode(c) = decoded {
            if USB_DEBUG_LOGS && USB_LOG_COUNT.fetch_add(1, Ordering::Relaxed) < 8 {
                crate::println!("[usb-kbd] {:?}", c);
            }
            push(c);
            crate::wm::request_repaint();
        }
    }
}

fn usb_modifier_keycode(bit: usize) -> Option<KeyCode> {
    match bit {
        0 => Some(KeyCode::ControlLeft),
        1 => Some(KeyCode::ShiftLeft),
        2 => Some(KeyCode::AltLeft),
        4 => Some(KeyCode::ControlRight),
        5 => Some(KeyCode::ShiftRight),
        6 => Some(KeyCode::AltRight),
        _ => None,
    }
}

fn usb_usage_to_keycode(usage: u8) -> Option<KeyCode> {
    Some(match usage {
        0x04 => KeyCode::A,
        0x05 => KeyCode::B,
        0x06 => KeyCode::C,
        0x07 => KeyCode::D,
        0x08 => KeyCode::E,
        0x09 => KeyCode::F,
        0x0A => KeyCode::G,
        0x0B => KeyCode::H,
        0x0C => KeyCode::I,
        0x0D => KeyCode::J,
        0x0E => KeyCode::K,
        0x0F => KeyCode::L,
        0x10 => KeyCode::M,
        0x11 => KeyCode::N,
        0x12 => KeyCode::O,
        0x13 => KeyCode::P,
        0x14 => KeyCode::Q,
        0x15 => KeyCode::R,
        0x16 => KeyCode::S,
        0x17 => KeyCode::T,
        0x18 => KeyCode::U,
        0x19 => KeyCode::V,
        0x1A => KeyCode::W,
        0x1B => KeyCode::X,
        0x1C => KeyCode::Y,
        0x1D => KeyCode::Z,
        0x1E => KeyCode::Key1,
        0x1F => KeyCode::Key2,
        0x20 => KeyCode::Key3,
        0x21 => KeyCode::Key4,
        0x22 => KeyCode::Key5,
        0x23 => KeyCode::Key6,
        0x24 => KeyCode::Key7,
        0x25 => KeyCode::Key8,
        0x26 => KeyCode::Key9,
        0x27 => KeyCode::Key0,
        0x28 => KeyCode::Enter,
        0x29 => KeyCode::Escape,
        0x2A => KeyCode::Backspace,
        0x2B => KeyCode::Tab,
        0x2C => KeyCode::Spacebar,
        0x2D => KeyCode::Minus,
        0x2E => KeyCode::Equals,
        0x2F => KeyCode::BracketSquareLeft,
        0x30 => KeyCode::BracketSquareRight,
        0x31 => KeyCode::BackSlash,
        0x32 => KeyCode::HashTilde,
        0x33 => KeyCode::SemiColon,
        0x34 => KeyCode::Quote,
        0x35 => KeyCode::BackTick,
        0x36 => KeyCode::Comma,
        0x37 => KeyCode::Fullstop,
        0x38 => KeyCode::Slash,
        0x39 => KeyCode::CapsLock,
        0x3A => KeyCode::F1,
        0x3B => KeyCode::F2,
        0x3C => KeyCode::F3,
        0x3D => KeyCode::F4,
        0x3E => KeyCode::F5,
        0x3F => KeyCode::F6,
        0x40 => KeyCode::F7,
        0x41 => KeyCode::F8,
        0x42 => KeyCode::F9,
        0x43 => KeyCode::F10,
        0x44 => KeyCode::F11,
        0x45 => KeyCode::F12,
        0x46 => KeyCode::PrintScreen,
        0x47 => KeyCode::ScrollLock,
        0x48 => KeyCode::PauseBreak,
        0x49 => KeyCode::Insert,
        0x4A => KeyCode::Home,
        0x4B => KeyCode::PageUp,
        0x4C => KeyCode::Delete,
        0x4D => KeyCode::End,
        0x4E => KeyCode::PageDown,
        0x4F => KeyCode::ArrowRight,
        0x50 => KeyCode::ArrowLeft,
        0x51 => KeyCode::ArrowDown,
        0x52 => KeyCode::ArrowUp,
        0x53 => KeyCode::NumpadLock,
        0x54 => KeyCode::NumpadSlash,
        0x55 => KeyCode::NumpadStar,
        0x56 => KeyCode::NumpadMinus,
        0x57 => KeyCode::NumpadPlus,
        0x58 => KeyCode::NumpadEnter,
        0x59 => KeyCode::Numpad1,
        0x5A => KeyCode::Numpad2,
        0x5B => KeyCode::Numpad3,
        0x5C => KeyCode::Numpad4,
        0x5D => KeyCode::Numpad5,
        0x5E => KeyCode::Numpad6,
        0x5F => KeyCode::Numpad7,
        0x60 => KeyCode::Numpad8,
        0x61 => KeyCode::Numpad9,
        0x62 => KeyCode::Numpad0,
        0x63 => KeyCode::NumpadPeriod,
        0x64 => KeyCode::BackSlash,
        _ => return None,
    })
}
