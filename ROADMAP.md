# coolOS Roadmap

The goal is to evolve coolOS from a kernel-mode GUI demo into a real desktop
operating system — one that can load and run user programs, manage storage, and
support multiple processes without any one of them being able to crash the machine.

Phases 1–9 are complete. Everything below builds directly on that foundation.

---

## ✅ Phases 1–9 — Complete

| Phase | Deliverable |
| :---: | :---------- |
| 1 | Pixel framebuffer (Mode 13h, 320×200, 8bpp) |
| 2 | PS/2 mouse driver + on-screen cursor |
| 3 | Window manager — shadow compositor, z-order, drag |
| 4 | Desktop shell — taskbar, context menu, terminal |
| 5 | Four built-in apps running as kernel-mode modules |
| 6 | High-res linear framebuffer via `bootloader 0.11` — 1280×720, 3/4bpp |
| 7 | Fluid input — lock-free keyboard queue, scratch-buffer blit, release build |
| 8 | Preemptive scheduler — naked timer ISR, round-robin context switching, 100 Hz PIT |
| 9 | Ring-3 userspace — GDT + TSS, SYSCALL/SYSRET, syscall table, iretq trampoline |

### Phase 7 implementation notes

- Removed `without_interrupts` wrapper from the main loop — it was blocking
  all IRQs (mouse, keyboard) for the entire frame blit, causing visible lag.
- Added lock-free keyboard ring buffer (`src/keyboard.rs`): the PS/2 IRQ
  handler was deadlocking by trying to acquire `WM.lock()` while `compose()`
  already held it. IRQ handler now just pushes chars into an atomic queue;
  `compose()` drains it at frame start.
- Replaced per-pixel volatile MMIO writes with a row scratch buffer:
  each row is converted from `u32` shadow pixels to packed BGR bytes into a
  stack-allocated `[u8; 5120]` (fast RAM→RAM), then flushed with one
  `copy_nonoverlapping`. Reduces framebuffer write transactions per frame from
  ~691,200 to 720 bulk copies.
- Switched to `--release` build: LLVM vectorises the pixel conversion loop
  with SSE2/AVX, removing bounds checks. Combined with the above, roughly
  10–20× faster than the debug blit.

### Phase 6 implementation notes

- Replaced `bootloader 0.9.x` + `cargo bootimage` with a host-side `disk-image`
  crate that calls `BiosBoot::new(&kernel).set_boot_config(&cfg).create_disk_image(...)`.
- `BootConfig` requests ≥1280×720; actual resolution negotiated at runtime via VBE.
- `framebuffer.rs` rewritten: accepts base address, width, height, stride, bpp, and
  pixel format from `bootloader_api::info::FrameBufferInfo` at boot time.
- Shadow buffer allocated from heap as `Vec<u32>` (width × height × 4 bytes).
- Compositor blit handles both 3bpp (24-bit, QEMU `-vga std`) and 4bpp (32-bit).
- Font rendered at 2× scale (8×8 glyph → 16×16 pixels) for readability at 1280×720.
- `build-std` moved from `.cargo/config.toml` into Makefile `-Z` flags to prevent
  it bleeding into the host-side `disk-image` crate build.
- Heap increased from 1 MiB to 32 MiB to accommodate the ~3.5 MiB shadow buffer
  and per-window pixel back-buffers.
- Full exception handler coverage added to IDT: page fault (with CR2), general
  protection fault, invalid opcode — all print diagnostics via `println!`.
- Debug console mirroring added (`-debugcon stdio`, port 0xE9) so `println!` output
  appears in the host terminal even when the desktop is rendering.

---

## ✅ Phase 8 — Preemptive Scheduler

**Goal:** Multiple concurrent execution contexts sharing the CPU via timer-driven
preemption. This is the hardest single phase and everything from Phase 9 onwards
depends on it.

- [x] Define a `Task` struct: kernel stack, saved register state (all GP registers +
      `rflags`, `rsp`, `rip`), task ID, status (`Ready` / `Running` / `Blocked`).
- [x] Allocate a fixed kernel stack per task (e.g. 64 KB from the heap).
- [x] Implement context switching — a naked timer ISR (`timer_naked` in
      `src/interrupts.rs`) pushes all 15 GP registers, calls `timer_inner` to
      get the next task's RSP, switches the stack pointer, pops the new task's
      registers, and `iretq`s back into it.
- [x] Build a simple round-robin run-queue (`Vec<Task>` in `src/scheduler.rs`).
- [x] Hook the timer IRQ (IRQ0) to call the scheduler: save the interrupted task's
      full register frame, pick the next `Ready` task, switch context.
- [x] `TaskStatus::Blocked` variant and structural support for `block()` / `unblock(id)`
      exist; full wiring to I/O events deferred to Phase 9.
- [x] Port the existing main loop (compositor tick + `hlt`) to run as the idle task
      (task 0, uses the kernel boot stack — no separate allocation needed).
- [x] Verify: `counter_task` (task 1) increments `BACKGROUND_COUNTER` in a tight
      loop while the WM loop (idle task) runs the desktop; the System Monitor
      displays the counter in cyan, confirming both tasks make progress.

**Exit criteria:** at least two kernel tasks preempt each other correctly under the
timer; no stack corruption; `hlt` in the idle task still fires when no other task is runnable.

### Phase 8 implementation notes

- Replaced the `extern "x86-interrupt"` timer handler with a `#[unsafe(naked)]`
  function (`timer_naked`) that manually pushes all 15 GP registers, calls the
  Rust helper `timer_inner` (which increments `TICKS`, requests a repaint, and
  sends PIC EOI), then does `mov rsp, rax` to switch stacks before popping the
  new task's registers and executing `iretq`. `sym timer_inner` in the
  `naked_asm!` block is the correct way to call a Rust function from a naked ISR.
- IDT timer entry set via `set_handler_addr(VirtAddr::new(timer_naked as *const () as u64))`
  instead of `set_handler_fn` because the naked function does not conform to the
  `extern "x86-interrupt"` ABI.
- New `src/scheduler.rs` owns `Task` (64 KiB heap stack + saved `stack_ptr`),
  `Scheduler` (round-robin `Vec<Task>`), and `pub static SCHEDULER: spin::Mutex<Scheduler>`.
  `Scheduler::empty()` is `const fn` so the global can be initialised without a heap.
- New-task stack initialisation writes a 20-word (160-byte) fake context block:
  15 zeroed GP-register slots followed by a synthetic 5-word interrupt frame
  (RIP = entry fn, CS/SS read live via inline asm, RFLAGS = 0x202, RSP = stack_top − 8).
  On first restore the `iretq` jumps straight to the entry function with correct
  System V AMD64 ABI stack alignment.
- Idle task (index 0) is the kernel boot stack — `stack_ptr` starts as 0 and is
  written on the very first timer preemption, before any switch-away can occur.
- Scheduler initialisation is wrapped in `without_interrupts` to prevent a
  deadlock if the timer fires while `SCHEDULER.lock()` is held during `spawn`.
- `timer_schedule` returns `current_rsp` unchanged when the task list is empty,
  making timer IRQs that fire before task initialisation completely harmless.
- `#[unsafe(naked)]` / `naked_asm!` (stable since Rust 1.88 nightly) replaced
  the old `#[naked]` + `asm!` + `options(noreturn)` spelling.

---

## ✅ Phase 9 — Userspace & System Calls

**Goal:** Ring-3 execution and a minimal syscall interface so that code outside the
kernel can request kernel services without being able to crash it.

- [x] Set up the GDT with four segments: kernel code (ring 0), kernel data (ring 0),
      user code (ring 3), user data (ring 3). Load via `lgdt`.
- [x] Set up the TSS — populate `rsp0` with a dedicated 64 KiB ISR stack so that
      IRQs/exceptions from ring 3 switch to a valid kernel stack.
- [x] Implement `SYSCALL`/`SYSRET` (set `STAR`, `LSTAR`, `SFMASK` MSRs). The syscall
      entry stub saves user registers, dispatches on `rax`, and returns.
- [x] Initial syscall table: `0 exit`, `1 write` (to terminal), `2 yield`, `3 getpid`.
- [x] Implement `jump_to_userspace(entry: u64, user_stack: u64)` — push a fake
      `iretq` frame (user CS/SS, `rflags` with IF set, entry RIP, user RSP) and `iretq`.
- [x] Verify: a minimal Rust userspace stub (syscall via `asm!`) calls `write` to
      print `[ring 3] Hello from userspace!` to the terminal, then calls `exit`.

**Exit criteria:** the kernel can jump to a ring-3 stub; the stub can make a
`write` syscall that prints to the terminal window; an illegal memory access in
userspace generates a #PF that the kernel handles without crashing.

### Phase 9 implementation notes

- New `src/gdt.rs`: `GlobalDescriptorTable` built with `Descriptor::kernel_code_segment`
  (0x08), `Descriptor::kernel_data_segment` (0x10), `Descriptor::user_data_segment`
  (0x18), `Descriptor::user_code_segment` (0x20), and a 64-bit TSS descriptor (0x28).
  `CS::set_reg` / `SS::set_reg` / `load_tss` called after `lgdt`. TSS `privilege_stack_table[0]`
  points to the top of a static 64 KiB `ISR_STACK`; the CPU switches to this on any
  IRQ/exception entry from ring 3.
- STAR MSR: bits[47:32] = 0x08 (kernel CS), bits[63:48] = 0x10 (SYSRET base).
  SYSCALL → CS=0x08, SS=0x10; SYSRET → CS=0x20|RPL3, SS=0x18|RPL3.
- `syscall_entry` (naked): saves user RSP in r10, switches to a static 64 KiB
  `SYSCALL_KERNEL_STACK` via `mov rsp, [rip + SYSCALL_KERNEL_STACK_TOP]`, pushes
  user RSP/RIP(rcx)/RFLAGS(r11) + callee-saved regs, shuffles rax/rdi/rsi/rdx into
  rdi/rsi/rdx/rcx for the SysV ABI call to `syscall_dispatch`, then restores with
  `pop rsp` + `sysretq`.
- `sys_write` pushes bytes into a lock-free ring buffer (`SYSCALL_OUTPUT`, same design
  as `keyboard.rs`). The compositor drains it into the terminal at the start of each
  `compose()` call — avoiding the WM lock deadlock that would arise if `sys_write`
  tried to acquire `WM.lock()` while the idle/WM task already holds it.
- `sys_exit` marks the current scheduler task `Blocked`. The naked handler still
  sysretqs back to ring 3; the stub then spins with `core::hint::spin_loop()` until
  the next timer tick, at which point the scheduler permanently switches away (Blocked
  tasks are never selected as `next`).
- `mark_all_user_accessible` (new in `memory.rs`) walks all four levels of the active
  page table and sets `USER_ACCESSIBLE` on every present PTE, then flushes the TLB.
  Phase 9 is a single-address-space model — the user stub lives in the kernel binary
  and the user stack is a kernel static; making all pages user-accessible lets ring-3
  code execute and access data without a #PF. Phase 10 replaces this with per-process
  page tables.
- PIT reprogrammed to 100 Hz (`init_pit(100)` in `interrupts.rs`) as part of Phase 8
  fix: divisor = 1,193,180 / 100 = 11,931. Renders go from ~9 fps to ~50 fps.

---

## ✅ Phase 10 — Virtual Memory per Process

**Goal:** Each process gets its own isolated page-table hierarchy so processes
cannot read or corrupt each other's memory.

- [x] Extend the `Task` struct with a `PhysFrame` pointing to its top-level PML4.
- [x] On task creation, clone the kernel's PML4 entries into the new process's PML4
      (so kernel mappings are shared), leaving user-space entries empty.
- [x] On context switch, load the new process's PML4 physical address into `cr3`.
      Flush the TLB (or use PCID/ASID to avoid full flushes).
- [x] Implement `mmap(addr, len, flags)` — find free virtual pages in the process's
      address space, allocate physical frames, insert PTEs.
- [x] Implement lazy allocation: map pages as present only on first access; handle
      `#PF` by allocating and mapping the faulting page.
- [x] Guard pages: map a kernel-only page below each stack to catch overflows.
- [x] Verify: two userspace processes with the same virtual addresses for their stacks
      and data cannot read each other's values.

**Exit criteria:** two concurrently running userspace processes are fully isolated;
a write to an unmapped address in one process does not affect the other.

### Phase 10 implementation notes

- New `src/vmm.rs` module holds a global `spin::Mutex<Option<BootInfoFrameAllocator>>`
  and the physical-memory offset.  All page-table work (frame allocation, PML4
  creation, page mapping, CR3 switching) goes through `vmm::`.
- `BootInfoFrameAllocator` gains `next()` and `init_from(regions, start)` so the
  heap can be initialised with one allocator instance and the VMM gets a second
  instance that picks up at the next free frame — no frames are double-allocated.
- `Task` gains `pml4: Option<PhysFrame>`.  Kernel tasks (`None`) share the boot PML4.
  User tasks (`Some`) get their own PML4.  The scheduler calls `vmm::switch_to` on
  every context switch to a task with `Some(pml4)`.
- `vmm::new_process_pml4()` allocates a zeroed 4 KiB frame and shallow-copies L4
  entries 256–511 from the active (boot) PML4.  Lower-half entries start empty so
  each process's user mappings are private.
- User stacks live at `USER_STACK_TOP = 0x0000_7FFF_0010_0000` (L4 index 0xFF —
  confirmed empty in the boot PML4).  Each process gets `USER_STACK_SIZE = 64 KiB`
  of writable, user-accessible pages mapped there, backed by private physical frames.
- Guard page: one kernel-only (`PRESENT`, no `WRITABLE`, no `USER_ACCESSIBLE`) page
  mapped at `USER_STACK_BOTTOM - 4096`.  A ring-3 stack overflow hits a protection-
  violation `#PF` which the fault handler does not lazily recover.
- Lazy `#PF` handler: if the fault is not-present + user-mode + lower-canonical-half,
  allocates a zeroed frame and maps it into the current process's PML4.  All other
  faults (protection violations, kernel faults) still panic.
- `sys_mmap(addr, len, flags)` (syscall 4): maps `len` bytes at `addr` in the
  calling process's address space.  `flags & 1` controls writability.
- `sys_getpid()` (syscall 3) now returns `scheduler.current` (the task index).
- Isolation proof: `userspace.rs` spawns two processes (`pid=1`, `pid=2`), both
  entering `user_stub` at the same kernel `.text` virtual address and using the
  same user stack VA.  Each writes `0xDEAD_0000 + pid` to the stack-top slot and
  reads it back.  Both print `sentinel ok` to the terminal, confirming their stacks
  map to different physical frames.

---

## Phase 11 — Filesystem & Storage ✓

**Goal:** Programs and data live on disk. The kernel can load files by name.

- [x] Write an ATA PIO driver to read 512-byte sectors from a virtual disk image.
- [x] Implement a read-only FAT32 parser — BPB parsing, FAT chain walking, 8.3
      directory traversal, file lookup by absolute path, cluster-to-sector mapping.
- [x] Expose a VFS layer: `vfs_open(path)`, `vfs_read(fd, buf, len)`, `vfs_close(fd)`.
- [x] Map VFS operations to syscalls: `sys_open` (5), `sys_read` (6), `sys_close` (7).
- [x] Build a 64 MiB FAT32 disk image in the Makefile using a host-side `fs-image`
      tool (`fatfs` crate) and attach it to QEMU as the IDE primary-bus slave.

**Implementation notes:**

- `src/ata.rs`: targets primary ATA bus, slave device (0xB0 in DRIVE_HDR).
  Writes `0x02` to the Device Control Register (port `0x3F6`) before each
  command to assert nIEN=1, preventing the drive from firing IRQ14.  Uses
  LBA28 mode with BSY→select→DRQ polling; two independent 10 M-iteration
  timeout loops return `false` without hanging.
- `src/fat32.rs`: `Bpb::load()` parses the boot sector.  `fat_next()` chases
  FAT32 chains 4 bytes at a time.  `find_entry()` scans directory clusters,
  skipping LFN entries.  `read_file(path)` walks `/`-split components top-down
  and returns `Option<Vec<u8>>`.
- `src/vfs.rs`: a 16-slot `FdTable` protected by a `spin::Mutex`.  `vfs_open`
  calls `fat32::read_file` and caches the entire file in a heap `Vec`; `vfs_read`
  copies into the caller's buffer with an offset cursor.
- `interrupts.rs`: `mask_unused_irqs()` called after PIC init masks IRQ3–7 on
  PIC1 and IRQ8–11, IRQ13–15 on PIC2.  Only IRQ0 (timer), IRQ1 (keyboard),
  IRQ2 (cascade), and IRQ12 (mouse) remain unmasked, preventing unhandled
  interrupt vectors from triggering `#GP → #DF`.
- `vmm.rs`: added `switch_to_boot()` which stores the boot PML4 physical address
  at `vmm::init` time and writes it to CR3 when the scheduler resumes a kernel
  task (`pml4 = None`).

**Exit criteria met:** the `fs-test` kernel task opens `/bin/hello.txt` from the
FAT32 image on boot and prints its contents to the console.

---

## Phase 12 — ELF Loader & Process Spawning

**Goal:** The kernel can load a compiled ELF binary from disk, map it into a new
address space, and jump to its entry point.

- [x] Parse ELF64 headers — validate magic, machine type (`x86_64`), entry point.
- [x] Walk `PT_LOAD` segments: allocate virtual pages in the process's address space,
      read segment data from the file into those pages, set PTE flags from segment
      flags (`R`, `W`, `X`).
- [x] Allocate a user stack and map it.
- [x] Build an `argv`/`envp` array on the user stack in the System V AMD64 ABI layout.
- [x] Create a new `Task`, set its `rip` to the ELF entry point and `rsp` to the top
      of the user stack, add it to the run-queue.
- [x] Add a `sys_exec(path)` syscall that calls the ELF loader and replaces the
      calling process's address space.
- [x] Compile a minimal `hello` binary (Rust `#![no_std]` + syscall shim) and
      ship it in `/bin/hello` on the disk image.
- [x] Add an `exec <path>` command to the terminal app.

**Exit criteria:** typing `exec /bin/hello` in the terminal spawns a real
userspace process that prints to the screen and exits cleanly.

**Current status:** complete.

### Phase 12 implementation notes

- `src/elf.rs` now validates ELF64 headers, walks `PT_LOAD` segments, allocates
  a fresh per-process PML4, maps a private user stack, builds a minimal
  `argc=1` / `argv[0]=path` / empty-`envp` startup frame, and can either spawn
  a new task or prepare a loaded image for `sys_exec`.
- `scheduler.rs` gained `spawn_user`, which builds an initial ring-3 interrupt
  frame directly instead of going through a trampoline stub.
- `syscall.rs` now exposes syscall 8, `exec(path, len)`. It loads a new ELF
  image, updates the current task's `pml4`, switches CR3 immediately, and
  rewrites the saved syscall return frame so `sysretq` enters the new image.
- `vmm::new_process_pml4()` now clones from the boot/kernel PML4 rather than
  the currently active user CR3. Without that fix, `sys_exec` inherited stale
  user mappings and collided while remapping the new stack/segments.
- The host-side build now produces two user binaries: `/bin/hello` prints a
  line and exits; `/bin/exec` demonstrates true in-place `sys_exec` by replacing
  itself with `/bin/hello`.

---

## Phase 13 — Inter-Process Communication

**Goal:** Processes can send data to each other and to the GUI without going through
the kernel's internal Rust data structures.

- [ ] Implement anonymous pipes — a fixed-size ring buffer in kernel memory; `sys_pipe`
      returns two file descriptors (read end, write end).
- [ ] Block a reader when the pipe is empty; unblock it when the writer produces data.
- [ ] Implement shared memory — `sys_shmem_create(len)` allocates physical frames and
      maps them into the caller's address space; `sys_shmem_map(id)` maps the same
      frames into another process.
- [ ] Design a simple message-passing protocol so GUI apps can send window events
      (key presses, mouse clicks) to user processes via a pipe rather than via the
      kernel's internal WM dispatch.
- [ ] Port one existing built-in app (e.g. Terminal) to run as a real userspace
      process communicating with the WM over a pipe.

**Exit criteria:** a userspace terminal process receives keyboard events from the
WM via a pipe and writes output back via `sys_write`; the WM renders it without
any shared Rust state.

---

## Phase 14 — USB & Modern Input

**Goal:** Input works on real hardware, not just in QEMU with PS/2 emulation.

- [ ] Write an xHCI host controller driver — detect the MMIO BAR via the PCI config
      space, initialise the command ring, event ring, and transfer rings.
- [ ] Implement USB enumeration — detect connected devices, read descriptors,
      assign addresses.
- [ ] Write a USB HID class driver — parse HID report descriptors for keyboards and
      mice; feed events into the existing keyboard/mouse state.
- [ ] Remove the PS/2 driver dependency for systems that do not support it.

**Exit criteria:** coolOS boots on real x86_64 hardware and accepts keyboard and
mouse input via USB.

---

## Phase 15 — Networking

**Goal:** The kernel can send and receive Ethernet frames; userspace can open TCP
connections.

- [ ] Write a virtio-net driver (MMIO or PCI) to transmit and receive raw Ethernet
      frames.
- [ ] Implement ARP, IPv4, ICMP (ping), UDP, and TCP in the kernel or as a userspace
      network stack over shared memory.
- [ ] Expose `sys_socket`, `sys_connect`, `sys_send`, `sys_recv` syscalls.
- [ ] Ship a `wget` binary in `/bin/` as a proof-of-concept.

**Exit criteria:** `wget http://93.184.216.34/` (example.com by IP) fetches a
response and writes it to a file on disk.

---

## Milestone summary

| Phase | Deliverable | Depends on |
| :---: | :---------- | :--------- |
| 6 | High-resolution framebuffer (`bootloader 0.11`, VBE) | 1–5 |
| 7 | Input lag fixes — keyboard queue, scratch blit, release build | 6 |
| 8 | Preemptive scheduler, context switching | 7 |
| 9 | Ring-3 userspace + syscall interface | 8 |
| 10 | Per-process virtual memory, isolation | 9 |
| 11 | Filesystem (FAT32), VFS, disk driver | 10 |
| 12 | ELF loader, `exec`, real user programs | 11 |
| 13 | Pipes, shared memory, IPC | 12 |
| 14 | USB HID — real hardware input | 9 |
| 15 | Networking (virtio-net, TCP/IP) | 13 |

---

## Technical notes

### The ordering is non-negotiable

Phase 8 (scheduler) is the hardest gate. Every phase from 9 onwards requires
multiple concurrent execution contexts. Don't skip it or fake it with cooperative
yielding — preemption is what makes the OS real.

### Rust in userspace

Userspace binaries can be written in `#![no_std]` Rust with a thin syscall shim.
Eventually a `libcool` crate can wrap the raw syscalls into safe Rust APIs
(`println!`, `File::open`, etc.) and be linked into every userspace binary.

### Real hardware vs QEMU

Phase 6 (VBE framebuffer) and Phase 14 (USB) are the two gates to booting on
real machines. Everything in between can be developed entirely in QEMU.

### Versioning

| Tag | Milestone |
| :-- | :-------- |
| v1.12 | Current — Phase 12 complete: ELF loader, terminal `exec`, working `sys_exec` |
| v3.0 | Phase 9 complete — first userspace process |
| v4.0 | Phase 12 complete — ELF binaries load from disk |
| v5.0 | Phase 15 complete — network-capable |
