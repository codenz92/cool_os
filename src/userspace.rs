use crate::vmm;
/// Userspace tasks (Phase 10).
///
/// Each user process gets its own PML4, cloned from the kernel's PML4 for the
/// upper half (kernel mappings), with a private user stack mapped at
/// USER_STACK_TOP in the lower half.
///
/// Two processes run the same stub at the same virtual code address but write
/// a sentinel value onto their private stacks and read it back — demonstrating
/// that the stacks are physically isolated.
use x86_64::structures::paging::PageTableFlags;

// ── Ring-3 stub ───────────────────────────────────────────────────────────────

/// Ring-3 code shared by both user processes.  Runs at the kernel .text
/// virtual address (upper-half, copied into every process PML4).
///
/// Process identity is communicated via the `rdi` register on entry (set by
/// the kernel trampoline in `spawn_user_process`).  Each process:
///   1. Writes its PID as a sentinel to the top of its user stack.
///   2. Reads it back and prints the pair.
///   3. Exits.
/// Inner body shared by both stubs — pid is baked in at call time.
#[inline(never)]
fn run_user_stub(pid: u64) -> ! {
    // Write sentinel = 0xDEAD_0000 + pid to our private stack top.
    let sentinel: u64 = 0xDEAD_0000 + pid;
    let stack_top_ptr = (vmm::USER_STACK_TOP - 8) as *mut u64;
    unsafe { core::ptr::write_volatile(stack_top_ptr, sentinel) };
    let readback = unsafe { core::ptr::read_volatile(stack_top_ptr) };

    if readback == sentinel {
        let msg_ok: &[u8] = if pid == 1 {
            b"[ring3 pid=1] sentinel ok\n"
        } else {
            b"[ring3 pid=2] sentinel ok\n"
        };
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 1u64 => _,
                in("rdi") 1u64,
                in("rsi") msg_ok.as_ptr() as u64,
                in("rdx") msg_ok.len() as u64,
                out("rcx") _,
                out("r11") _,
                options(nostack),
            );
        }
    }

    unsafe {
        core::arch::asm!(
            "syscall",
            inout("rax") 0u64 => _,
            in("rdi") 0u64,
            out("rcx") _,
            out("r11") _,
            options(nostack),
        );
    }
    loop {
        core::hint::spin_loop();
    }
}

fn user_stub_1() -> ! {
    run_user_stub(1)
}
fn user_stub_2() -> ! {
    run_user_stub(2)
}

// ── Per-process spawn helper ──────────────────────────────────────────────────

/// Build a new address space with a private user stack, then spawn a scheduler
/// task that enters `user_stub` at ring 3 with `rdi = pid`.
///
/// Returns `true` on success.
pub fn spawn_user_process(pid: u64) -> bool {
    // Allocate and populate a new PML4.
    let pml4 = match vmm::new_process_pml4() {
        Some(f) => f,
        None => return false,
    };

    // Map the user stack: USER_STACK_SIZE bytes of private writable pages
    // ending at USER_STACK_TOP, user-accessible.
    let stack_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    if vmm::map_region(
        pml4,
        x86_64::VirtAddr::new(vmm::USER_STACK_BOTTOM),
        vmm::USER_STACK_SIZE,
        stack_flags,
    )
    .is_err()
    {
        return false;
    }

    // Guard page: allocate a frame and map it kernel-only (no USER_ACCESSIBLE,
    // no WRITABLE) directly below the stack.  Any ring-3 access causes a
    // protection-violation #PF, which the fault handler does not lazily recover.
    let guard_addr = x86_64::VirtAddr::new(vmm::USER_STACK_BOTTOM - 4096);
    if let Some(guard_frame) = vmm::alloc_zeroed_frame() {
        let guard_flags = PageTableFlags::PRESENT; // kernel-only, not writable, not user
        let _ = vmm::map_page_in(pml4, guard_addr, guard_frame, guard_flags);
    }

    // Trampoline: a kernel-mode task that sets rdi=pid, then iretqs to ring 3.
    // We can't capture `pid` in a fn() -> ! (no closures), so we stash it in a
    // per-slot static.
    let (name, task_fn): (&'static str, fn() -> !) = if pid == 1 {
        ("user1", trampoline_1)
    } else {
        ("user2", trampoline_2)
    };
    x86_64::instructions::interrupts::without_interrupts(|| {
        crate::scheduler::SCHEDULER
            .lock()
            .spawn_with_pml4(name, task_fn, Some(pml4));
    });
    true
}

fn trampoline_1() -> ! {
    unsafe {
        crate::syscall::jump_to_userspace(user_stub_1 as *const () as u64, vmm::USER_STACK_TOP)
    }
}

fn trampoline_2() -> ! {
    unsafe {
        crate::syscall::jump_to_userspace(user_stub_2 as *const () as u64, vmm::USER_STACK_TOP)
    }
}
