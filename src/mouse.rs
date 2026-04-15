/// PS/2 mouse driver — hardware init, 3-byte packet processing, and cursor.
///
/// Call `init()` once after the heap is ready.  After that, the IRQ12
/// handler in `interrupts.rs` feeds packets to `handle_packet()`.

use crate::framebuffer::{self, HEIGHT, WIDTH};
use spin::Mutex;
use x86_64::instructions::port::Port;

// ── Cursor shape ─────────────────────────────────────────────────────────────

const CURSOR_W: usize = 8;
const CURSOR_H: usize = 8;

/// 1-bit mask per pixel row — bit 7 is the leftmost pixel.
/// Draws a small arrow pointing top-left.
const CURSOR_SHAPE: [u8; CURSOR_H] = [
    0b11111110,
    0b11111100,
    0b11111000,
    0b11110000,
    0b11111000,
    0b11001100,
    0b10000110,
    0b00000011,
];

// ── State ─────────────────────────────────────────────────────────────────────

struct Cursor {
    x:       usize,
    y:       usize,
    left:    bool,
    right:   bool,
    /// Pixels saved from under the cursor so we can restore them on move.
    saved:   [u8; CURSOR_W * CURSOR_H],
    visible: bool,
}

impl Cursor {
    const fn new() -> Self {
        Cursor {
            x:       WIDTH  / 2,
            y:       HEIGHT / 2,
            left:    false,
            right:   false,
            saved:   [0u8; CURSOR_W * CURSOR_H],
            visible: false,
        }
    }
}

static CURSOR: Mutex<Cursor> = Mutex::new(Cursor::new());

// ── Public API ────────────────────────────────────────────────────────────────

/// Initialise the PS/2 mouse and draw the cursor at the centre of the screen.
pub fn init() {
    unsafe { init_hardware(); }

    // Draw the initial cursor with interrupts disabled.
    //
    // Bug fixed: after init_hardware() unmasks IRQ12, an IRQ can fire before
    // we finish drawing the cursor.  The IRQ12 handler also locks CURSOR.
    // If it fires while we hold the lock, the handler spins forever with
    // interrupts disabled → deadlock.  without_interrupts prevents that.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut c = CURSOR.lock();
        save_pixels(&mut c);
        draw_cursor(&c);
        c.visible = true;
    });
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

    let mut c = CURSOR.lock();

    // Restore pixels at old position.
    if c.visible {
        restore_pixels(&c);
    }

    // Clamp new position to screen bounds.
    c.x = ((c.x as i32 + dx).max(0) as usize).min(WIDTH  - CURSOR_W);
    c.y = ((c.y as i32 - dy).max(0) as usize).min(HEIGHT - CURSOR_H);
    c.left  = left;
    c.right = right;

    // Save pixels at new position and draw cursor.
    save_pixels(&mut c);
    draw_cursor(&c);
    c.visible = true;
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
    //
    //    Bug fixed: without this, a keypress during init causes wait_for_read()
    //    to return early and we read a scancode as the CCB.  Writing it back
    //    clears bit 0 (keyboard IRQ enable), silencing all future keypresses.
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

// ── Cursor helpers ────────────────────────────────────────────────────────────

fn save_pixels(c: &mut Cursor) {
    for row in 0..CURSOR_H {
        for col in 0..CURSOR_W {
            c.saved[row * CURSOR_W + col] = framebuffer::get_pixel(c.x + col, c.y + row);
        }
    }
}

fn restore_pixels(c: &Cursor) {
    for row in 0..CURSOR_H {
        for col in 0..CURSOR_W {
            framebuffer::put_pixel(c.x + col, c.y + row, c.saved[row * CURSOR_W + col]);
        }
    }
}

fn draw_cursor(c: &Cursor) {
    for (row, &byte) in CURSOR_SHAPE.iter().enumerate() {
        for bit in 0..8usize {
            if byte & (0x80 >> bit) != 0 {
                framebuffer::put_pixel(c.x + bit, c.y + row, framebuffer::WHITE);
            }
        }
    }
}
