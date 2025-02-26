#!/bin/sh

set -e

cd $(dirname $0)

export RUSTFLAGS="-C link-arg=-Tmemory.x -C target-feature=+relax"

cargo build --release
cargo objcopy --release -- -O binary ../pkg/ROM.BIN
cargo objdump --release -- -d -x -s > ../pkg/ROM.txt

ls -lh ../pkg/ROM.BIN
