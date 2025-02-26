use crate::MTIME_HZ;
use core::arch::asm;
use core::time::Duration;

pub fn sleep(duration: Duration) {
    // Convert in number of ticks
    let micros = duration.as_micros() as u64;
    let f = u64::from(MTIME_HZ);

    let ticks = (micros * f + 1_000_000 / 2) / 1_000_000;

    syscall(SYS_SLEEP, ticks as usize, (ticks >> 32) as usize);
}

pub fn wait_for_vsync() {
    syscall(SYS_WAIT_EVENT, events::EV_VSYNC, 0);
}

pub fn spawn_task(f: fn() -> !, prio: i32) {
    syscall(SYS_SPAWN_TASK, f as usize, prio as usize);
}

fn syscall(code: usize, mut arg0: usize, arg1: usize) -> usize {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            in("a1") arg1,
        );
    }

    arg0
}

/// Suspend task for [a1:a0] MTIME ticks
pub const SYS_SLEEP: usize = 0x01;
/// Wait for the event described in a0
pub const SYS_WAIT_EVENT: usize = 0x02;
/// Spawn a thread with entry function in a0 and prio in a'
pub const SYS_SPAWN_TASK: usize = 0x03;

pub mod events {
    pub const EV_VSYNC: usize = 1;
}
