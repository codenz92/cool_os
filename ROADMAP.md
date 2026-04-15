# coolOS Desktop Roadmap

The goal is to evolve coolOS from a VGA text-mode shell into a graphical desktop OS
with a windowing system, mouse support, and runnable applications.

Each phase builds directly on the last. Nothing in a later phase is possible without
completing the phase before it.

---

## Phase 1 — Pixel Framebuffer

**Goal:** Replace the VGA text driver with a pixel-addressable display.

- [x] Add `vga_320x200` feature to the `bootloader` crate and `font8x8 = "0.2.4"` to
      `Cargo.toml`. The bootloader sets Mode 13h (320×200, 8bpp at `0xA0000`) before
      jumping to the kernel.
- [x] Write `src/framebuffer.rs` — exposes `put_pixel`, `scroll_up`, and `draw_char`
      backed by the identity-mapped `0xA0000` buffer.
- [x] Rewrote `src/vga_buffer.rs` — `Writer` now calls `framebuffer::draw_char` instead
      of writing to the text buffer. Public API (`println!`, `print!`) unchanged.
- [x] Added `mod framebuffer` to `src/main.rs`.

Status: COMPLETE — builds clean, shell runs in Mode 13h pixel graphics.

---

## Phase 2 — PS/2 Mouse Driver

**Goal:** Read mouse movement and button clicks from hardware.

- [x] Enable IRQ12 on the secondary PIC — unmask bit 4 of PIC2 IMR (port `0xA1`) and
      bit 2 of PIC1 IMR (cascade line) inside `mouse::init()`.
- [x] Write `src/mouse.rs` — full PS/2 init sequence (disable keyboard during CCB read,
      flush buffer, enable aux device, set CCB, 0xF6 defaults, 0xF4 enable reporting).
      `handle_packet(b0, b1, b2)` decodes 9-bit signed X/Y deltas and button state.
- [x] Global mouse state with clamped `(x, y)`, `left`, `right` fields; public
      `pos()` and `buttons()` getters.
- [x] IRQ12 handler in `interrupts.rs` collects bytes via three `AtomicU8`s, validates
      sync bit, calls `mouse::handle_packet`.

Status: COMPLETE

> **QEMU note:** click inside the QEMU window to capture mouse input.
> Press **Ctrl+Alt+G** to release it.

---

## Phase 3 — Window Manager Core

**Goal:** Create, draw, and stack rectangular windows on screen.

- [x] Write `src/wm/window.rs` — `Window` struct with `i32` x/y, width/height, title,
      a pixel back-buffer, and hit-test methods (`hit`, `hit_title`, `hit_close`).
- [x] Write `src/wm/compositor.rs` — `WindowManager` with a `Vec<AppWindow>`, z-order
      list, focused index, and drag state. `compose()` renders desktop → windows
      back-to-front → cursor into a 64 KB shadow buffer, then blits to VGA in one
      `ptr::copy_nonoverlapping` call (no tearing).
- [x] Window chrome: title bar (blue when focused, dark grey otherwise), 1-px border,
      red close button (`x`).
- [x] Focus-on-click: hit-test title bars front-to-back, raise matching window, set focus.
- [x] Window dragging: `DragState` tracks grabbed window + cursor offset; releases on
      button-up. Off-screen dragging handled safely (clamped `s_fill`).
- [x] Timer interrupt calls `wm::request_repaint()` each tick; main loop calls
      `compose_if_needed()` then `hlt()`.

Status: COMPLETE — windows appear, can be raised, dragged, and closed.

---

## Phase 4 — Desktop Shell

**Goal:** A usable desktop with a taskbar and the ability to open/close windows.

- [x] Taskbar at the bottom of the screen (12 px); one button per open window; click
      to raise/focus that window.
- [x] Right-click context menu on the desktop with app launch items.
- [x] Close button (`x`) hit detection removes the window from the WM.
- [x] Ported the keyboard shell into `TerminalApp` — renders output into a window's
      pixel back-buffer. Commands: `help`, `clear`, `reboot`, `echo`, `info`, `uptime`.
- [x] Keyboard handler routes all input through `wm::handle_key()`, which dispatches to
      the focused window.

Status: COMPLETE — boots into a desktop; right-click opens context menu;
"Terminal" spawns a working shell window that can be moved and closed.

---

## Phase 5 — Applications

**Goal:** Multiple useful apps running as windows side-by-side.

- [x] **Terminal** — ported shell from Phase 4; all existing commands supported.
- [x] **System Monitor** — live CPU vendor, heap used/total, and uptime; re-renders
      its back-buffer on every compositor frame.
- [x] **Text Viewer** — scrollable "About coolOS" document baked into the kernel
      binary; `j`/`k` to scroll.
- [x] **Color Picker** — 8×2 grid of all 16 VGA palette swatches; click to select;
      status bar shows name and palette index. Demonstrates per-app content-area
      mouse-click dispatch.

Status: COMPLETE — all four apps launch from the right-click menu and run
side-by-side as independent windows.

---

## Technical notes

### Shadow-buffer compositing

The compositor renders the full frame (desktop fill → windows → cursor) into a
64 KB `Vec<u8>` shadow buffer in RAM. At the end of `compose()` a single
`ptr::copy_nonoverlapping` blits the finished frame to the VGA address `0xA0000`.
The display only ever sees complete frames, eliminating tearing and flicker.

### Why not upgrade to `bootloader 0.10+`?

The 0.10 API is cleaner for framebuffer access but requires significant changes to
`main.rs`, `memory.rs`, and `allocator.rs`. Staying on 0.9.23 and enabling the
`vga_320x200` feature is the lowest-risk path and avoids churn in stable code.

### Target spec

CPU features (`-mmx`, `-sse`, `+soft-float`) and `"rustc-abi": "softfloat"` are
set in `x86_64-unknown-none.json` at the target-spec level rather than via
`-Ctarget-feature` rustflags. This avoids the deprecation warnings introduced in
Rust issue #116344.

### No processes, no userspace

All apps run as kernel-mode Rust code. There is no scheduler, no privilege
separation, and no system call interface. That is a separate, later project.

---

## Milestone summary

| Phase | Deliverable | Key new files |
| :---: | :---------- | :------------ |
| 1 | Pixel framebuffer + font rendering | `src/framebuffer.rs` |
| 2 | Mouse cursor on screen | `src/mouse.rs` |
| 3 | Draggable windows, shadow compositor | `src/wm/window.rs`, `src/wm/compositor.rs` |
| 4 | Desktop + taskbar + terminal window | `src/wm/mod.rs` (expanded) |
| 5 | Four apps: terminal, sysmon, viewer, picker | `src/apps/` |
