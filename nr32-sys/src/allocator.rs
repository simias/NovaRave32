//! Implementation of an allocator suitable for use as a `global_allocator`:
//!
//! ```rust
//! use nr32_sys::allocator::Allocator;
//!
//! #[global_allocator]
//! static ALLOCATOR: Allocator = Allocator::new();
//! ```
//!
//! Subsequent Allocs/frees go through the syscall interface.

use crate::syscall;
use core::alloc::{GlobalAlloc, Layout};

pub struct Allocator;

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub const fn new() -> Allocator {
        Allocator
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        syscall::alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        syscall::free(ptr)
    }
}

