TARGET ?= aarch64-unknown-linux-gnu
HOST_TARGET ?= x86_64-unknown-linux-gnu
BIN := $(shell grep '^name' Cargo.toml | cut -d'"' -f2)
BIN_PATH := target/$(TARGET)/release/$(BIN)

PI_HOST ?= 10.41.110.216
PI_USER ?= pi
PI_DIR ?= /home/pi
PI_TARGET := $(PI_USER)@$(PI_HOST)

-include local.mk

PI_PASS ?=
SSH_CMD ?= sshpass -p $(PI_PASS) ssh
SCP_CMD ?= sshpass -p $(PI_PASS) scp

PATTERN ?= layout
UNIT ?= epaper-layout

.PHONY: all build test lint deploy run clean

all: build

build:
	cargo build --target $(TARGET) --release

test:
	cargo test --target $(HOST_TARGET)

lint:
	cargo clippy --target $(TARGET) -- -D warnings
	cargo fmt --check

deploy: build
	@printf '%s\n' '$(PI_PASS)' | $(SSH_CMD) $(PI_TARGET) 'sudo -S -p "" systemctl stop $(UNIT) 2>/dev/null; true'
	@$(SCP_CMD) $(BIN_PATH) $(PI_TARGET):$(PI_DIR)/$(BIN)
	@printf '%s\n' '$(PI_PASS)' | $(SSH_CMD) $(PI_TARGET) 'sudo -S -p "" systemd-run --unit=$(UNIT) --uid=$(PI_USER) --property="SupplementaryGroups=spi gpio" --property=Restart=on-failure --property=RestartSec=10 --working-directory=$(PI_DIR) $(PI_DIR)/$(BIN) $(PATTERN)'

run:
	$(SSH_CMD) $(PI_TARGET) 'cd $(PI_DIR) && timeout 60 ./$(BIN) $(PATTERN)'

clean:
	cargo clean
