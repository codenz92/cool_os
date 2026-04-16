# coolOS Roadmap

The goal is to evolve coolOS from a kernel-mode GUI demo into a real desktop
operating system — one that can load and run user programs, manage storage, and
support multiple processes without any one of them being able to crash the machine.

Phases 1–8 are complete. Everything below builds directly on that foundation.

---

## ✅ Phases 1–8 — Complete

| Phase | Deliverable |
| :---: | :---------- |
| 1 | Pixel framebuffer (Mode 13h, 320×200, 8bpp) |
| 2 | PS/2 mouse driver + on-screen cursor |
| 3 | Window manager — shadow compositor, z-order, drag |
| 4 | Desktop shell — taskbar, context menu, terminal |
| 5 | Four built-in apps running as kernel-mode modules |
| 6 | High-res linear framebuffer via `bootloader 0.11` — 1280×720, 3/4bpp |
| 7 | Fluid input — lock-free keyboard queue, scratch-buffer blit, release build |
| 8 | Preemptive scheduler — naked timer ISR, round-robin context switching, idle + counter tasks |

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

## Phase 9 — Userspace & System Calls

**Goal:** Ring-3 execution and a minimal syscall interface so that code outside the
kernel can request kernel services without being able to crash it.

- [ ] Set up the GDT with four segments: kernel code (ring 0), kernel data (ring 0),
      user code (ring 3), user data (ring 3). Load via `lgdt`.
- [ ] Set up the TSS — populate `rsp0` with the current kernel stack pointer so that
      hardware task-switches on interrupt save state to the right place.
- [ ] Implement `SYSCALL`/`SYSRET` (set `STAR`, `LSTAR`, `SFMASK` MSRs). The syscall
      entry stub saves user registers, dispatches on `rax`, and returns.
- [ ] Initial syscall table (numbers subject to change):
      `0 exit`, `1 write` (to terminal), `2 yield`, `3 getpid`.
- [ ] Implement `jump_to_userspace(entry: u64, user_stack: u64)` — push a fake
      `iretq` frame (user CS/SS, `rflags` with IF set, entry RIP, user RSP) and `iretq`.
- [ ] Verify: a minimal Rust userspace binary (compiled with `#![no_std]`, syscall
      via `asm!`) can call `write` to print a string and `exit` without triple-faulting.

**Exit criteria:** the kernel can jump to a ring-3 stub; the stub can make a
`write` syscall that prints to the terminal window; an illegal memory access in
userspace generates a #PF that the kernel handles without crashing.

---

## Phase 10 — Virtual Memory per Process

**Goal:** Each process gets its own isolated page-table hierarchy so processes
cannot read or corrupt each other's memory.

- [ ] Extend the `Task` struct with a `PhysFrame` pointing to its top-level PML4.
- [ ] On task creation, clone the kernel's PML4 entries into the new process's PML4
      (so kernel mappings are shared), leaving user-space entries empty.
- [ ] On context switch, load the new process's PML4 physical address into `cr3`.
      Flush the TLB (or use PCID/ASID to avoid full flushes).
- [ ] Implement `mmap(addr, len, flags)` — find free virtual pages in the process's
      address space, allocate physical frames, insert PTEs.
- [ ] Implement lazy allocation: map pages as present only on first access; handle
      `#PF` by allocating and mapping the faulting page.
- [ ] Guard pages: map a zero-size sentinel page below each stack to catch overflows.
- [ ] Verify: two userspace processes with the same virtual addresses for their stacks
      and data cannot read each other's values.

**Exit criteria:** two concurrently running userspace processes are fully isolated;
a write to an unmapped address in one process does not affect the other.

---

## Phase 11 — Filesystem & Storage

**Goal:** Programs and data live on disk. The kernel can load files by name.

- [ ] Write a virtio-blk driver (or ATA PIO driver for real hardware) to read 512-byte
      sectors from a virtual disk image.
- [ ] Implement a read-only FAT32 parser — directory traversal, file lookup by path,
      reading file data into a heap buffer.
- [ ] Add write support to FAT32 — allocate clusters, update FAT chains, write
      directory entries.
- [ ] Expose a VFS layer with a minimal trait: `open(path)`, `read(fd, buf)`,
      `write(fd, buf)`, `close(fd)`.
- [ ] Map VFS operations to syscalls: `sys_open`, `sys_read`, `sys_write`, `sys_close`.
- [ ] Build a disk image in the Makefile (`dd` + `mkfs.fat`) and mount it as a QEMU
      virtio-blk device. Populate it with a `/bin/` directory.

**Exit criteria:** the kernel can open `/bin/hello` from the disk and read its bytes
into memory via the VFS syscall interface.

---

## Phase 12 — ELF Loader & Process Spawning

**Goal:** The kernel can load a compiled ELF binary from disk, map it into a new
address space, and jump to its entry point.

- [ ] Parse ELF64 headers — validate magic, machine type (`x86_64`), entry point.
- [ ] Walk `PT_LOAD` segments: allocate virtual pages in the process's address space,
      read segment data from the file into those pages, set PTE flags from segment
      flags (`R`, `W`, `X`).
- [ ] Allocate a user stack (e.g. 1 MB starting at `0x7fff_0000_0000`) and map it.
- [ ] Build an `argv`/`envp` array on the user stack in the System V AMD64 ABI layout.
- [ ] Create a new `Task`, set its `rip` to the ELF entry point and `rsp` to the top
      of the user stack, add it to the run-queue.
- [ ] Add a `sys_exec(path)` syscall that calls the ELF loader and replaces the
      calling process's address space.
- [ ] Compile a minimal `hello` binary (Rust `#![no_std]` + syscall shim) and
      ship it in `/bin/hello` on the disk image.
- [ ] Add an `exec <path>` command to the terminal app.

**Exit criteria:** typing `exec /bin/hello` in the terminal spawns a real
userspace process that prints to the screen and exits cleanly.

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
| 6  | High-resolution framebuffer (`bootloader 0.11`, VBE) | 1–5 |
| 7  | Input lag fixes — keyboard queue, scratch blit, release build | 6 |
| 8  | Preemptive scheduler, context switching | 7 |
| 9  | Ring-3 userspace + syscall interface | 8 |
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
| v1.7 | Current — high-res framebuffer, fluid input, release build |
| v3.0 | Phase 9 complete — first userspace process |
| v4.0 | Phase 12 complete — ELF binaries load from disk |
| v5.0 | Phase 15 complete — network-capable |
