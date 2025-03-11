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

fn sub_task() {
    let key_noise = include_bytes!("assets/key.nrad");

    spu_upload(0, &key_noise[8..]);
    spu_volume_main(5000, 5000);

    info!("Sub-task launched");
    loop {
        unsafe {
            *SPU_VOICE_ON = 1;
        }
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

fn spu_upload(addr: u16, d: &[u8]) {
    assert_eq!(addr & 3, 0, "SPU addr misaligned");
    unsafe {
        *SPU_RAM_ADDR = addr as u32;
    }

    for b in d.chunks_exact(4) {
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        unsafe {
            *SPU_RAM_W = w;
        }
    }
}

fn spu_volume_main(vleft: i16, vright: i16) {
    let v = ((vleft as u32) << 16) | (vright as u32);

    unsafe {
        *SPU_VOLUME_MAIN = v;
    }
}

fn send_model(model: &[u8]) {
    for b in model.chunks_exact(4) {
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        send_to_gpu(w);
    }
}

const SPU_BASE: u32 = 0x1002_0000;
const SPU_VOLUME_MAIN: *mut u32 = SPU_BASE as *mut u32;
const SPU_VOICE_ON: *mut u32 = (SPU_BASE + 4) as *mut u32;
const SPU_RAM_ADDR: *mut u32 = (SPU_BASE + 4 * 4) as *mut u32;
const SPU_RAM_W: *mut u32 = (SPU_BASE + 5 * 4) as *mut u32;
