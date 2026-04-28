#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod apps;
mod ata;
mod elf;
mod fat32;
mod framebuffer;
mod gdt;
mod interrupts;
mod keyboard;
mod memory;
mod mouse;
mod pci;
mod scheduler;
mod syscall;
mod usb;
mod userspace;
mod vfs;
mod vga_buffer;
mod vmm;
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
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::init_pit(100);
    interrupts::mask_unused_irqs();
    syscall::init();
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
    // We need two separate frame allocators: one consumed by heap init, one kept
    // for the VMM.  The BootInfoFrameAllocator is cheap to reconstruct from the
    // same regions slice; each tracks its own `next` index independently.
    let mut heap_frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(regions) };

    allocator::init_heap(&mut mapper, &mut heap_frame_allocator).expect("heap init failed");

    // Build a fresh allocator starting after the frames consumed by the heap
    // (the heap allocator's `next` counter tells us how many frames it used).
    let vmm_frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init_from(regions, heap_frame_allocator.next()) };

    // Initialise the VMM with the physical-memory offset and the remaining
    // frame supply.  From here on, all page-table work goes through vmm::.
    vmm::init(phys_mem_offset, vmm_frame_allocator);

    // Mark all present pages user-accessible so ring-3 code (living in the
    // kernel .text) can execute.  Per-process stacks are mapped separately
    // with USER_ACCESSIBLE in their private PML4.
    unsafe { memory::mark_all_user_accessible(phys_mem_offset) };

    // ── Scheduler ─────────────────────────────────────────────────────────────
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = scheduler::SCHEDULER.lock();
        sched.add_idle();
        sched.spawn("counter", counter_task);
        // fs_test runs on its own 64 KiB kernel stack — avoids blowing the
        // limited boot stack with the 512-byte sector buffers.
        sched.spawn("fs-test", fs_test_task);
    });

    // Spawn two isolated user processes (each gets its own PML4 + user stack).
    userspace::spawn_user_process(1);
    userspace::spawn_user_process(2);

    // ── Desktop ───────────────────────────────────────────────────────────────
    mouse::init_cursor();
    wm::init();

    // xHCI probe runs last so that a broken controller can't mask earlier init.
    usb::init();

    loop {
        // Do NOT disable interrupts here — the WM mutex inside compose()
        // provides the only exclusion needed.  Holding interrupts off for
        // an entire frame (≈2.8 M MMIO writes at 1280×720×3 bpp) would
        // block mouse and keyboard for tens of milliseconds per frame.
        usb::poll();
        wm::compose_if_needed();
        x86_64::instructions::hlt();
    }
}

/// One-shot task: reads /bin/hello.txt from the FAT32 disk and prints it.
fn fs_test_task() -> ! {
    println!("[fs] task started");
    match fat32::read_file("/bin/hello.txt") {
        Some(bytes) => {
            print!("[fs] /bin/hello.txt: ");
            for b in &bytes {
                vga_buffer::_print(core::format_args!("{}", *b as char));
            }
        }
        None => println!("[fs] /bin/hello.txt: NOT FOUND"),
    }

    // Block this task so it doesn't spin forever.
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = scheduler::SCHEDULER.lock();
        let cur = sched.current;
        sched.tasks[cur].status = scheduler::TaskStatus::Blocked;
    });
    loop {
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
