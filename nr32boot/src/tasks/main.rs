//! Main task

use crate::gpu::send_to_gpu;
use crate::math::{matrix, matrix::MAT1, Angle, Fp32};
use crate::syscalls::{msleep, spawn_task, wait_for_vsync};
use core::time::Duration;

pub fn main() -> ! {
    info!("Task is running!");

    matrix::perspective(
        MAT1,
        Angle::from_degrees(80.into()),
        Fp32::ratio(640, 480),
        1.into(),
        1000.into(),
    );

    // Switch to matrix 1 for drawing
    send_to_gpu(
        (0x03 << 24) // Draw config
        | (1 << 16) // Set draw matrix
        | 1,
    );

    // Start draw
    send_to_gpu(0x01 << 24);

    // Flat triangle
    send_to_gpu(
        (0x40 << 24) | 0x2f4f4f, // Triangle color
    );
    let z = -800;
    // V1 Z
    send_coords(z, 0);
    // V1 Y | X
    send_coords(600, 900);
    // V2 Z
    send_coords(z, 0);
    // V2 Y | X
    send_coords(-300, -200);
    // V3 Z
    send_coords(z, 0);
    // V3 Y | X
    send_coords(800, -1000);

    // Gouraud triangle
    send_to_gpu(
        (0x40 << 24)
        | (2 << 25) // Gouraud
        | 0x00ff00, // V1 color
    );
    // V1 Z
    send_coords(-500, 0);
    // V1 Y | X
    send_coords(0, 500);
    // V2 color
    send_to_gpu(0xff0000);
    // V2 Z
    send_coords(-800, 0);
    // V2 Y | X
    send_coords(-500, -500);
    // V3 color
    send_to_gpu(0x0000ff);
    // V3 Z
    send_coords(-900, 0);
    // V3 Y | X
    send_coords(500, -500);

    // End draw
    send_to_gpu(0x02 << 24);

    info!("Sleeping 1s...");
    msleep(Duration::from_secs(1));
    info!("Done");

    spawn_task(sub_task, 1);

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

fn sub_task() -> ! {
    info!("Sub-task launched");
    loop {
        info!("Sub-task sleeping...");
        msleep(Duration::from_secs(3));
        info!("Sub-task done sleeping");
    }
}
