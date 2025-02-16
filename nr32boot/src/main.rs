#![no_std]
#![no_main]

// Default panic handler that just loops
extern crate alloc;

#[macro_use]
extern crate log;

mod console;
mod scheduler;
mod tasks;
mod utils;

use embedded_alloc::LlffHeap as Heap;
use riscv_rt::entry;

// Linker symbols
extern "C" {
    static _sheap: u8;
    static _hart_stack_size: u8;
    static _stack_start: u8;
}

#[entry]
fn main() -> ! {
    let stack_start = unsafe { &_stack_start as *const u8 as usize };
    let stack_size = unsafe { &_hart_stack_size as *const u8 as usize };
    let heap_start = unsafe { &_sheap as *const u8 as usize };
    let heap_size = stack_start - heap_start;

    log::set_logger(&console::LOGGER)
        .map(|()| log::set_max_level(log::LevelFilter::Trace))
        .unwrap();

    info!("BOOTING v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "System stack: 0x{:x?} - 0x{:x?} [{:x}KiB]",
        stack_start - stack_size,
        stack_start,
        stack_size / 1024
    );
    info!(
        "Heap:         0x{:x?} - 0x{:x?} [{}KiB]",
        heap_start,
        heap_start + heap_size,
        heap_size / 1024
    );

    // Init allocator
    unsafe { HEAP.init(heap_start, heap_size) };

    utils::log_heap_stats();

    scheduler::start(tasks::run_main_task);

    utils::shutdown(0)
}

#[global_allocator]
static HEAP: Heap = Heap::empty();

mod panic_handler {
    use crate::utils::shutdown;
    use core::panic::PanicInfo;

    #[inline(never)]
    #[panic_handler]
    fn panic(info: &PanicInfo) -> ! {
        error!("!PANIC!");
        error!("{}", info);
        shutdown(!0)
    }
}
