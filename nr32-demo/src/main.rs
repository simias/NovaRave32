#![no_std]
#![no_main]

#[macro_use]
extern crate log;

use core::time::Duration;

use nr32_sys::gpu::send_to_gpu;
use nr32_sys::math::{
    matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3, MAT4, MAT5, MAT7},
    Angle, Fp32,
};
use nr32_sys::syscall::{input_device, sleep, wait_for_vsync, Allocator, ThreadBuilder};

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

mod panic_handler {
    // use crate::utils::shutdown;
    use core::panic::PanicInfo;

    #[inline(never)]
    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        error!("!PANIC!");
        error!("{}", info);
        // shutdown(!0)
        panic!();
    }
}

#[no_mangle]
pub fn nr32_main() {
    info!("Task is running!");

    start_audio();

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
    let mut angle_x = Angle::from_degrees(0.into());
    let a_increment = Angle::from_degrees((0.5).into());

    let ship = include_bytes!("assets/ship.nr3d");
    let beach = include_bytes!("assets/beach.nr3d");

    let mut prev_touch: Option<(u16, u16)> = None;

    loop {
        let touch = read_touch_screen();

        if let (Some(p), Some(t)) = (prev_touch, touch) {
            let (px, py) = p;
            let (tx, ty) = t;
            let px = px as i16;
            let py = py as i16;
            let tx = tx as i16;
            let ty = ty as i16;

            let dx = tx - px;
            let dy = ty - py;

            let angle_dy = a_increment * dx.unsigned_abs();
            let angle_dx = a_increment * dy.unsigned_abs();

            if dx >= 0 {
                angle_y += angle_dy;
            } else {
                angle_y -= angle_dy;
            }

            if dy >= 0 {
                angle_x += angle_dx;
            } else {
                angle_x -= angle_dx;
            }
        }

        prev_touch = touch;

        // Start draw
        send_to_gpu(0x01 << 24);

        matrix::translate(m_mat, 0.into(), (0).into(), (-50).into());
        matrix::rotate_x(MAT7, angle_x);
        matrix::multiply(m_mat, m_mat, MAT7);
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

/// Returns the current coordinates of the screen touch (if any).
/// The values are in the range [0; 0x400] where [0, 0] is top-left and [0x400, 0x400] is
/// bottom-right
fn read_touch_screen() -> Option<(u16, u16)> {
    let cmd: &mut [u8] = &mut [
        // Address touchscreen
        b'T', // Read state
        b'S', // x high
        0,    // x low
        0,    // y high
        0,    // y low
        0,
    ];
    input_device(0, cmd);

    if cmd[1] != b'a' {
        // Device didn't respond
        return None;
    }

    let x = u16::from(cmd[2]) << 8;
    let x = u16::from(cmd[3]) | x;
    let y = u16::from(cmd[4]) << 8;
    let y = u16::from(cmd[5]) | y;

    if x > 0x400 || y > 0x400 {
        // Out of range -> no touch
        None
    } else {
        Some((x, y))
    }
}

/// 12th root of 2
const SEMITONE_RATIO: Fp32 = Fp32::from_f32(1.0594631);

fn start_audio() {
    let note = include_bytes!("assets/A440.nrad");

    let a_step = nrad_step(note) as i32;

    nrad_upload(0, note);

    spu_main_volume(i16::MAX / 2, i16::MAX / 2);
    spu_voice_volume(0, i16::MAX, i16::MAX);
    spu_voice_volume(1, i16::MAX, i16::MAX);

    spu_voice_start_block(0, 0);
    spu_voice_start_block(1, 0);

    ThreadBuilder::new()
        .stack_size(1024)
        .priority(1)
        .spawn(move || {
            let mut pitch = Fp32::ONE;

            loop {
                pitch *= SEMITONE_RATIO;

                if pitch > 4.into() {
                    pitch = Fp32::ONE;
                }

                spu_voice_step(0, (pitch * a_step).round() as u16);

                unsafe {
                    *SPU_VOICE_ON = 0x1;
                }
                sleep(Duration::from_millis(500));
            }
        });

    ThreadBuilder::new()
        .stack_size(1024)
        .priority(1)
        .spawn(move || {
            let mut pitch = Fp32::ONE;

            loop {
                pitch *= SEMITONE_RATIO;

                if pitch > 4.into() {
                    pitch = Fp32::ONE;
                }

                spu_voice_step(1, (pitch * a_step).round() as u16);

                unsafe {
                    *SPU_VOICE_ON = 1 << 1;
                }
                sleep(Duration::from_millis(490));
            }
        });
}

fn send_model(model: &[u8]) {
    for b in model.chunks_exact(4) {
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        send_to_gpu(w);
    }
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

fn spu_main_volume(vleft: i16, vright: i16) {
    let v = ((vleft as u32) << 16) | (vright as u32);

    unsafe {
        *SPU_VOLUME_MAIN = v;
    }
}

fn spu_voice_volume(voice: u32, vleft: i16, vright: i16) {
    let v = ((vleft as u32) << 16) | (vright as u32);
    let p = (SPU_VOICE_VOLUME + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        *p = v;
    }
}

fn spu_voice_step(voice: u32, step: u16) {
    let p = (SPU_VOICE_STEP + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        *p = step as u32;
    }
}

fn spu_voice_start_block(voice: u32, addr: u32) {
    let p = (SPU_VOICE_START_BLOCK + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        *p = addr;
    }
}

fn nrad_step(nrad_buf: &[u8]) -> u16 {
    let step_lo = u16::from(nrad_buf[6]);
    let step_hi = u16::from(nrad_buf[7]) << 8;

    step_lo | step_hi
}

fn nrad_upload(addr: u16, nrad_buf: &[u8]) {
    spu_upload(addr, &nrad_buf[8..]);
}

const SPU_BASE: u32 = 0x1002_0000;
const SPU_VOLUME_MAIN: *mut u32 = SPU_BASE as *mut u32;
const SPU_VOICE_ON: *mut u32 = (SPU_BASE + 4) as *mut u32;
const SPU_RAM_ADDR: *mut u32 = (SPU_BASE + 4 * 4) as *mut u32;
const SPU_RAM_W: *mut u32 = (SPU_BASE + 5 * 4) as *mut u32;

const SPU_VOICE_BASE: u32 = SPU_BASE + 0x100;
const SPU_VOICE_OFF: u32 = 0x20;

const SPU_VOICE_STEP: u32 = SPU_VOICE_BASE;
const SPU_VOICE_START_BLOCK: u32 = SPU_VOICE_BASE + 4;
const SPU_VOICE_VOLUME: u32 = SPU_VOICE_BASE + 2 * 4;
