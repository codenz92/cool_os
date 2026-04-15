# coolOS Desktop Roadmap

The goal is to evolve coolOS from a VGA text-mode shell into a graphical desktop OS
with a windowing system, mouse support, and runnable applications.

Each phase builds directly on the last. Nothing in a later phase is possible without
completing the phase before it.

---

## Phase 1 — Pixel Framebuffer
**Goal:** Replace the VGA text driver with a pixel-addressable display.

Everything graphical — fonts, windows, a cursor — requires drawing individual pixels.
The existing `vga_buffer.rs` only writes character cells to the 80×25 text buffer at
`0xb8000`. This phase swaps it out for a real framebuffer.

### Crates

- [`font8x8`](https://crates.io/crates/font8x8) — pure `no_std` 8×8 bitmap font glyphs,
  no dependencies, no rendering backend required. Ideal for early boot text.

> **Note on Redox OS:** `redox-os/vesad` (their VESA driver) and `redox-os/orbfont`
> (their font renderer) were evaluated but both require Redox's scheme/IPC runtime and
> are not usable in a bare-metal kernel. `font8x8` is the correct replacement.

### Tasks

- [x] Add `vga_320x200` feature to the `bootloader` crate and `font8x8 = "0.2.4"` to
      `Cargo.toml`. The bootloader sets Mode 13h (320×200, 8bpp at `0xA0000`) before
      jumping to the kernel — no VGA register programming needed in the kernel.
- [x] Write `src/framebuffer.rs` — exposes `put_pixel`, `fill_rect`, `clear`,
      `scroll_up`, and `draw_char` backed by the identity-mapped `0xA0000` buffer.
- [x] Rewrote `src/vga_buffer.rs` — `Writer` now tracks `(col, row)` in character
      cells and calls `framebuffer::draw_char` instead of writing to the text buffer.
      Public API (`println!`, `print!`, `clear_screen`, `backspace`, `set_color`,
      `Color` enum) unchanged so nothing else in the kernel needed touching.
- [x] Added `mod framebuffer` to `src/main.rs`.

### Definition of done
Kernel boots into a solid-colour background with "coolOS booting..." rendered in pixels.

**Status: COMPLETE** — builds clean, shell runs in Mode 13h pixel graphics.

---

## Phase 2 — PS/2 Mouse Driver
**Goal:** Read mouse movement and button clicks from hardware.

The keyboard already works via IRQ1. The PS/2 mouse sits on IRQ12 (PIC2, line 4).
This phase adds a second interrupt handler and a global mouse state.

### Tasks

- [x] Enable IRQ12 on the secondary PIC — unmask bit 4 of PIC2 IMR (port 0xA1) and
      bit 2 of PIC1 IMR (cascade line) inside `mouse::init()`.
- [x] Write `src/mouse.rs` — full PS/2 init sequence (enable aux, set CCB, 0xF6
      defaults, 0xF4 enable reporting). `handle_packet(b0, b1, b2)` decodes 9-bit
      signed X/Y deltas and button state.
- [x] Global `Cursor` state with clamped `(x, y)`, `left`, `right` fields.
- [x] Software cursor: 8×8 arrow bitmap, save/restore pixels underneath on every move.
- [x] IRQ12 handler in `interrupts.rs` collects bytes via three `AtomicU8`s (no lock
      needed — interrupts are disabled during the handler), validates sync bit, calls
      `mouse::handle_packet`.

### Definition of done
A visible cursor moves around the screen in response to the physical/QEMU mouse.
Left-click state is readable from kernel code.

**Status: COMPLETE** — builds clean, cursor appears at screen centre on boot.

> **QEMU note:** click inside the QEMU window to capture mouse input.
> Press **Ctrl+Alt+G** to release it.

---

## Phase 3 — Window Manager Core
**Goal:** Create, draw, and stack rectangular windows on screen.

This is the heart of a desktop OS. A window manager owns a list of windows and is
responsible for painting them in order (back to front) whenever anything changes.

### Tasks
- [ ] Write `src/wm/window.rs` — a `Window` struct with `x, y, width, height, title`,
      a pixel back-buffer for its content area, and a `dirty` flag.
- [ ] Write `src/wm/compositor.rs` — a global `WindowManager` (behind a `Mutex`) that
      holds a `Vec<Window>`, a z-order list, and a `focused` index. Implement
      `compose()` which paints the desktop background, then each window back-to-front.
- [ ] Implement window chrome drawing: title bar (with title text), border, and a
      placeholder close button (×).
- [ ] Implement focus-on-click: on left mouse button down, hit-test the title bars
      from front to back, raise the matching window, and set focus.
- [ ] Implement window dragging: track a `drag_state` (grabbed window + offset), update
      window `x/y` on mouse move, release on button up.
- [ ] Hook the compositor into the timer interrupt so the screen repaints at ~30 fps.

### Definition of done
Two or more windows appear on screen. Clicking a window raises it. Dragging its title
bar moves it.

---

## Phase 4 — Desktop Shell
**Goal:** A usable desktop with a taskbar and the ability to open/close windows.

With the WM working, this phase wraps it in a desktop environment.

### Tasks
- [ ] Draw a desktop background (solid colour, gradient, or tiled pattern).
- [ ] Draw a taskbar bar at the bottom of the screen showing open window titles.
      Clicking a taskbar entry focuses/raises that window.
- [ ] Implement close button (×) hit detection — remove the window from the WM on click.
- [ ] Add a right-click context menu on the desktop with an "Open Terminal" option.
- [ ] Port the existing keyboard shell into a `TerminalWindow` app that renders its
      output into a window's back-buffer rather than directly to VGA.

### Definition of done
OS boots into a desktop. Right-click opens a menu. "Open Terminal" spawns a working
shell window. The window can be moved and closed.

---

## Phase 5 — Applications
**Goal:** Multiple useful apps running as windows side-by-side.

### Planned apps
- [ ] **Terminal** — the ported shell from Phase 4, supporting all existing commands.
- [ ] **System Monitor** — a window showing CPU vendor, heap usage, and uptime; updates
      every second via the timer tick count.
- [ ] **Text Viewer** — a scrollable read-only text display, useful for showing a
      "welcome" or "about" document baked into the kernel binary.
- [ ] **Color Picker** — clickable palette of colours; demonstrates mouse input in an
      app window.

---

## Technical notes

### Why not upgrade to `bootloader 0.10+`?
The 0.10 API is cleaner for framebuffer access but requires significant changes to
`main.rs`, `memory.rs`, and `allocator.rs`. Staying on 0.9.23 and enabling the
`vesa-framebuffer` feature is the lowest-risk path to Phase 1.

### Compositing strategy
A full double-buffer (one off-screen buffer + one blit-to-hardware per frame) avoids
tearing but requires 2× framebuffer memory. For Phase 3, a single-buffer dirty-region
approach is acceptable. Double-buffering can be added in Phase 4.

### No processes, no userspace
All apps in Phase 5 run as kernel-mode Rust code. There is no scheduler, no privilege
separation, and no system call interface. That is a separate, later project.

---

## Milestone summary

| Phase | Deliverable | Key new file(s) |
|-------|-------------|-----------------|
| 1 | Pixel framebuffer + font rendering | `src/framebuffer.rs` |
| 2 | Mouse cursor on screen | `src/mouse.rs` |
| 3 | Draggable windows | `src/wm/window.rs`, `src/wm/compositor.rs` |
| 4 | Desktop + taskbar + terminal window | `src/desktop.rs` |
| 5 | Multiple apps | `src/apps/` |
