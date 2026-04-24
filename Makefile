.PHONY: run run-usb run-usb-init run-headless run-headless-usb run-headless-usb-init smoke smoke-usb-init build build-usb-init clean

TARGET  := x86_64-unknown-none.json
KERNEL  := $(CURDIR)/target/x86_64-unknown-none/release/cool_os
BIOS    := $(CURDIR)/target/x86_64-unknown-none/release/bios.img
FSIMG   := $(CURDIR)/target/x86_64-unknown-none/release/fs.img
USB_INIT_TARGET_DIR := $(CURDIR)/target/usb-init
USB_INIT_KERNEL := $(USB_INIT_TARGET_DIR)/x86_64-unknown-none/release/cool_os
USB_INIT_BIOS := $(USB_INIT_TARGET_DIR)/x86_64-unknown-none/release/bios.img
USB_INIT_FSIMG := $(USB_INIT_TARGET_DIR)/x86_64-unknown-none/release/fs.img
USER_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/hello_user
USER_EXEC_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/exec
USER_PIPE_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/pipe
USER_READ_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/read
USER_PIPERD_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/piperd
USER_PIPEWR_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/pipewr
USER_KEYECHO_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/keyecho
USER_TERMINAL_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/terminal

run: build
	@echo "Booting coolOS in QEMU..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(BIOS)",snapshot=on \
		-drive file="$(FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-display cocoa \
		-debugcon stdio

run-usb: build
	@echo "Booting coolOS in QEMU with xHCI-attached USB devices..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(BIOS)",snapshot=on \
		-drive file="$(FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-device qemu-xhci,id=xhci \
		-device usb-kbd,bus=xhci.0 \
		-device usb-mouse,bus=xhci.0 \
		-display cocoa \
		-debugcon stdio

run-usb-init: build-usb-init
	@echo "Booting coolOS in QEMU with active xHCI init..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(USB_INIT_BIOS)",snapshot=on \
		-drive file="$(USB_INIT_FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-device qemu-xhci,id=xhci \
		-device usb-kbd,bus=xhci.0 \
		-device usb-mouse,bus=xhci.0 \
		-display cocoa \
		-debugcon stdio

run-headless: build
	@echo "Booting coolOS headless in QEMU..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(BIOS)",snapshot=on \
		-drive file="$(FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-display none \
		-debugcon stdio

run-headless-usb: build
	@echo "Booting coolOS headless in QEMU with xHCI-attached USB devices..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(BIOS)",snapshot=on \
		-drive file="$(FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-device qemu-xhci,id=xhci \
		-device usb-kbd,bus=xhci.0 \
		-device usb-mouse,bus=xhci.0 \
		-display none \
		-debugcon stdio

run-headless-usb-init: build-usb-init
	@echo "Booting coolOS headless in QEMU with active xHCI init..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(USB_INIT_BIOS)",snapshot=on \
		-drive file="$(USB_INIT_FSIMG)",if=ide,format=raw,index=1,snapshot=on \
		-m 512M \
		-vga std \
		-device qemu-xhci,id=xhci \
		-device usb-kbd,bus=xhci.0 \
		-device usb-mouse,bus=xhci.0 \
		-display none \
		-debugcon stdio

smoke: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds 6 \
		--expect "[fs] /bin/hello.txt: Hello from /bin/hello.txt!"

smoke-usb-init: build-usb-init
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--bios "$(USB_INIT_BIOS)" \
		--fsimg "$(USB_INIT_FSIMG)" \
		--usb \
		--seconds 8 \
		--expect "[xhci] active init ready" \
		--expect "[input] USB keyboard detected; PS/2 keyboard fallback disabled" \
		--expect "[input] USB mouse detected; PS/2 mouse fallback disabled"

build:
	cargo build --release --target $(TARGET) \
		-Z build-std=core,compiler_builtins,alloc \
		-Z build-std-features=compiler-builtins-mem
	RUSTFLAGS="-C link-arg=-T$(CURDIR)/userspace/hello/linker.ld" \
		cargo build --manifest-path $(CURDIR)/userspace/hello/Cargo.toml \
		--release \
		--target $(TARGET) \
		--target-dir $(CURDIR)/target/userspace/hello \
		-Z build-std=core,compiler_builtins
	(cd disk-image && cargo run --bin disk-image -- "$(KERNEL)")
	(cd disk-image && cargo run --bin fs-image -- "$(FSIMG)" "$(USER_TARGET)" "$(USER_EXEC_TARGET)" "$(USER_PIPE_TARGET)" "$(USER_READ_TARGET)" "$(USER_PIPERD_TARGET)" "$(USER_PIPEWR_TARGET)" "$(USER_KEYECHO_TARGET)" "$(USER_TERMINAL_TARGET)")

build-usb-init:
	COOLOS_XHCI_ACTIVE_INIT=1 cargo build --release --target $(TARGET) \
		--target-dir $(USB_INIT_TARGET_DIR) \
		-Z build-std=core,compiler_builtins,alloc \
		-Z build-std-features=compiler-builtins-mem
	RUSTFLAGS="-C link-arg=-T$(CURDIR)/userspace/hello/linker.ld" \
		COOLOS_XHCI_ACTIVE_INIT=1 cargo build --manifest-path $(CURDIR)/userspace/hello/Cargo.toml \
		--release \
		--target $(TARGET) \
		--target-dir $(CURDIR)/target/userspace/hello \
		-Z build-std=core,compiler_builtins
	(cd disk-image && COOLOS_XHCI_ACTIVE_INIT=1 cargo run --bin disk-image -- "$(USB_INIT_KERNEL)")
	(cd disk-image && COOLOS_XHCI_ACTIVE_INIT=1 cargo run --bin fs-image -- "$(USB_INIT_FSIMG)" "$(USER_TARGET)" "$(USER_EXEC_TARGET)" "$(USER_PIPE_TARGET)" "$(USER_READ_TARGET)" "$(USER_PIPERD_TARGET)" "$(USER_PIPEWR_TARGET)" "$(USER_KEYECHO_TARGET)" "$(USER_TERMINAL_TARGET)")

clean:
	cargo clean
	rm -rf target
	rm -rf disk-image/target
