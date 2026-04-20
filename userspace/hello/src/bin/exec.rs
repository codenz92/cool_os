#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_EXEC: u64 = 8;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let banner = b"exec: replacing self with /bin/hello\n";
    let path = b"/bin/hello";

    let _ = syscall3(SYS_WRITE, 1, banner.as_ptr() as u64, banner.len() as u64);
    let rc = syscall2(SYS_EXEC, path.as_ptr() as u64, path.len() as u64);
    if rc == u64::MAX {
        let fail = b"exec: sys_exec failed\n";
        let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

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
