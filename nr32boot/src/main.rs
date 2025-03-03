#![no_std]
#![no_main]

#[macro_use]
extern crate log;

use core::time::Duration;
use nr32_rt::gpu::send_to_gpu;
use nr32_rt::math::{
    matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3, MAT4, MAT5, MAT7},
    Angle, Fp32,
};
use nr32_rt::syscalls::{sleep, spawn_task, wait_for_vsync};

#[export_name = "nr32_main"]
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
    let _draw_mat = MAT0;
    let mvp_mat = MAT1;
    let p_mat = MAT2;
    let v_mat = MAT3;
    let m_mat = MAT4;
    let _n_mat = MAT5;

    matrix::perspective(
        p_mat,
        Angle::from_degrees(80.into()),
        Fp32::ratio(640, 480),
        10.into(),
        1000.into(),
    );

    matrix::look_at(
        v_mat,
        [0, 30, 0].into(),
        [0, 0, -50].into(),
        [0, 1, 0].into(),
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
        matrix::rotate_y(MAT7, angle_y);

        matrix::multiply(m_mat, m_mat, MAT7);
        matrix::scale(MAT7, 1.1.into(), 1.1.into(), 1.1.into());
        matrix::multiply(m_mat, m_mat, MAT7);

        matrix::multiply(mvp_mat, p_mat, v_mat);
        matrix::multiply(mvp_mat, mvp_mat, m_mat);

        send_model(ship);
        send_model(beach);

        // End draw
        send_to_gpu(0x02 << 24);
        wait_for_vsync();
    }
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
