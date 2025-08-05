wasm_out := "nr32-web/src/wasm"

build-wasm:
    wasm-pack build --target web --out-dir {{wasm_out}}
    cp {{wasm_out}}/novarave32_bg.wasm nr32-web/public/

build-rom $RUSTFLAGS="-C link-arg=-Tmemory.x -C target-feature=+relax":
    cd nr32-rt && cargo build --release
    cd nr32-rt && cargo objcopy --release -- -O binary ../nr32-web/public/ROM.BIN
    cd nr32-rt && cargo objdump --release -- -d -x -s > ../nr32-web/public/ROM.txt

web-build:
    cd nr32-web && npm run build

build:
    just build-wasm
    just build-rom
    just web-build

format:
    cargo fmt
    cd nr32boot && cargo fmt
    cd nr32-rt && cargo fmt
    cd nr32-web && npx prettier --write .

lint:
    cargo clippy
    cd nr32boot && cargo clippy
    cd nr32-rt && cargo clippy
    cd nr32-web && npx eslint

web-dev:
    just build-wasm
    just build-rom
    cd nr32-web && npm run dev

