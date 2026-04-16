#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod apps;
mod framebuffer;
mod interrupts;
mod keyboard;
mod memory;
mod mouse;
mod scheduler;
mod vga_buffer;
mod wm;

use bootloader_api::{config::Mapping, entry_point, BootInfo, BootloaderConfig};
use core::panic::PanicInfo;

/// Tell the bootloader to map all physical memory at a dynamic virtual
/// address so `boot_info.physical_memory_offset` is valid.
static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut cfg = BootloaderConfig::new_default();
    cfg.mappings.physical_memory = Some(Mapping::Dynamic);
    cfg
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // ── Framebuffer ───────────────────────────────────────────────────────────
    // Grab the bootloader-provided framebuffer before any other init so that
    // `println!` (used by the panic handler) works as early as possible.
    if let Some(fb) = boot_info.framebuffer.as_mut() {
        let info = fb.info();
        let base = fb.buffer_mut().as_mut_ptr() as u64;
        let fmt = match info.pixel_format {
            bootloader_api::info::PixelFormat::Rgb => framebuffer::PixFmt::Rgb,
            _ => framebuffer::PixFmt::Bgr,
        };
        framebuffer::init(
            base,
            info.width,
            info.height,
            info.stride,
            info.bytes_per_pixel,
            fmt,
        );
        println!(
            "FB {}x{} stride={} bpp={} base={:#x}",
            info.width, info.height, info.stride, info.bytes_per_pixel, base
        );
    } else {
        // No framebuffer — nothing will render but at least we don't crash silently.
        panic!("bootloader did not provide a framebuffer");
    }

    // ── Core kernel services ──────────────────────────────────────────────────
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    let phys_mem_offset = x86_64::VirtAddr::new(
        boot_info
            .physical_memory_offset
            .into_option()
            .expect("physical memory offset not provided by bootloader"),
    );
    let mut mapper = unsafe { memory::init(phys_mem_offset) };

    // Convert the bootloader's MemoryRegions to a plain &'static slice.
    let regions: &'static [bootloader_api::info::MemoryRegion] =
        unsafe { core::mem::transmute(boot_info.memory_regions.as_ref()) };
    let mut frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(regions) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap init failed");

    // ── Scheduler ─────────────────────────────────────────────────────────────
    // Disable interrupts while holding the scheduler lock to avoid a deadlock
    // if the timer ISR fires while we are mid-initialisation.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = scheduler::SCHEDULER.lock();
        sched.add_idle();
        sched.spawn("counter", counter_task);
    });

    // ── Desktop ───────────────────────────────────────────────────────────────
    let term = apps::TerminalApp::new(20, 20);
    wm::add_window(wm::AppWindow::Terminal(term));

    mouse::init();
    wm::init();

    loop {
        // Do NOT disable interrupts here — the WM mutex inside compose()
        // provides the only exclusion needed.  Holding interrupts off for
        // an entire frame (≈2.8 M MMIO writes at 1280×720×3 bpp) would
        // block mouse and keyboard for tens of milliseconds per frame.
        wm::compose_if_needed();
        x86_64::instructions::hlt();
    }
}

/// Background task: increments BACKGROUND_COUNTER as fast as possible.
/// The sysmon window displays this counter, proving that preemption works —
/// the counter advances even while the WM (idle task) is compositing.
fn counter_task() -> ! {
    loop {
        scheduler::BACKGROUND_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
