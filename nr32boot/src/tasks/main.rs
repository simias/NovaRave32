//! Main task

use crate::gpu::send_to_gpu;
use crate::math::{
    matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3},
    Angle, Fp32,
};
use crate::syscalls::{msleep, spawn_task, wait_for_vsync};
use core::time::Duration;

pub fn main() -> ! {
    info!("Task is running!");

    spawn_task(sub_task, 1);

    // MAT1: Camera matrix
    matrix::perspective(
        MAT1,
        Angle::from_degrees(80.into()),
        Fp32::ratio(640, 480),
        1.into(),
        1000.into(),
    );

    let mut angle_x = Angle::from_degrees(0.into());
    let mut angle_y = Angle::from_degrees(0.into());
    let mut angle_z = Angle::from_degrees(0.into());
    let x_increment = Angle::from_degrees((0.1).into());
    let y_increment = Angle::from_degrees((-1).into());
    let z_increment = Angle::from_degrees((1).into());

    loop {
        angle_x += x_increment;
        angle_y += y_increment;
        angle_z += z_increment;

        // Build rotation matrix in MAT3
        matrix::rotate_y(MAT3, angle_y);

        matrix::rotate_z(MAT2, angle_x);
        matrix::multiply(MAT3, MAT3, MAT2);

        matrix::rotate_x(MAT2, angle_z);
        matrix::multiply(MAT3, MAT3, MAT2);

        // Build scaling matrix in MAT2
        matrix::scale(MAT2, 0.1.into(), 0.1.into(), 0.into());

        // M2 = Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        matrix::translate(MAT3, 0.into(), 0.into(), (-100).into());

        // M2 = Translation * Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        // M0 = Camera * Object
        matrix::multiply(MAT0, MAT1, MAT2);

        matrix::set_draw_matrix(MAT0);

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

        matrix::rotate_y(MAT3, angle_z);

        matrix::rotate_z(MAT2, angle_y);
        matrix::multiply(MAT3, MAT3, MAT2);

        matrix::rotate_x(MAT2, angle_x);
        matrix::multiply(MAT3, MAT3, MAT2);

        // Build scaling matrix in MAT2
        matrix::scale(MAT2, 0.1.into(), 0.1.into(), 0.into());

        // M2 = Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        matrix::translate(MAT3, 0.into(), 0.into(), (-100).into());

        // M2 = Translation * Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        // M0 = Camera * Object
        matrix::multiply(MAT0, MAT1, MAT2);

        matrix::set_draw_matrix(MAT0);

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
        wait_for_vsync();
    }
}

fn send_coords(a: i16, b: i16) {
    send_to_gpu(((b as u16 as u32) << 16) | (a as u16 as u32));
}

fn sub_task() -> ! {
    info!("Sub-task launched");
    loop {
        info!("Sub-task sleeping 3s..");
        msleep(Duration::from_secs(3));
        info!("Sub-task done sleeping");
    }
}
