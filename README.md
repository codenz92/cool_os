# coolOS

A 64-bit operating system kernel written in Rust, currently evolving from a text-mode
shell into a graphical desktop OS with a windowing system.

See [ROADMAP.md](ROADMAP.md) for the full plan.

---

## Current state (v1.2)

The kernel boots into an interactive shell running over the VGA text buffer.
Memory management, hardware interrupts, and a heap allocator are all functional.

### Features

- **Dynamic heap** — `LockedHeap` allocator enabling `String`, `Vec`, and `Box` in a `no_std` kernel.
- **4-level paging** — `OffsetPageTable` maps physical frames into virtual address space.
- **Physical frame allocator** — discovers usable RAM from the bootloader's E820 memory map.
- **Custom IDT** — handles Breakpoint and Double Fault CPU exceptions.
- **Hardware interrupts** — 8259 PIC configured for the system timer (IRQ0) and PS/2 keyboard (IRQ1).
- **Thread-safe VGA driver** — `spin::Mutex`-protected text buffer with colour, scrolling, and `println!`.
- **Interactive shell** — tokenizer, heap-backed command history, and the commands below.

### Shell commands

| Command | Description |
| :------ | :---------- |
| `help` | List available commands. |
| `clear` | Clear the screen. |
| `echo <text>` | Print text back. |
| `color <name>` | Change text colour (`red`, `green`, `blue`, `yellow`, `white`). |
| `info` | Show CPU vendor and heap usage. |
| `uptime` | Show timer ticks since boot. |
| `reboot` | Hardware reset. |

![coolOS shell](https://github.com/user-attachments/assets/dd88a04d-e211-46e4-bf6f-8166c41e3628)

---

## Roadmap

The project is being built toward a graphical desktop OS. The five phases are:

| Phase | Goal | Status |
| ----- | ---- | ------ |
| 1 | Pixel framebuffer + font rendering | Not started |
| 2 | PS/2 mouse driver + on-screen cursor | Not started |
| 3 | Window manager — draggable windows | Not started |
| 4 | Desktop shell — taskbar, context menu, terminal window | Not started |
| 5 | Applications — terminal, system monitor, text viewer | Not started |

Full details in [ROADMAP.md](ROADMAP.md).

---

## Getting started

### Prerequisites

```bash
rustup toolchain install nightly
rustup component add rust-src
cargo install bootimage
# macOS:
brew install qemu
```

### Build and run

```bash
make run
```

### Architecture

```text
src/
  main.rs          # Kernel entry point, hardware init, heap setup
  interrupts.rs    # IDT, PIC, keyboard + timer handlers, shell command processor
  memory.rs        # Page table init, physical frame allocator
  allocator.rs     # Heap allocator (linked_list_allocator wrapper)
  vga_buffer.rs    # Text-mode VGA driver (will be replaced in Phase 1)
```

The bootloader crate handles the transition from real mode to 64-bit long mode and
passes a `BootInfo` struct (physical memory map, memory offset) to `kernel_main`.
