https://github.com/user-attachments/assets/a6491da6-a8f3-489c-a1ad-bf6abd71e81f
# coolOS

A 64-bit operating system kernel written in Rust. Boots bare-metal into a
graphical desktop with draggable windows, a taskbar, a PS/2 mouse cursor,
and four built-in applications — now with a preemptive scheduler, ring-3
userspace, per-process virtual memory, process isolation, a FAT32
filesystem with VFS/syscalls, an ELF loader with `exec`, and a full IPC
layer with pipes, shared memory, and per-task fd tables.

---

# Current state — v1.14

The kernel boots into a graphical desktop at **1280×720, 24bpp** via a
`bootloader 0.11` linear framebuffer (VBE BIOS path). A terminal window opens
on boot. Right-clicking the desktop opens a context menu to launch additional
apps. A preemptive round-robin scheduler runs five boot tasks driven by the PIT
timer at **100 Hz**; the terminal can also spawn additional ring-3 ELF tasks
from disk with `exec`:

| Task | Ring | Description |
| :--- | :--- | :---------- |
| **idle/wm** | 0 | The kernel boot stack — runs `compose_if_needed()` + `hlt`. |
| **counter** | 0 | Tight loop incrementing `BACKGROUND_COUNTER`. Visible in System Monitor. |
| **fs-test** | 0 | One-shot: reads `/bin/hello.txt` from disk and prints it, then blocks. |
| **user1** | 3 | Own PML4 + private user stack. Writes sentinel `0xDEAD0001`, reads it back, prints `[ring3 pid=1] sentinel ok`. |
| **user2** | 3 | Own PML4 + private user stack at the same VA as user1. Writes `0xDEAD0002` — cannot see user1's memory. |

On boot, the contents of `/bin/hello.txt` are printed to the console by the
`fs-test` task. Both `[ring3 pid=1] sentinel ok` and `[ring3 pid=2] sentinel ok`
appear in the terminal, proving process isolation: same virtual address,
different physical frames. Typing `exec /bin/hello` launches a real userspace
ELF from disk, `exec /bin/exec` demonstrates `sys_exec` by replacing a running
userspace image with `/bin/hello`, `exec /bin/pipe` exercises per-process
anonymous pipes, `ipc` allocates a fresh shared pipe and launches the
reader/writer demo pair by inheriting one pipe end into each child, `keydemo`
streams focused terminal keystrokes into a userspace process over an inherited
pipe, and `exec /bin/read` exercises userspace `open/read/close` against the
FAT32-backed VFS.

### What's working

| Subsystem | Details |
| :-------- | :------ |
| **Framebuffer** | `bootloader 0.11` linear framebuffer at ≥1280×720. 3bpp and 4bpp both handled. Shadow-buffer compositor — full frame rendered in a heap `Vec<u32>`, blitted per-row with correct bpp conversion. No tearing. |
| **PS/2 mouse** | Full hardware init (CCB, 0xF6/0xF4), 9-bit signed X/Y deltas, IRQ12 packet collection via atomics. |
| **Window manager** | Z-ordered windows, focus-on-click, title-bar drag, close button, per-window pixel back-buffer. |
| **Taskbar** | 24 px bar at the bottom; one button per open window. |
| **Context menu** | Right-click the desktop to spawn any of the four apps. |
| **Heap** | `LockedHeap` allocator — `String`, `Vec`, `Box` all work. 32 MiB heap to accommodate large shadow and window buffers. |
| **Paging / VMM** | 4-level `OffsetPageTable` + global `BootInfoFrameAllocator`. Per-process PML4 cloned from kernel upper half; private user-space mappings in lower half. `vmm::` module exposes `new_process_pml4`, `map_page_in`, `map_region`, `switch_to`. |
| **IDT** | Breakpoint, Double Fault, Page Fault (lazy allocator for user faults), General Protection Fault, Invalid Opcode, Timer (IRQ0), Keyboard (IRQ1), Mouse (IRQ12). |
| **Scheduler** | Preemptive round-robin at 100 Hz. Each task carries `pml4: Option<PhysFrame>`; the scheduler calls `vmm::switch_to` on context switch when `Some`. 64 KiB heap-allocated kernel stack per task. Basic `block_current` / `unblock(id)` wakeup helpers back blocking pipe reads. |
| **Process isolation** | Two user processes share the same user-stack virtual address (`0x7FFF_0010_0000`) but map it to different physical frames. Guard pages (kernel-only) sit below each stack. |
| **GDT + TSS** | Four segments (kernel code/data ring 0, user code/data ring 3) + TSS with RSP0 pointing to a dedicated 64 KiB ISR stack used when an IRQ fires during ring-3 execution. |
| **SYSCALL/SYSRET** | EFER.SCE enabled. STAR/LSTAR/SFMASK MSRs configured. Naked `syscall_entry` saves context, switches to the currently scheduled task's private kernel stack top, dispatches on rax, restores context, and executes `sysretq`. |
| **Syscall table** | `0 exit`, `1 write`, `2 yield`, `3 getpid`, `4 mmap(addr, len, flags)`, `5 open(path, len)`, `6 read(fd, buf, len)`, `7 close(fd)`, `8 exec(path, len)`, `9 pipe(fds_ptr)`, `10 dup(fd)`, `11 shmem_create(len)`, `12 shmem_map(id)`. `sys_write` writes to stdout/stderr through the compositor ring buffer or to pipe write-ends through the VFS fd table. |
| **Userspace** | Ring-3 code can run either as the original isolation stubs or as real ELF64 binaries loaded from `/bin`. `sys_exec` replaces the current userspace image in-place by swapping CR3 and rewriting the saved syscall return frame. `keydemo` relays fixed-size key and click event packets into a userspace child over a pipe. Shared memory (`sys_shmem_create`/`sys_shmem_map`) maps a region of physical frames into the caller's address space at a fixed VA. |
| **ELF loader** | Validates ELF64 headers, maps `PT_LOAD` segments into a fresh address space, allocates a private user stack, builds an initial `argc/argv/envp` stack frame, and can either spawn a new task or prepare an image for `sys_exec`. |
| **ATA PIO driver** | Primary-bus slave device (QEMU `if=ide,index=1`). LBA28 PIO reads, BSY/DRQ polling with timeout, nIEN=1 (device interrupts disabled). Wrapped in `without_interrupts` to prevent preemption mid-transfer. |
| **FAT32 parser** | Read-only. BPB parsing, FAT chain walking, 8.3 filename lookup, directory traversal, cluster→sector mapping. `fat32::read_file(path)` returns `Option<Vec<u8>>`. |
| **VFS** | Task-local fd tables (16 slots, fds 0–2 reserved) backed by shared file/pipe/shmem objects. `vfs_open` reads whole files into heap buffers; `vfs_pipe` allocates a 512-byte kernel ring buffer and returns per-task read/write fds; `vfs_read_blocking` blocks tasks on empty pipes and wakes them on write/EOF; `ipc` selectively inherits pipe ends into child processes; `vfs_shmem_create`/`vfs_shmem_map` manage a shared memory region pool indexed by ID. |
| **Disk image** | `disk-image/src/fs-image.rs` builds `fs.img` (64 MiB FAT32) with `/bin/hello.txt`, `/bin/hello`, `/bin/exec`, `/bin/pipe`, `/bin/piperd`, `/bin/pipewr`, `/bin/keyecho`, and `/bin/read`. The Makefile attaches it to QEMU as the IDE slave. |

### Applications

| App | How to open | Description |
| :-- | :---------- | :---------- |
| **Terminal** | Boot / right-click | Interactive shell. Type commands, press Enter. |
| **System Monitor** | Right-click | Live CPU vendor, heap usage, uptime. |
| **Text Viewer** | Right-click | Scrollable "About" doc; `j`/`k` to scroll. |
| **Color Picker** | Right-click | Clickable 16-colour EGA palette grid. |

### Terminal commands

| Command | Description |
| :------ | :---------- |
| `help` | List available commands |
| `echo <text>` | Print text |
| `exec <path> [args...]` | Load a userspace ELF from disk and spawn it with argv |
| `ipc` | Create a shared pipe and launch the userspace reader/writer demo |
| `keydemo` | Send fixed-size WM key and click event packets to `/bin/keyecho` over an inherited pipe until `~` closes it |
| `term` | Launch a userspace terminal as a ring-3 process with pipe-based input (Ctrl+D to exit) |
| `info` | CPU vendor and heap usage |
| `uptime` | Timer ticks and seconds since boot |
| `clear` | Clear the terminal |
| `reboot` | Hardware reset |

---

## Getting started

### Prerequisites

```bash
rustup toolchain install nightly
rustup component add rust-src
# macOS:
brew install qemu
```

### Build and run

```bash
make run
```

The build process compiles the kernel ELF, compiles the userspace ELF binaries
in `userspace/hello/`, wraps the kernel into a BIOS-bootable `bios.img`, and
builds `fs.img` with the userspace binaries embedded into `/bin`.

Click inside the QEMU window to capture mouse input. Press **Ctrl+Alt+G** to
release it.

---

## Architecture

```
disk-image/
  src/main.rs      Host tool — wraps kernel ELF into bios.img via bootloader 0.11
src/fs-image.rs  Host tool — builds fs.img (64 MiB FAT32) with /bin/hello.txt,
                    /bin/hello, /bin/exec, /bin/pipe, /bin/piperd, /bin/pipewr,
                    /bin/keyecho, and /bin/read (Phase 13 binaries)
src/
  main.rs          Kernel entry point — framebuffer init, GDT, heap, scheduler, main loop
  gdt.rs           GDT (ring-0/ring-3 segments) + TSS (RSP0 for ring-3 IRQ entry)
  interrupts.rs    IDT, PIC, PIT (100 Hz), IRQ masking, keyboard/timer(naked)/mouse/fault handlers
  syscall.rs       SYSCALL/SYSRET — naked entry, dispatcher, lock-free output buffer,
                   jump_to_userspace (iretq trampoline); sys_open/read/write/close/exec/pipe
  elf.rs           ELF64 loader — parse headers, map PT_LOAD segments, build user images
  userspace.rs     Two isolated ring-3 processes — spawn_user_process(pid), user_stub
  memory.rs        Page table init, BootInfoFrameAllocator (with next/init_from),
                   mark_all_user_accessible
  vmm.rs           Virtual Memory Manager — global frame alloc, new_process_pml4,
                   map_page_in, map_region, switch_to, switch_to_boot, alloc_zeroed_frame
  allocator.rs     Heap allocator (linked_list_allocator, 32 MiB)
  scheduler.rs     Preemptive scheduler — Task (with pml4 field), Scheduler,
                   SCHEDULER global, timer_schedule, spawn_with_pml4
  ata.rs           ATA PIO driver — LBA28 read_sector, BSY/DRQ polling, nIEN disable
  fat32.rs         Read-only FAT32 — BPB, FAT chain, 8.3 directory lookup, read_file
vfs.rs           VFS — task-local fd tables over shared file/pipe/shmem objects,
                    selective child-fd inheritance, vfs_open/vfs_pipe/vfs_read/vfs_write/vfs_close,
                    vfs_shmem_create/vfs_shmem_map
  framebuffer.rs   Linear framebuffer driver — 3bpp/4bpp, draw_char, scroll
  vga_buffer.rs    Text layer over framebuffer — used by print!/panic handler
  mouse.rs         PS/2 mouse hardware init and packet decoder
  keyboard.rs      Lock-free ring buffer — IRQ handler pushes chars, compositor drains
  wm/
    mod.rs         Public WM API — request_repaint, compose_if_needed
    compositor.rs  WindowManager — shadow buffer, z-order, drag, taskbar,
                   context menu, syscall output drain, AppWindow enum dispatch, blit
    window.rs      Window struct — back-buffer, hit tests
  apps/
    terminal.rs    TerminalApp — keyboard input, shell commands, text render
    sysmon.rs      SysMonApp   — live CPU/heap/uptime/scheduler display
    textviewer.rs  TextViewerApp — scrollable static text
    colorpicker.rs ColorPickerApp — clickable EGA palette swatches
userspace/
  hello/
    src/main.rs    `/bin/hello` — minimal userspace ELF that writes and exits
    src/bin/exec.rs `/bin/exec` — userspace `sys_exec` demo that replaces itself with `/bin/hello`
    src/bin/piperd.rs `/bin/piperd` — userspace shared-pipe reader demo
    src/bin/pipewr.rs `/bin/pipewr` — userspace shared-pipe writer demo
    src/bin/keyecho.rs `/bin/keyecho` — userspace WM key/click event packet demo
    src/bin/read.rs `/bin/read` — userspace VFS demo that opens and reads `/bin/motd.txt`
    linker.ld      Fixed-address linker script for the userspace ELF binaries
```

---

## Design notes

**Why Rust?** The `#[no_std]` ecosystem is mature enough for kernel work, and
memory safety at the kernel level eliminates whole categories of bugs
(use-after-free, buffer overflows) that make C kernel development painful. The
borrow checker enforces ownership of hardware resources at compile time.

**`bootloader 0.11` and the disk-image tool.** The old `bootimage` approach
(bootloader 0.9.x + `cargo bootimage`) shipped a fixed 320×200 VGA framebuffer.
Phase 6 replaced it with a host-side `disk-image` crate that calls
`BiosBoot::new(&kernel).set_boot_config(&cfg).create_disk_image(...)`,
requesting ≥1280×720. The bootloader negotiates a VBE mode with QEMU's SeaBIOS
and hands the kernel a `FrameBufferInfo` struct at boot time.

**3bpp vs 4bpp.** QEMU's standard VGA (`-vga std`) delivers a 24bpp (3
bytes/pixel) framebuffer even when 32bpp is requested. The compositor and all
direct-write paths now check `bytes_per_pixel` at runtime and write 3 or 4
bytes per pixel accordingly. The shadow buffer stays `u32` (0x00RRGGBB)
throughout; the bpp conversion happens only at blit time.

**Shadow-buffer compositing** renders the full frame — desktop fill, windows
back-to-front, cursor sprite — into a heap-allocated `Vec<u32>`, then blits
the finished frame to the hardware framebuffer row by row. The display sees
only complete frames, eliminating tearing and partial redraws.

**Preemptive scheduler (Phase 8).** The timer ISR (`timer_naked` in
`interrupts.rs`) is a `#[unsafe(naked)]` function using `naked_asm!`. It
pushes all 15 GP registers onto the current stack, calls `timer_inner` (which
handles ticks/repaint/EOI and delegates to `scheduler::timer_schedule`), then
does `mov rsp, rax` to switch to the winning task's stack before popping its
registers and executing `iretq`. New tasks are given a 64 KiB heap-allocated
kernel stack pre-populated with a fake 20-word interrupt frame so the first
`iretq` drops straight into the entry function. The scheduler also tracks each
task's kernel stack top so syscall entry can follow the currently running task
instead of relying on one shared syscall stack.

**Ring-3 userspace (Phase 9).** The GDT now has four segments (kernel code
0x08, kernel data 0x10, user data 0x18, user code 0x20) plus a TSS whose RSP0
points to a dedicated 64 KiB ISR stack used when an IRQ fires during ring-3
execution. SYSCALL/SYSRET is enabled via EFER.SCE; STAR is set so that
SYSCALL enters kernel CS=0x08/SS=0x10 and SYSRET returns to user
CS=0x20/SS=0x18. The naked `syscall_entry` stub saves user RSP in r10,
switches to the currently scheduled task's private kernel stack top, builds a
register frame, calls the Rust `syscall_dispatch`, and restores with `pop rsp`
and `sysretq`.
`sys_write` output goes through a lock-free ring buffer (same pattern as the
keyboard queue) that the compositor drains into the terminal each frame —
avoiding the deadlock that would result from locking WM from syscall context.

**FAT32 + VFS (Phase 11).** A 64 MiB FAT32 disk image (`fs.img`) is built at
compile time by a host-side `fs-image` tool and attached to QEMU as the IDE
primary-bus slave (`if=ide,index=1`). The ATA PIO driver targets ports
0x1F0–0x1F7; it sets nIEN=1 in the Device Control Register (0x3F6) before
issuing any command so the drive never fires IRQ14. Unused PIC IRQs (including
IRQ14/15) are masked after PIC initialisation to prevent unhandled interrupt
vectors from reaching the CPU. The read-only FAT32 layer parses the BPB, walks
the FAT chain, and resolves 8.3 absolute paths. A thin VFS layer wraps this
into a 16-slot FD table. Syscalls 5–7 (`open`, `read`, `close`) expose the VFS
to ring-3 code, and the kernel's `fs-test` task reads `/bin/hello.txt` on boot.

**Per-process virtual memory (Phase 10).** Each user task owns a PML4 cloned
from the kernel's boot PML4 (upper-half entries 256–511 copied; lower half
empty). `vmm::new_process_pml4` handles the clone; `vmm::map_page_in` / `vmm::map_region`
insert PTEs into any address space by constructing a temporary `OffsetPageTable`
over the target PML4 frame. The scheduler writes the winning task's PML4 into
CR3 on every context switch. User stacks are mapped at `0x7FFF_0010_0000` in the
lower half — L4 index 0xFF, which the kernel never populates — so two processes
at the same VA have completely separate physical frames. A kernel-only guard page
sits below each stack. The `#PF` handler lazily allocates zeroed frames for
not-present user-mode faults in the lower half; protection violations and kernel
faults still panic.

---

## Roadmap

| Phase | Deliverable | Status |
| :---: | :---------- | :----- |
| 1 | Pixel framebuffer + font rendering | **Done** |
| 2 | PS/2 mouse driver + on-screen cursor | **Done** |
| 3 | Window manager — draggable windows, focus, close | **Done** |
| 4 | Desktop shell — taskbar, context menu, terminal app | **Done** |
| 5 | Applications — system monitor, text viewer, color picker | **Done** |
| 6 | High-resolution framebuffer via `bootloader 0.11` (1280×720) | **Done** |
| 7 | Input lag fixes — lock-free keyboard queue, scratch-buffer blit, release build | **Done** |
| 8 | Preemptive scheduler + context switching (100 Hz PIT) | **Done** |
| 9 | Ring-3 userspace + SYSCALL/SYSRET interface | **Done** |
| 10 | Per-process virtual memory + isolation | **Done** |
| 11 | Filesystem (FAT32) + VFS + disk driver | **Done** |
| 12 | ELF loader — real programs run from disk | **Done** |
| 13 | Pipes + shared memory + IPC + userspace terminal | **Done** |
| 14 | USB HID — real hardware input | Planned |
| 15 | Networking — virtio-net, TCP/IP | Planned |
| 16 | UI Polish — desktop icons, start menu, wallpaper | Planned |

Full task checklists and technical notes in [ROADMAP.md](ROADMAP.md).
