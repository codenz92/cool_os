#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_OPEN: u64 = 5;
const SYS_READ: u64 = 6;
const SYS_CLOSE: u64 = 7;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let path = b"/bin/motd.txt";
    let banner = b"read: opening /bin/motd.txt\n";
    let fail = b"read: syscall failed\n";

    let _ = syscall3(SYS_WRITE, 1, banner.as_ptr() as u64, banner.len() as u64);

    let fd = syscall2(SYS_OPEN, path.as_ptr() as u64, path.len() as u64);
    if fd == u64::MAX {
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let mut buf = [0u8; 64];
    let n = syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64);
    if n == u64::MAX {
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_CLOSE, fd);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let _ = syscall3(SYS_WRITE, 1, buf.as_ptr() as u64, n);
    let _ = syscall1(SYS_CLOSE, fd);
    let _ = syscall1(SYS_EXIT, 0);

    loop {
        core::hint::spin_loop();
    }
}

fn syscall1(nr: u64, a1: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") a1,
            lateout("rcx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn syscall2(nr: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") a1,
            in("rsi") a2,
            lateout("rcx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

fn syscall3(nr: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") a1,
            in("rsi") a2,
            in("rdx") a3,
            lateout("rcx") _,
            lateout("r8") _,
            lateout("r9") _,
            lateout("r10") _,
            lateout("r11") _,
            options(nostack),
        );
    }
    ret
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
