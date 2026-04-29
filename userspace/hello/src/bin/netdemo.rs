#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_DNS: u64 = 17;
const SYS_HTTP: u64 = 18;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let host = b"example.com";

    write_str(b"netdemo: dns example.com = ");
    let addr = syscall2(SYS_DNS, host.as_ptr() as u64, host.len() as u64);
    if addr == u64::MAX {
        write_str(b"failed\n");
        exit(1);
    }
    write_ipv4(addr as u32);
    write_str(b"\n");

    write_str(b"netdemo: http example.com\n");
    let bytes = syscall2(SYS_HTTP, host.as_ptr() as u64, host.len() as u64);
    if bytes == u64::MAX {
        write_str(b"netdemo: http failed\n");
        exit(1);
    }
    write_str(b"netdemo: http bytes ");
    write_u64(bytes);
    write_str(b"\n");

    exit(0);
}

fn exit(code: u64) -> ! {
    let _ = syscall1(SYS_EXIT, code);
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

fn write_str(s: &[u8]) {
    let _ = syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64);
}

fn write_byte(b: u8) {
    let buf = [b];
    let _ = syscall3(SYS_WRITE, 1, buf.as_ptr() as u64, buf.len() as u64);
}

fn write_ipv4(addr: u32) {
    write_u64(((addr >> 24) & 0xff) as u64);
    write_byte(b'.');
    write_u64(((addr >> 16) & 0xff) as u64);
    write_byte(b'.');
    write_u64(((addr >> 8) & 0xff) as u64);
    write_byte(b'.');
    write_u64((addr & 0xff) as u64);
}

fn write_u64(mut n: u64) {
    if n == 0 {
        write_byte(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut len = 0usize;
    while n > 0 {
        buf[len] = b'0' + (n % 10) as u8;
        n /= 10;
        len += 1;
    }
    while len > 0 {
        len -= 1;
        write_byte(buf[len]);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
