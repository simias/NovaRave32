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

pub fn log_heap_stats() {
    use crate::HEAP;

    let used = HEAP.used();
    let free = HEAP.free();
    let tot = used + free;

    info!(
        "{}KiB used {}KiB free ({}% used)",
        used / 1024,
        free / 1024,
        (used * 100 + tot / 2) / tot,
    );
}

const SIM_EXIT: *mut u32 = 0x1000_0020 as *mut u32;
