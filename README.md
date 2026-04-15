

https://github.com/user-attachments/assets/13f6d9bf-6710-47c9-8f77-65353ecb8c0b

# coolOS

A 64-bit operating system kernel written in Rust. Boots bare-metal into a
graphical desktop with draggable windows, a taskbar, a PS/2 mouse cursor,
and four built-in applications — all running as kernel-mode code with no
scheduler and no userspace. Yet.

---

## Current state — v1.7

The kernel boots into a graphical desktop at **1280×720, 24bpp** via a
`bootloader 0.11` linear framebuffer (VBE BIOS path). A terminal window opens
on boot. Right-clicking the desktop opens a context menu to launch additional
apps.

### What's working

| Subsystem | Details |
| :-------- | :------ |
| **Framebuffer** | `bootloader 0.11` linear framebuffer at ≥1280×720. 3bpp and 4bpp both handled. Shadow-buffer compositor — full frame rendered in a heap `Vec<u32>`, blitted per-row with correct bpp conversion. No tearing. |
| **PS/2 mouse** | Full hardware init (CCB, 0xF6/0xF4), 9-bit signed X/Y deltas, IRQ12 packet collection via atomics. |
| **Window manager** | Z-ordered windows, focus-on-click, title-bar drag, close button, per-window pixel back-buffer. |
| **Taskbar** | 24 px bar at the bottom; one button per open window. |
| **Context menu** | Right-click the desktop to spawn any of the four apps. |
| **Heap** | `LockedHeap` allocator — `String`, `Vec`, `Box` all work. 32 MiB heap to accommodate large shadow and window buffers. |
| **Paging** | 4-level `OffsetPageTable` + bootloader E820 frame allocator. |
| **IDT** | Breakpoint, Double Fault, Page Fault, General Protection Fault, Invalid Opcode, Timer (IRQ0), Keyboard (IRQ1), Mouse (IRQ12). |

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

The build is a two-step process: `cargo build` compiles the kernel ELF, then
`cargo run -p disk-image` wraps it into a BIOS-bootable `bios.img` using
`bootloader 0.11`'s `BiosBoot` builder.

Click inside the QEMU window to capture mouse input. Press **Ctrl+Alt+G** to
release it.

---

## Architecture

```
disk-image/
  src/main.rs      Host tool — wraps kernel ELF into bios.img via bootloader 0.11
src/
  main.rs          Kernel entry point — framebuffer init, heap, windows, main loop
  interrupts.rs    IDT, PIC, keyboard/timer/mouse/fault handlers
  memory.rs        Page table init, physical frame allocator
  allocator.rs     Heap allocator (linked_list_allocator, 32 MiB)
  framebuffer.rs   Linear framebuffer driver — 3bpp/4bpp, draw_char, scroll
  vga_buffer.rs    Text layer over framebuffer — used by print!/panic handler
  mouse.rs         PS/2 mouse hardware init and packet decoder
  keyboard.rs      Lock-free ring buffer — IRQ handler pushes chars, compositor drains
  wm/
    mod.rs         Public WM API — request_repaint, compose_if_needed
    compositor.rs  WindowManager — shadow buffer, z-order, drag, taskbar,
                   context menu, AppWindow enum dispatch, bpp-aware blit
    window.rs      Window struct — back-buffer, hit tests
  apps/
    terminal.rs    TerminalApp — keyboard input, shell commands, text render
    sysmon.rs      SysMonApp   — live CPU/heap/uptime display
    textviewer.rs  TextViewerApp — scrollable static text
    colorpicker.rs ColorPickerApp — clickable EGA palette swatches
```

All app code runs in kernel mode (ring 0). There is no scheduler, no privilege
separation, and no system call interface. That is what the roadmap is for.

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
| 8 | Preemptive scheduler + context switching | Planned |
| 8 | Ring-3 userspace + syscall interface | Planned |
| 9 | Per-process virtual memory + isolation | Planned |
| 10 | Filesystem (FAT32) + VFS + disk driver | Planned |
| 11 | ELF loader — real programs run from disk | Planned |
| 12 | Pipes + shared memory + IPC | Planned |
| 13 | USB HID — real hardware input | Planned |
| 14 | Networking — virtio-net, TCP/IP | Planned |

Full task checklists and technical notes in [ROADMAP.md](ROADMAP.md).

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

**No processes (yet)** — all apps are Rust structs dispatched from the WM's
main loop. Phase 7 (scheduler) and Phase 8 (userspace) replace this with real
concurrent processes. Until then, a crash in any app takes down the whole
kernel, which is expected and fine for this stage of development.
