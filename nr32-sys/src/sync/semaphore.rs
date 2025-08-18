use crate::syscall;
use core::sync::atomic::{
    AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};

/// A semaphore
#[repr(C)]
pub struct Semaphore {
    val: AtomicUsize,
    /// Upper bound on the number of threads currently waiting on the semaphore
    waiting: AtomicUsize,
}

impl Semaphore {
    pub const fn new(v: usize) -> Semaphore {
        Semaphore {
            val: AtomicUsize::new(v),
            waiting: AtomicUsize::new(0),
        }
    }

    /// Attempt to decrement the semaphore, returns `true` if it was successful, `false` if the
    /// semaphore value is <= 0
    pub fn try_wait(&self) -> bool {
        // Start with an arbitrary "optimistic" value, if it's incorrect compare_exchange_weak will
        // fail below
        let mut cur_v = 1;

        while cur_v > 0 {
            match self
                .val
                .compare_exchange_weak(cur_v, cur_v - 1, AcqRel, Relaxed)
            {
                // Success
                Ok(_) => return true,
                // We got raced (or our inital guess was wrong), try again
                Err(v) => cur_v = v,
            }
        }

        // Semaphore value is 0
        false
    }

    /// Decrement the semaphore, blocking if it's <= 0
    pub fn wait(&self) {
        loop {
            if self.try_wait() {
                // Success
                break;
            }

            self.waiting.fetch_add(1, Release);

            unsafe {
                // We ignore the return value because we'll just try again if we got raced
                let _ = syscall::syscall_4(syscall::SYS_FUTEX_WAIT, self.futex_addr(), 0, 0, 0);
            }

            self.waiting.fetch_sub(1, Release);
        }
    }

    pub fn post(&self) {
        self.val.fetch_add(1, Release);

        if self.waiting.load(Acquire) > 0 {
            unsafe {
                syscall::syscall_2(syscall::SYS_FUTEX_WAKE, self.futex_addr(), 1).unwrap();
            }
        }
    }

    fn futex_addr(&self) -> usize {
        &self.val as *const _ as usize
    }
}

unsafe impl Send for Semaphore {}
unsafe impl Sync for Semaphore {}
