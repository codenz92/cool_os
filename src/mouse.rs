/// PS/2 mouse driver — hardware init and 3/4-byte packet processing.
///
/// The window manager owns cursor rendering; this module just tracks
/// the logical mouse position, button state, and scroll wheel delta,
/// then signals the WM that a repaint is needed.
///
/// Call `init_cursor()` once after the framebuffer is ready. If PS/2 mouse
/// fallback is needed, follow it with `enable_ps2_fallback()`. After that,
/// the IRQ12 handler in `interrupts.rs` feeds packets to `handle_packet()`.

use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use spin::Mutex;
use x86_64::instructions::port::Port;

// ── State ─────────────────────────────────────────────────────────────────────

struct MouseState {
    x:     usize,
    y:     usize,
    left:  bool,
    right: bool,
}

impl MouseState {
    const fn new() -> Self {
        // Start at (0,0); mouse::init_cursor() will centre after framebuffer is ready.
        MouseState { x: 0, y: 0, left: false, right: false }
    }
}

static MOUSE: Mutex<MouseState> = Mutex::new(MouseState::new());
const USB_DEBUG_LOGS: bool = option_env!("COOLOS_XHCI_ACTIVE_INIT").is_some();
static USB_MOUSE_LOG_COUNT: AtomicI32 = AtomicI32::new(0);
static PS2_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// True when the mouse was detected as an IntelliMouse (4-byte packets).
static INTELLIMOUSE: AtomicBool = AtomicBool::new(false);
/// Scroll delta accumulated between frames (positive = wheel down = content up).
static SCROLL_ACCUM: AtomicI32 = AtomicI32::new(0);

// ── Public API ────────────────────────────────────────────────────────────────

/// Centre the logical cursor on screen.
pub fn init_cursor() {
    {
        let mut m = MOUSE.lock();
        m.x = crate::framebuffer::width()  / 2;
        m.y = crate::framebuffer::height() / 2;
    }
}

/// Ensure the PS/2 mouse fallback is live when no USB mouse is active.
pub fn enable_ps2_fallback() {
    if !PS2_INITIALIZED.swap(true, Ordering::AcqRel) {
        unsafe { init_hardware(); }
    } else {
        unsafe { set_ps2_fallback_mask(false); }
    }
}

/// Disable the PS/2 mouse fallback IRQ when USB mouse input is active.
pub fn disable_ps2_fallback() {
    if PS2_INITIALIZED.load(Ordering::Acquire) {
        unsafe { set_ps2_fallback_mask(true); }
    }
}

/// Returns the current cursor position as `(x, y)`.
pub fn pos() -> (usize, usize) {
    let m = MOUSE.lock();
    (m.x, m.y)
}

/// Returns `(left_down, right_down)`.
pub fn buttons() -> (bool, bool) {
    let m = MOUSE.lock();
    (m.left, m.right)
}

/// True when the PS/2 mouse is in IntelliMouse (4-byte packet) mode.
pub fn is_4byte() -> bool {
    INTELLIMOUSE.load(Ordering::Relaxed)
}

/// Drain and return the accumulated scroll-wheel delta since the last call.
/// Positive = wheel scrolled down (content should scroll up).
pub fn scroll_delta() -> i32 {
    SCROLL_ACCUM.swap(0, Ordering::Relaxed)
}

/// Process a decoded PS/2 packet.  In standard mode b3 is unused (pass 0).
/// Called from the IRQ12 handler (interrupts already disabled by the CPU).
pub fn handle_packet(b0: u8, b1: u8, b2: u8, b3: u8) {
    // Bit 3 of byte 0 is always 1 — if not, we're out of sync.
    if b0 & 0x08 == 0 {
        return;
    }

    // 9-bit signed X delta (sign bit is bit 4 of b0).
    let dx: i32 = if b0 & 0x10 != 0 { b1 as i32 - 256 } else { b1 as i32 };
    // 9-bit signed Y delta (sign bit is bit 5 of b0).  PS/2 Y increases
    // upward, so negate it for screen coordinates.
    let dy: i32 = if b0 & 0x20 != 0 { b2 as i32 - 256 } else { b2 as i32 };

    let left  = b0 & 0x01 != 0;
    let right = b0 & 0x02 != 0;

    apply_motion(dx, -dy, left, right);

    // Accumulate scroll-wheel delta from byte 3 in IntelliMouse mode.
    // PS/2 Z-axis is 4-bit two's complement: positive = scroll up, negative = scroll down.
    // Negate so SCROLL_ACCUM is positive-down (matches screen offset direction).
    if INTELLIMOUSE.load(Ordering::Relaxed) {
        let z_raw = b3 & 0x0F;
        let z: i8 = if z_raw & 0x08 != 0 {
            (z_raw | 0xF0) as i8
        } else {
            z_raw as i8
        };
        if z != 0 {
            SCROLL_ACCUM.fetch_sub(z as i32, Ordering::Relaxed);
        }
    }

    // Tell the compositor a frame is needed.
    crate::wm::request_repaint();
}

/// Process a USB HID boot-protocol mouse report (3 or 4 bytes).
pub fn handle_usb_boot_report(report: &[u8]) {
    if report.len() < 3 {
        return;
    }

    let buttons = report[0];
    let dx = report[1] as i8 as i32;
    let dy = report[2] as i8 as i32;
    let left = buttons & 0x01 != 0;
    let right = buttons & 0x02 != 0;

    if USB_DEBUG_LOGS
        && (dx != 0 || dy != 0 || buttons != 0 || report.get(3).copied().unwrap_or(0) != 0)
        && USB_MOUSE_LOG_COUNT.fetch_add(1, Ordering::Relaxed) < 8
    {
        crate::println!(
            "[usb-mouse] buttons={:#x} dx={} dy={} wheel={}",
            buttons,
            dx,
            dy,
            report.get(3).copied().unwrap_or(0) as i8,
        );
    }

    // HID boot mouse reports use signed relative deltas; positive Y moves down.
    apply_motion(dx, dy, left, right);

    if report.len() >= 4 {
        let wheel = report[3] as i8 as i32;
        if wheel != 0 {
            SCROLL_ACCUM.fetch_sub(wheel, Ordering::Relaxed);
        }
    }

    crate::wm::request_repaint();
}

fn apply_motion(dx: i32, dy: i32, left: bool, right: bool) {
    let mut m = MOUSE.lock();
    let w = crate::framebuffer::width().saturating_sub(1);
    let h = crate::framebuffer::height().saturating_sub(1);
    m.x = ((m.x as i32 + dx).max(0) as usize).min(w);
    m.y = ((m.y as i32 + dy).max(0) as usize).min(h);
    m.left = left;
    m.right = right;
}

// ── Hardware init ─────────────────────────────────────────────────────────────

/// Spin until the PS/2 input buffer (bit 1 of status) is empty.
/// Has a timeout so a broken controller can't hang the kernel forever.
unsafe fn wait_for_write() {
    let mut status: Port<u8> = Port::new(0x64);
    for _ in 0..100_000u32 {
        if status.read() & 0x02 == 0 {
            return;
        }
    }
}

/// Spin until the PS/2 output buffer (bit 0 of status) has data.
/// Has a timeout — returns even if no data arrives.
unsafe fn wait_for_read() {
    let mut status: Port<u8> = Port::new(0x64);
    for _ in 0..100_000u32 {
        if status.read() & 0x01 != 0 {
            return;
        }
    }
}

/// Send the magic sample-rate sequence that enables IntelliMouse 4-byte mode,
/// then read back the device ID.  Returns true if the device confirmed ID 0x03.
unsafe fn try_enable_intellimouse(cmd: &mut Port<u8>, data: &mut Port<u8>) -> bool {
    for &rate in &[200u8, 100u8, 80u8] {
        wait_for_write(); cmd.write(0xD4u8);
        wait_for_write(); data.write(0xF3u8); // Set Sample Rate
        wait_for_read();  let _ = data.read(); // ACK
        wait_for_write(); cmd.write(0xD4u8);
        wait_for_write(); data.write(rate);
        wait_for_read();  let _ = data.read(); // ACK
    }
    // Read Device ID
    wait_for_write(); cmd.write(0xD4u8);
    wait_for_write(); data.write(0xF2u8); // Read Device ID
    wait_for_read();  let _ = data.read(); // ACK
    wait_for_read();  let id = data.read();
    id == 0x03
}

unsafe fn init_hardware() {
    let mut cmd:  Port<u8> = Port::new(0x64);
    let mut data: Port<u8> = Port::new(0x60);

    // 1. Disable keyboard scanning so its scancodes cannot land in the output
    //    buffer while we are waiting for PS/2 controller responses.
    wait_for_write(); cmd.write(0xADu8); // disable keyboard

    // 2. Flush any bytes that arrived before we disabled the keyboard.
    for _ in 0..16u8 {
        if cmd.read() & 0x01 == 0 {
            break;
        }
        let _ = data.read();
    }

    // 3. Enable the auxiliary (mouse) device.
    wait_for_write(); cmd.write(0xA8u8);

    // 4. Read-modify-write the Controller Command Byte:
    //    set bit 1 (enable IRQ12) and clear bit 5 (enable mouse clock).
    wait_for_write(); cmd.write(0x20u8);
    wait_for_read();
    let ccb = data.read();
    wait_for_write(); cmd.write(0x60u8);
    wait_for_write(); data.write((ccb | 0x02) & !0x20);

    // 5. Send 0xF6 (Set Defaults) to the mouse.
    wait_for_write(); cmd.write(0xD4u8);
    wait_for_write(); data.write(0xF6u8);
    wait_for_read(); let _ = data.read(); // ACK

    // 5a. Attempt IntelliMouse activation via the magic sample-rate sequence.
    //     Must happen before data reporting is enabled (0xF4) so no movement
    //     packets land in the output buffer during the detection handshake.
    let is_intellimouse = try_enable_intellimouse(&mut cmd, &mut data);
    INTELLIMOUSE.store(is_intellimouse, Ordering::Relaxed);

    // 6. Send 0xF4 (Enable Data Reporting) to the mouse.
    wait_for_write(); cmd.write(0xD4u8);
    wait_for_write(); data.write(0xF4u8);
    wait_for_read(); let _ = data.read(); // ACK

    // 7. Re-enable keyboard scanning.
    wait_for_write(); cmd.write(0xAEu8);

    // 8. Unmask IRQ12 (bit 4) on the secondary PIC, and ensure the cascade
    //    line (bit 2) is open on the primary PIC.
    let mut pic2_mask: Port<u8> = Port::new(0xA1);
    let m = pic2_mask.read();
    pic2_mask.write(m & !(1 << 4));

    let mut pic1_mask: Port<u8> = Port::new(0x21);
    let m = pic1_mask.read();
    pic1_mask.write(m & !(1 << 2));
}

unsafe fn set_ps2_fallback_mask(masked: bool) {
    let mut pic2_mask: Port<u8> = Port::new(0xA1);
    let mask = pic2_mask.read();
    let next = if masked {
        mask | (1 << 4)
    } else {
        mask & !(1 << 4)
    };
    pic2_mask.write(next);
}
