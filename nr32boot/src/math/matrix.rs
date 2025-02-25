use super::{Angle, Fp32};

use crate::gpu::send_to_gpu;

/// Hardware matrix index
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Matrix(u8);

// pub const MAT0: Matrix = Matrix(0);
pub const MAT1: Matrix = Matrix(1);
// pub const MAT2: Matrix = Matrix(2);
// pub const MAT3: Matrix = Matrix(3);

/// Configure `m` to hold the given camera perspective matrix
pub fn perspective(m: Matrix, fovy: Angle, aspect_ratio: Fp32, near: Fp32, far: Fp32) {
    let f = (fovy / 2).cot();

    // info!("fovy: {}, f: {}", fovy, f);

    let mat_0_0 = f / aspect_ratio;
    let mat_1_1 = f;
    let mat_2_2 = (near + far) / (near - far);
    let mat_3_2 = (far * near * 2) / (near - far);
    let mat_2_3 = (-1).into();

    identity(m);
    set_matrix_component(m, 0, 0, mat_0_0);
    set_matrix_component(m, 1, 1, mat_1_1);
    set_matrix_component(m, 2, 2, mat_2_2);
    set_matrix_component(m, 3, 2, mat_3_2);
    set_matrix_component(m, 2, 3, mat_2_3);
}

/// Reset matrix to identity
pub fn identity(m: Matrix) {
    let m_select = u32::from(m.0 & 3) << 17;

    send_to_gpu((0x10 << 24) | m_select | (0b11 << 14));
}

pub fn set_matrix_component(m: Matrix, i: u8, j: u8, v: Fp32) {
    let m_select = u32::from(m.0 & 3) << 17;
    let i = u32::from(i & 3);
    let j = u32::from(j & 3);

    send_to_gpu((0x10 << 24) | m_select | (i << 4) | j);
    send_to_gpu(v.to_s16_16() as u32);
}
