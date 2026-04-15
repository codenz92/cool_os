.PHONY: run build clean

TARGET  := x86_64-unknown-none.json
KERNEL  := $(CURDIR)/target/x86_64-unknown-none/debug/cool_os
BIOS    := $(CURDIR)/target/x86_64-unknown-none/debug/bios.img

run: build
	@echo "Booting coolOS in QEMU..."
	qemu-system-x86_64 \
		-drive format=raw,file="$(BIOS)" \
		-m 512M \
		-vga std \
		-display cocoa \
		-debugcon stdio

build:
	cargo build --target $(TARGET) \
		-Z build-std=core,compiler_builtins,alloc \
		-Z build-std-features=compiler-builtins-mem
	cargo run -p disk-image -- "$(KERNEL)"

clean:
	cargo clean
	rm -rf target
