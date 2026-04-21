#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 6;
const SYS_CLOSE: u64 = 7;
const SYS_PIPE: u64 = 9;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut fds = [0u64; 2];
    let banner = b"pipe: creating anonymous pipe\n";
    let msg = b"hello through pipe\n";
    let fail = b"pipe: syscall failed\n";
    let _ = syscall3(SYS_WRITE, 1, banner.as_ptr() as u64, banner.len() as u64);

    if syscall1(SYS_PIPE, fds.as_mut_ptr() as u64) == u64::MAX {
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let wrote = syscall3(SYS_WRITE, fds[1], msg.as_ptr() as u64, msg.len() as u64);
    if wrote == u64::MAX {
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let mut buf = [0u8; 32];
    let read = syscall3(SYS_READ, fds[0], buf.as_mut_ptr() as u64, buf.len() as u64);
    if read == u64::MAX {
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let _ = syscall3(SYS_WRITE, 1, buf.as_ptr() as u64, read);
    let _ = syscall1(SYS_CLOSE, fds[0]);
    let _ = syscall1(SYS_CLOSE, fds[1]);
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
