#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rdi, rsp",
        "jmp {entry}",
        entry = sym rust_start,
    );
}

extern "C" fn rust_start(rsp: u64) -> ! {
    let argc = unsafe { *(rsp as *const u64) };
    let argv0 = unsafe { *((rsp + 8) as *const u64) as *const u8 };

    let prefix: &[u8] = if argc == 1 {
        b"Hello from "
    } else {
        b"Hello with bad argc from "
    };
    let _ = syscall3(SYS_WRITE, 1, prefix.as_ptr() as u64, prefix.len() as u64);
    let _ = syscall3(SYS_WRITE, 1, argv0 as u64, c_strlen(argv0) as u64);
    let newline = b"\n";
    let _ = syscall3(SYS_WRITE, 1, newline.as_ptr() as u64, newline.len() as u64);

    let _ = syscall1(SYS_EXIT, 0);
    loop {
        core::hint::spin_loop();
    }
}

fn c_strlen(mut s: *const u8) -> usize {
    let mut n = 0usize;
    unsafe {
        while *s != 0 {
            n += 1;
            s = s.add(1);
        }
    }
    n
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
