//! Main task

use crate::gpu::send_to_gpu;
use crate::math::{
    matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3},
    Angle, Fp32,
    Vec3
};
use crate::syscalls::{sleep, spawn_task, wait_for_vsync};
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
        matrix::scale(MAT2, 0.05.into(), 0.05.into(), 0.05.into());

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
            (0x40 << 24) | 0x4f4f2f, // Triangle color
        );

        // V1 Z
        send_coords(50, 0);
        // V1 Y | X
        send_coords(600, 900);
        // V2 Z
        send_coords(-50, 0);
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
        matrix::scale(MAT2, 0.1.into(), 0.1.into(), 0.2.into());

        // M2 = Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        matrix::translate(MAT3, 0.into(), 0.into(), (-100).into());

        // M2 = Translation * Rotation * Scaling
        matrix::multiply(MAT2, MAT3, MAT2);

        // M0 = Camera * Object
        matrix::multiply(MAT0, MAT1, MAT2);

        matrix::set_draw_matrix(MAT0);


        for y in 0..18 {
            for x in 0..18 {
                // Gouraud octahedron
                let (vertices, indices) = build_octahedron_strip([(9 - x) * 50, (9 - y) * 50, 0].into(), 100);

                for chunk in indices.chunks(3) {
                    if let &[a, b, c] = chunk {
                        let va = vertices[usize::from(a)];
                        let vb = vertices[usize::from(b)];
                        let vc = vertices[usize::from(c)];

                        send_to_gpu((0x40 << 24) | (2 << 25) | (0x0001ff << (a * 3)));
                        send_coords(va.z(), 0);
                        send_coords(va.x(), va.y());
                        send_to_gpu(0x0001ff << (b * 3));
                        send_coords(vb.z(), 0);
                        send_coords(vb.x(), vb.y());
                        send_to_gpu(0x0001ff << (c * 3));
                        send_coords(vc.z(), 0);
                        send_coords(vc.x(), vc.y());
                    }
                }
            }
        }
        // send_to_gpu(
        //     (0x40 << 24)
        //     | (2 << 25) // Gouraud
        //     | 0x00ff00, // V1 color
        // );
        // // V1 Z
        // send_coords(0, 0);
        // // V1 Y | X
        // send_coords(0, 500);
        // // V2 color
        // send_to_gpu(0xff0000);
        // // V2 Z
        // send_coords(0, 0);
        // // V2 Y | X
        // send_coords(-500, -500);
        // // V3 color
        // send_to_gpu(0x0000ff);
        // // V3 Z
        // send_coords(0, 0);
        // // V3 Y | X
        // send_coords(500, -500);

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
        sleep(Duration::from_secs(3));
        info!("Sub-task done sleeping");
    }
}

// Constructs a regular octahedron centered on `c` and with all vertices at a distance `r` from the
// `c`. Returns the 10 vertices and the indices to build the 8 triangles
fn build_octahedron_strip(c: Vec3<i16>, r: i16) -> ([Vec3<i16>; 6], [u8; 3 * 8]) {
    let vertices = [
        c + [ 0, r, 0 ], // 0 +y
        c + [ 0, -r, 0 ],// 1 -y
        c + [ r, 0, 0 ], // 2 +x
        c + [ -r, 0, 0 ],// 3 -x
        c + [ 0, 0, r ], // 4 +z
        c + [ 0, 0, -r ],// 5 -z
    ];

    let indices = [
        0, 3, 5,
        1, 5, 3,
        0, 5, 2,
        1, 2, 5,
        0, 2, 4,
        1, 4, 2,
        0, 4, 3,
        1, 3, 4,
    ];

    (vertices, indices)
}
