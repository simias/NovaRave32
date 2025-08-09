wasm_out := "nr32-web/src/wasm"

build-wasm:
    wasm-pack build --target web --out-dir {{wasm_out}}
    cp {{wasm_out}}/novarave32_bg.wasm nr32-web/public/

build-rom $RUSTFLAGS="-C link-arg=-Tmemory.x -C target-feature=+relax":
    cd nr32-rt && cargo build --release
    cd nr32-demo && cargo build --release

build-cart:
    just build-rom
    cd multitool && cargo run -- cart ../nr32-rt/target/riscv32imac-unknown-none-elf/release/nr32-rt ../nr32-demo/target/riscv32imac-unknown-none-elf/release/nr32-demo -o ../nr32-web/public/cart.nr32

web-build:
    cd nr32-web && npm run build

build:
    just build-wasm
    just build-cart
    just web-build

format:
    cargo fmt
    cd nr32-demo && cargo fmt
    cd nr32-rt && cargo fmt
    cd nr32-web && npx prettier --write .

lint:
    cargo clippy
    cd nr32-demo && cargo clippy
    cd nr32-rt && cargo clippy
    cd nr32-web && npx eslint

web-dev:
    just build-wasm
    just build-cart
    cd nr32-web && npm run dev

