#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_READ: u64 = 6;
const SYS_CLOSE: u64 = 7;
const SYS_EXEC: u64 = 8;
const STDIN_FD: u64 = 3;

const EVENT_PACKET_SIZE: usize = 8;
const EVENT_KIND_KEY_CHAR: u8 = 1;

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
    let _ = syscall2(SYS_WRITE, 1, b as u64);
}

fn eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}

fn run() -> ! {
    write_str(b"terminal: ready\n");

    let mut cmd_buf = [0u8; 256];
    let mut cmd_len = 0usize;

    write_str(b"> ");

    loop {
        let mut packet = [0u8; EVENT_PACKET_SIZE];
        let n = syscall3(SYS_READ, STDIN_FD, packet.as_mut_ptr() as u64, EVENT_PACKET_SIZE as u64);

        if n == 0 {
            write_str(b"\nterminal: eof\n");
            break;
        }
        if n == u64::MAX || n != EVENT_PACKET_SIZE as u64 {
            write_str(b"\nterminal: read error\n");
            break;
        }

        if packet[0] != EVENT_KIND_KEY_CHAR {
            continue;
        }

        let char_len = packet[1] as usize;
        if char_len == 0 || char_len > 4 {
            continue;
        }

        for i in 0..char_len {
            let c = packet[2 + i];
            if c == b'\n' || c == b'\r' {
                write_str(b"\n");
                if cmd_len > 0 {
                    do_command(&cmd_buf[..cmd_len]);
                    cmd_len = 0;
                }
                write_str(b"> ");
            } else if c == 8 || c == 127 {
                if cmd_len > 0 {
                    cmd_len -= 1;
                    write_byte(8);
                    write_byte(32);
                    write_byte(8);
                }
            } else if cmd_len < 255 {
                cmd_buf[cmd_len] = c;
                cmd_len += 1;
                write_byte(c);
            }
        }
    }

    let _ = syscall1(SYS_CLOSE, STDIN_FD);
    let _ = syscall1(SYS_EXIT, 0);
    loop {
        core::hint::spin_loop();
    }
}

fn do_command(cmd: &[u8]) {
    let cmd_start = cmd.iter().position(|&c| c != b' ').unwrap_or(cmd.len());
    if cmd_start == cmd.len() {
        return;
    }

    let cmd_name_end = cmd[cmd_start..].iter().position(|&c| c == b' ').map(|e| e + cmd_start).unwrap_or(cmd.len());
    let cmd_name = &cmd[cmd_start..cmd_name_end];

    if eq(cmd_name, b"help") {
        write_str(b"Commands: help clear echo exec info uptime\n");
    } else if eq(cmd_name, b"clear") {
        for _ in 0..24 { write_str(b"\n"); }
    } else if eq(cmd_name, b"echo") {
        let args_start = cmd_name_end + 1;
        if args_start < cmd.len() {
            let args = &cmd[args_start..];
            let mut wrote = false;
            for &c in args {
                if c != b' ' {
                    write_byte(c);
                    wrote = true;
                } else if wrote {
                    write_byte(32);
                    wrote = false;
                }
            }
        }
        write_str(b"\n");
    } else if eq(cmd_name, b"exec") {
        let path_start = cmd_name_end + 1;
        let path_start = cmd[path_start..].iter().position(|&c| c != b' ').map(|e| e + path_start).unwrap_or(cmd.len());
        if path_start < cmd.len() {
            let path = &cmd[path_start..];
            let res = syscall3(SYS_EXEC, path.as_ptr() as u64, path.len() as u64, 0);
            if res == u64::MAX {
                write_str(b"exec failed\n");
            }
        } else {
            write_str(b"usage: exec /bin/name\n");
        }
    } else if eq(cmd_name, b"info") {
        write_str(b"Heap: (unknown)\n");
    } else if eq(cmd_name, b"uptime") {
        write_str(b"Uptime: (unknown)\n");
    } else {
        write_str(b"Unknown: ");
        write_str(cmd_name);
        write_str(b"\n");
    }
}

fn rust_start(_rsp: u64) -> ! {
    run()
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}