#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_SHMEM_CREATE: u64 = 11;
const SYS_SHMEM_MAP: u64 = 12;

#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov rdi, rsp",
        "jmp {entry}",
        entry = sym rust_start,
    );
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

fn syscall2(nr: u64, a1: u64) -> u64 {
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

fn write_str(s: &[u8]) {
    let _ = syscall3(SYS_WRITE, 1, s.as_ptr() as u64, s.len() as u64);
}

fn write_hex(n: u64) {
    static mut HEX: [u8; 18] = [
        b'0', b'x', b'0', b'0', b'0', b'0', b'0', b'0',
        b'0', b'0', b'0', b'0', b'0', b'0', b'0', b'0',
        b'0', b'\n',
    ];
    if n == 0 {
        write_str(b"0x0\n");
        return;
    }
    let mut i = 16usize;
    unsafe { HEX[i] = b'\n'; }
    i -= 1;
    let mut val = n;
    while val > 0 {
        let nibble = (val & 0xF) as u8;
        unsafe { HEX[i] = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 }; }
        val >>= 4;
        i -= 1;
    }
    unsafe {
        HEX[i] = b'x';
        HEX[i - 1] = b'0';
    }
    write_str(unsafe { &HEX[i - 1..18] });
}

fn test_shmem() {
    write_str(b"shmem_create(8192) = ");
    let id = syscall2(SYS_SHMEM_CREATE, 8192);
    write_hex(id);
    if id == u64::MAX {
        write_str(b"FAILED\n");
        return;
    }
    write_str(b"shmem_map() = ");
    let addr = syscall2(SYS_SHMEM_MAP, id);
    write_hex(addr);
    if addr == u64::MAX {
        write_str(b"FAILED\n");
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

extern "C" fn rust_start(rsp: u64) -> ! {
    test_shmem();

    let argc = unsafe { *(rsp as *const u64) };
    let argv0 = unsafe { *((rsp + 8) as *const u64) as *const u8 };

    let prefix: &[u8] = if argc == 1 {
        b"Hello from "
    } else {
        b"Hello with bad argc from "
    };
    write_str(prefix);
    write_str(unsafe { core::slice::from_raw_parts(argv0, c_strlen(argv0)) });
    write_str(b"\n");

    let _ = syscall1(SYS_EXIT, 0);
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}