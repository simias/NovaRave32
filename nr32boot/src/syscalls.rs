use crate::MTIME_HZ;
use core::arch::asm;
use core::time::Duration;

pub fn msleep(duration: Duration) {
    // Convert in number of ticks
    let micros = duration.as_micros() as u64;
    let f = u64::from(MTIME_HZ);

    let ticks = (micros * f + 1_000_000 / 2) / 1_000_000;

    syscall(SYS_SLEEP, ticks as u32, (ticks >> 32) as u32);
}

fn syscall(code: u32, mut arg0: u32, arg1: u32) -> u32 {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            in("a1") arg1,
        );
    }

    return arg0;
}

const SYS_SLEEP: u32 = 0x01;
