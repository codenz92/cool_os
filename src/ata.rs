/// ATA PIO driver (Phase 11).
///
/// Targets the primary ATA bus (I/O ports 0x1F0–0x1F7), slave device (drive 1).
/// The filesystem disk image is attached to QEMU as `-drive if=ide,index=1`
/// which maps to primary-bus slave.
///
/// Only LBA 28 reads are implemented — the FAT32 image is 64 MiB, well within
/// the 128 GiB LBA28 limit.
use x86_64::instructions::port::Port;

// ── Primary ATA bus I/O ports ─────────────────────────────────────────────────

const DATA: u16 = 0x1F0;
const FEATURES: u16 = 0x1F1; // write: features, read: error
const SECCOUNT: u16 = 0x1F2;
const LBA_LO: u16 = 0x1F3;
const LBA_MID: u16 = 0x1F4;
const LBA_HI: u16 = 0x1F5;
const DRIVE_HDR: u16 = 0x1F6;
const STATUS_CMD: u16 = 0x1F7; // write: command, read: status
const DEV_CTRL: u16 = 0x3F6; // Device Control Register: nIEN (bit 1) disables IRQs

// ── Drive / status constants ──────────────────────────────────────────────────

const DRIVE_SLAVE: u8 = 0xF0; // bits 7/5 set, LBA mode (bit 6), drive 1 (bit 4)
const CMD_READ: u8 = 0x20; // READ SECTORS (LBA28, PIO)
const STATUS_BSY: u8 = 0x80;
const STATUS_DF: u8 = 0x20;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;

// ── Public API ────────────────────────────────────────────────────────────────

/// Read one 512-byte sector at `lba` from the slave device into `buf`.
/// Returns `true` on success, `false` on device error.
pub fn read_sector(lba: u32, buf: &mut [u8; 512]) -> bool {
    // Disable interrupts for the duration of the PIO transfer to prevent the
    // timer ISR from preempting in the middle of a multi-step I/O transaction.
    x86_64::instructions::interrupts::without_interrupts(|| read_sector_inner(lba, buf))
}

fn read_sector_inner(lba: u32, buf: &mut [u8; 512]) -> bool {
    unsafe {
        let mut status = Port::<u8>::new(STATUS_CMD);

        // Disable device interrupts (nIEN=1) so the drive never asserts IRQ14.
        // We poll for completion, so interrupts are not needed.
        Port::<u8>::new(DEV_CTRL).write(0x02);

        // Wait for BSY to clear before issuing commands.
        let mut bsy_iters: u32 = 0;
        while status.read() & STATUS_BSY != 0 {
            bsy_iters += 1;
            if bsy_iters > 10_000_000 {
                crate::println!("[ata] BSY timeout lba={}", lba);
                return false;
            }
        }

        // Select slave device, embed LBA bits 24–27.
        Port::<u8>::new(DRIVE_HDR).write(DRIVE_SLAVE | ((lba >> 24) as u8 & 0x0F));

        // 400 ns delay: read status register 4 times (ATA spec §7.2.3).
        for _ in 0..4 {
            let _ = status.read();
        }

        // Write LBA address and sector count.
        Port::<u8>::new(FEATURES).write(0);
        Port::<u8>::new(SECCOUNT).write(1);
        Port::<u8>::new(LBA_LO).write(lba as u8);
        Port::<u8>::new(LBA_MID).write((lba >> 8) as u8);
        Port::<u8>::new(LBA_HI).write((lba >> 16) as u8);

        // Issue READ SECTORS command.
        Port::<u8>::new(STATUS_CMD).write(CMD_READ);

        // Wait for DRQ (data ready) or ERR (with timeout).
        let mut drq_iters: u32 = 0;
        loop {
            let s = status.read();
            if s & STATUS_ERR != 0 || s & STATUS_DF != 0 {
                let err = Port::<u8>::new(FEATURES).read();
                crate::println!(
                    "[ata] read error lba={} status={:#x} err={:#x}",
                    lba,
                    s,
                    err
                );
                return false;
            }
            if s & STATUS_BSY == 0 && s & STATUS_DRQ != 0 {
                break;
            }
            drq_iters += 1;
            if drq_iters > 10_000_000 {
                crate::println!("[ata] DRQ timeout lba={}", lba);
                return false;
            }
        }

        // Read 256 16-bit words = 512 bytes.
        let mut data = Port::<u16>::new(DATA);
        let words = core::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u16, 256);
        for w in words.iter_mut() {
            *w = data.read();
        }
    }
    true
}
