use crate::syscall::{SysResult, syscall_6};
use alloc::boxed::Box;
use core::arch::asm;
use nr32_common::syscall::SYS_SPAWN_TASK;

#[derive(Copy, Clone)]
pub struct ThreadBuilder {
    priority: i32,
    stack_size: usize,
    /// Global pointer. Defaults to the builder thread's current GP.
    gp: usize,
    name: [u8; 4],
}

impl ThreadBuilder {
    #[inline(never)]
    pub fn new(name: [u8; 4]) -> ThreadBuilder {
        let gp: usize;
        unsafe {
            asm!("mv {0}, gp", out(reg) gp);
        }

        ThreadBuilder {
            priority: 0,
            gp,
            stack_size: 4096,
            name,
        }
    }

    pub fn stack_size(mut self, stack_size: usize) -> ThreadBuilder {
        self.stack_size = stack_size;

        self
    }

    pub fn gp(mut self, gp: usize) -> ThreadBuilder {
        self.gp = gp;

        self
    }

    pub fn priority(mut self, priority: i32) -> ThreadBuilder {
        self.priority = priority;

        self
    }

    pub fn spawn<F>(self, f: F) -> SysResult<usize>
    where
        F: FnOnce() + Send + 'static,
    {
        // Box the closure before sending it to the kernel
        let closure: Box<dyn FnOnce()> = Box::new(f);
        let closure: *mut dyn FnOnce() = Box::into_raw(closure);

        let trampoline = Self::trampoline::<F>;

        unsafe {
            syscall_6(
                SYS_SPAWN_TASK,
                trampoline as usize,
                closure as *mut u8 as usize,
                self.priority as usize,
                self.stack_size,
                self.gp,
                u32::from_le_bytes(self.name) as usize,
            )
        }
    }

    unsafe extern "C" fn trampoline<F>(closure: *mut F)
    where
        F: FnOnce(),
    {
        unsafe {
            let closure: Box<F> = Box::from_raw(closure);

            (*closure)()
        }
    }
}
