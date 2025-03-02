#![no_std]

extern crate alloc;

#[macro_use]
extern crate log;

mod asm;
mod console;
pub mod gpu;
pub mod math;
mod scheduler;
pub mod syscalls;
pub mod utils;

use embedded_alloc::LlffHeap as Heap;

// Linker symbols
extern "C" {
    static __sstack: u8;
    static __estack: u8;
    static __sheap: u8;
    static __eheap: u8;
}

// Declared by the main project
extern "C" {
    fn nr32_main();
}

/// This task much schedule the first task (by setting mepc, mscratch etc...) and return
#[export_name = "_system_entry"]
pub fn rust_start() {
    system_init();

    let mut sched = scheduler::get();
    sched.start();
    sched.spawn_task(nr32_main as usize, 0, TASK_STACK_SIZE);
    sched.schedule();
}

/// Called for trap handling
#[export_name = "_system_trap"]
pub fn rust_trap() {
    let cause = riscv::register::mcause::read();

    match (cause.is_interrupt(), cause.code()) {
        // MTIME interrupt
        (true, 7) => {
            let mut sched = scheduler::get();
            sched.schedule();
        }
        // External interrupt
        (true, 11) => handle_irqs(),
        // ECALL from user mode
        (false, 8) => handle_ecall(),
        _ => panic!("Unhandled trap {:x?}", cause),
    }
}

fn handle_irqs() {
    let pending = unsafe { IRQ_PENDING.read() };

    // VSYNC
    if pending & 1 != 0 {
        let mut sched = scheduler::get();
        sched.got_vsync();
    }

    // ACK everything
    unsafe {
        IRQ_PENDING.write(pending);
    }
}

fn handle_ecall() {
    // First we have to adjust MEPC to point after the ecall instruction, otherwise it'll be
    // executed again upon return
    let pc = riscv::register::mepc::read();
    riscv::register::mepc::write(pc + 4);

    // We need to get the syscall code and arguments from its task since that's where the trap
    // handler will have banked them
    let task_sp = riscv::register::mscratch::read();

    let task_reg = |reg: usize| -> usize {
        let p = task_sp + (33 - reg) * 4;

        unsafe {
            let p = p as *const usize;

            *p
        }
    };

    /* a7 */
    let code = task_reg(17);
    /* a0 */
    let arg0 = task_reg(10);
    /* a1 */
    let arg1 = task_reg(11);

    let mut sched = scheduler::get();
    let ret = match code {
        // Can also be used for yielding with `ticks` set to 0
        syscalls::SYS_SLEEP => {
            let ticks = (arg0 as u64) | ((arg1 as u64) << 32);

            sched.sleep_current_task(ticks);
            0
        }
        syscalls::SYS_WAIT_EVENT => sched.wait_event_current_task(arg0),
        syscalls::SYS_SPAWN_TASK => {
            let entry = arg0;
            let prio = arg1 as i32;

            sched.spawn_task(entry, prio, TASK_STACK_SIZE);
            0
        }
        syscalls::SYS_EXIT => {
            sched.exit_current_task();
            0
        }
        _ => panic!("Unknown syscall 0x{:02x}", code),
    };

    // Set return value in a0
    unsafe {
        let p = task_sp + (23 * 4);
        let p = p as *mut usize;
        p.write(ret);
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

    // Activate VSYNC IRQ (for tasks that block on VSync, we could only enable it when needed but
    // it's a minor load)
    unsafe {
        // ACK everything just in case
        IRQ_PENDING.write(!0);
        // Enable VSYNC IRQ
        IRQ_ENABLED.write(1);
        riscv::register::mie::set_mext();
    }
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

/// External Interrupt Controller: IRQ pending register
const IRQ_PENDING: *mut usize = 0xffff_ffe0 as *mut usize;
/// External Interrupt Controller: IRQ enabled register
const IRQ_ENABLED: *mut usize = 0xffff_ffe4 as *mut usize;

const TASK_STACK_SIZE: usize = 4096 - 128;
