/// Task called when nobody else can run
pub fn idle_main() -> ! {
    loop {
        riscv::asm::wfi();
    }
}
