use crate::println;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn reboot() -> ! {
    let mut port = x86_64::instructions::port::Port::new(0x64u16);
    unsafe {
        port.write(0xFEu8);
    }
    loop {
        x86_64::instructions::hlt();
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,     // 32
    Keyboard,                 // 33
    Mouse = PIC_2_OFFSET + 4, // 44  (IRQ12)
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
    fn as_usize(self) -> usize {
        self as usize
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_usize()].set_handler_fn(mouse_interrupt_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(sf: InterruptStackFrame, _err: u64) -> ! {
    panic!("DOUBLE FAULT\n{:#?}", sf);
}

extern "x86-interrupt" fn page_fault_handler(
    sf: InterruptStackFrame,
    err: x86_64::structures::idt::PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    panic!("PAGE FAULT\naddr={:?} err={:?}\n{:#?}", Cr2::read(), err, sf);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    sf: InterruptStackFrame,
    err: u64,
) {
    panic!("GENERAL PROTECTION FAULT err={:#x}\n{:#?}", err, sf);
}

extern "x86-interrupt" fn invalid_opcode_handler(sf: InterruptStackFrame) {
    panic!("INVALID OPCODE\n{:#?}", sf);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    // Request a repaint at ~30 fps (timer fires at ~18.2 Hz × 2 ticks ≈ every ~55 ms).
    // We repaint every tick to keep the cursor smooth; compose() is fast.
    let _ = ticks;
    crate::wm::request_repaint();
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    use x86_64::instructions::port::Port;

    lazy_static! {
        static ref KEYBOARD: spin::Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> =
            spin::Mutex::new(Keyboard::<layouts::Us104Key, ScancodeSet1>::new(
                HandleControl::Ignore
            ));
    }

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            if let DecodedKey::Unicode(c) = key {
                crate::wm::handle_key(c);
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

// ── PS/2 mouse interrupt (IRQ12, vector 44) ───────────────────────────────────

use core::sync::atomic::AtomicU8;

/// Which byte of the 3-byte packet we are currently collecting.
static MOUSE_CYCLE: AtomicU8 = AtomicU8::new(0);
/// Raw bytes of the in-flight packet.
static MOUSE_BYTES: [AtomicU8; 3] = [AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0)];

extern "x86-interrupt" fn mouse_interrupt_handler(_sf: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let byte: u8 = unsafe { Port::<u8>::new(0x60).read() };
    let cycle = MOUSE_CYCLE.load(Ordering::Relaxed);

    // Byte 0 must have the sync bit (bit 3) set — drop it if not.
    if cycle == 0 && byte & 0x08 == 0 {
        unsafe {
            PICS.lock()
                .notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
        }
        return;
    }

    MOUSE_BYTES[cycle as usize].store(byte, Ordering::Relaxed);

    if cycle == 2 {
        let b0 = MOUSE_BYTES[0].load(Ordering::Relaxed);
        let b1 = MOUSE_BYTES[1].load(Ordering::Relaxed);
        let b2 = MOUSE_BYTES[2].load(Ordering::Relaxed);
        crate::mouse::handle_packet(b0, b1, b2);
        MOUSE_CYCLE.store(0, Ordering::Relaxed);
    } else {
        MOUSE_CYCLE.store(cycle + 1, Ordering::Relaxed);
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}
