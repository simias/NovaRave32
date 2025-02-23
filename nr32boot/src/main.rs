#![no_std]
#![no_main]

// Default panic handler that just loops
extern crate alloc;

#[macro_use]
extern crate log;

mod asm;
mod console;
mod scheduler;
mod tasks;
mod utils;

use embedded_alloc::LlffHeap as Heap;

// Linker symbols
extern "C" {
    static __sstack: u8;
    static __estack: u8;
    static __sheap: u8;
    static __eheap: u8;
}

/// This task much schedule the first task (by setting mepc, mscratch etc...) and return
#[export_name = "_system_entry"]
pub fn rust_start() {
    system_init();

    let mut sched = scheduler::get();
    sched.start(tasks::idle_task, tasks::main_task);
}

/// Called for trap handling
#[export_name = "_system_trap"]
pub fn rust_trap() {
    let cause = riscv::register::mcause::read();

    match (cause.is_interrupt(), cause.code()) {
        // MTIME interrupt
        (true, 7) => {
            let mut sched = scheduler::get();
            sched.preempt_current_task();
        }
        _ => panic!("Unhandled trap {:x?}", cause),
    }
}

fn system_init() {
    let stack_start = unsafe { &__sstack as *const u8 as usize };
    let stack_end = unsafe { &__estack as *const u8 as usize };
    let stack_size = stack_end - stack_start;
    let heap_start = unsafe { &__sheap as *const u8 as usize };
    let heap_end = unsafe { &__eheap as *const u8 as usize };
    let heap_size = heap_end - heap_start;

    log::set_logger(&console::LOGGER)
        .map(|()| log::set_max_level(log::LevelFilter::Trace))
        .unwrap();

    info!("BOOTING v{}", env!("CARGO_PKG_VERSION"));
    info!(
        "System stack: 0x{:x?} - 0x{:x?} [{:x}KiB]",
        stack_start,
        stack_end,
        stack_size / 1024
    );
    info!(
        "Heap:         0x{:x?} - 0x{:x?} [{}KiB]",
        heap_start,
        heap_end,
        heap_size / 1024
    );

    // Init allocator
    unsafe { HEAP.init(heap_start, heap_size) };

    utils::log_heap_stats();
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

/// Frequency of the MTIME timer tick
const MTIME_HZ: u32 = 48_000 * 16;

/// Length of one millisecond in number of MTIME ticks
const MTIME_1MS: u32 = (MTIME_HZ + 500) / 1000;
