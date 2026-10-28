TARGET ?= aarch64-unknown-linux-gnu
BIN := $(shell grep '^name' Cargo.toml | cut -d'"' -f2)
BIN_PATH := target/$(TARGET)/release/$(BIN)

PI_HOST ?= 192.168.45.50
PI_USER ?= pi
PI_DIR ?= /home/pi
PI_TARGET := $(PI_USER)@$(PI_HOST)

-include local.mk

PI_PASS ?=
SSH_CMD ?= sshpass -p $(PI_PASS) ssh
SCP_CMD ?= sshpass -p $(PI_PASS) scp

PATTERN ?= checker

.PHONY: all build lint deploy run clean

all: build

build:
	cargo build --target $(TARGET) --release

lint:
	cargo clippy --target $(TARGET) -- -D warnings
	cargo fmt --check

deploy: build
	$(SCP_CMD) $(BIN_PATH) $(PI_TARGET):$(PI_DIR)/$(BIN)
	$(MAKE) run

run:
	$(SSH_CMD) $(PI_TARGET) 'cd $(PI_DIR) && timeout 60 ./$(BIN) $(PATTERN)'

clean:
	cargo clean
