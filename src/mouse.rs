/// PS/2 mouse driver — hardware init and 3-byte packet processing.
///
/// The window manager owns cursor rendering; this module just tracks
/// the logical mouse position and button state, then signals the WM
/// that a repaint is needed.
///
/// Call `init()` once after the heap is ready.  After that, the IRQ12
/// handler in `interrupts.rs` feeds packets to `handle_packet()`.

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
        // Start at (0,0); mouse::init() will centre after framebuffer is ready.
        MouseState { x: 0, y: 0, left: false, right: false }
    }
}

static MOUSE: Mutex<MouseState> = Mutex::new(MouseState::new());

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialise the PS/2 mouse and centre the cursor on screen.
pub fn init() {
    {
        let mut m = MOUSE.lock();
        m.x = crate::framebuffer::width()  / 2;
        m.y = crate::framebuffer::height() / 2;
    }
    unsafe { init_hardware(); }
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

/// Process a decoded 3-byte PS/2 packet.  Called from the IRQ12 handler
/// (interrupts already disabled by the CPU on handler entry).
pub fn handle_packet(b0: u8, b1: u8, b2: u8) {
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

    {
        let mut m = MOUSE.lock();
        let w = crate::framebuffer::width().saturating_sub(1);
        let h = crate::framebuffer::height().saturating_sub(1);
        m.x = ((m.x as i32 + dx).max(0) as usize).min(w);
        m.y = ((m.y as i32 - dy).max(0) as usize).min(h);
        m.left  = left;
        m.right = right;
    }

    // Tell the compositor a frame is needed.
    crate::wm::request_repaint();
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
