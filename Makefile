.PHONY: run run-usb run-usb-init run-headless run-headless-usb run-headless-usb-init smoke smoke-ui smoke-ui-ready-state smoke-framebuffer smoke-ui-goldens smoke-ui-settings smoke-ui-visual-assertions smoke-start-menu smoke-net-api smoke-usb-init smoke-hotplug-usb-init smoke-kernel-units smoke-boot-budget smoke-lowmem smoke-smp2 smoke-vga-cirrus build build-usb-init clean

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
USER_NETDEMO_TARGET := $(CURDIR)/target/userspace/hello/x86_64-unknown-none/release/netdemo
SMOKE_SECONDS ?= 18
SMOKE_FRAMEBUFFER_SECONDS ?= 30
SMOKE_USB_SECONDS ?= 18
SMOKE_BOOT_BUDGET_SECONDS ?= 8
SMOKE_VGA_SECONDS ?= 24
SMOKE_ARTIFACT_DIR ?= $(CURDIR)/target/smoke-artifacts

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
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_SECONDS) \
		--expect "[fs] /bin/hello.txt: Hello from /bin/hello.txt!" \
		--expect "[ring3 pid=1] sentinel ok" \
		--expect "[ring3 pid=2] sentinel ok" \
		--expect "[boot] desktop ready"

smoke-ui: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_SECONDS) \
		--expect "FB 1280x720" \
		--expect "[fs] /bin/hello.txt: Hello from /bin/hello.txt!" \
		--expect "[ring3 pid=1] sentinel ok" \
		--expect "[ring3 pid=2] sentinel ok" \
		--expect "[boot] desktop ready"

smoke-ui-ready-state: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-esc" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-ready-state.ppm" \
		--expect-framebuffer-start-menu \
		--expect "[boot] desktop ready" \
		--expect "[ui] ready pinned=Terminal|File Manager|System Monitor|Diagnostics|Display Settings|Personalize"

smoke-framebuffer: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--screendump "$(SMOKE_ARTIFACT_DIR)/framebuffer-smoke.ppm" \
		--expect-framebuffer-desktop \
		--expect "[boot] desktop ready"

smoke-ui-goldens: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "ui-golden-desktop" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-golden-desktop.ppm" \
		--expect-framebuffer-desktop \
		--expect "[boot] desktop ready"
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "ui-golden-file-manager" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-2" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-golden-file-manager.ppm" \
		--expect-framebuffer-window \
		--expect "[boot] desktop ready"
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "ui-golden-diagnostics" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-4" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-golden-diagnostics.ppm" \
		--expect-framebuffer-window \
		--expect "[boot] desktop ready"
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "ui-golden-crash-dialog" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-spc" \
		--type-text "crash dialog\n" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-golden-crash-dialog.ppm" \
		--expect-framebuffer-dialog \
		--expect "[boot] desktop ready"

smoke-ui-settings: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-5" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/ui-golden-settings.ppm" \
		--expect-framebuffer-window \
		--expect "[boot] desktop ready"

smoke-ui-visual-assertions:
	python3 $(CURDIR)/scripts/ppm_visual_assert.py \
		start-menu="$(SMOKE_ARTIFACT_DIR)/start-menu-smoke.ppm" \
		settings="$(SMOKE_ARTIFACT_DIR)/ui-golden-settings.ppm" \
		diagnostics="$(SMOKE_ARTIFACT_DIR)/ui-golden-diagnostics.ppm" \
		crash-dialog="$(SMOKE_ARTIFACT_DIR)/ui-golden-crash-dialog.ppm"

smoke-start-menu: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_FRAMEBUFFER_SECONDS) \
		--hmp "sendkey ctrl-esc" \
		--post-hmp-delay 0.8 \
		--screendump "$(SMOKE_ARTIFACT_DIR)/start-menu-smoke.ppm" \
		--expect-framebuffer-start-menu \
		--expect "[boot] desktop ready"

smoke-net-api: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_SECONDS) \
		--hmp "sendkey ctrl-spc" \
		--type-text "> exec /bin/netdemo\n" \
		--post-hmp-delay 2.0 \
		--expect "netdemo: dns example.com =" \
		--expect "GET / HTTP/1.0" \
		--expect "netdemo: http bytes" \
		--expect "[boot] desktop ready"

smoke-usb-init: build-usb-init
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(USB_INIT_BIOS)" \
		--fsimg "$(USB_INIT_FSIMG)" \
		--usb \
		--seconds $(SMOKE_USB_SECONDS) \
		--expect "[xhci] active init ready" \
		--expect "[input] USB keyboard detected; PS/2 keyboard fallback disabled" \
		--expect "[input] USB mouse detected; PS/2 mouse fallback disabled" \
		--expect "[ring3 pid=1] sentinel ok" \
		--expect "[ring3 pid=2] sentinel ok" \
		--expect "[boot] desktop ready"

smoke-hotplug-usb-init: build-usb-init
	python3 $(CURDIR)/scripts/qemu_hotplug_smoke.py \
		--bios "$(USB_INIT_BIOS)" \
		--fsimg "$(USB_INIT_FSIMG)"

smoke-kernel-units: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_SECONDS) \
		--expect "[selftest] kernel unit checks ok=9 fail=0" \
		--expect "[boot] desktop ready"

smoke-boot-budget: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--seconds $(SMOKE_BOOT_BUDGET_SECONDS) \
		--expect "[boot] desktop ready"

smoke-lowmem: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--memory 256M \
		--seconds $(SMOKE_SECONDS) \
		--expect "[boot] desktop ready"

smoke-smp2: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--smp 2 \
		--seconds $(SMOKE_SECONDS) \
		--expect "[boot] desktop ready"

smoke-vga-cirrus: build
	python3 $(CURDIR)/scripts/qemu_smoke.py \
		--artifact-dir "$(SMOKE_ARTIFACT_DIR)" \
		--artifact-name "$@" \
		--bios "$(BIOS)" \
		--fsimg "$(FSIMG)" \
		--vga cirrus \
		--seconds $(SMOKE_VGA_SECONDS) \
		--expect "[boot] desktop ready"

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
	(cd disk-image && cargo run --bin fs-image -- "$(FSIMG)" "$(USER_TARGET)" "$(USER_EXEC_TARGET)" "$(USER_PIPE_TARGET)" "$(USER_READ_TARGET)" "$(USER_PIPERD_TARGET)" "$(USER_PIPEWR_TARGET)" "$(USER_KEYECHO_TARGET)" "$(USER_TERMINAL_TARGET)" "$(USER_NETDEMO_TARGET)")

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
	(cd disk-image && COOLOS_XHCI_ACTIVE_INIT=1 cargo run --bin fs-image -- "$(USB_INIT_FSIMG)" "$(USER_TARGET)" "$(USER_EXEC_TARGET)" "$(USER_PIPE_TARGET)" "$(USER_READ_TARGET)" "$(USER_PIPERD_TARGET)" "$(USER_PIPEWR_TARGET)" "$(USER_KEYECHO_TARGET)" "$(USER_TERMINAL_TARGET)" "$(USER_NETDEMO_TARGET)")

clean:
	cargo clean
	rm -rf target
	rm -rf disk-image/target
