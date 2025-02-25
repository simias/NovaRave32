use crate::syscalls::msleep;
use core::time::Duration;

pub fn send_to_gpu(cmd: u32) {
    while !gpu_can_write() {
        msleep(Duration::from_millis(1))
    }

    unsafe {
        GPU_CMD.write_volatile(cmd);
    }
}

pub fn gpu_can_write() -> bool {
    // Command FIFO full
    gpu_status() & 1 == 0
}

pub fn gpu_status() -> u32 {
    unsafe { GPU_CMD.read_volatile() }
}

const GPU_CMD: *mut u32 = 0x1001_0000 as *mut u32;
