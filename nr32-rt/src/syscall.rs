use crate::MTIME_HZ;
use alloc::boxed::Box;
use core::alloc::Layout;
use core::arch::asm;
use core::time::Duration;

pub fn sleep(duration: Duration) {
    // Convert in number of ticks
    let micros = duration.as_micros() as u64;
    let f = u64::from(MTIME_HZ);

    let ticks = (micros * f + 1_000_000 / 2) / 1_000_000;

    syscall_2(SYS_SLEEP, ticks as usize, (ticks >> 32) as usize);
}

pub fn wait_for_vsync() {
    syscall_1(SYS_WAIT_EVENT, events::EV_VSYNC);
}

pub fn spawn_task(f: fn(), prio: i32) {
    syscall_2(SYS_SPAWN_TASK, f as usize, prio as usize);
}

pub fn exit() -> ! {
    syscall_0(SYS_EXIT);

    unreachable!()
}

pub fn alloc(layout: Layout) -> *mut u8 {
    syscall_2(SYS_ALLOC, layout.size(), layout.align()) as *mut u8
}

pub fn free(ptr: *mut u8, layout: Layout) {
    syscall_3(SYS_FREE, ptr as usize, layout.size(), layout.align());
}

fn syscall_0(code: usize) -> usize {
    let mut arg0;

    unsafe {
        asm!("ecall",
            in("a7") code,
            out("a0") arg0,
        );
    }

    arg0
}

fn syscall_1(code: usize, mut arg0: usize) -> usize {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
        );
    }

    arg0
}

fn syscall_2(code: usize, mut arg0: usize, arg1: usize) -> usize {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            in("a1") arg1,
        );
    }

    arg0
}

fn syscall_3(code: usize, mut arg0: usize, arg1: usize, arg2: usize) -> usize {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
        );
    }

    arg0
}

fn syscall_4(code: usize, mut arg0: usize, arg1: usize, arg2: usize, arg3: usize) -> usize {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
        );
    }

    arg0
}

#[derive(Copy, Clone)]
pub struct ThreadBuilder {
    priority: i32,
    stack_size: usize,
}

impl ThreadBuilder {
    pub fn new() -> ThreadBuilder {
        ThreadBuilder {
            priority: 0,
            stack_size: 4096,
        }
    }

    pub fn stack_size(mut self, stack_size: usize) -> ThreadBuilder {
        self.stack_size = stack_size;

        self
    }

    pub fn priority(mut self, priority: i32) -> ThreadBuilder {
        self.priority = priority;

        self
    }

    pub fn spawn<F>(self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        // Box the closure before sending it to the kernel
        let closure: Box<dyn FnOnce()> = Box::new(f);
        let closure: *mut dyn FnOnce() = Box::into_raw(closure);

        let trampoline = Self::trampoline::<F>;

        syscall_4(
            SYS_SPAWN_TASK,
            trampoline as usize,
            closure as *mut u8 as usize,
            self.priority as usize,
            self.stack_size,
        );
    }

    unsafe extern "C" fn trampoline<F>(closure: *mut F)
    where
        F: FnOnce(),
    {
        let closure: Box<F> = Box::from_raw(closure);

        (*closure)()
    }
}

/// Suspend task for [a1:a0] MTIME ticks
pub const SYS_SLEEP: usize = 0x01;
/// Wait for the event described in a0
pub const SYS_WAIT_EVENT: usize = 0x02;
/// Spawn a thread
///
/// - a0: thread entry point
/// - a1: thread data
/// - a2: priority
/// - a3: stack size
pub const SYS_SPAWN_TASK: usize = 0x03;
/// Kills the current thread
pub const SYS_EXIT: usize = 0x04;
/// Allocate memory
///
/// - a0: size to allocate
/// - a1: alignment (must be power of 2)
pub const SYS_ALLOC: usize = 0x05;
/// Free memory
///
/// - a0: pointer to free
/// - a1: block size
/// - a2: alignment (must be power of 2)
pub const SYS_FREE: usize = 0x06;

pub mod events {
    pub const EV_VSYNC: usize = 1;
}
