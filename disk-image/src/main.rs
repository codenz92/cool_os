use bootloader::{BiosBoot, BootConfig};
/// Host-side tool: wraps the kernel ELF into a BIOS-bootable disk image
/// using bootloader 0.11's BiosBoot builder.
///
/// Usage: disk-image <path-to-kernel-elf>
/// Writes <kernel-dir>/bios.img and prints the path.
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let kernel_path = args.next().expect("Usage: disk-image <path-to-kernel-elf>");

    let kernel = PathBuf::from(&kernel_path);
    let out_dir = kernel.parent().unwrap_or_else(|| std::path::Path::new("."));
    let bios_path = out_dir.join("bios.img");

    // Request at least 1280×720 so the desktop is readable.
    let mut boot_config = BootConfig::default();
    boot_config.frame_buffer.minimum_framebuffer_width = Some(1280);
    boot_config.frame_buffer.minimum_framebuffer_height = Some(720);
    // Keep bootloader diagnostics on the debug console, but don't paint them
    // onto the visible framebuffer during normal desktop boots.
    boot_config.frame_buffer_logging = false;

    BiosBoot::new(&kernel)
        .set_boot_config(&boot_config)
        .create_disk_image(&bios_path)
        .unwrap_or_else(|e| panic!("failed to create disk image: {}", e));

    println!("{}", bios_path.display());
}
