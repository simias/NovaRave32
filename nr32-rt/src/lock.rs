
use core::cell::{Cell, UnsafeCell};
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{compiler_fence, Ordering};

/// Simple Mutex that is really just a flag given that we have a single-core CPU and we run the
/// kernel with IRQ disabled.
pub struct Mutex<T> {
    locked: Cell<bool>,
    value:  UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for Mutex<T> {}
unsafe impl<T: Send> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub const fn new(val: T) -> Self {
        Self { locked: Cell::new(false), value: UnsafeCell::new(val) }
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let was_locked = self.locked.replace(true);

        if was_locked {
            None
        } else {
            compiler_fence(Ordering::Acquire);
            Some(MutexGuard { m: self })
        }
    }

    #[unsafe(link_section = ".text.fast")]
    fn unlock(&self) {
        compiler_fence(Ordering::Release);
        self.locked.set(false);
    }
}

pub struct MutexGuard<'a, T> { m: &'a Mutex<T> }

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T { unsafe { &*self.m.value.get() } }
}
impl<'a, T> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T { unsafe { &mut *self.m.value.get() } }
}
impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) { self.m.unlock(); }
}
