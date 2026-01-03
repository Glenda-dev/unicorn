#!/bin/bash
set -e
for arg in "$@"; do
  if [ "$arg" = "--release" ]; then
    MODE="release"
  fi
done

# Build the project
cargo build --release --target riscv64gc-unknown-none-elf

# Create build directory
mkdir -p build

# Convert ELF to binary
# riscv64-unknown-elf-objcopy -O binary ${CARGO_MANIFEST_DIR}/../target/riscv64gc-unknown-none-elf/release/unicorn build/unicorn.bin

cp ${CARGO_MANIFEST_DIR}/../target/riscv64gc-unknown-none-elf/release/unicorn build/unicorn.elf
