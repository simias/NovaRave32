use crate::lock::{Mutex, MutexGuard};
use crate::{SysError, SysResult};
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;

pub struct Allocator {
    /// Heap for use by the kernel
    system_heap: Mutex<NrHeap>,
    /// Heap for use by the userland
    user_heap: Mutex<NrHeap>,
}

impl Allocator {
    pub const fn empty() -> Allocator {
        Allocator {
            system_heap: Mutex::new(NrHeap::empty()),
            user_heap: Mutex::new(NrHeap::empty()),
        }
    }

    pub fn system_heap<'a>(&'a self) -> MutexGuard<'a, NrHeap> {
        match self.system_heap.try_lock() {
            Some(h) => h,
            None => panic!("Couldn't lock system allocator!"),
        }
    }

    pub fn user_heap<'a>(&'a self) -> MutexGuard<'a, NrHeap> {
        match self.user_heap.try_lock() {
            Some(h) => h,
            None => panic!("Couldn't lock system allocator!"),
        }
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.system_heap().raw_alloc(layout.size(), layout.align())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let _ = self.system_heap().raw_free(ptr);
    }
}

/// Simple linked-list based allocator
pub struct NrHeap {
    heap_start: usize,
}

impl NrHeap {
    pub const fn empty() -> NrHeap {
        NrHeap { heap_start: 0 }
    }

    fn block_iter(&self) -> MemBlockIter {
        MemBlockIter::new(self.heap_start)
    }

    pub unsafe fn init(&mut self, start_addr: usize, size: usize) {
        let heap_start = align_up(start_addr, MemBlock::ALIGN);
        let off = heap_start - start_addr;
        let heap_size = align_down(size - off, MemBlock::ALIGN);

        if size >= MemBlock::MIN_SIZE && heap_start != 0 {
            self.heap_start = heap_start;

            let b = MemBlock::block_at(self.heap_start);

            unsafe {
                (*b).magic = MemBlock::MAGIC;
                (*b).prev = ptr::null_mut();
                (*b).next = ptr::null_mut();
                (*b).flags = 0;
                (*b).size = heap_size - MemBlock::HEADER_SIZE;
            }

            info!("Heap init {}B at 0x{:x}", heap_size, heap_start);
        } else {
            self.heap_start = 0;
        }
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn try_alloc(&self, size: usize, align: usize) -> SysResult<NonNull<u8>> {
        let ptr = self.raw_alloc(size, align);

        NonNull::new(ptr).ok_or(SysError::NoMem)
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn raw_alloc(&self, size: usize, align: usize) -> *mut u8 {
        if align > MemBlock::ALIGN {
            error!("Unimplemented alloc align {}", align);
            return ptr::null_mut();
        }

        let size = align_up(size, MemBlock::ALIGN);

        for b in self.block_iter() {
            unsafe {
                if (*b).is_used() {
                    continue;
                }

                let bsz = (*b).size;

                if bsz >= size {
                    let pstart = (*b).payload_start();

                    (*b).flags |= MemBlock::FLAG_USED;

                    let remaining_size = bsz - size;

                    if remaining_size > MemBlock::MIN_SIZE {
                        // Create a new block for the remainder
                        let nblock = (pstart as usize) + size;

                        let nb = MemBlock::block_at(nblock);

                        (*b).size = size;

                        (*nb).size = remaining_size - MemBlock::HEADER_SIZE;
                        (*nb).magic = MemBlock::MAGIC;
                        (*nb).flags = 0;
                        (*nb).next = (*b).next;
                        (*nb).prev = b;

                        (*nb).relink();
                    }

                    return pstart;
                }
            }
        }

        error!("Alloc of size {} failed", size);
        ptr::null_mut()
    }

    #[unsafe(link_section = ".text.fast")]
    pub fn raw_free(&self, ptr: *mut u8) -> SysResult<()> {
        if ptr.is_null() {
            return Err(SysError::Invalid);
        }

        let block_addr = (ptr as usize) - MemBlock::HEADER_SIZE;

        let b = MemBlock::block_at(block_addr);

        unsafe {
            if !(*b).is_valid() {
                error!("Attempt to free invalid block at {:x}", block_addr);
                return Err(SysError::Invalid);
            }

            (*b).flags &= !MemBlock::FLAG_USED;

            let nb = (*b).next;

            if !nb.is_null() && !(*nb).is_used() {
                // Collapse the next block
                (*b).size += (*nb).size + MemBlock::HEADER_SIZE;
                (*b).next = (*nb).next;

                (*b).relink();
            }

            let pb = (*b).prev;

            if !pb.is_null() && !(*pb).is_used() {
                // Collapse the prev block
                (*pb).size += (*b).size + MemBlock::HEADER_SIZE;
                (*pb).next = (*b).next;

                (*pb).relink();
            }
        }

        Ok(())
    }
}

#[repr(C)]
struct MemBlock {
    /// Size of the block, not including this header. The next block (if any) will be at the start
    /// address of this block + size
    ///
    /// `size` should always be aligned to `MemBlock::ALIGN`
    size: usize,
    /// Address of the next block (if any)
    next: *mut MemBlock,
    /// Address of the prev block (if any)
    prev: *mut MemBlock,
    /// Block flags
    flags: u16,
    /// Sentinel value to catch bogus free()
    magic: u16,
}

impl MemBlock {
    const MAGIC: u16 = 0x1337;

    const FLAG_USED: u16 = 1;

    /// Default block alignment (in bytes)
    const ALIGN: usize = 16;

    const HEADER_SIZE: usize = align_up(core::mem::size_of::<MemBlock>(), Self::ALIGN);

    /// Minimal size of a block. A block should never be created if it's smaller than this.
    ///
    /// Technically we could have a block with only enough size for a MemBlock header, but what
    /// would be the point of that?
    const MIN_SIZE: usize = Self::HEADER_SIZE + Self::ALIGN;

    fn block_at(block_addr: usize) -> *mut MemBlock {
        block_addr as *mut MemBlock
    }

    fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC
    }

    fn is_used(&self) -> bool {
        self.flags & Self::FLAG_USED != 0
    }

    fn payload_start(&mut self) -> *mut u8 {
        unsafe { (self as *mut MemBlock as *mut u8).add(Self::HEADER_SIZE) }
    }

    /// Make sure that prev->next and next->prev are self
    fn relink(&mut self) {
        unsafe {
            let nb = self.next;
            let pb = self.prev;

            if !nb.is_null() {
                (*nb).prev = self as *mut _;
            }
            if !pb.is_null() {
                (*pb).next = self as *mut _;
            }
        }
    }
}

struct MemBlockIter {
    next_block: *mut MemBlock,
}

impl MemBlockIter {
    fn new(heap_start: usize) -> MemBlockIter {
        MemBlockIter {
            next_block: MemBlock::block_at(heap_start),
        }
    }
}

impl Iterator for MemBlockIter {
    type Item = *mut MemBlock;

    fn next(&mut self) -> Option<Self::Item> {
        let b = self.next_block;

        if b.is_null() {
            return None;
        }

        unsafe {
            if !(*b).is_valid() {
                panic!("Corrupted heap at 0x{:x}!", b as usize);
            }

            self.next_block = (*b).next;
        }

        Some(b)
    }
}

/// Align `addr` to `alignment` (which should be a power of 2), rounding up
const fn align_up(addr: usize, align: usize) -> usize {
    (addr.wrapping_add(align - 1)) & !(align - 1)
}

/// Align `addr` to `alignment` (which should be a power of 2), rounding down
const fn align_down(addr: usize, align: usize) -> usize {
    addr & !(align - 1)
}
