/// Preemptive round-robin scheduler for cool-os (Phase 8).
///
/// The timer ISR saves 15 GP registers on top of the CPU's 5-word interrupt
/// frame, giving a 20-word (160-byte) context block on the current task's
/// stack.  `timer_schedule` is called with the RSP value that points to the
/// bottom of that block and returns the RSP of whichever task should run next.
///
/// Stack layout (each slot = 8 bytes, lower address = lower index):
///
///   [stack_ptr +   0]  r15   ← stack_ptr points here (pushed last by ISR)
///   [stack_ptr +   8]  r14
///   [stack_ptr +  16]  r13
///   [stack_ptr +  24]  r12
///   [stack_ptr +  32]  r11
///   [stack_ptr +  40]  r10
///   [stack_ptr +  48]  r9
///   [stack_ptr +  56]  r8
///   [stack_ptr +  64]  rbp
///   [stack_ptr +  72]  rdi
///   [stack_ptr +  80]  rsi
///   [stack_ptr +  88]  rdx
///   [stack_ptr +  96]  rcx
///   [stack_ptr + 104]  rbx
///   [stack_ptr + 112]  rax   (pushed first by ISR)
///   [stack_ptr + 120]  RIP   ← CPU interrupt frame begins here
///   [stack_ptr + 128]  CS
///   [stack_ptr + 136]  RFLAGS
///   [stack_ptr + 144]  RSP   (task's stack pointer restored by iretq)
///   [stack_ptr + 152]  SS
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;

extern crate alloc;

// ── Public counter — incremented by the counter_task demo ────────────────────

pub static BACKGROUND_COUNTER: AtomicU64 = AtomicU64::new(0);

// ── Constants ─────────────────────────────────────────────────────────────────

/// Size of each task's private kernel stack (64 KiB).
const STACK_SIZE: usize = 64 * 1024;

// ── TaskStatus ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TaskStatus {
    Ready,
    Running,
    Blocked,
}

// ── Task ──────────────────────────────────────────────────────────────────────

pub struct Task {
    /// Human-readable name (for debugging).
    #[allow(dead_code)]
    pub name: &'static str,
    /// Heap-allocated kernel stack.  Empty for the idle task (uses boot stack).
    #[allow(dead_code)]
    stack: Vec<u8>,
    /// Saved RSP — the address of the bottom of the 20-word context block.
    /// For the idle task this starts as 0 and is filled in on the first timer
    /// preemption.
    pub stack_ptr: usize,
    pub status: TaskStatus,
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

pub struct Scheduler {
    tasks: Vec<Task>,
    /// Index of the currently running task.
    current: usize,
}

impl Scheduler {
    /// Create an empty scheduler.  `const fn` so the global static can be
    /// initialised without a heap allocation (Vec::new() is allocation-free).
    pub const fn empty() -> Self {
        Scheduler {
            tasks: Vec::new(),
            current: 0,
        }
    }

    /// Register the idle task (index 0).
    ///
    /// The idle task represents the current kernel boot stack; we don't
    /// allocate a separate stack for it.  Its `stack_ptr` will be filled in
    /// the first time the timer preempts it.
    pub fn add_idle(&mut self) {
        self.tasks.push(Task {
            name: "idle",
            stack: Vec::new(), // uses the existing kernel boot stack
            stack_ptr: 0,      // written on first preemption
            status: TaskStatus::Running,
        });
    }

    /// Allocate a 64 KiB kernel stack for a new task, pre-populate its saved
    /// context so that the first `iretq` begins execution at `entry`, and
    /// add the task to the run queue as `Ready`.
    pub fn spawn(&mut self, name: &'static str, entry: fn() -> !) {
        // Allocate and zero-initialise the stack buffer.
        let mut stack: Vec<u8> = Vec::new();
        stack.resize(STACK_SIZE, 0u8);

        // Round the top of the buffer down to a 16-byte boundary so that the
        // stack pointer is properly aligned for the System V AMD64 ABI.
        let stack_top = (stack.as_ptr() as usize + STACK_SIZE) & !0xf;

        // The saved RSP is the bottom of the 20-word (160-byte) context block.
        let stack_ptr_addr = stack_top - 20 * 8; // stack_top - 160

        // Read the current code-segment and stack-segment selectors.  These
        // must match exactly what the CPU expects for kernel-mode iretq.
        let cs: u64;
        let ss: u64;
        unsafe {
            core::arch::asm!("mov {0:x}, cs", out(reg) cs);
            core::arch::asm!("mov {0:x}, ss", out(reg) ss);
        }

        // Populate the context block.
        //
        // frame[0..15]  → GP registers (r15 first, rax last) — all 0
        // frame[15]     → RIP  (task entry point)
        // frame[16]     → CS
        // frame[17]     → RFLAGS: IF=1 (bit 9) + reserved bit 1 = 0x202
        // frame[18]     → RSP  (initial stack pointer; 16n+8 = ABI entry RSP)
        // frame[19]     → SS
        //
        // SAFETY: stack_ptr_addr is 16-byte aligned (stack_top is 16-byte
        // aligned and 160 = 10×16), and the entire 160-byte range lies within
        // the allocated Vec buffer.
        let frame = unsafe { core::slice::from_raw_parts_mut(stack_ptr_addr as *mut u64, 20) };

        for slot in frame[0..15].iter_mut() {
            *slot = 0;
        }
        frame[15] = entry as usize as u64; // RIP
        frame[16] = cs; // CS
        frame[17] = 0x202; // RFLAGS: IF=1, reserved bit 1
        frame[18] = (stack_top - 8) as u64; // RSP  (16n+8 — correct ABI entry)
        frame[19] = ss; // SS

        self.tasks.push(Task {
            name,
            stack,
            stack_ptr: stack_ptr_addr,
            status: TaskStatus::Ready,
        });
    }

    /// Core round-robin scheduling decision.
    ///
    /// 1. Saves `current_rsp` as the current task's stack pointer and marks
    ///    it `Ready` (if it was `Running`).
    /// 2. Searches forward (wrapping) for the next `Ready` task.
    /// 3. Falls back to task 0 (idle) if none found.
    /// 4. Marks the winner `Running`, updates `self.current`, and returns its
    ///    saved stack pointer.
    pub fn schedule(&mut self, current_rsp: usize) -> usize {
        if self.tasks.is_empty() {
            return current_rsp;
        }

        // ── Save the current task ────────────────────────────────────────────
        let cur = self.current;
        self.tasks[cur].stack_ptr = current_rsp;
        if self.tasks[cur].status == TaskStatus::Running {
            self.tasks[cur].status = TaskStatus::Ready;
        }

        // ── Find the next Ready task (round-robin) ───────────────────────────
        let n = self.tasks.len();
        let mut next = (cur + 1) % n;
        let mut found = false;
        for _ in 0..n {
            if self.tasks[next].status == TaskStatus::Ready {
                found = true;
                break;
            }
            next = (next + 1) % n;
        }
        if !found {
            // No runnable task — fall back to the idle task.
            next = 0;
        }

        // ── Activate the winner ──────────────────────────────────────────────
        self.tasks[next].status = TaskStatus::Running;
        self.current = next;
        self.tasks[next].stack_ptr
    }
}

// ── Global scheduler instance ─────────────────────────────────────────────────

pub static SCHEDULER: spin::Mutex<Scheduler> = spin::Mutex::new(Scheduler::empty());

// ── Timer ISR entry point (called from timer_naked in interrupts.rs) ──────────

/// Called from the naked timer ISR with `current_rsp` equal to RSP after
/// the ISR has pushed all 15 GP registers.  Returns the RSP of the task that
/// should run next.
///
/// Handles the empty-task-list case gracefully (returns `current_rsp`
/// unchanged) so that timer preemptions before `add_idle` / `spawn` are
/// harmless.
///
/// # Safety
/// Must only be called from the naked timer ISR with all GP registers already
/// pushed onto the stack and interrupts disabled by the CPU.
pub unsafe extern "C" fn timer_schedule(current_rsp: usize) -> usize {
    let mut sched = SCHEDULER.lock();
    if sched.tasks.is_empty() {
        return current_rsp;
    }
    sched.schedule(current_rsp)
}
