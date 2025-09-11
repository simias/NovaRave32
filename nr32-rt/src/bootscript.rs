use crate::scheduler;
use alloc::string::String;
use core::slice;
use nr32_common::bootscript;

pub fn run_boot_script() {
    for entry in bootscript::get() {
        run_op(entry.code, entry.params);

        // let s = String::from_utf8_lossy(&entry.code);
        // info!("Got op {} {:x?}", s, entry.params);
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

            sched
                .spawn_task(scheduler::TaskType::User, entry, 0, 0, stack_size, gp)
                .unwrap();
        }
        _ => {
            let desc = String::from_utf8_lossy(&code);

            if code[0] == b'%' {
                // Reserved for userland usage
                info!("Got user op {} {:x?}", desc, params);
            } else {
                warn!("Got unknown op {} {:x?}", desc, params);
            }
        }
    }
}
