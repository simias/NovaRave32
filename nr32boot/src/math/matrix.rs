use super::{Angle, Fp32};

use crate::gpu::send_to_gpu;

/// Hardware matrix index
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Matrix(u8);

pub const MAT0: Matrix = Matrix(0);
pub const MAT1: Matrix = Matrix(1);
pub const MAT2: Matrix = Matrix(2);
pub const MAT3: Matrix = Matrix(3);
pub const MAT4: Matrix = Matrix(4);
pub const MAT5: Matrix = Matrix(5);
pub const MAT6: Matrix = Matrix(6);
pub const MAT7: Matrix = Matrix(7);

/// Tell the GPU to use `m` to transform the vertices while drawing
pub fn set_draw_matrix(m: Matrix) {
    let m_select = u32::from(m.0 & 7);
    send_to_gpu((0x03 << 24) | (1 << 16) | m_select);
}

/// Reset matrix to identity
pub fn identity(m: Matrix) {
    let m_select = u32::from(m.0 & 7) << 12;

    send_to_gpu((0x10 << 24) | m_select);
}

/// Calculate `ma` x `mx` and put the result in `mout`
pub fn multiply(mout: Matrix, ma: Matrix, mb: Matrix) {
    let m_select = u32::from(mout.0 & 7) << 12;
    let ma = u32::from(ma.0 & 7) << 4;
    let mb = u32::from(mb.0 & 7);

    send_to_gpu((0x10 << 24) | (0x02 << 16) | m_select | ma | mb);
}

/// Configure `m` to hold the given camera perspective matrix
pub fn perspective(m: Matrix, fovy: Angle, aspect_ratio: Fp32, near: Fp32, far: Fp32) {
    let f = (fovy / 2).cot();

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

/// Configure `m` to hold the given translation matrix
pub fn translate(m: Matrix, tx: Fp32, ty: Fp32, tz: Fp32) {
    identity(m);
    set_matrix_component(m, 3, 0, tx);
    set_matrix_component(m, 3, 1, ty);
    set_matrix_component(m, 3, 2, tz);
}

/// Configure `m` to hold the given scaling matrix
pub fn scale(m: Matrix, sx: Fp32, sy: Fp32, sz: Fp32) {
    identity(m);
    set_matrix_component(m, 0, 0, sx);
    set_matrix_component(m, 1, 1, sy);
    set_matrix_component(m, 2, 2, sz);
}

/// Configure `m` to hold the given rotation matrix along the X axis
pub fn rotate_x(m: Matrix, angle: Angle) {
    let sin = angle.sin();
    let cos = angle.cos();

    identity(m);
    set_matrix_component(m, 1, 1, cos);
    set_matrix_component(m, 2, 1, -sin);
    set_matrix_component(m, 1, 2, sin);
    set_matrix_component(m, 2, 2, cos);
}

/// Configure `m` to hold the given rotation matrix along the Y axis
pub fn rotate_y(m: Matrix, angle: Angle) {
    let sin = angle.sin();
    let cos = angle.cos();

    identity(m);
    set_matrix_component(m, 0, 0, cos);
    set_matrix_component(m, 2, 0, sin);
    set_matrix_component(m, 0, 2, -sin);
    set_matrix_component(m, 2, 2, cos);
}

/// Configure `m` to hold the given rotation matrix along the Z axis
pub fn rotate_z(m: Matrix, angle: Angle) {
    let sin = angle.sin();
    let cos = angle.cos();

    identity(m);
    set_matrix_component(m, 0, 0, cos);
    set_matrix_component(m, 1, 0, -sin);
    set_matrix_component(m, 0, 1, sin);
    set_matrix_component(m, 1, 1, cos);
}

pub fn set_matrix_component(m: Matrix, i: u8, j: u8, v: Fp32) {
    let m_select = u32::from(m.0 & 7) << 12;
    let i = u32::from(i & 3);
    let j = u32::from(j & 3);

    send_to_gpu((0x10 << 24) | (0x01 << 16) | m_select | (i << 4) | j);
    send_to_gpu(v.to_s16_16() as u32);
}
