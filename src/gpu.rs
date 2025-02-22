use crate::drawTriangles3D;
use crate::NoRa32;
use glam::{Mat4, Vec4};
use std::fmt;

pub struct Gpu {
    /// State of the command decoding pipeline
    command_state: CommandState,
    /// State of the rasterizer
    raster_state: RasterState,
    /// Perspective matrices
    mat: [Mat4; 4],
    /// Currently buffered vertices for triangle draw commands
    vertices: [Vertex; 3],
    /// Float vertex attributes for OpenGL:
    ///
    /// [0]: X
    /// [1]: Y
    /// [2]: Z
    /// [3]: W
    attribs_f32: Vec<f32>,
    /// UNSIGNED_BYTE vertex attributes for OpenGL:
    ///
    /// [0]: R
    /// [1]: G
    /// [2]: B
    /// [3]: A
    attribs_u8: Vec<u8>,
}

impl Gpu {
    pub fn new() -> Gpu {
        Gpu {
            command_state: CommandState::Idle,
            raster_state: RasterState::Idle,
            mat: [Mat4::IDENTITY; 4],
            vertices: [Vertex::new(); 3],
            attribs_f32: Vec::new(),
            attribs_u8: Vec::new(),
        }
    }

    fn status(&self) -> u32 {
        // bit 0: Command FIFO full
        0
    }

    fn set_matrix_component(&mut self, mindex: u8, i: u8, j: u8, v: Fp32) {
        debug_assert!(i < 4);
        debug_assert!(j < 4);

        let mindex = mindex as usize;
        if mindex >= self.mat.len() {
            return;
        }

        self.mat[mindex].col_mut(usize::from(i))[usize::from(j)] = v.to_f32();
    }
}

/// Draws the triangle in `gpu.vertices`
fn draw_flat_triangle(m: &mut NoRa32) {
    if m.gpu.raster_state != RasterState::Drawing {
        // Can't draw
        return;
    }

    // Retrieve the screen coordinates with the perspective division
    let w0 = m.gpu.vertices[0].coords[3];
    let w1 = m.gpu.vertices[1].coords[3];
    let w2 = m.gpu.vertices[2].coords[3];

    let v0 = m.gpu.vertices[0].coords * (1. / w0);
    let v1 = m.gpu.vertices[1].coords * (1. / w1);
    let v2 = m.gpu.vertices[2].coords * (1. / w2);

    let [x0, y0, z0, _] = v0.to_array();
    let [x1, y1, z1, _] = v1.to_array();
    let [x2, y2, z2, _] = v2.to_array();

    if (x0 > 1. && x1 > 1. && x2 > 1.) || (x0 < -1. && x1 < -1. && x2 < -1.) {
        // Fully clipped
        return;
    }

    if (y0 > 1. && y1 > 1. && y2 > 1.) || (y0 < -1. && y1 < -1. && y2 < -1.) {
        // Fully clipped
        return;
    }

    if (z0 > 1. && z1 > 1. && z2 > 1.) || (z0 < -1. && z1 < -1. && z2 < -1.) {
        // Fully clipped
        return;
    }

    // We should also handle the partially clipped case since it would reduce the number of pixels
    // drawn but that seems expensive. Alternatively we could try a rough proportional estimate I
    // suppose

    // Calculate the oriented/signed area of the triangle. The 0.25 is because the screen
    // coordinates go from -1 to +1 in either direction, so a quad covering the entire screen would
    // have an area of 4. Since we want pixels, we have to ajust.
    let area = (640. * 480. * 0.25 * 0.5)
        * ((x0 * y1 + x1 * y2 + x2 * y0) - (x0 * y2 + x1 * y0 + x2 * y1));

    if area <= 0. {
        // Clockwise triangle -> cull
        return;
    }

    for i in 0..3 {
        let v = &m.gpu.vertices[i];

        m.gpu.attribs_f32.push(v.coords.x);
        m.gpu.attribs_f32.push(v.coords.y);
        m.gpu.attribs_f32.push(v.coords.z);
        m.gpu.attribs_f32.push(v.coords.w);

        m.gpu.attribs_u8.push(v.color[0]);
        m.gpu.attribs_u8.push(v.color[1]);
        m.gpu.attribs_u8.push(v.color[2]);
        m.gpu.attribs_u8.push(255);
    }
}

fn handle_command(m: &mut NoRa32, cmd: u32) {
    m.gpu.command_state = match m.gpu.command_state {
        CommandState::Idle => handle_new_command(m, cmd),
        CommandState::TriangleRgb { vindex, gouraud } => {
            let r = (cmd >> 16) as u8;
            let g = (cmd >> 8) as u8;
            let b = cmd as u8;

            m.gpu.vertices[usize::from(vindex)].color = [r, g, b];

            CommandState::TriangleZ { vindex, gouraud }
        }
        CommandState::TriangleZ { vindex, gouraud } => {
            let z = (cmd & 0xffff) as f32;

            m.gpu.vertices[usize::from(vindex)].coords[2] = z;

            CommandState::TriangleYX { vindex, gouraud }
        }
        CommandState::TriangleYX { vindex, gouraud } => {
            let x = (cmd & 0xffff) as i16 as f32;
            let y = (cmd >> 16) as i16 as f32;

            m.gpu.vertices[usize::from(vindex)].coords[0] = x;
            m.gpu.vertices[usize::from(vindex)].coords[1] = y;

            // We could let the vertex shader deal with this of course but we need to compute that to
            // figure out how many pixels will need to be rendered and therefore how much time we should
            // deduct for them.
            m.gpu.vertices[usize::from(vindex)].coords =
                m.gpu.mat[0] * m.gpu.vertices[usize::from(vindex)].coords;

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
            if m.gpu.raster_state == RasterState::Drawing {
                drawTriangles3D(
                    m.gpu.attribs_f32.as_ptr(),
                    m.gpu.attribs_u8.as_ptr(),
                    m.gpu.attribs_f32.len() / 4,
                );

                m.gpu.raster_state = RasterState::Idle;
            }
            CommandState::Idle
        }
        // Matrix
        0x10 => {
            let mindex = ((cmd >> 17) & 0xf) as usize;

            let valid = mindex < m.gpu.mat.len();

            if !valid {
                warn!("Specified matrix out of range: {}", mindex);
            }

            match (cmd >> 14) & 3 {
                // Set single component
                0b00 => CommandState::MatrixSetComponent {
                    mindex: mindex as u8,
                    i: ((cmd >> 4) & 3) as u8,
                    j: (cmd & 3) as u8,
                },
                // Reset matrix to identity
                0b11 => {
                    if valid {
                        m.gpu.mat[mindex] = Mat4::IDENTITY;
                    }
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

            let r = (cmd >> 16) as u8;
            let g = (cmd >> 8) as u8;
            let b = cmd as u8;

            for i in 0..3 {
                m.gpu.vertices[i].color = [r, g, b];
                m.gpu.vertices[i].coords[3] = 1.;
            }

            CommandState::TriangleZ { vindex: 0, gouraud }
        }
        _ => panic!("Unhandled GPU command {:x}", op),
    }
}

pub fn load_word(m: &mut NoRa32, addr: u32) -> u32 {
    if addr == 0 {
        m.gpu.status()
    } else {
        warn!("Unhandled GPU read at {:x}", addr);
        !0
    }
}

pub fn store_word(m: &mut NoRa32, addr: u32, v: u32) {
    if addr == 0 {
        handle_command(m, v);
    } else {
        warn!("Unhandled GPU write at {:x}", addr);
    }
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
    coords: Vec4,
}

impl Vertex {
    fn new() -> Vertex {
        Vertex {
            color: [0; 3],
            coords: Vec4::ZERO,
        }
    }
}

const FP_SHIFT: u32 = 16;

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
