/// Host-side tool: creates a FAT32 disk image and populates it with
/// /bin/hello.txt (and any other files needed by Phase 11+).
///
/// Usage: fs-image <output-path> [hello-elf] [exec-elf]
/// Output: a 64 MiB raw FAT32 disk image ready to attach as a QEMU IDE drive.

use std::io::Write;

const IMAGE_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB

fn main() {
    let mut args = std::env::args().skip(1);
    let out_path = args.next().expect("Usage: fs-image <output-path>");
    let hello_elf = args.next();
    let exec_elf = args.next();

    // Create or truncate the file, set it to the desired size.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&out_path)
        .unwrap_or_else(|e| panic!("cannot open {}: {}", out_path, e));

    file.set_len(IMAGE_SIZE)
        .expect("failed to set image size");

    // Format as FAT32.
    fatfs::format_volume(
        &file,
        fatfs::FormatVolumeOptions::new()
            .fat_type(fatfs::FatType::Fat32)
            .volume_label(*b"COOLOS     ")  // 11 ASCII bytes
    )
    .expect("FAT32 format failed");

    // Populate the filesystem.
    let fs = fatfs::FileSystem::new(&file, fatfs::FsOptions::new())
        .expect("failed to open FAT32 filesystem");

    let root = fs.root_dir();

    // /bin/
    root.create_dir("bin").expect("failed to create /bin");
    let bin = root.open_dir("bin").expect("failed to open /bin");

    // /bin/hello.txt
    let mut hello = bin.create_file("hello.txt").expect("failed to create hello.txt");
    hello.truncate().unwrap();
    hello
        .write_all(b"Hello from /bin/hello.txt!\n")
        .expect("failed to write hello.txt");

    // /bin/motd.txt — message of the day, for a second file test
    let mut motd = bin.create_file("motd.txt").expect("failed to create motd.txt");
    motd.truncate().unwrap();
    motd.write_all(b"coolOS Phase 11 - filesystem alive!\n")
        .expect("failed to write motd.txt");

    if let Some(hello_path) = hello_elf {
        let hello_bytes = std::fs::read(&hello_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", hello_path, e));
        let mut hello_bin = bin.create_file("hello").expect("failed to create hello");
        hello_bin.truncate().unwrap();
        hello_bin
            .write_all(&hello_bytes)
            .expect("failed to write hello");
    }

    if let Some(exec_path) = exec_elf {
        let exec_bytes = std::fs::read(&exec_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", exec_path, e));
        let mut exec_bin = bin.create_file("exec").expect("failed to create exec");
        exec_bin.truncate().unwrap();
        exec_bin
            .write_all(&exec_bytes)
            .expect("failed to write exec");
    }

    println!("{}", out_path);
}
