//! Main task

use crate::syscalls::{msleep, wait_for_vsync};
use core::time::Duration;

pub fn main() -> ! {
    info!("Task is running!");

    // Matrix 0 reset to identity
    let op = (0x10 << 24) // Matrix command
        | (0b11 << 14); // Clear

    send_to_gpu(op);

    for i in 0..3 {
        // Set matrix scale to 1/1024 on all axes
        let op = (0x10 << 24) // Matrix command
            | (i << 4) | i; // set [i][i]

        send_to_gpu(op);
        send_to_gpu((1 << 16) / 1024);
    }

    // Start draw
    send_to_gpu(0x01 << 24);

    // Flat triangle
    send_to_gpu(
        (0x40 << 24) | 0x2f4f4f, // Triangle color
    );
    // V1 Z
    send_coords(0, 0);
    // V1 Y | X
    send_coords(600, 900);
    // V2 Z
    send_coords(0, 0);
    // V2 Y | X
    send_coords(-300, -200);
    // V3 Z
    send_coords(0, 0);
    // V3 Y | X
    send_coords(800, -1000);

    // Gouraud triangle
    send_to_gpu(
        (0x40 << 24)
        | (2 << 25) // Gouraud
        | 0x00ff00, // V1 color
    );
    // V1 Z
    send_coords(0, 0);
    // V1 Y | X
    send_coords(0, 500);
    // V2 color
    send_to_gpu(0xff0000);
    // V2 Z
    send_coords(0, 0);
    // V2 Y | X
    send_coords(-500, -500);
    // V3 color
    send_to_gpu(0x0000ff);
    // V3 Z
    send_coords(0, 0);
    // V3 Y | X
    send_coords(500, -500);

    // End draw
    send_to_gpu(0x02 << 24);

    info!("Sleeping 1s...");
    msleep(Duration::from_secs(1));
    info!("Done");

    loop {
        for _ in 0..30 {
            wait_for_vsync();
        }
        info!("Got vsync 1s");
    }
}

fn send_coords(a: i16, b: i16) {
    send_to_gpu(((b as u16 as u32) << 16) | (a as u16 as u32));
}

fn send_to_gpu(cmd: u32) {
    while !gpu_can_write() {
        // yield()
    }

    unsafe {
        GPU_CMD.write_volatile(cmd);
    }
}

fn gpu_can_write() -> bool {
    // Command FIFO full
    gpu_status() & 1 == 0
}

fn gpu_status() -> u32 {
    unsafe { GPU_CMD.read_volatile() }
}

const GPU_CMD: *mut u32 = 0x1001_0000 as *mut u32;
