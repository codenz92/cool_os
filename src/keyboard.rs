//! Lock-free keyboard ring buffer.
//!
//! The PS/2 keyboard IRQ handler and the USB HID runtime both push decoded
//! chars here without touching the WM lock. The compositor drains the buffer
//! at the start of each frame while it already holds the WM lock. This
//! prevents the classic interrupt-context deadlock: IRQ fires while
//! compose() holds WM.lock(), IRQ tries to acquire WM.lock(), single-core
//! deadlock.

use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize, Ordering};
use lazy_static::lazy_static;
use pc_keyboard::{
    layouts, DecodedKey, HandleControl, KeyCode, KeyEvent, KeyState, Keyboard, ScancodeSet1,
};
use spin::Mutex;
use x86_64::instructions::port::Port;

const QUEUE_SIZE: usize = 64;
const USB_DEBUG_LOGS: bool = option_env!("COOLOS_XHCI_ACTIVE_INIT").is_some();

pub const MOD_SHIFT: u8 = 1 << 0;
pub const MOD_CTRL: u8 = 1 << 1;
pub const MOD_ALT: u8 = 1 << 2;

const EVENT_KIND_CHAR: u32 = 1;
const EVENT_KIND_KEY: u32 = 2;
const EVENT_KIND_SHIFT: u32 = 24;
const EVENT_MOD_SHIFT: u32 = 21;
const EVENT_PAYLOAD_MASK: u32 = 0x001F_FFFF;

const ZERO: AtomicU32 = AtomicU32::new(0);
static QUEUE: [AtomicU32; QUEUE_SIZE] = [ZERO; QUEUE_SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0); // written by IRQ handler
static TAIL: AtomicUsize = AtomicUsize::new(0); // read  by main loop
static USB_LOG_COUNT: AtomicUsize = AtomicUsize::new(0);
static MODIFIERS: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Character(char),
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Backspace,
    Enter,
    Escape,
    Tab,
    Space,
    F4,
    F5,
}

#[derive(Clone, Copy)]
pub struct KeyInput {
    pub key: Key,
    pub modifiers: u8,
}

impl KeyInput {
    pub fn legacy_char(self) -> Option<char> {
        match self.key {
            Key::Character(c) => Some(c),
            Key::ArrowUp => Some('\u{F700}'),
            Key::ArrowDown => Some('\u{F701}'),
            Key::ArrowLeft => Some('\u{F702}'),
            Key::ArrowRight => Some('\u{F703}'),
            Key::Home => Some('\u{F704}'),
            Key::End => Some('\u{F705}'),
            Key::PageUp => Some('\u{F706}'),
            Key::PageDown => Some('\u{F707}'),
            Key::Delete => Some('\u{007F}'),
            Key::Backspace => Some('\u{0008}'),
            Key::Enter => Some('\n'),
            Key::Escape => Some('\u{001B}'),
            Key::Tab => Some('\t'),
            Key::Space => Some(' '),
            Key::F4 | Key::F5 => None,
        }
    }

    pub fn has_ctrl(self) -> bool {
        self.modifiers & MOD_CTRL != 0
    }

    pub fn has_alt(self) -> bool {
        self.modifiers & MOD_ALT != 0
    }
}

lazy_static! {
    static ref USB_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
        Keyboard::<layouts::Us104Key, ScancodeSet1>::new(HandleControl::MapLettersToUnicode)
    );
    static ref USB_PREV_REPORT: Mutex<[u8; 8]> = Mutex::new([0; 8]);
}

/// Push a legacy character from interrupt context. Silently drops if the buffer is full.
#[allow(dead_code)]
pub fn push(c: char) {
    push_input(KeyInput {
        key: Key::Character(c),
        modifiers: current_modifiers(),
    });
}

/// Push a structured key event from interrupt context.
pub fn push_input(input: KeyInput) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE_SIZE;
    if next == TAIL.load(Ordering::Acquire) {
        return; // full — drop
    }
    QUEUE[head].store(encode_input(input), Ordering::Relaxed);
    HEAD.store(next, Ordering::Release);
}

/// Pop a legacy character from the main loop. Returns `None` when empty.
#[allow(dead_code)]
pub fn pop() -> Option<char> {
    pop_input().and_then(KeyInput::legacy_char)
}

/// Pop a structured key event from the main loop. Returns `None` when empty.
pub fn pop_input() -> Option<KeyInput> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let v = QUEUE[tail].load(Ordering::Relaxed);
    TAIL.store((tail + 1) % QUEUE_SIZE, Ordering::Release);
    decode_input(v)
}

pub fn current_modifiers() -> u8 {
    MODIFIERS.load(Ordering::Relaxed)
}

pub fn handle_driver_event(code: KeyCode, state: KeyState, decoded: Option<DecodedKey>) -> bool {
    update_modifier(code, state);
    if state == KeyState::Up {
        return false;
    }

    let modifiers = current_modifiers();
    if modifiers & (MOD_CTRL | MOD_ALT) != 0 {
        if let Some(key) = shortcut_key_from_raw(code) {
            push_input(KeyInput { key, modifiers });
            return true;
        }
    }

    let key = decoded
        .and_then(decoded_key_to_key)
        .or_else(|| raw_key_to_special(code));
    if let Some(key) = key {
        push_input(KeyInput { key, modifiers });
        return true;
    }
    false
}

fn update_modifier(code: KeyCode, state: KeyState) {
    let Some(bit) = modifier_bit(code) else {
        return;
    };
    match state {
        KeyState::Down | KeyState::SingleShot => {
            MODIFIERS.fetch_or(bit, Ordering::Relaxed);
        }
        KeyState::Up => {
            MODIFIERS.fetch_and(!bit, Ordering::Relaxed);
        }
    }
}

fn modifier_bit(code: KeyCode) -> Option<u8> {
    match code {
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Some(MOD_SHIFT),
        KeyCode::ControlLeft | KeyCode::ControlRight => Some(MOD_CTRL),
        KeyCode::AltLeft | KeyCode::AltRight => Some(MOD_ALT),
        _ => None,
    }
}

fn decoded_key_to_key(decoded: DecodedKey) -> Option<Key> {
    match decoded {
        DecodedKey::Unicode('\n') => Some(Key::Enter),
        DecodedKey::Unicode('\r') => Some(Key::Enter),
        DecodedKey::Unicode('\t') => Some(Key::Tab),
        DecodedKey::Unicode('\u{0008}') => Some(Key::Backspace),
        DecodedKey::Unicode('\u{007F}') => Some(Key::Delete),
        DecodedKey::Unicode('\u{001B}') => Some(Key::Escape),
        DecodedKey::Unicode(c) => Some(Key::Character(c)),
        DecodedKey::RawKey(code) => raw_key_to_special(code),
    }
}

fn raw_key_to_special(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::ArrowUp => Some(Key::ArrowUp),
        KeyCode::ArrowDown => Some(Key::ArrowDown),
        KeyCode::ArrowLeft => Some(Key::ArrowLeft),
        KeyCode::ArrowRight => Some(Key::ArrowRight),
        KeyCode::Home => Some(Key::Home),
        KeyCode::End => Some(Key::End),
        KeyCode::PageUp => Some(Key::PageUp),
        KeyCode::PageDown => Some(Key::PageDown),
        KeyCode::Delete => Some(Key::Delete),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Escape => Some(Key::Escape),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::Spacebar => Some(Key::Space),
        KeyCode::F4 => Some(Key::F4),
        KeyCode::F5 => Some(Key::F5),
        _ => None,
    }
}

fn shortcut_key_from_raw(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::A => Some(Key::Character('a')),
        KeyCode::C => Some(Key::Character('c')),
        KeyCode::F => Some(Key::Character('f')),
        KeyCode::N => Some(Key::Character('n')),
        KeyCode::P => Some(Key::Character('p')),
        KeyCode::R => Some(Key::Character('r')),
        KeyCode::V => Some(Key::Character('v')),
        KeyCode::W => Some(Key::Character('w')),
        KeyCode::X => Some(Key::Character('x')),
        KeyCode::M => Some(Key::Character('m')),
        KeyCode::Key1 => Some(Key::Character('1')),
        KeyCode::Key2 => Some(Key::Character('2')),
        KeyCode::Key3 => Some(Key::Character('3')),
        KeyCode::Key4 => Some(Key::Character('4')),
        KeyCode::Spacebar => Some(Key::Space),
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::Escape => Some(Key::Escape),
        KeyCode::F4 => Some(Key::F4),
        KeyCode::F5 => Some(Key::F5),
        _ => raw_key_to_special(code),
    }
}

fn encode_input(input: KeyInput) -> u32 {
    let (kind, payload) = match input.key {
        Key::Character(c) => (EVENT_KIND_CHAR, c as u32),
        Key::ArrowUp
        | Key::ArrowDown
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown
        | Key::Delete
        | Key::Backspace
        | Key::Enter
        | Key::Escape
        | Key::Tab
        | Key::Space
        | Key::F4
        | Key::F5 => (EVENT_KIND_KEY, special_key_id(input.key) as u32),
    };
    (kind << EVENT_KIND_SHIFT)
        | (((input.modifiers & (MOD_SHIFT | MOD_CTRL | MOD_ALT)) as u32) << EVENT_MOD_SHIFT)
        | (payload & EVENT_PAYLOAD_MASK)
}

fn decode_input(value: u32) -> Option<KeyInput> {
    let kind = value >> EVENT_KIND_SHIFT;
    let modifiers = ((value >> EVENT_MOD_SHIFT) as u8) & (MOD_SHIFT | MOD_CTRL | MOD_ALT);
    let payload = value & EVENT_PAYLOAD_MASK;
    let key = match kind {
        EVENT_KIND_CHAR => Key::Character(char::from_u32(payload)?),
        EVENT_KIND_KEY => special_key_from_id(payload as u8)?,
        _ => return None,
    };
    Some(KeyInput { key, modifiers })
}

fn special_key_id(key: Key) -> u8 {
    match key {
        Key::Character(_) => 0,
        Key::ArrowUp => 1,
        Key::ArrowDown => 2,
        Key::ArrowLeft => 3,
        Key::ArrowRight => 4,
        Key::Home => 5,
        Key::End => 6,
        Key::PageUp => 7,
        Key::PageDown => 8,
        Key::Delete => 9,
        Key::Backspace => 10,
        Key::Enter => 11,
        Key::Escape => 12,
        Key::Tab => 13,
        Key::Space => 14,
        Key::F4 => 15,
        Key::F5 => 16,
    }
}

fn special_key_from_id(id: u8) -> Option<Key> {
    match id {
        1 => Some(Key::ArrowUp),
        2 => Some(Key::ArrowDown),
        3 => Some(Key::ArrowLeft),
        4 => Some(Key::ArrowRight),
        5 => Some(Key::Home),
        6 => Some(Key::End),
        7 => Some(Key::PageUp),
        8 => Some(Key::PageDown),
        9 => Some(Key::Delete),
        10 => Some(Key::Backspace),
        11 => Some(Key::Enter),
        12 => Some(Key::Escape),
        13 => Some(Key::Tab),
        14 => Some(Key::Space),
        15 => Some(Key::F4),
        16 => Some(Key::F5),
        _ => None,
    }
}

/// Enable the PS/2 keyboard IRQ as a fallback when no USB keyboard is active.
pub fn enable_ps2_fallback() {
    set_ps2_fallback_mask(false);
}

/// Disable the PS/2 keyboard IRQ when USB keyboard input is active.
pub fn disable_ps2_fallback() {
    set_ps2_fallback_mask(true);
}

fn set_ps2_fallback_mask(masked: bool) {
    unsafe {
        let mut pic1_mask: Port<u8> = Port::new(0x21);
        let mask = pic1_mask.read();
        let next = if masked {
            mask | (1 << 1)
        } else {
            mask & !(1 << 1)
        };
        pic1_mask.write(next);
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
                handle_usb_key_event(
                    code,
                    if is_down {
                        KeyState::Down
                    } else {
                        KeyState::Up
                    },
                );
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
    let decoded = keyboard.process_keyevent(KeyEvent::new(code, state));
    if handle_driver_event(code, state, decoded) {
        if USB_DEBUG_LOGS && USB_LOG_COUNT.fetch_add(1, Ordering::Relaxed) < 8 {
            crate::println!("[usb-kbd] event");
        }
        crate::wm::request_repaint();
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
