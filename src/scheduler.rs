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
use alloc::{format, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::structures::paging::PhysFrame;

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
    Exited,
    Reaped,
}

// ── Task ──────────────────────────────────────────────────────────────────────

pub struct Task {
    /// Human-readable name (for debugging).
    #[allow(dead_code)]
    pub name: &'static str,
    /// Heap-allocated kernel stack.  Empty for the idle task (uses boot stack).
    #[allow(dead_code)]
    stack: Vec<u8>,
    /// Top of the private kernel stack used for syscall entry on this task.
    pub syscall_stack_top: usize,
    /// Saved RSP — the address of the bottom of the 20-word context block.
    /// For the idle task this starts as 0 and is filled in on the first timer
    /// preemption.
    pub stack_ptr: usize,
    pub status: TaskStatus,
    pub exit_code: Option<u64>,
    pub parent: Option<usize>,
    pub process_group: usize,
    pub pending_signal: Option<crate::process_model::Signal>,
    pub wake_tick: Option<u64>,
    /// Per-process PML4 frame.  None = kernel task, shares the boot PML4.
    pub pml4: Option<PhysFrame>,
}

// ── Scheduler ─────────────────────────────────────────────────────────────────

pub struct Scheduler {
    pub tasks: Vec<Task>,
    /// Index of the currently running task.
    pub current: usize,
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
            stack: Vec::new(),
            syscall_stack_top: 0,
            stack_ptr: 0,
            status: TaskStatus::Running,
            exit_code: None,
            parent: None,
            process_group: 0,
            pending_signal: None,
            wake_tick: None,
            pml4: None,
        });
        crate::vfs::init_task(0);
        CURRENT_SYSCALL_STACK_TOP.store(0, Ordering::Relaxed);
    }

    fn spawn_context(
        &mut self,
        name: &'static str,
        rip: u64,
        cs: u64,
        rsp: Option<u64>,
        ss: u64,
        pml4: Option<PhysFrame>,
    ) -> usize {
        // Allocate and zero-initialise the stack buffer.
        let mut stack: Vec<u8> = Vec::new();
        stack.resize(STACK_SIZE, 0u8);
        crate::slab::record_alloc("task-stack", STACK_SIZE);

        // Round the top of the buffer down to a 16-byte boundary so that the
        // stack pointer is properly aligned for the System V AMD64 ABI.
        let stack_top = (stack.as_ptr() as usize + STACK_SIZE) & !0xf;

        // The saved RSP is the bottom of the 20-word (160-byte) context block.
        let stack_ptr_addr = stack_top - 20 * 8; // stack_top - 160

        // Populate the context block.
        //
        // frame[0..15]  → GP registers (r15 first, rax last) — all 0
        // frame[15]     → RIP  (task entry point)
        // frame[16]     → CS
        // frame[17]     → RFLAGS: IF=1 (bit 9) + reserved bit 1 = 0x202
        // frame[18]     → RSP
        // frame[19]     → SS
        //
        // SAFETY: stack_ptr_addr is 16-byte aligned (stack_top is 16-byte
        // aligned and 160 = 10×16), and the entire 160-byte range lies within
        // the allocated Vec buffer.
        let frame = unsafe { core::slice::from_raw_parts_mut(stack_ptr_addr as *mut u64, 20) };

        for slot in frame[0..15].iter_mut() {
            *slot = 0;
        }
        frame[15] = rip;
        frame[16] = cs; // CS
        frame[17] = 0x202; // RFLAGS: IF=1, reserved bit 1
        frame[18] = rsp.unwrap_or((stack_top - 8) as u64);
        frame[19] = ss; // SS

        let parent = if self.tasks.is_empty() {
            None
        } else {
            Some(self.current)
        };
        let process_group = parent
            .and_then(|parent_id| self.tasks.get(parent_id).map(|task| task.process_group))
            .unwrap_or_else(|| self.tasks.len());
        self.tasks.push(Task {
            name,
            stack,
            syscall_stack_top: stack_top,
            stack_ptr: stack_ptr_addr,
            status: TaskStatus::Ready,
            exit_code: None,
            parent,
            process_group,
            pending_signal: None,
            wake_tick: None,
            pml4,
        });
        let task_id = self.tasks.len() - 1;
        crate::vfs::init_task(task_id);
        task_id
    }

    /// Allocate a 64 KiB kernel stack for a new task, pre-populate its saved
    /// context so that the first `iretq` begins execution at `entry`, and
    /// add the task to the run queue as `Ready`.
    /// Spawn a kernel-mode task (shares the boot PML4, ring 0).
    pub fn spawn(&mut self, name: &'static str, entry: fn() -> !) {
        self.spawn_with_pml4(name, entry, None);
    }

    /// Spawn a task with an optional private PML4.  When `pml4` is `Some`,
    /// the scheduler loads it into CR3 whenever this task is scheduled.
    pub fn spawn_with_pml4(
        &mut self,
        name: &'static str,
        entry: fn() -> !,
        pml4: Option<PhysFrame>,
    ) {
        // Read the current kernel selectors. These must match exactly what the
        // CPU expects for a ring-0 iretq frame.
        let cs: u64;
        let ss: u64;
        unsafe {
            core::arch::asm!("mov {0:x}, cs", out(reg) cs);
            core::arch::asm!("mov {0:x}, ss", out(reg) ss);
        }
        self.spawn_context(name, entry as usize as u64, cs, None, ss, pml4);
    }

    /// Spawn a ring-3 task that will enter at `entry` with the given user stack.
    #[allow(dead_code)]
    pub fn spawn_user(&mut self, name: &'static str, entry: u64, user_rsp: u64, pml4: PhysFrame) {
        self.spawn_user_with_fds(name, entry, user_rsp, pml4, &[]);
    }

    /// Spawn a ring-3 task and selectively inherit fd mappings from the
    /// currently running task.
    pub fn spawn_user_with_fds(
        &mut self,
        name: &'static str,
        entry: u64,
        user_rsp: u64,
        pml4: PhysFrame,
        inherited_fds: &[(usize, usize)],
    ) -> bool {
        let user_cs = crate::gdt::user_code_selector().0 as u64;
        let user_ss = crate::gdt::user_data_selector().0 as u64;
        let parent = self.current;
        let task_id = self.spawn_context(name, entry, user_cs, Some(user_rsp), user_ss, Some(pml4));
        if crate::vfs::inherit_fds(parent, task_id, inherited_fds) {
            true
        } else {
            crate::vfs::drop_task(task_id);
            self.tasks.pop();
            false
        }
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

        let now = crate::interrupts::ticks();
        for (idx, task) in self.tasks.iter_mut().enumerate() {
            if task.status == TaskStatus::Blocked
                && task.wake_tick.map(|wake| now >= wake).unwrap_or(false)
            {
                task.wake_tick = None;
                task.status = TaskStatus::Ready;
            }
            if let Some(signal) = task.pending_signal.take() {
                match signal {
                    crate::process_model::Signal::Term | crate::process_model::Signal::Int => {
                        if idx != 0
                            && task.status != TaskStatus::Exited
                            && task.status != TaskStatus::Reaped
                        {
                            task.status = TaskStatus::Exited;
                            task.exit_code =
                                Some(if signal == crate::process_model::Signal::Term {
                                    143
                                } else {
                                    130
                                });
                            task.wake_tick = None;
                        }
                    }
                    crate::process_model::Signal::User1 => {
                        task.wake_tick = None;
                        if task.status == TaskStatus::Blocked {
                            task.status = TaskStatus::Ready;
                        }
                    }
                }
            }
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
        CURRENT_SYSCALL_STACK_TOP
            .store(self.tasks[next].syscall_stack_top as u64, Ordering::Relaxed);

        // Switch address space: load the winning task's PML4, or restore the
        // boot PML4 for kernel tasks (pml4=None) so they never run with a
        // user process's address space accidentally loaded.
        match self.tasks[next].pml4 {
            Some(pml4) => unsafe { crate::vmm::switch_to(pml4) },
            None => unsafe { crate::vmm::switch_to_boot() },
        }

        self.tasks[next].stack_ptr
    }
}

// ── Global scheduler instance ─────────────────────────────────────────────────

pub static SCHEDULER: spin::Mutex<Scheduler> = spin::Mutex::new(Scheduler::empty());
pub static CURRENT_SYSCALL_STACK_TOP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum KillError {
    CannotKillIdle,
    CannotKillCurrent,
    InvalidTask,
    AlreadyExited,
    AlreadyReaped,
    NotChild,
}

impl KillError {
    pub const fn as_str(self) -> &'static str {
        match self {
            KillError::CannotKillIdle => "cannot kill idle task",
            KillError::CannotKillCurrent => "cannot kill current task",
            KillError::InvalidTask => "no such task",
            KillError::AlreadyExited => "task already exited",
            KillError::AlreadyReaped => "task already reaped",
            KillError::NotChild => "not a child task",
        }
    }
}

pub enum WaitError {
    InvalidTask,
    NotChild,
    NotExited,
    AlreadyReaped,
}

impl WaitError {
    pub const fn as_str(self) -> &'static str {
        match self {
            WaitError::InvalidTask => "no such task",
            WaitError::NotChild => "not a child task",
            WaitError::NotExited => "task has not exited",
            WaitError::AlreadyReaped => "task already reaped",
        }
    }
}

pub enum SignalError {
    InvalidTask,
    CannotSignalIdle,
    AlreadyReaped,
}

impl SignalError {
    pub const fn as_str(self) -> &'static str {
        match self {
            SignalError::InvalidTask => "no such task",
            SignalError::CannotSignalIdle => "cannot signal idle task",
            SignalError::AlreadyReaped => "task already reaped",
        }
    }
}

// ── Blocking helpers ─────────────────────────────────────────────────────────

pub fn current_task_id() -> usize {
    SCHEDULER.lock().current
}

pub fn task_name(task_id: usize) -> Option<&'static str> {
    SCHEDULER.lock().tasks.get(task_id).map(|task| task.name)
}

pub fn current_task_blocked() -> bool {
    let sched = SCHEDULER.lock();
    sched
        .tasks
        .get(sched.current)
        .map(|task| task.status == TaskStatus::Blocked)
        .unwrap_or(false)
}

pub fn current_has_pending_signal() -> bool {
    let sched = SCHEDULER.lock();
    sched
        .tasks
        .get(sched.current)
        .map(|task| task.pending_signal.is_some())
        .unwrap_or(false)
}

pub fn block_current() {
    let mut sched = SCHEDULER.lock();
    let cur = sched.current;
    if let Some(task) = sched.tasks.get_mut(cur) {
        task.status = TaskStatus::Blocked;
    }
}

pub fn block_current_until(wake_tick: u64) {
    let mut sched = SCHEDULER.lock();
    let cur = sched.current;
    if let Some(task) = sched.tasks.get_mut(cur) {
        task.status = TaskStatus::Blocked;
        task.wake_tick = Some(wake_tick);
    }
}

pub fn unblock(task_id: usize) {
    let mut sched = SCHEDULER.lock();
    if let Some(task) = sched.tasks.get_mut(task_id) {
        if task.status == TaskStatus::Blocked {
            task.wake_tick = None;
            task.status = TaskStatus::Ready;
        }
    }
}

pub fn exit_current(code: u64) {
    let (task_id, name) = {
        let mut sched = SCHEDULER.lock();
        let task_id = sched.current;
        let name = sched
            .tasks
            .get(task_id)
            .map(|task| task.name)
            .unwrap_or("task");
        if let Some(task) = sched.tasks.get_mut(task_id) {
            task.status = TaskStatus::Exited;
            task.exit_code = Some(code);
        }
        (task_id, name)
    };

    crate::vfs::drop_task(task_id);
    crate::profiler::record_task(task_id, name, "exited");
    crate::crashdump::record_task_report(task_id, "task exited");
    crate::notifications::push_transient("Task exited", &format!("pid {} exit {}", task_id, code));
    crate::deferred::enqueue(crate::deferred::DeferredWork::PersistTaskSnapshot);
    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushKernelLog);
}

pub fn kill_task(task_id: usize, code: u64) -> Result<(), KillError> {
    if task_id == 0 {
        return Err(KillError::CannotKillIdle);
    }

    let current = {
        let sched = SCHEDULER.lock();
        sched.current
    };
    if task_id == current {
        return Err(KillError::CannotKillCurrent);
    }

    {
        let sched = SCHEDULER.lock();
        let Some(task) = sched.tasks.get(task_id) else {
            return Err(KillError::InvalidTask);
        };
        if task.status == TaskStatus::Exited {
            return Err(KillError::AlreadyExited);
        }
        if task.status == TaskStatus::Reaped {
            return Err(KillError::AlreadyReaped);
        }
    }

    let name = {
        let mut sched = SCHEDULER.lock();
        let task = sched.tasks.get_mut(task_id).ok_or(KillError::InvalidTask)?;
        let name = task.name;
        task.status = TaskStatus::Exited;
        task.exit_code = Some(code);
        name
    };
    crate::vfs::drop_task(task_id);
    crate::profiler::record_task(task_id, name, "killed");
    crate::crashdump::record_task_report(task_id, "task killed");
    crate::notifications::push_transient("Task killed", &format!("pid {} exit {}", task_id, code));
    crate::deferred::enqueue(crate::deferred::DeferredWork::PersistTaskSnapshot);
    crate::deferred::enqueue(crate::deferred::DeferredWork::FlushKernelLog);
    Ok(())
}

pub fn fault_current(code: u64, reason: &'static str) -> usize {
    let (task_id, name) = {
        let mut sched = SCHEDULER.lock();
        let task_id = sched.current;
        let name = sched
            .tasks
            .get(task_id)
            .map(|task| task.name)
            .unwrap_or("task");
        if let Some(task) = sched.tasks.get_mut(task_id) {
            task.status = TaskStatus::Exited;
            task.exit_code = Some(code);
        }
        (task_id, name)
    };
    crate::vfs::drop_task(task_id);
    crate::profiler::record_task(task_id, name, reason);
    crate::crashdump::record_task_report(task_id, reason);
    crate::deferred::enqueue(crate::deferred::DeferredWork::PersistTaskSnapshot);
    task_id
}

pub fn send_signal(
    task_id: usize,
    signal: crate::process_model::Signal,
) -> Result<(), SignalError> {
    if task_id == 0 {
        return Err(SignalError::CannotSignalIdle);
    }
    {
        let mut sched = SCHEDULER.lock();
        let task = sched
            .tasks
            .get_mut(task_id)
            .ok_or(SignalError::InvalidTask)?;
        if task.status == TaskStatus::Reaped {
            return Err(SignalError::AlreadyReaped);
        }
        task.pending_signal = Some(signal);
    }
    crate::notifications::push_transient(
        "Signal queued",
        &format!("pid {} {}", task_id, signal.label()),
    );
    Ok(())
}

pub fn set_process_group(task_id: usize, group: usize) -> Result<(), SignalError> {
    let mut sched = SCHEDULER.lock();
    let task = sched
        .tasks
        .get_mut(task_id)
        .ok_or(SignalError::InvalidTask)?;
    task.process_group = group;
    Ok(())
}

pub fn waitpid(parent: usize, task_id: usize) -> Result<u64, WaitError> {
    if task_id == 0 {
        return Err(WaitError::InvalidTask);
    }
    let result = {
        let mut sched = SCHEDULER.lock();
        let task = sched.tasks.get_mut(task_id).ok_or(WaitError::InvalidTask)?;
        if task.parent != Some(parent) && parent != 0 {
            return Err(WaitError::NotChild);
        }
        match task.status {
            TaskStatus::Exited => {
                let code = task.exit_code.unwrap_or(0);
                task.status = TaskStatus::Reaped;
                task.stack.clear();
                crate::slab::record_free("task-stack", STACK_SIZE);
                task.stack_ptr = 0;
                task.syscall_stack_top = 0;
                task.pml4 = None;
                Ok(code)
            }
            TaskStatus::Reaped => Err(WaitError::AlreadyReaped),
            _ => Err(WaitError::NotExited),
        }
    };
    if result.is_ok() {
        crate::deferred::enqueue(crate::deferred::DeferredWork::PersistTaskSnapshot);
    }
    result
}

pub fn reap_all_exited(parent: usize) -> usize {
    let count = {
        let mut sched = SCHEDULER.lock();
        let mut count = 0usize;
        for (idx, task) in sched.tasks.iter_mut().enumerate() {
            if idx == 0 || task.status != TaskStatus::Exited {
                continue;
            }
            if task.parent != Some(parent) && parent != 0 {
                continue;
            }
            task.status = TaskStatus::Reaped;
            task.stack.clear();
            crate::slab::record_free("task-stack", STACK_SIZE);
            task.stack_ptr = 0;
            task.syscall_stack_top = 0;
            task.pml4 = None;
            count += 1;
        }
        count
    };
    if count > 0 {
        crate::deferred::enqueue(crate::deferred::DeferredWork::PersistTaskSnapshot);
    }
    count
}

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
    let Some(mut sched) = SCHEDULER.try_lock() else {
        return current_rsp;
    };
    if sched.tasks.is_empty() {
        return current_rsp;
    }
    sched.schedule(current_rsp)
}
