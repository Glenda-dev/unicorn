#!/bin/bash
set -e

# Build the project
cargo build --release --target riscv64gc-unknown-none-elf

# Create build directory
mkdir -p build

# Convert ELF to binary
riscv64-unknown-elf-objcopy -O binary ../../target/riscv64gc-unknown-none-elf/release/unicorn build/unicorn.bin
