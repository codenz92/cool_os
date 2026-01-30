#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

mod interrupts;
mod vga_buffer;

use core::panic::PanicInfo;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    crate::vga_buffer::clear_screen();
    println!("coolOS Shell v1.0");
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
