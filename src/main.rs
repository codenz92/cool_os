#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod abi;
mod accessibility;
mod acpi;
mod allocator;
mod app_lifecycle;
mod app_metadata;
mod apps;
mod ata;
mod boot_splash;
mod boot_watchdog;
mod branding;
mod clipboard;
mod crashdump;
mod deferred;
mod desktop_settings;
mod device_registry;
mod drivers;
mod elf;
mod event_bus;
mod fat32;
mod font;
mod framebuffer;
mod fs_hardening;
mod gdt;
mod interrupts;
mod jobs;
mod keyboard;
mod klog;
mod memory;
mod mouse;
mod net;
mod notifications;
mod packages;
mod pci;
mod process_model;
mod profiler;
mod rtc;
mod scheduler;
mod search_index;
mod security;
mod services;
mod shortcuts;
mod syscall;
mod task_snapshot;
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
        crate::vga_buffer::set_framebuffer_output(false);
        boot_splash::show("starting kernel", 0, boot_splash::BOOT_PROGRESS_TOTAL);
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
    boot_splash::show(
        "loading descriptor tables",
        1,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );

    interrupts::init_idt();
    boot_splash::show(
        "registering interrupts",
        2,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );

    unsafe { interrupts::PICS.lock().initialize() };
    interrupts::init_pit(interrupts::TIMER_HZ);
    boot_splash::show(
        "starting interrupt controller",
        3,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );

    interrupts::mask_unused_irqs();
    syscall::init();
    x86_64::instructions::interrupts::enable();
    boot_splash::show("enabling syscalls", 4, boot_splash::BOOT_PROGRESS_TOTAL);

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
    boot_splash::show("reading memory map", 5, boot_splash::BOOT_PROGRESS_TOTAL);

    // We need two separate frame allocators: one consumed by heap init, one kept
    // for the VMM.  The BootInfoFrameAllocator is cheap to reconstruct from the
    // same regions slice; each tracks its own `next` index independently.
    let mut heap_frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(regions) };
    boot_splash::show("reserving heap pages", 6, boot_splash::BOOT_PROGRESS_TOTAL);

    allocator::init_heap(&mut mapper, &mut heap_frame_allocator).expect("heap init failed");
    boot_splash::show("allocating heap", 7, boot_splash::BOOT_PROGRESS_TOTAL);
    klog::init();
    profiler::record_boot_stage("allocating heap", 7);
    fs_hardening::init();
    event_bus::emit("boot", "heap", "kernel heap online");
    security::init();
    app_lifecycle::init();
    packages::init();
    accessibility::load_from_disk();
    device_registry::refresh_pci();
    drivers::init();
    acpi::init(
        boot_info.rsdp_addr.as_ref().copied(),
        phys_mem_offset.as_u64(),
    );
    net::init();
    services::init();
    deferred::enqueue(deferred::DeferredWork::RefreshSearchIndex);

    // Build a fresh allocator starting after the frames consumed by the heap
    // (the heap allocator's `next` counter tells us how many frames it used).
    let vmm_frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init_from(regions, heap_frame_allocator.next()) };
    boot_splash::show("preparing page tables", 8, boot_splash::BOOT_PROGRESS_TOTAL);

    // Initialise the VMM with the physical-memory offset and the remaining
    // frame supply.  From here on, all page-table work goes through vmm::.
    vmm::init(phys_mem_offset, vmm_frame_allocator);
    boot_splash::show(
        "mapping virtual memory",
        9,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );

    // Mark all present pages user-accessible so ring-3 code (living in the
    // kernel .text) can execute.  Per-process stacks are mapped separately
    // with USER_ACCESSIBLE in their private PML4.
    unsafe { memory::mark_all_user_accessible(phys_mem_offset) };
    boot_splash::show("enabling user pages", 10, boot_splash::BOOT_PROGRESS_TOTAL);

    // USB probing needs heap/VMM/interrupts, but it should not be gated on the
    // scheduler or first desktop frame. Running it here keeps headless device
    // smoke tests deterministic even as later shell setup grows more complex.
    usb::init();
    let _ = klog::flush_to_disk();

    // ── Scheduler ─────────────────────────────────────────────────────────────
    boot_splash::show("starting scheduler", 11, boot_splash::BOOT_PROGRESS_TOTAL);
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut sched = scheduler::SCHEDULER.lock();
        sched.add_idle();
        // fs_test runs on its own 64 KiB kernel stack — avoids blowing the
        // limited boot stack with the 512-byte sector buffers.
        sched.spawn("fs-test", fs_test_task);
    });
    boot_splash::show(
        "staging filesystem checks",
        12,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );

    // Spawn two isolated user processes (each gets its own PML4 + user stack).
    userspace::spawn_user_process(1);
    userspace::spawn_user_process(2);
    boot_splash::show("launching userspace", 13, boot_splash::BOOT_PROGRESS_TOTAL);

    // ── Desktop ───────────────────────────────────────────────────────────────
    mouse::init_cursor();
    boot_splash::show(
        "preparing desktop shell",
        14,
        boot_splash::BOOT_PROGRESS_TOTAL,
    );
    wm::prepare();
    shortcuts::load_from_disk();
    wm::init();
    boot_splash::show("drawing desktop", 23, boot_splash::BOOT_PROGRESS_TOTAL);
    wm::compose_if_needed();
    println!("[boot] desktop ready");
    profiler::record_boot_stage("desktop ready", boot_splash::BOOT_PROGRESS_TOTAL);
    boot_watchdog::complete();

    loop {
        // Do NOT disable interrupts here — the WM mutex inside compose()
        // provides the only exclusion needed.  Holding interrupts off for
        // an entire frame (≈2.8 M MMIO writes at 1280×720×3 bpp) would
        // block mouse and keyboard for tens of milliseconds per frame.
        services::supervise();
        deferred::drain_budget(2);
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

    // Mark this one-shot task complete so it leaves the run queue.
    scheduler::exit_current(0);
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    klog::log("kernel panic");
    crashdump::record_panic(info);
    klog::dump_to_console();
    crate::vga_buffer::reset_cursor();
    crate::vga_buffer::set_framebuffer_output(true);
    println!("{}", info);
    loop {}
}
