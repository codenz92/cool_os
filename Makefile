.PHONY: run build clean

# Using absolute path
BIN_PATH := $(CURDIR)/target/x86_64-unknown-none/debug/bootimage-cool_os.bin

run: build
	@echo "Checking image size..."
	@ls -lh $(BIN_PATH)
	qemu-system-x86_64 -drive format=raw,file="$(BIN_PATH)" -m 512M -display cocoa

build:
	@# We force the CARGO_MANIFEST_DIR and run the bootimage tool
	export CARGO_MANIFEST_DIR="$(CURDIR)" && cargo bootimage

clean:
	cargo clean
	rm -rf target