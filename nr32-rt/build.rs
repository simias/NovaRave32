//! Build script based on
//!
//! https://docs.rs/riscv-rt/latest/riscv32imac-unknown-none-elf/riscv_rt/index.html

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Put the linker script somewhere the linker can find it.
    fs::write(out_dir.join("memory.x"), include_bytes!("memory.x")).unwrap();
    println!("cargo:rustc-link-search={}", out_dir.display());
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rerun-if-changed=build.rs");

    // I don't think there's a builtin way to check if overflow checks are enabled, so I use this
    // hack instead
    println!("cargo::rustc-check-cfg=cfg(with_overflow_checks)");
    if ::std::panic::catch_unwind(|| {
        #[allow(arithmetic_overflow)]
        let _ = 255_u8 + 1;
    })
    .is_err()
    {
        println!("cargo:rustc-cfg=with_overflow_checks");
    }
}
