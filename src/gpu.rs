use crate::{displayFramebuffer, drawTriangles3D, irq, sync, CycleCounter, NoRa32, CPU_FREQ};
use glam::Mat4;
use std::fmt;

pub struct Gpu {
    /// State of the command decoding pipeline
    command_state: CommandState,
    /// State of the rasterizer
    raster_state: RasterState,
    /// Matrices
    mat: [Mat4; 8],
    /// Currently buffered vertices for triangle draw commands
    vertices: [Vertex; 3],
    /// Matrix used for perspective transform of vertices
    draw_mat: u8,
    /// Float vertex attributes for OpenGL:
    ///
    /// [0]: X
    /// [1]: Y
    /// [2]: Z
    attribs_i16: Vec<i16>,
    /// 4x4 f32 per matrix
    matrices_f32: Vec<[[f32; 4]; 4]>,
    /// Index of every Gpu.mat in matrices_f32 (if any).
    matrix_lut: [Option<u8>; 8],
    /// UNSIGNED_BYTE vertex attributes for OpenGL:
    ///
    /// [0]: R
    /// [1]: G
    /// [2]: B
    /// [3]: A
    /// [4]: Matrix index
    attribs_u8: Vec<u8>,
    /// Counter that decrements and generates a frame when it reaches 0
    frame_cycles: CycleCounter,
}

impl Gpu {
    pub fn new() -> Gpu {
        Gpu {
            command_state: CommandState::Idle,
            raster_state: RasterState::Idle,
            mat: [Mat4::IDENTITY; 8],
            vertices: [Vertex::new(); 3],
            draw_mat: 0,
            attribs_i16: Vec::new(),
            attribs_u8: Vec::new(),
            matrices_f32: Vec::new(),
            matrix_lut: [None; 8],
            frame_cycles: FRAME_CYCLES_30FPS,
        }
    }

    fn status(&self) -> u32 {
        // bit 0: Command FIFO full
        0
    }

    fn set_matrix_component(&mut self, mindex: u8, i: u8, j: u8, v: Fp32) {
        debug_assert!(i < 4);
        debug_assert!(j < 4);

        let v = v.to_f32();
        let i = usize::from(i);
        let j = usize::from(j);

        let mindex = mindex as usize;
        if mindex >= self.mat.len() {
            return;
        }

        if self.mat[mindex].col_mut(i)[j] == v {
            return;
        }

        // Invalidate the LUT entry
        self.matrix_lut[mindex] = None;

        self.mat[mindex].col_mut(i)[j] = v;
    }
}

/// Draws the triangle in `gpu.vertices`
fn draw_flat_triangle(m: &mut NoRa32) {
    if m.gpu.raster_state != RasterState::Drawing {
        // Can't draw
        return;
    }

    let mindex = usize::from(m.gpu.draw_mat);

    let matrix_off = match m.gpu.matrix_lut[mindex] {
        Some(i) => i,
        None => {
            // We haven't drawn with this matrix yet
            if m.gpu.matrices_f32.len() >= MAX_BUFFERED_MATRIX {
                // Too many matrices bufferized, force a draw to flush them
                do_draw(m);
            }

            let off = (m.gpu.matrices_f32.len()) as u8;

            let mat = &m.gpu.mat[mindex];

            m.gpu.matrices_f32.push([
                [mat.col(0)[0], mat.col(0)[1], mat.col(0)[2], mat.col(0)[3]],
                [mat.col(1)[0], mat.col(1)[1], mat.col(1)[2], mat.col(1)[3]],
                [mat.col(2)[0], mat.col(2)[1], mat.col(2)[2], mat.col(2)[3]],
                [mat.col(3)[0], mat.col(3)[1], mat.col(3)[2], mat.col(3)[3]],
            ]);

            m.gpu.matrix_lut[mindex] = Some(off);

            off
        }
    };

    for i in 0..3 {
        let v = &m.gpu.vertices[i];

        m.gpu.attribs_i16.push(v.coords[0]);
        m.gpu.attribs_i16.push(v.coords[1]);
        m.gpu.attribs_i16.push(v.coords[2]);

        m.gpu.attribs_u8.push(v.color[0]);
        m.gpu.attribs_u8.push(v.color[1]);
        m.gpu.attribs_u8.push(v.color[2]);
        m.gpu.attribs_u8.push(255);
        m.gpu.attribs_u8.push(matrix_off);
    }

    if m.gpu.attribs_i16.len() > 4000 {
        // Flush to OpenGL
        do_draw(m);
    }
}

fn handle_command(m: &mut NoRa32, cmd: u32) {
    m.gpu.command_state = match m.gpu.command_state {
        CommandState::Idle => handle_new_command(m, cmd),
        CommandState::TriangleRgb { vindex, gouraud } => {
            let b = (cmd >> 16) as u8;
            let g = (cmd >> 8) as u8;
            let r = cmd as u8;

            m.gpu.vertices[usize::from(vindex)].color = [r, g, b];

            CommandState::TriangleZ { vindex, gouraud }
        }
        CommandState::TriangleZ { vindex, gouraud } => {
            let z = (cmd & 0xffff) as i16;

            m.gpu.vertices[usize::from(vindex)].coords[2] = z;

            CommandState::TriangleYX { vindex, gouraud }
        }
        CommandState::TriangleYX { vindex, gouraud } => {
            let x = (cmd & 0xffff) as i16;
            let y = (cmd >> 16) as i16;

            m.gpu.vertices[usize::from(vindex)].coords[0] = x;
            m.gpu.vertices[usize::from(vindex)].coords[1] = y;

            if vindex == 2 {
                draw_flat_triangle(m);
                CommandState::Idle
            } else if gouraud {
                CommandState::TriangleRgb {
                    vindex: vindex + 1,
                    gouraud,
                }
            } else {
                CommandState::TriangleZ {
                    vindex: vindex + 1,
                    gouraud,
                }
            }
        }
        CommandState::MatrixSetComponent { mindex, i, j } => {
            let v = Fp32(cmd as i32);
            m.gpu.set_matrix_component(mindex, i, j, v);
            CommandState::Idle
        }
    }
}

/// Send draw commands to OpenGL and reset all the buffers
fn do_draw(m: &mut NoRa32) {
    if m.gpu.attribs_i16.is_empty() {
        return;
    }

    drawTriangles3D(
        m.gpu.matrices_f32.as_ptr(),
        m.gpu.matrices_f32.len(),
        m.gpu.attribs_i16.as_ptr(),
        m.gpu.attribs_u8.as_ptr(),
        m.gpu.attribs_i16.len() / 3,
    );

    m.gpu.attribs_i16.clear();
    m.gpu.attribs_u8.clear();
    m.gpu.matrices_f32.clear();
    m.gpu.matrix_lut = [None; 8];
}

fn handle_new_command(m: &mut NoRa32, cmd: u32) -> CommandState {
    let op = (cmd >> 24) as u8;

    match op {
        // NOP
        0x00 => CommandState::Idle,
        // Draw start
        0x01 => {
            m.gpu.raster_state = RasterState::Drawing;
            CommandState::Idle
        }
        // Draw end
        0x02 => {
            do_draw(m);
            displayFramebuffer();
            if m.gpu.raster_state == RasterState::Drawing {
                do_draw(m);
                m.gpu.raster_state = RasterState::Idle;
            }
            CommandState::Idle
        }
        // Draw config
        0x03 => {
            match (cmd >> 16) as u8 {
                // Set draw matrix
                0x01 => m.gpu.draw_mat = (cmd & 0xf) as u8,
                conf => warn!("Unknown config command {}", conf),
            }
            CommandState::Idle
        }
        // Matrix
        0x10 => {
            let mindex = ((cmd >> 12) & 7) as usize;

            match (cmd >> 16) & 0xff {
                // Reset matrix to identity
                0x00 => {
                    m.gpu.mat[mindex] = Mat4::IDENTITY;
                    CommandState::Idle
                }
                // Set single component
                0x01 => CommandState::MatrixSetComponent {
                    mindex: mindex as u8,
                    i: ((cmd >> 4) & 3) as u8,
                    j: (cmd & 3) as u8,
                },
                // Multiply
                0x02 => {
                    let maindex = ((cmd >> 4) & 0x7) as usize;
                    let mbindex = (cmd & 0x7) as usize;

                    m.gpu.mat[mindex] = m.gpu.mat[maindex] * m.gpu.mat[mbindex];

                    // Invalidate the LUT entry
                    m.gpu.matrix_lut[mindex] = None;

                    CommandState::Idle
                }
                mop => {
                    warn!("Unhandled matrix operation {}", mop);
                    CommandState::Idle
                }
            }
        }
        // Draw triangle
        0x40..=0x7f => {
            let blend_mode = (op >> 1) & 7;

            let gouraud = blend_mode == 2;

            let b = (cmd >> 16) as u8;
            let g = (cmd >> 8) as u8;
            let r = cmd as u8;

            for i in 0..3 {
                m.gpu.vertices[i].color = [r, g, b];
            }

            CommandState::TriangleZ { vindex: 0, gouraud }
        }
        _ => panic!("Unhandled GPU command {:x}", op),
    }
}

pub fn load_word(m: &mut NoRa32, addr: u32) -> u32 {
    run(m);
    if addr == 0 {
        m.gpu.status()
    } else {
        warn!("Unhandled GPU read at {:x}", addr);
        !0
    }
}

pub fn store_word(m: &mut NoRa32, addr: u32, v: u32) {
    run(m);

    if addr == 0 {
        handle_command(m, v);
    } else {
        warn!("Unhandled GPU write at {:x}", addr);
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, GPUSYNC);

    m.gpu.frame_cycles -= elapsed;

    if m.gpu.frame_cycles <= 0 {
        m.frame_counter = m.frame_counter.wrapping_add(1);
        m.gpu.frame_cycles += FRAME_CYCLES_30FPS;
        irq::trigger(m, irq::Interrupt::VSync);
        do_draw(m);
    }

    sync::next_event(m, GPUSYNC, m.gpu.frame_cycles);
}

enum CommandState {
    Idle,
    MatrixSetComponent { mindex: u8, i: u8, j: u8 },
    TriangleZ { vindex: u8, gouraud: bool },
    TriangleYX { vindex: u8, gouraud: bool },
    TriangleRgb { vindex: u8, gouraud: bool },
}

#[derive(PartialEq, Eq, Copy, Clone)]
enum RasterState {
    Idle,
    Drawing,
}

// s15.16 Fixed Point
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
struct Fp32(i32);

impl Fp32 {
    fn to_f32(self) -> f32 {
        (self.0 as f32) / ((1u32 << FP_SHIFT) as f32)
    }
}

impl fmt::Display for Fp32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.to_f32(), f)
    }
}

impl fmt::Debug for Fp32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.to_f32(), f)
    }
}

impl fmt::LowerExp for Fp32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerExp::fmt(&self.to_f32(), f)
    }
}

#[derive(Debug, Copy, Clone)]
struct Vertex {
    color: [u8; 3],
    coords: [i16; 3],
}

impl Vertex {
    fn new() -> Vertex {
        Vertex {
            color: [0; 3],
            coords: [0; 3],
        }
    }
}

const FP_SHIFT: u32 = 16;

const FRAME_CYCLES_30FPS: CycleCounter = (CPU_FREQ + 15) / 30;

const GPUSYNC: sync::SyncToken = sync::SyncToken::GpuTimer;

/// Max number of buffered matrices before we force a draw.
///
/// WebGL 2 specifies that the max number of vectors per uniform has to be at least 256 which means
/// at least 64 matrices. Of course we could also adjust based on what the system reports (my
/// Linux/Nvidia/Firefox system reports 1024 max vectors for instance).
///
/// https://webglreport.com/?v=2
///
/// If this is modified the size of the array in the vertex shader should also be adjusted
const MAX_BUFFERED_MATRIX: usize = 32;

#[test]
fn test_fp32_to_f32() {
    let t = &[
        (0, 0.),
        (1 << FP_SHIFT, 1.),
        (-1 << FP_SHIFT, -1.),
        (-500 << FP_SHIFT, -500.),
        (1, 1. / 65_536.),
        (-1, -1. / 65_536.),
        (2, 2. / 65_536.),
        (-2, -2. / 65_536.),
        (i32::MAX, 32_768.),
        (i32::MIN, -32_768.),
        (0xDEADC0DEu32 as i32, -8530.247),
    ];

    for &(fp, f) in t {
        let fp = Fp32(fp);

        assert_eq!(fp.to_f32(), f);
    }
}
