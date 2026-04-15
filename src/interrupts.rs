use crate::{print, println};
use core::ops::Deref;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame}; // Essential for .lock() on lazy_static

extern crate alloc;
use alloc::string::String;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

static TICKS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer    = PIC_1_OFFSET,       // 32
    Keyboard,                       // 33
    Mouse    = PIC_2_OFFSET + 4,   // 44  (IRQ12)
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
    static ref COMMAND_BUFFER: spin::Mutex<String> = spin::Mutex::new(String::with_capacity(80));
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
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
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", sf);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
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
            spin::Mutex::new(Keyboard::new(
                ScancodeSet1::new(),
                layouts::Us104Key,
                HandleControl::Ignore
            ));
    }

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => match character {
                    '\n' => {
                        println!();
                        process_command();
                    }
                    '\u{0008}' => {
                        if COMMAND_BUFFER.lock().pop().is_some() {
                            crate::vga_buffer::backspace();
                        }
                    }
                    c => {
                        print!("{}", c);
                        COMMAND_BUFFER.lock().push(c);
                    }
                },
                _ => {}
            }
        }
    }
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
    }
}

fn process_command() {
    let mut cmd = COMMAND_BUFFER.lock();
    let mut words = cmd.split_whitespace();

    match words.next() {
        Some("clear") => crate::vga_buffer::clear_screen(),
        Some("reboot") => reboot(),
        Some("help") => {
            println!("--- coolOS Help ---");
            println!("clear, reboot, help, echo [text], info, uptime, color [name]");
        }
        Some("echo") => {
            for word in words {
                print!("{} ", word);
            }
            println!();
        }
        Some("uptime") => {
            println!("Uptime: {} ticks", TICKS.load(Ordering::Relaxed));
        }
        Some("info") => {
            println!("Used Heap: {} bytes", crate::allocator::heap_used());
            let cpuid = raw_cpuid::CpuId::new();
            if let Some(v) = cpuid.get_vendor_info() {
                println!("CPU: {}", v.as_str());
            }
        }
        Some("color") => {
            if let Some(c) = words.next() {
                set_shell_color(c);
            }
        }
        Some(unknown) => println!("Unknown: {}", unknown),
        None => {}
    }
    cmd.clear();
    print!("> ");
}

fn set_shell_color(name: &str) {
    use crate::vga_buffer::{set_color, Color};
    let color = match name {
        "red" => Color::Red,
        "blue" => Color::Blue,
        "green" => Color::Green,
        "white" => Color::White,
        "yellow" => Color::Yellow,
        _ => return,
    };
    set_color(color, Color::Black);
}

fn reboot() {
    let mut port = x86_64::instructions::port::Port::new(0x64);
    unsafe {
        port.write(0xFEu8);
    }
}

// ── PS/2 mouse interrupt (IRQ12, vector 44) ───────────────────────────────────

use core::sync::atomic::AtomicU8;

/// Which byte of the 3-byte packet we are currently collecting.
static MOUSE_CYCLE: AtomicU8 = AtomicU8::new(0);
/// Raw bytes of the in-flight packet.
static MOUSE_BYTES: [AtomicU8; 3] = [
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
        unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse.as_u8()); }
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

    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Mouse.as_u8()); }
}
