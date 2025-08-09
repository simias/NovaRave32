use crate::scheduler;
use alloc::string::String;
use core::slice;

pub fn run_boot_script() {
    let script = unsafe { slice::from_raw_parts(0x2000_0010 as *const u8, 0x100 - 0x10) };

    for b in script.chunks_exact(16) {
        let code = [b[0], b[1], b[2], b[3]];

        let mut params = [0; 3];

        if code == [0xff; 4] {
            continue;
        }

        for (i, b) in b[4..16].chunks_exact(4).enumerate() {
            params[i] = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        }

        run_op(code, params);

        // let s = String::from_utf8_lossy(&code);
        // info!("Got op {} {:x?}", s, params);
    }
}

fn run_op(code: [u8; 4], params: [u32; 3]) {
    match &code {
        b"COPY" => {
            let rom_base = params[0] as *const u8;
            let ram_base = params[1] as *mut u8;
            let len = params[2] as usize;

            if len == 0 {
                return;
            }

            let src = unsafe { slice::from_raw_parts(rom_base, len) };

            let dst = unsafe { slice::from_raw_parts_mut(ram_base, len) };

            dst.copy_from_slice(src);
        }
        b"ZERO" => {
            let ram_base = params[0] as *mut u8;
            let len = params[1] as usize;

            if len == 0 {
                return;
            }

            let dst = unsafe { slice::from_raw_parts_mut(ram_base, len) };

            dst.fill(0);
        }
        b"HEAP" => {
            let heap_base = params[0] as usize;
            let len = params[1] as usize;

            unsafe {
                crate::ALLOCATOR.user_heap().init(heap_base, len);
            }
        }
        b"EXEC" => {
            let entry = params[0] as usize;
            let stack_size = params[1] as usize;
            let gp = params[2] as usize;

            let mut sched = scheduler::get();

            sched.spawn_task(scheduler::TaskType::User, entry, 0, 0, stack_size, gp);
        }
        _ => {
            let desc = String::from_utf8_lossy(&code);
            info!("Got unknown op {} {:x?}", desc, params);
        }
    }
}
