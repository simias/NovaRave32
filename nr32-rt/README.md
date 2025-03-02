# `nr32boot-boot`

## Setup

```sh
rustup target add riscv32imac-unknown-none-elf
rustup component add llvm-tools-preview
cargo objcopy --release -- -O binary target/release/ROM.BIN
```
