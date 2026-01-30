coolOS 🚀
A minimal, 64-bit operating system kernel written in Rust.

This project demonstrates the fundamentals of OS development, including hardware interrupt handling, VGA text-mode graphics, and an extensible interactive shell.

🛠 Features
Custom IDT: Implements an Interrupt Descriptor Table to handle critical CPU exceptions such as Breakpoints and Double Faults.

Hardware Interrupts: Configures the 8259 PIC to handle asynchronous events from the system Timer and Keyboard.

Thread-Safe VGA Driver: A spin::Mutex protected VGA buffer writer supporting standard colors, scrolling, and formatted output via println!.

Advanced Shell: A CLI featuring a custom tokenizer that supports command-argument splitting (e.g., color red).

Hardware Interrogation: Integrated raw-cpuid support to identify the host CPU vendor and features.

No Standard Library: Built entirely with #[no_std] and #[no_main] for a pure bare-metal experience.

⌨️ Shell Commands
help: Displays the available command list and usage information.

clear: Wipes the VGA display buffer and resets the cursor.

color <name>: Changes the shell's foreground text color (supports red, green, blue, yellow, white).

cpu: Queries the processor via the cpuid instruction to display vendor information.

reboot: Triggers a hardware reset via the PS/2 keyboard controller.

🏗 Technical Architecture
Keyboard Input Flow

Hardware: A keypress triggers an IRQ 1 via the PS/2 controller.

PIC: The Programmable Interrupt Controller maps this IRQ to interrupt vector 33.

IDT: The CPU looks up vector 33 in the IDT and jumps to the keyboard_interrupt_handler.

Driver: The handler reads scancodes from Port 0x60, decodes them using the pc-keyboard crate, and updates the global COMMAND_BUFFER.

Shell: Upon a newline character, the process_command logic parses the buffer to execute the requested kernel function.

🚀 Getting Started
Prerequisites

You will need the Rust nightly toolchain and the following components:

Bash
rustup component add rust-src
cargo install bootimage
brew install qemu  # For macOS users