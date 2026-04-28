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

/// PIT frequency used for the preemptive scheduler and desktop repaint cadence.
///
/// The compositor lives in the idle task. With the always-ready demo counter
/// task also running, the idle task gets roughly every other timeslice, so
/// 288 Hz yields about 144 desktop frames per second on a typical boot.
pub const TIMER_HZ: u32 = 288;

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime_secs() -> u64 {
    ticks() / TIMER_HZ as u64
}

pub const fn ticks_for_millis(ms: u64) -> u64 {
    ((TIMER_HZ as u64 * ms) + 999) / 1000
}

/// Reprogram the PIT channel 0 to fire at `hz` Hz.
///
/// Default PIT rate is ~18.2 Hz (divisor 65535). At that rate, round-robin
/// between the idle and counter tasks gives ~9 desktop renders/second.
pub fn init_pit(hz: u32) {
    use x86_64::instructions::port::Port;
    // PIT input clock is 1_193_180 Hz. Clamp divisor to [1, 65535].
    let divisor = (1_193_180_u32 / hz).clamp(1, 65535) as u16;
    unsafe {
        // Command: channel 0, lobyte/hibyte, mode 2 (rate generator).
        Port::<u8>::new(0x43).write(0x34);
        Port::<u8>::new(0x40).write((divisor & 0xFF) as u8);
        Port::<u8>::new(0x40).write((divisor >> 8) as u8);
    }
}

/// Mask all PIC IRQs except the ones we always handle:
///   IRQ0 (timer), IRQ2 (PIC2 cascade).
/// IRQ1 (PS/2 keyboard) and IRQ12 (PS/2 mouse) stay masked until their
/// respective fallbacks are explicitly enabled by `keyboard::enable_ps2_fallback()`
/// and `mouse::enable_ps2_fallback()`. All other IRQs — including IRQ14/IRQ15 (IDE) —
/// are masked so that unhandled interrupts cannot reach the CPU and trigger a #GP.
pub fn mask_unused_irqs() {
    use x86_64::instructions::port::Port;
    unsafe {
        // PIC1: unmask IRQ0 (timer) and IRQ2 (cascade); keep IRQ1 masked until needed.
        Port::<u8>::new(0x21).write(0xFA);
        // PIC2: keep all secondary IRQs masked until a driver explicitly enables one.
        Port::<u8>::new(0xA1).write(0xFF);
    }
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
        idt.general_protection_fault
            .set_handler_fn(general_protection_fault_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        // Use set_handler_addr for our naked context-switch handler — it does
        // not conform to the `extern "x86-interrupt"` ABI because it manages
        // the full register save/restore itself.
        unsafe {
            idt[InterruptIndex::Timer.as_usize()]
                .set_handler_addr(x86_64::VirtAddr::new(timer_naked as *const () as usize as u64));
        }
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Mouse.as_usize()].set_handler_fn(mouse_interrupt_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

// ── Fault handlers ────────────────────────────────────────────────────────────

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
    use x86_64::{registers::control::Cr2, structures::paging::PageTableFlags};

    let fault_addr = Cr2::read();

    // Only attempt lazy allocation for user-mode faults on unmapped pages.
    // Conditions: not a protection violation (P bit clear in error), user-mode
    // access (U bit set), fault address is in the lower canonical half.
    let is_not_present =
        !err.contains(x86_64::structures::idt::PageFaultErrorCode::PROTECTION_VIOLATION);
    let is_user = err.contains(x86_64::structures::idt::PageFaultErrorCode::USER_MODE);
    let is_lower_half = fault_addr.as_u64() < 0x0000_8000_0000_0000;

    if is_not_present && is_user && is_lower_half {
        // Allocate and map the missing page with user-accessible writable flags.
        let flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        let pml4 = crate::vmm::current_pml4();
        if let Some(frame) = crate::vmm::alloc_zeroed_frame() {
            if crate::vmm::map_page_in(pml4, fault_addr.align_down(4096u64), frame, flags).is_ok() {
                return; // resume the faulting instruction
            }
        }
    }

    panic!(
        "PAGE FAULT\naddr={:#x} err={:?}\n{:#?}",
        fault_addr.as_u64(),
        err,
        sf
    );
}

extern "x86-interrupt" fn general_protection_fault_handler(sf: InterruptStackFrame, err: u64) {
    panic!("GENERAL PROTECTION FAULT err={:#x}\n{:#?}", err, sf);
}

extern "x86-interrupt" fn invalid_opcode_handler(sf: InterruptStackFrame) {
    panic!("INVALID OPCODE\n{:#?}", sf);
}

// ── Timer interrupt — naked context-switch handler ────────────────────────────
//
// The CPU pushes its 5-word interrupt frame (RIP, CS, RFLAGS, RSP, SS) before
// jumping here.  We then push all 15 GP registers, giving a 20-word (160-byte)
// context block whose base address (RSP after all pushes) is passed to
// `timer_inner` as its first argument via rdi (System V AMD64 ABI).
//
// `timer_inner` returns the RSP of whichever task should run next.  We write
// that value into rsp, pop the GP registers from the (possibly new) stack, and
// iretq back into the winning task.

/// Naked entry point for the timer IRQ.  Saves / restores all GP registers and
/// delegates the scheduling decision to `timer_inner`.
#[unsafe(naked)]
unsafe extern "C" fn timer_naked() {
    core::arch::naked_asm!(
        // Push GP registers in this exact order so the layout matches the
        // stack map documented in scheduler.rs.
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push rbp",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Pass current RSP (= base of context block) as the first argument.
        "mov rdi, rsp",
        // Call the Rust helper; return value (new RSP) lands in rax.
        "call {inner}",
        // Switch to the returned stack (may be a different task's stack).
        "mov rsp, rax",
        // Restore GP registers from whichever stack we are now on.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rbp",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        // Return from interrupt — restores RIP, CS, RFLAGS, RSP, SS.
        "iretq",
        inner = sym timer_inner,
    );
}

/// Rust body of the timer handler.  Called from `timer_naked` with interrupts
/// disabled (the CPU clears IF on interrupt entry through an interrupt gate).
///
/// Returns the RSP of the task that should run next (may equal `current_rsp`
/// if no context switch is needed).
#[inline(never)]
extern "C" fn timer_inner(current_rsp: usize) -> usize {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::scheduler::BACKGROUND_COUNTER.fetch_add(1, Ordering::Relaxed);
    crate::wm::request_repaint();
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
    unsafe { crate::scheduler::timer_schedule(current_rsp) }
}

// ── Keyboard interrupt (IRQ1, vector 33) ─────────────────────────────────────

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
            // Push to the lock-free queue — never touch WM.lock() from
            // interrupt context (compose() may already hold it).
            let ch = match key {
                DecodedKey::Unicode(c) => Some(c),
                DecodedKey::RawKey(pc_keyboard::KeyCode::ArrowUp) => Some('\u{F700}'),
                DecodedKey::RawKey(pc_keyboard::KeyCode::ArrowDown) => Some('\u{F701}'),
                DecodedKey::RawKey(pc_keyboard::KeyCode::ArrowLeft) => Some('\u{F702}'),
                DecodedKey::RawKey(pc_keyboard::KeyCode::ArrowRight) => Some('\u{F703}'),
                _ => None,
            };
            if let Some(c) = ch {
                crate::keyboard::push(c);
                crate::wm::request_repaint();
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

/// Which byte of the current PS/2 packet we are collecting (0-based).
static MOUSE_CYCLE: AtomicU8 = AtomicU8::new(0);
/// Raw bytes of the in-flight packet (4 bytes to accommodate IntelliMouse).
static MOUSE_BYTES: [AtomicU8; 4] = [
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
    AtomicU8::new(0),
];

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

    // IntelliMouse uses 4-byte packets (last byte index = 3); standard = 3-byte (last = 2).
    let last_byte: u8 = if crate::mouse::is_4byte() { 3 } else { 2 };
    if cycle == last_byte {
        let b0 = MOUSE_BYTES[0].load(Ordering::Relaxed);
        let b1 = MOUSE_BYTES[1].load(Ordering::Relaxed);
        let b2 = MOUSE_BYTES[2].load(Ordering::Relaxed);
        let b3 = MOUSE_BYTES[3].load(Ordering::Relaxed);
        crate::mouse::handle_packet(b0, b1, b2, b3);
        MOUSE_CYCLE.store(0, Ordering::Relaxed);
    } else {
        MOUSE_CYCLE.store(cycle + 1, Ordering::Relaxed);
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Mouse.as_u8());
    }
}
