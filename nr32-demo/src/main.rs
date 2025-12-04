#![no_std]
#![no_main]

#[macro_use]
extern crate log;
extern crate alloc;

use core::time::Duration;

use alloc::sync::Arc;
use nr32_sys::allocator;
use nr32_sys::dma::{DmaAddr, do_dma};
use nr32_sys::fs::Fs;
use nr32_sys::gpu::send_to_gpu;
use nr32_sys::math::{
    Angle, Fp32, matrix,
    matrix::{MAT0, MAT1, MAT2, MAT3, MAT4, MAT5, MAT7},
};
use nr32_sys::sync::{Fifo, Semaphore};
use nr32_sys::syscall::{input_device, sleep, wait_for_vsync};
use nr32_sys::thread::ThreadBuilder;

#[global_allocator]
static ALLOCATOR: allocator::Allocator = allocator::Allocator::new();

mod panic_handler {
    use core::panic::PanicInfo;

    #[inline(never)]
    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        error!("!PANIC!");
        error!("{}", info);
        nr32_sys::syscall::shutdown(!0)
    }
}

struct DmaOp {
    from: DmaAddr,
    to: DmaAddr,
    len_words: usize,
    dma_done: Option<Arc<Semaphore>>,
}

#[unsafe(no_mangle)]
pub extern "C" fn nr32_main() {
    log::set_logger(&nr32_sys::logger::LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    info!("Task is running!");

    let fs = Fs::from_bootscript().unwrap();

    info!("Loaded FS: {}", fs.fsck().unwrap());

    let dma_fifo: Arc<Fifo<DmaOp, 8>> = Arc::new(Fifo::new());

    let fifo_c = dma_fifo.clone();

    ThreadBuilder::new(*b"DMA ")
        .stack_size(1024)
        .priority(-1)
        .spawn(move || {
            info!("DMA task start");
            loop {
                let c = fifo_c.as_ref().pop();

                let r = do_dma(c.from, c.to, c.len_words);

                if let Err(e) = r {
                    error!(
                        "DMA {:?} -> {:?} [{}W] failed: {:?}",
                        c.from, c.to, c.len_words, e
                    );
                }

                if let Some(s) = c.dma_done {
                    s.post();
                }
            }
        })
        .unwrap();

    start_audio(fs);

    info!("Audio started");

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

    let ship = fs.contents(&[b"assets", b"models", b"ship.nr3d"]).unwrap();
    let beach = fs.contents(&[b"assets", b"models", b"beach.nr3d"]).unwrap();

    let mut prev_touch: Option<(u16, u16)> = None;

    let render_done = Arc::new(Semaphore::new(0));

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

        dma_fifo.as_ref().push(DmaOp {
            from: DmaAddr::from_memory(ship.as_ptr() as usize).unwrap(),
            to: DmaAddr::GPU,
            len_words: ship.len() / 4,
            dma_done: None,
        });
        dma_fifo.as_ref().push(DmaOp {
            from: DmaAddr::from_memory(beach.as_ptr() as usize).unwrap(),
            to: DmaAddr::GPU,
            len_words: beach.len() / 4,
            dma_done: Some(render_done.clone()),
        });

        render_done.wait();

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
        b'T', // Address touchscreen
        b'S', // Read state
        0,    // x high
        0,    // x low
        0,    // y high
        0,    // y low
    ];
    input_device(0, cmd).unwrap();

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

fn start_audio(fs: Fs) {
    let note = fs.contents(&[b"assets", b"audio", b"A440.nrad"]).unwrap();

    info!(
        "Note: {}B {}",
        note.len(),
        note.iter().fold(0u32, |acc, &b| acc + b as u32)
    );

    let a_step = nrad_step(note) as i32;

    nrad_upload(0, note);

    spu_main_volume(i16::MAX / 2, i16::MAX / 2);
    spu_voice_volume(0, i16::MAX, i16::MAX);
    spu_voice_volume(1, i16::MAX, i16::MAX);

    spu_voice_start_block(0, 0);
    spu_voice_start_block(1, 0);

    ThreadBuilder::new(*b"TON0")
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
        })
        .unwrap();

    ThreadBuilder::new(*b"TON1")
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
        })
        .unwrap();
}

fn spu_upload(addr: u16, d: &[u8]) {
    assert_eq!(addr & 3, 0, "SPU addr misaligned");
    unsafe {
        SPU_RAM_ADDR.write_volatile(addr as u32);
    }

    for b in d.chunks_exact(4) {
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        unsafe {
            SPU_RAM_W.write_volatile(w);
        }
    }
}

fn spu_main_volume(vleft: i16, vright: i16) {
    let v = ((vleft as u32) << 16) | (vright as u32);

    unsafe {
        SPU_VOLUME_MAIN.write_volatile(v);
    }
}

fn spu_voice_volume(voice: u32, vleft: i16, vright: i16) {
    let v = ((vleft as u32) << 16) | (vright as u32);
    let p = (SPU_VOICE_VOLUME + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        p.write_volatile(v);
    }
}

fn spu_voice_step(voice: u32, step: u16) {
    let p = (SPU_VOICE_STEP + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        p.write_volatile(step as u32);
    }
}

fn spu_voice_start_block(voice: u32, addr: u32) {
    let p = (SPU_VOICE_START_BLOCK + voice * SPU_VOICE_OFF) as *mut u32;

    unsafe {
        p.write_volatile(addr);
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

const SPU_BASE: u32 = 0x4002_0000;
const SPU_VOLUME_MAIN: *mut u32 = SPU_BASE as *mut u32;
const SPU_VOICE_ON: *mut u32 = (SPU_BASE + 4) as *mut u32;
const SPU_RAM_ADDR: *mut u32 = (SPU_BASE + 4 * 4) as *mut u32;
const SPU_RAM_W: *mut u32 = (SPU_BASE + 5 * 4) as *mut u32;

const SPU_VOICE_BASE: u32 = SPU_BASE + 0x100;
const SPU_VOICE_OFF: u32 = 0x20;

const SPU_VOICE_STEP: u32 = SPU_VOICE_BASE;
const SPU_VOICE_START_BLOCK: u32 = SPU_VOICE_BASE + 4;
const SPU_VOICE_VOLUME: u32 = SPU_VOICE_BASE + 2 * 4;
