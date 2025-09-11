use crate::lock::{Mutex, MutexGuard};
use nr32_common::error::{SysError, SysResult};
use nr32_common::memmap;
use nr32_common::syscall::DmaAddr;

pub struct Dma {
    /// True if a DMA transaction is running. We only allow one DMA request at a time.
    in_progress: bool,
}

static DMA: Mutex<Dma> = Mutex::new(Dma { in_progress: false });

pub fn get() -> MutexGuard<'static, Dma> {
    match DMA.try_lock() {
        Some(lock) => lock,
        None => {
            panic!("Couldn't lock DMA!")
        }
    }
}

impl Dma {
    pub fn done(&mut self) {
        self.in_progress = false;
    }

    pub fn start(&mut self, src: usize, dst: usize, len_words: usize) -> SysResult<()> {
        if len_words == 0 {
            return Err(SysError::Invalid);
        }

        let src = DmaAddr::src_from_raw(src as u32)?;
        let dst = DmaAddr::dst_from_raw(dst as u32)?;

        if self.in_progress {
            return Err(SysError::Busy);
        }

        unsafe {
            DMA_SRC.write_volatile(src.raw());
            DMA_DST.write_volatile(dst.raw());
            // This also starts the transfer
            DMA_LEN.write_volatile(len_words as u32);
        }

        Ok(())
    }
}

const DMA_BASE: usize = memmap::DMA.base as usize;
const DMA_SRC: *mut u32 = DMA_BASE as *mut u32;
const DMA_DST: *mut u32 = (DMA_BASE + 4) as *mut u32;
const DMA_LEN: *mut u32 = (DMA_BASE + 8) as *mut u32;
