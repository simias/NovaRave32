use crate::asm::{current_mode, PrivilegeMode};
use crate::syscall;
use core::alloc::{GlobalAlloc, Layout};
use embedded_alloc::LlffHeap;

/// Wrapper around an allocator to handle allocs from both kernel and user modes
pub struct Allocator {
    heap: LlffHeap,
}

impl Allocator {
    pub const fn empty() -> Allocator {
        Allocator {
            heap: LlffHeap::empty(),
        }
    }

    pub unsafe fn init(&self, start_addr: usize, size: usize) {
        self.heap.init(start_addr, size);
    }

    pub fn log_heap_stats(&self) {
        let used = self.heap.used();
        let free = self.heap.free();
        let tot = used + free;

        info!(
            "{}KiB used ({}%) {}KiB free",
            used / 1024,
            (used * 100 + tot / 2) / tot,
            free / 1024,
        );
    }

    pub fn raw_alloc(&self, size: usize, align: usize) -> *mut u8 {
        match Layout::from_size_align(size, align) {
            Ok(l) => unsafe { self.alloc(l) },
            Err(_) => core::ptr::null_mut(),
        }
    }

    pub fn raw_free(&self, ptr: *mut u8, size: usize, align: usize) {
        if let Ok(l) = Layout::from_size_align(size, align) {
            unsafe { self.dealloc(ptr, l) }
        }
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match current_mode() {
            PrivilegeMode::Kernel => self.heap.alloc(layout),
            PrivilegeMode::User => syscall::alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match current_mode() {
            PrivilegeMode::Kernel => self.heap.dealloc(ptr, layout),
            PrivilegeMode::User => syscall::free(ptr, layout),
        }
    }
}
