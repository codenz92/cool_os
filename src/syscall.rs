/// SYSCALL/SYSRET interface (Phase 9).
///
/// Register convention on SYSCALL entry (Linux-compatible):
///   rax = syscall number
///   rdi = arg1, rsi = arg2, rdx = arg3
///   rcx = saved user RIP (by CPU), r11 = saved user RFLAGS (by CPU)
///   RSP = user stack (NOT switched by SYSCALL — we do it manually)
///
/// Syscall table:
///   0  exit(code)
///   1  write(fd, buf, len) → bytes written
///   2  yield()
///   3  getpid() → current task id
///   4  mmap(addr, len, flags) → addr on success, u64::MAX on failure
///   5  open(path_ptr, path_len) → fd on success, u64::MAX on failure
///   6  read(fd, buf_ptr, len) → bytes read, u64::MAX on error
///   7  close(fd) → 0
///   8  exec(path_ptr, path_len) → 0 on success, u64::MAX on error
///   9  pipe(fds_ptr) → 0 on success, u64::MAX on failure
///   10 dup(fd) → new fd on success, u64::MAX on failure
///   11 shmem_create(len) → id on success, u64::MAX on failure
///   12 shmem_map(id) → virtual address on success, u64::MAX on failure
///
/// Output path: sys_write pushes bytes into SYSCALL_OUTPUT (a lock-free ring
/// buffer modelled on keyboard.rs). compositor::compose() drains it into the
/// terminal window each frame, avoiding any lock contention with the WM.
use core::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};

// ── Syscall output ring buffer ────────────────────────────────────────────────

const OUTPUT_SIZE: usize = 1024;
const ZERO8: AtomicU8 = AtomicU8::new(0);
static OUTPUT_BUF: [AtomicU8; OUTPUT_SIZE] = [ZERO8; OUTPUT_SIZE];
static OUTPUT_HEAD: AtomicUsize = AtomicUsize::new(0);
static OUTPUT_TAIL: AtomicUsize = AtomicUsize::new(0);

pub fn push_output_byte(b: u8) {
    let head = OUTPUT_HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % OUTPUT_SIZE;
    if next == OUTPUT_TAIL.load(Ordering::Acquire) {
        return; // drop if full
    }
    OUTPUT_BUF[head].store(b, Ordering::Relaxed);
    OUTPUT_HEAD.store(next, Ordering::Release);
}

pub fn pop_output_byte() -> Option<u8> {
    let tail = OUTPUT_TAIL.load(Ordering::Relaxed);
    if tail == OUTPUT_HEAD.load(Ordering::Acquire) {
        return None;
    }
    let b = OUTPUT_BUF[tail].load(Ordering::Relaxed);
    OUTPUT_TAIL.store((tail + 1) % OUTPUT_SIZE, Ordering::Release);
    Some(b)
}

// ── Bootstrap syscall stack ───────────────────────────────────────────────────
//
// Normal syscall entry now switches to the currently running task's private
// kernel stack top (tracked by the scheduler). This fallback exists only for
// early/bootstrap edge cases where no per-task stack top is available yet.

const BOOTSTRAP_SYSCALL_STACK_SIZE: usize = 64 * 1024;
static mut BOOTSTRAP_SYSCALL_STACK: [u8; BOOTSTRAP_SYSCALL_STACK_SIZE] =
    [0; BOOTSTRAP_SYSCALL_STACK_SIZE];
static BOOTSTRAP_SYSCALL_STACK_TOP: AtomicU64 = AtomicU64::new(0);

// ── MSR init ─────────────────────────────────────────────────────────────────

pub fn init() {
    unsafe {
        BOOTSTRAP_SYSCALL_STACK_TOP.store(
            core::ptr::addr_of!(BOOTSTRAP_SYSCALL_STACK) as u64
                + BOOTSTRAP_SYSCALL_STACK_SIZE as u64,
            Ordering::Relaxed,
        );

        let mut efer = x86_64::registers::model_specific::Msr::new(0xC000_0080);
        efer.write(efer.read() | 1); // SCE = bit 0

        // STAR bits[47:32] = kernel CS (0x08), bits[63:48] = SYSRET base (0x10).
        let mut star = x86_64::registers::model_specific::Msr::new(0xC000_0081);
        star.write((0x0010u64 << 48) | (0x0008u64 << 32));

        let mut lstar = x86_64::registers::model_specific::Msr::new(0xC000_0082);
        lstar.write(syscall_entry as *const () as u64);

        // SFMASK: clear IF (bit 9) on SYSCALL entry so IRQs can't fire mid-handler.
        let mut sfmask = x86_64::registers::model_specific::Msr::new(0xC000_0084);
        sfmask.write(0x200);
    }
}

// ── Naked syscall entry ───────────────────────────────────────────────────────
//
// On entry from SYSCALL: rax=nr, rdi=a1, rsi=a2, rdx=a3,
//                        rcx=user RIP, r11=user RFLAGS, rsp=user RSP.
// We temporarily borrow r10 (arg4, unused in our ABI) to hold the user RSP
// while we switch onto the dedicated syscall kernel stack.
//
// Stack frame built on the kernel stack (each slot = 8 bytes):
//   [rsp+64]  user RSP   (bottom — pushed first after stack switch)
//   [rsp+56]  user RIP   (rcx — needed for sysretq)
//   [rsp+48]  user RFLAGS(r11 — needed for sysretq)
//   [rsp+40]  rbp
//   [rsp+32]  rbx
//   [rsp+24]  r12
//   [rsp+16]  r13
//   [rsp+ 8]  r14
//   [rsp+ 0]  r15        (top of frame — pushed last)

#[repr(C)]
struct SyscallFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    user_rflags: u64,
    user_rip: u64,
    user_rsp: u64,
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // Save user RSP in r10 (clobbers arg4 which our table doesn't use).
        "mov r10, rsp",
        // Switch to the current task's private kernel stack.
        "mov r9, qword ptr [rip + {stack_top}]",
        "test r9, r9",
        "jnz 2f",
        "mov r9, qword ptr [rip + {bootstrap}]",
        "2:",
        "mov rsp, r9",
        // Build stack frame.
        "push r10",      // user RSP  — restored by `pop rsp` before sysretq
        "push rcx",      // user RIP  — must be in rcx for sysretq
        "push r11",      // user RFLAGS — must be in r11 for sysretq
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // Shuffle registers for dispatch(frame, nr, a1, a2, a3) using SysV:
        //   rdi=frame  rsi=nr  rdx=a1  rcx=a2  r8=a3
        // Input: rax=nr  rdi=a1  rsi=a2  rdx=a3
        "mov r8, rdx",
        "mov rcx, rsi",
        "mov rdx, rdi",
        "mov rsi, rax",
        "mov rdi, rsp",
        "call {dispatch}",
        // Return value in rax.  Restore saved registers.
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "pop r11",       // user RFLAGS → r11
        "pop rcx",       // user RIP   → rcx
        "pop rsp",       // restore user RSP
        "sysretq",
        stack_top = sym crate::scheduler::CURRENT_SYSCALL_STACK_TOP,
        bootstrap = sym BOOTSTRAP_SYSCALL_STACK_TOP,
        dispatch = sym syscall_dispatch,
    );
}

// ── Dispatcher and handlers ───────────────────────────────────────────────────

extern "C" fn syscall_dispatch(
    frame: &mut SyscallFrame,
    nr: u64,
    a1: u64,
    a2: u64,
    a3: u64,
) -> u64 {
    match nr {
        0 => {
            sys_exit(a1);
            0
        }
        1 => sys_write(a1, a2 as *const u8, a3),
        2 => {
            sys_yield();
            0
        }
        3 => sys_getpid(),
        4 => sys_mmap(a1, a2, a3),
        5 => sys_open(a1 as *const u8, a2),
        6 => sys_read(a1, a2 as *mut u8, a3),
        7 => {
            sys_close(a1);
            0
        }
        8 => sys_exec(frame, a1 as *const u8, a2),
        9 => sys_pipe(a1 as *mut u64),
        10 => sys_dup(a1),
        11 => sys_shmem_create(a1),
        12 => sys_shmem_map(a1),
        _ => u64::MAX,
    }
}

fn sys_write(fd: u64, buf: *const u8, len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(buf, len as usize) };

    if fd == 1 || fd == 2 {
        for &b in bytes {
            push_output_byte(b);
            // Mirror to QEMU debugcon (port 0xE9) for headless verification.
            unsafe { x86_64::instructions::port::Port::<u8>::new(0xE9).write(b) };
        }
        crate::wm::request_repaint();
        return len;
    }

    let n = crate::vfs::vfs_write(fd as usize, bytes);
    if n == usize::MAX {
        u64::MAX
    } else {
        n as u64
    }
}

fn sys_pipe(fds_ptr: *mut u64) -> u64 {
    match crate::vfs::vfs_pipe() {
        Some((read_fd, write_fd)) => unsafe {
            *fds_ptr.add(0) = read_fd as u64;
            *fds_ptr.add(1) = write_fd as u64;
            0
        },
        None => u64::MAX,
    }
}

fn sys_getpid() -> u64 {
    let sched = crate::scheduler::SCHEDULER.lock();
    sched.current as u64
}

/// Map `len` bytes at virtual address `addr` in the calling process's address
/// space with the given protection flags (bit 0 = writable).  Allocates
/// physical frames and inserts PTEs.  Returns `addr` on success, `u64::MAX`
/// on failure.
fn sys_mmap(addr: u64, len: u64, flags: u64) -> u64 {
    use x86_64::{structures::paging::PageTableFlags, VirtAddr};

    if addr == 0 || len == 0 {
        return u64::MAX;
    }

    // Round length up to page boundary.
    let len_aligned = (len + 4095) & !4095;

    let mut pte_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if flags & 1 != 0 {
        pte_flags |= PageTableFlags::WRITABLE;
    }

    // Determine the current process's PML4.
    let pml4 = crate::vmm::current_pml4();

    match crate::vmm::map_region(pml4, VirtAddr::new(addr), len_aligned, pte_flags) {
        Ok(()) => addr,
        Err(_) => u64::MAX,
    }
}

fn sys_exit(_code: u64) {
    crate::scheduler::exit_current(_code);
    // Interrupts are still disabled here (SFMASK cleared IF on SYSCALL entry).
    // The naked handler will sysretq back to ring 3; the task spins with
    // core::hint::spin_loop() until the timer fires and switches it out
    // permanently (Exited tasks are never picked by the round-robin scheduler).
}

/// Open a file by path.  `path_ptr` is a user-space pointer to a UTF-8 string
/// of length `path_len` (no nul terminator required).
fn sys_open(path_ptr: *const u8, path_len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len as usize) };
    match core::str::from_utf8(bytes) {
        Ok(path) => {
            let fd = crate::vfs::vfs_open(path);
            if fd == usize::MAX {
                u64::MAX
            } else {
                fd as u64
            }
        }
        Err(_) => u64::MAX,
    }
}

/// Read up to `len` bytes from `fd` into the user buffer at `buf_ptr`.
fn sys_read(fd: u64, buf_ptr: *mut u8, len: u64) -> u64 {
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len as usize) };
    let n = crate::vfs::vfs_read_blocking(fd as usize, buf, len as usize);
    if n == usize::MAX {
        u64::MAX
    } else {
        n as u64
    }
}

fn sys_close(fd: u64) {
    crate::vfs::vfs_close(fd as usize);
}

fn sys_exec(frame: &mut SyscallFrame, path_ptr: *const u8, path_len: u64) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(path_ptr, path_len as usize) };
    let path = match core::str::from_utf8(bytes) {
        Ok(path) => path,
        Err(_) => return u64::MAX,
    };

    let image = match crate::elf::load_elf_image(path) {
        Ok(image) => image,
        Err(_) => return u64::MAX,
    };

    {
        let mut sched = crate::scheduler::SCHEDULER.lock();
        let cur = sched.current;
        sched.tasks[cur].pml4 = Some(image.pml4);
    }

    unsafe { crate::vmm::switch_to(image.pml4) };

    // Replace the return frame so sysretq enters the new program instead of
    // resuming the old one.
    frame.r15 = 0;
    frame.r14 = 0;
    frame.r13 = 0;
    frame.r12 = 0;
    frame.rbx = 0;
    frame.rbp = 0;
    frame.user_rflags = 0x202;
    frame.user_rip = image.entry;
    frame.user_rsp = image.user_rsp;

    0
}

fn sys_dup(fd: u64) -> u64 {
    let new_fd = crate::vfs::vfs_dup(fd as usize);
    if new_fd == usize::MAX {
        u64::MAX
    } else {
        new_fd as u64
    }
}

fn sys_shmem_create(len: u64) -> u64 {
    if len == 0 {
        return u64::MAX;
    }
    let id = crate::vfs::vfs_shmem_create(len as usize);
    if id == usize::MAX {
        u64::MAX
    } else {
        id as u64
    }
}

fn sys_shmem_map(id: u64) -> u64 {
    let pml4 = crate::vmm::current_pml4();
    crate::vfs::vfs_shmem_map(id as usize, pml4)
}

fn sys_yield() {
    // No-op: the preemptive timer will preempt voluntarily yielding tasks.
}

// ── jump_to_userspace ─────────────────────────────────────────────────────────

/// Switch the current ring-0 context to ring-3 by pushing a synthetic iretq
/// frame and executing iretq.  Does not return.
///
/// `entry`    — virtual address of the first ring-3 instruction.
/// `user_rsp` — initial ring-3 stack pointer (must be 16-byte aligned).
pub unsafe fn jump_to_userspace(entry: u64, user_rsp: u64) -> ! {
    let user_cs = crate::gdt::user_code_selector().0 as u64;
    let user_ss = crate::gdt::user_data_selector().0 as u64;
    core::arch::asm!(
        "push {ss}",
        "push {rsp}",
        "push {rflags}",
        "push {cs}",
        "push {rip}",
        "iretq",
        ss     = in(reg) user_ss,
        rsp    = in(reg) user_rsp,
        rflags = in(reg) 0x202u64,   // IF=1 (interrupts enabled in ring 3), reserved bit 1
        cs     = in(reg) user_cs,
        rip    = in(reg) entry,
        options(noreturn),
    );
}
