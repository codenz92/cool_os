#![no_std]
#![no_main]

use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 6;
const SYS_CLOSE: u64 = 7;
const INPUT_FD: u64 = 3;
const EVENT_PACKET_SIZE: u64 = 8;
const EVENT_KIND_KEY_CHAR: u8 = 1;
const EVENT_KIND_MOUSE_DOWN: u8 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let banner = b"keyecho: ready\n";
    let done = b"\nkeyecho: eof\n";
    let bad = b"keyecho: bad event\n";
    let click_prefix = b"\nclick ";
    let fail = b"keyecho: read failed\n";
    let _ = syscall3(SYS_WRITE, 1, banner.as_ptr() as u64, banner.len() as u64);

    let mut buf = [0u8; EVENT_PACKET_SIZE as usize];
    loop {
        let n = syscall3(SYS_READ, INPUT_FD, buf.as_mut_ptr() as u64, EVENT_PACKET_SIZE);
        if n == 0 {
            break;
        }
        if n == u64::MAX {
            let _ = syscall3(SYS_WRITE, 1, fail.as_ptr() as u64, fail.len() as u64);
            let _ = syscall1(SYS_EXIT, 1);
        }
        if n != EVENT_PACKET_SIZE {
            let _ = syscall3(SYS_WRITE, 1, bad.as_ptr() as u64, bad.len() as u64);
            let _ = syscall1(SYS_EXIT, 1);
        }
        match buf[0] {
            EVENT_KIND_KEY_CHAR => {
                let len = buf[1] as usize;
                if len == 0 || len > 4 {
                    let _ = syscall3(SYS_WRITE, 1, bad.as_ptr() as u64, bad.len() as u64);
                    let _ = syscall1(SYS_EXIT, 1);
                }
                let _ = syscall3(SYS_WRITE, 1, buf[2..2 + len].as_ptr() as u64, len as u64);
            }
            EVENT_KIND_MOUSE_DOWN => {
                let x = u16::from_le_bytes([buf[2], buf[3]]) as u64;
                let y = u16::from_le_bytes([buf[4], buf[5]]) as u64;
                let _ = syscall3(SYS_WRITE, 1, click_prefix.as_ptr() as u64, click_prefix.len() as u64);
                write_u64(x);
                let _ = syscall3(SYS_WRITE, 1, b",".as_ptr() as u64, 1);
                write_u64(y);
            }
            _ => {
                let _ = syscall3(SYS_WRITE, 1, bad.as_ptr() as u64, bad.len() as u64);
                let _ = syscall1(SYS_EXIT, 1);
            }
        }
    }

    let _ = syscall1(SYS_CLOSE, INPUT_FD);
    let _ = syscall3(SYS_WRITE, 1, done.as_ptr() as u64, done.len() as u64);
    let _ = syscall1(SYS_EXIT, 0);

    loop {
        core::hint::spin_loop();
    }
}

fn write_u64(mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    if n == 0 {
        let _ = syscall3(SYS_WRITE, 1, b"0".as_ptr() as u64, 1);
        return;
    }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    let _ = syscall3(SYS_WRITE, 1, buf[i..].as_ptr() as u64, (buf.len() - i) as u64);
}

fn syscall1(nr: u64, a1: u64) -> u64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
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
        core::arch::asm!(
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
