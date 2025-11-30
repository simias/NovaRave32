use crate::syscall::{SysResult, syscall_3};
pub use nr32_common::syscall::DmaAddr;
use nr32_common::syscall::SYS_DO_DMA;

pub fn do_dma(source: DmaAddr, target: DmaAddr, len_words: usize) -> SysResult<()> {
    unsafe {
        syscall_3(
            SYS_DO_DMA,
            source.raw() as usize,
            target.raw() as usize,
            len_words,
        )
    }
    .map(|_| ())
}
