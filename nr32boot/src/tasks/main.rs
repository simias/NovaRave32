//! Main task

use crate::gpu::send_to_gpu;
use crate::math::{
    matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3, MAT4, MAT5, MAT6, MAT7},
    Angle, Fp32, Vec3,
};
use crate::syscalls::{sleep, spawn_task, wait_for_vsync};
use core::time::Duration;

pub fn main() {
    info!("Task is running!");

    spawn_task(sub_task, 1);

    // MAT0: Draw matrix
    // MAT1: MVP matrix
    // MAT2: Projection matrix
    // MAT3: View matrix
    // MAT4: Model matrix
    // MAT5: Normal matrix
    // MAT6: Custom
    // MAT7: multitool model loading
    let draw_mat = MAT0;
    let mvp_mat = MAT1;
    let p_mat = MAT2;
    let _v_mat = MAT3;
    let m_mat = MAT4;
    let _n_mat = MAT5;

    matrix::perspective(
        p_mat,
        Angle::from_degrees(80.into()),
        Fp32::ratio(640, 480),
        10.into(),
        1000.into(),
    );

    let mut angle_y = Angle::from_degrees(0.into());
    let y_increment = Angle::from_degrees((0.5).into());

    let ship = include_bytes!("assets/ship.nr3d");
    let beach = include_bytes!("assets/beach.nr3d");

    loop {
        angle_y += y_increment;

        // Start draw
        send_to_gpu(0x01 << 24);

        matrix::translate(m_mat, 0.into(), (0).into(), (-50).into());
        matrix::rotate_x(MAT6, Angle::from_degrees(5.into()));
        matrix::multiply(m_mat, m_mat, MAT6);
        matrix::rotate_y(MAT7, angle_y);
        matrix::multiply(m_mat, m_mat, MAT7);
        matrix::rotate_z(MAT7, angle_y);

        matrix::multiply(m_mat, m_mat, MAT7);
        matrix::scale(MAT7, 1.1.into(), 1.1.into(), 1.1.into());
        matrix::multiply(m_mat, m_mat, MAT7);

        matrix::multiply(mvp_mat, p_mat, m_mat);
        matrix::multiply(draw_mat, p_mat, m_mat);

        matrix::set_draw_matrix(draw_mat);

        // Build octahedron
        {
            let (vertices, indices) = build_octahedron([30, 10, 10].into(), 3);

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

        send_model(ship);
        send_model(beach);

        // End draw
        send_to_gpu(0x02 << 24);
        wait_for_vsync();
    }
}

fn send_coords(a: i16, b: i16) {
    send_to_gpu(((b as u16 as u32) << 16) | (a as u16 as u32));
}

fn send_model(model: &[u8]) {
    for b in model.chunks_exact(4) {
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        send_to_gpu(w);
    }
}

fn sub_task() {
    info!("Sub-task launched");
    loop {
        info!("Sub-task sleeping 3s..");
        sleep(Duration::from_secs(3));
        info!("Sub-task done sleeping");
        spawn_task(one_shot_task, -1);
    }
}

fn one_shot_task() {
    info!("One-shot-task launched");
    sleep(Duration::from_secs(1));
    info!("One-shot-task ended");
}

// Constructs a regular octahedron centered on `c` and with all vertices at a distance `r` from the
// `c`. Returns the 10 vertices and the indices to build the 8 triangles
fn build_octahedron(c: Vec3<i16>, r: i16) -> ([Vec3<i16>; 6], [u8; 3 * 8]) {
    let vertices = [
        c + [0, r, 0],
        c + [0, -r, 0],
        c + [r, 0, 0],
        c + [-r, 0, 0],
        c + [0, 0, r],
        c + [0, 0, -r],
    ];

    let indices = [
        0, 3, 5, 1, 5, 3, 0, 5, 2, 1, 2, 5, 0, 2, 4, 1, 4, 2, 0, 4, 3, 1, 3, 4,
    ];

    (vertices, indices)
}
