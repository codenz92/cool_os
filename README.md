# coolOS

A 64-bit operating system kernel written in Rust that boots into a graphical
desktop — windowed apps, a taskbar, a PS/2 mouse cursor, and a live system
monitor, all running as bare-metal kernel code.

See [ROADMAP.md](ROADMAP.md) for the full development history.

---

## Current state (v1.5)

The OS boots directly into a graphical desktop running in VGA Mode 13h
(320×200, 256 colours). A terminal window opens on boot; right-clicking the
desktop spawns more apps from a context menu.

### Features

- **Pixel framebuffer** — VGA Mode 13h (320×200, 8 bpp) enabled by the
  bootloader. Shadow-buffer compositing eliminates screen tearing: the full
  frame is rendered in RAM and blitted to `0xA0000` in one `memcpy`.
- **PS/2 mouse driver** — full hardware init sequence (CCB, 0xF6/0xF4),
  9-bit signed X/Y deltas, IRQ12 packet collection via atomic bytes.
- **Window manager** — z-ordered windows with focus-on-click, title-bar
  dragging, and a close button. Each window has its own pixel back-buffer.
- **Taskbar** — 12 px bar at the bottom; one button per open window, click
  to raise and focus.
- **Right-click context menu** — spawns any of the four built-in apps.
- **Shadow cursor** — 8×8 arrow sprite drawn on top of every frame.
- **Dynamic heap** — `LockedHeap` allocator; `String`, `Vec`, `Box` all work.
- **4-level paging** — `OffsetPageTable` + bootloader E820 frame allocator.
- **IDT** — Breakpoint, Double Fault, Timer (IRQ0), Keyboard (IRQ1),
  Mouse (IRQ12).

### Apps

| App | Open via | Description |
| :-- | :------- | :---------- |
| **Terminal** | boot / right-click | Interactive shell — type commands, press Enter |
| **System Monitor** | right-click | Live CPU vendor, heap usage, and uptime |
| **Text Viewer** | right-click | Scrollable "About" doc; `j`/`k` to scroll |
| **Color Picker** | right-click | Clickable 16-colour VGA palette grid |

### Terminal commands

| Command | Description |
| :------ | :---------- |
| `help` | List available commands |
| `echo <text>` | Print text |
| `info` | CPU vendor and heap usage |
| `uptime` | Timer ticks and approximate seconds since boot |
| `clear` | Clear the terminal |
| `reboot` | Hardware reset |

---

## Roadmap

| Phase | Deliverable | Status |
| :---: | :---------- | :----- |
| 1 | Pixel framebuffer + font rendering | **Done** |
| 2 | PS/2 mouse driver + on-screen cursor | **Done** |
| 3 | Window manager — draggable windows, focus, close | **Done** |
| 4 | Desktop shell — taskbar, context menu, terminal app | **Done** |
| 5 | Applications — system monitor, text viewer, color picker | **Done** |

Full details and task checklists in [ROADMAP.md](ROADMAP.md).

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

Click inside the QEMU window to capture mouse input. Press **Ctrl+Alt+G** to
release it.

---

## Architecture

```text
src/
  main.rs            Kernel entry point — heap init, window setup, main loop
  interrupts.rs      IDT, PIC, keyboard/timer/mouse handlers
  memory.rs          Page table init, physical frame allocator
  allocator.rs       Heap allocator (linked_list_allocator wrapper)
  framebuffer.rs     VGA Mode 13h pixel driver — put_pixel, draw_char, scroll
  vga_buffer.rs      Text layer over framebuffer — used by panic handler
  mouse.rs           PS/2 mouse hardware init and packet decoder
  wm/
    mod.rs           Public WM API — request_repaint, compose_if_needed
    compositor.rs    WindowManager — shadow buffer, z-order, drag, taskbar,
                     context menu, AppWindow enum dispatch
    window.rs        Window struct — back-buffer, hit tests
  apps/
    terminal.rs      TerminalApp — keyboard input, shell commands, text render
    sysmon.rs        SysMonApp   — live CPU/heap/uptime display
    textviewer.rs    TextViewerApp — scrollable static text
    colorpicker.rs   ColorPickerApp — clickable VGA palette swatches
```

The bootloader sets VGA Mode 13h before jumping to the kernel. The kernel
identity-maps the framebuffer at `0xA0000` and writes pixels directly. All
app code runs in kernel mode — no scheduler, no privilege separation.
