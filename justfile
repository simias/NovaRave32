wasm_out := "nr32-web/src/wasm"

build-wasm:
    wasm-pack build --target web --out-dir {{wasm_out}}
    mkdir -p nr32-web/public/
    cp {{wasm_out}}/novarave32_bg.wasm nr32-web/public/

build-rom $RUSTFLAGS="-C link-arg=-Tmemory.x -C target-feature=+relax":
    mkdir -p nr32-web/public/
    cd nr32-rt && cargo build --release && cargo objdump --release -- -d -x -s -r > ../nr32-web/public/nr32-rt.txt
    cd nr32-demo && cargo build --release && cargo objdump --release -- -d -x -s -r > ../nr32-web/public/demo.txt

build-cart:
    just build-rom
    cd multitool && cargo run -- -v cart \
        --boot-elf ../nr32-rt/target/riscv32imac-unknown-none-elf/release/nr32-rt \
        --main-elf ../nr32-demo/target/riscv32imac-unknown-none-elf/release/nr32-demo \
        --fs ../nr32-demo/src/assets/ \
        -o ../nr32-web/public/cart.nr32

web-build:
    cd nr32-web && npm run build

build:
    just build-wasm
    just build-cart
    just web-build

format:
    cargo fmt
    cd multitool && cargo fmt
    cd nr32-demo && cargo fmt
    cd nr32-rt && cargo fmt
    cd nr32-sys && cargo fmt
    cd nr32-web && npx prettier --write .

lint:
    cargo clippy
    cd multitool && cargo fmt
    cd nr32-demo && cargo clippy
    cd nr32-rt && cargo clippy
    cd nr32-sys && cargo clippy
    cd nr32-web && npx eslint

web-dev:
    just build-wasm
    just build-cart
    cd nr32-web && npm run dev

