use riscv::asm::wfi;

/// Tell the simulator to stop with the given exit code
pub fn shutdown(code: u16) -> ! {
    let v = 0x0d1e0000 | u32::from(code);

    loop {
        unsafe {
            SIM_EXIT.write_volatile(v);
        }

        wfi();
    }
}

const SIM_EXIT: *mut u32 = 0x1000_0020 as *mut u32;
