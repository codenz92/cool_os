# coolOS 🚀

A minimal, 64-bit operating system kernel written in Rust.

This project demonstrates the core fundamentals of OS development, including hardware interrupt handling, physical and virtual memory management, and an extensible interactive shell.

## 🛠 Features

* **Dynamic Memory (Heap)**: Implemented a `LockedHeap` allocator allowing the use of dynamic types like `String`, `Vec`, and `Box`.
* **4-Level Paging**: Full virtual memory management using an `OffsetPageTable` to map physical frames into the virtual address space.
* **Physical Frame Allocation**: A custom allocator that discovers "Usable" RAM blocks via the bootloader’s memory map.
* **Advanced Shell**: A CLI featuring a custom tokenizer that supports command-argument splitting and a heap-backed command history.
* **Custom IDT**: Implements an Interrupt Descriptor Table to handle critical CPU exceptions such as Breakpoints and Double Faults.
* **Hardware Interrupts**: Configures the 8259 PIC to manage asynchronous events from the system Timer and Keyboard.
* **Thread-Safe VGA Driver**: A `spin::Mutex` protected VGA buffer writer supporting standard colors, scrolling, and formatted output via `println!`.



## 🏗 Technical Architecture

### 1. Memory Management Stack
To support dynamic allocation in a `no_std` environment, coolOS implements a three-tier memory stack:
1.  **Physical Layer**: The `BootInfoFrameAllocator` identifies "Usable" blocks in the E820 memory map provided by the BIOS/UEFI.
2.  **Virtual Layer**: An `OffsetPageTable` creates a 1:1 mapping of physical memory to a virtual offset, allowing the kernel to access all physical RAM safely.
3.  **Allocation Layer**: The `linked_list_allocator` manages a dedicated virtual address range (`0x4444_4444_0000`) to provide a 100 KiB heap for the kernel.



### 2. Input & Shell Processing
* **Hardware**: A keypress triggers an IRQ 1 via the PS/2 controller.
* **Interrupt Flow**: The Programmable Interrupt Controller (PIC) maps this IRQ to interrupt vector 33. The CPU looks up this vector in the IDT and jumps to the handler.
* **Decoding**: The handler reads scancodes from Port 0x60, decodes them using the `pc-keyboard` crate, and updates the global, heap-allocated `COMMAND_BUFFER`.
* **Execution**: Upon a newline character, the shell tokenizes the buffer using whitespace splitting to execute the requested function.

## ⌨️ Shell Commands

| Command | Description |
| :--- | :--- |
| `help` | Displays the available command list and usage information. |
| `clear` | Wipes the VGA display buffer and resets the cursor. |
| `color <name>` | Changes foreground text color (supports red, green, blue, yellow, white). |
| `echo <text>` | Prints the provided text back to the shell (demonstrates argument parsing). |
| `info` | Queries CPUID for vendor info and displays current Heap usage stats. |
| `history` | Displays the list of previously executed commands stored in the heap. |
| `uptime` | Shows system ticks since boot. |
| `reboot` | Triggers a hardware reset via the PS/2 keyboard controller. |


<img width="735" height="433" alt="image" src="https://github.com/user-attachments/assets/dd88a04d-e211-46e4-bf6f-8166c41e3628" />


## 🚀 Getting Started

### Prerequisites
You will need the Rust nightly toolchain and the following components:

```bash
rustup component add rust-src
cargo install bootimage
# For macOS users:
brew install qemu
