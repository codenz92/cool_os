#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_CLOSE: u64 = 7;
const PIPE_FD: u64 = 3;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let banner = b"pipewr: spinning before write\n";
    let msg = b"hello from user writer\n";
    let _ = syscall3(SYS_WRITE, 1, banner.as_ptr() as u64, banner.len() as u64);

    for _ in 0..100_000u64 {
        core::hint::spin_loop();
    }

    let n = syscall3(SYS_WRITE, PIPE_FD, msg.as_ptr() as u64, msg.len() as u64);
    if n == u64::MAX {
        let write_fail = b"pipewr: write failed\n";
        let _ = syscall3(SYS_WRITE, 1, write_fail.as_ptr() as u64, write_fail.len() as u64);
        let _ = syscall1(SYS_EXIT, 1);
    }

    let ok = b"pipewr: write ok\n";
    let _ = syscall3(SYS_WRITE, 1, ok.as_ptr() as u64, ok.len() as u64);
    let _ = syscall1(SYS_CLOSE, PIPE_FD);
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
