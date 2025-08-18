//! Vyukov-style MPMC thread-safe queue implementation

use super::Semaphore;
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{
    AtomicUsize,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};

/// Bounded MPMC FIFO. N must be a power of two.
pub struct Fifo<T, const N: usize> {
    /// Sequence number for each FIFO slot
    seq: [AtomicUsize; N],
    elems: [UnsafeCell<MaybeUninit<T>>; N],
    write_idx: AtomicUsize,
    read_idx: AtomicUsize,
    empty_cells: Semaphore,
    filled_cells: Semaphore,
}

unsafe impl<T: Send, const N: usize> Send for Fifo<T, N> {}
unsafe impl<T: Send, const N: usize> Sync for Fifo<T, N> {}

impl<T, const N: usize> Fifo<T, N> {
    pub fn new() -> Self {
        assert_ne!(N, 0, "Attempted to build an 0-len FIFO");
        assert!(N <= (1 << (usize::BITS - 1)), "N too large");
        assert_eq!(N & (N - 1), 0, "N is not a power of two");

        let seq = core::array::from_fn(AtomicUsize::new);

        let elems = core::array::from_fn(|_| UnsafeCell::new(MaybeUninit::uninit()));

        Self {
            seq,
            elems,
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
            empty_cells: Semaphore::new(N),
            filled_cells: Semaphore::new(0),
        }
    }

    pub fn do_push(&self, v: T) {
        let mut wp = self.write_idx.load(Relaxed);

        loop {
            let wseq = self.seq[wp & (N - 1)].load(Acquire);

            if wseq == wp {
                // Slot is free, attempt to claim it by moving the write pointer forward
                match self
                    .write_idx
                    .compare_exchange_weak(wp, wp.wrapping_add(1), AcqRel, Relaxed)
                {
                    Ok(_) => {
                        // We have ownership of the slot, we can write the new value
                        unsafe {
                            (*self.elems[wp & (N - 1)].get()).write(v);
                        }
                        // Increment seq so that the reader knows it's available
                        self.seq[wp & (N - 1)].store(wp.wrapping_add(1), Release);
                        self.filled_cells.post();
                        break;
                    }
                    // Somebody claimed this slot already, retry
                    Err(wp_new) => wp = wp_new,
                }
            } else {
                // We got raced and we need to try again by looping
                wp = self.write_idx.load(Relaxed);
            }

            core::hint::spin_loop();
        }
    }

    /// Push to the queue, blocking if the queue is full
    pub fn push(&self, v: T) {
        self.empty_cells.wait();

        self.do_push(v)
    }

    /// Attempt to push to the queue, returns the item in an error if the queue is full
    pub fn try_push(&self, v: T) -> Result<(), T> {
        if !self.empty_cells.try_wait() {
            // No empty cells left
            return Err(v);
        }

        self.do_push(v);

        Ok(())
    }

    fn do_pop(&self) -> T {
        let mut rp = self.read_idx.load(Relaxed);

        loop {
            let rseq = self.seq[rp & (N - 1)].load(Acquire);

            if rseq == rp.wrapping_add(1) {
                // Slot is available to be read
                match self
                    .read_idx
                    .compare_exchange_weak(rp, rp.wrapping_add(1), AcqRel, Relaxed)
                {
                    Ok(_) => {
                        // We have ownership of the slot, we can read the value
                        let v = unsafe { (*self.elems[rp & (N - 1)].get()).assume_init_read() };
                        // Move seq to the next writer round
                        self.seq[rp & (N - 1)].store(rp.wrapping_add(N), Release);
                        self.empty_cells.post();
                        break v;
                    }
                    // Somebody claimed this slot already, retry
                    Err(rp_new) => rp = rp_new,
                }
            } else {
                rp = self.read_idx.load(Relaxed);
            }

            core::hint::spin_loop();
        }
    }

    pub fn pop(&self) -> T {
        self.filled_cells.wait();

        self.do_pop()
    }

    pub fn try_pop(&self) -> Option<T> {
        if !self.filled_cells.try_wait() {
            return None;
        }

        Some(self.do_pop())
    }
}

impl<T, const N: usize> Default for Fifo<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
