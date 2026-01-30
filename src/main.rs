#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod interrupts;
mod memory;
mod vga_buffer;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    // Use physical_memory_offset from boot_info (enabled via Cargo feature)
    let phys_mem_offset = x86_64::VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    crate::vga_buffer::clear_screen();
    println!("coolOS Shell v1.2 [Heap Online]");
    print!("> ");

    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
