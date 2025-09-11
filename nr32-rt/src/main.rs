//! BIOS/minimalist kernel for the NovaRave32

#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate log;

mod allocator;
mod asm;
mod bootscript;
mod console;
mod dma;
mod input_dev;
mod lock;
mod scheduler;
mod utils;

use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering::Acquire};
use nr32_common::error::{SysError, SysResult};
use nr32_common::syscall;

// Linker symbols
unsafe extern "C" {
    static __sstack: u8;
    static __estack: u8;
    static __sheap: u8;
    static __eheap: u8;
}

/// The system entry must schedule the first task (by setting mepc, mscratch etc...) and return
#[unsafe(export_name = "_system_entry")]
pub extern "C" fn rust_start() {
    system_init();

    {
        let mut sched = scheduler::get();
        sched.start();
    }
    info!("Kernel is running");
    bootscript::run_boot_script();
    {
        let mut sched = scheduler::get();
        sched.schedule();
    }
}

/// Called for trap handling *except* ecall (MCAUSE = 8) that gets forwarded to handle_ecall
#[unsafe(export_name = "_system_trap")]
#[unsafe(link_section = ".text.fast")]
pub extern "C" fn rust_trap() {
    let cause = riscv::register::mcause::read();

    match (cause.is_interrupt(), cause.code()) {
        // MTIME interrupt
        (true, 7) => {
            let mut sched = scheduler::get();
            sched.schedule();
        }
        // External interrupt
        (true, 11) => handle_irqs(),
        _ => panic!("Unhandled trap {:x?}", cause),
    }
}

#[unsafe(link_section = ".text.fast")]
fn handle_irqs() {
    let pending = unsafe { IRQ_PENDING.read() };

    // VSYNC
    if pending & 1 != 0 {
        let mut sched = scheduler::get();
        sched.wake_up_state(scheduler::TaskState::WaitingForVSync);
    }

    // Input dev
    if pending & (1 << 1) != 0 {
        let mut input_dev = input_dev::get();
        input_dev.xmit_done();

        let mut sched = scheduler::get();
        sched.wake_up_state(scheduler::TaskState::WaitingForInputDev);
    }

    // DMA
    if pending & (1 << 2) != 0 {
        let mut dma = dma::get();
        dma.done();

        let mut sched = scheduler::get();
        sched.wake_up_state(scheduler::TaskState::WaitingForDma);
    }

    // ACK everything
    unsafe {
        IRQ_PENDING.write_volatile(pending);
    }
}

#[repr(usize)]
pub enum CallerState {
    /// ECall was handled and we can directly return control to the caller
    NoChange = 0,
    /// The caller got preempted, we need to save its registers before returning to the newly
    /// scheduled task
    Preempted = 1,
    /// The task that triggered the ECALL was killed, we can ignore its register and directly
    /// switch to the newly scheduled task
    DeadTask = 2,
}

/// handle_ecall's return value. The layout is what's expected from the ASM trampoline.
///
/// I pack cstate and result in the same value because the RISC-V calling convention will not allow
/// for more than 2 return values in registers, if I try to return 3 words it'll switch to the
/// stack which is annoying.
#[repr(C)]
pub struct ECallRet {
    /// - The low 16 bits are 0 if the call was successful, otherwise a SysError value
    /// - The top 16 bits are the CallerState value
    cstate_result: usize,
    value: usize,
}

impl ECallRet {
    fn from_result_cstate(result: SysResult<usize>, cstate: CallerState) -> ECallRet {
        let cstate = cstate as usize;

        let (result, value) = match result {
            Ok(v) => (0, v),
            Err(e) => (e as usize, 0),
        };

        let cstate_result = (cstate << 16) | result;

        ECallRet {
            cstate_result,
            value,
        }
    }
}

/// Handle ECALL from user mode.
///
/// This is separate
#[unsafe(export_name = "_system_ecall")]
#[unsafe(link_section = ".text.fast")]
pub extern "C" fn handle_ecall(
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    _arg5: usize,
    _arg6: usize,
    sys_no: usize,
) -> ECallRet {
    // First we have to adjust MEPC to point after the ecall instruction, otherwise it'll be
    // executed again upon return
    let pc = riscv::register::mepc::read();
    riscv::register::mepc::write(pc + 4);

    let mut sched = scheduler::get();

    let caller_task = sched.cur_task_id();

    let result: SysResult<usize> = match sys_no as u32 {
        // Can also be used for yielding with `ticks` set to 0
        syscall::SYS_SLEEP => {
            let ticks = (arg0 as u64) | ((arg1 as u64) << 32);

            sched.sleep_current_task(!0, ticks);
            // Sleep always returns a timeout
            Err(SysError::Timeout)
        }
        syscall::SYS_WAIT_FOR_VSYNC => {
            sched.current_task_set_state(scheduler::TaskState::WaitingForVSync);
            Ok(0)
        }
        syscall::SYS_SPAWN_TASK => {
            let entry = arg0;
            let data = arg1;
            let prio = arg2 as i32;
            let stack_size = arg3;
            let gp = arg4;

            sched.spawn_task(scheduler::TaskType::User, entry, data, prio, stack_size, gp)
        }
        syscall::SYS_EXIT => {
            sched.exit_current_task();
            // Task is dead, so we don't want to touch its stack anymore
            return ECallRet::from_result_cstate(Ok(0), CallerState::DeadTask);
        }
        syscall::SYS_ALLOC => ALLOCATOR
            .user_heap()
            .try_alloc(arg0, arg1)
            .map(|p| p.addr().get()),
        syscall::SYS_FREE => ALLOCATOR.user_heap().raw_free(arg0 as *mut u8).map(|_| 0),
        syscall::SYS_INPUT_DEV => {
            let port = arg0 as u8;
            let ptr = arg1 as *mut u8;
            let len = arg2;

            let buf = unsafe { core::slice::from_raw_parts_mut(ptr, len) };

            let mut input_dev = input_dev::get();

            input_dev.xmit(port, buf).map(|_| {
                sched.current_task_set_state(scheduler::TaskState::WaitingForInputDev);
                0
            })
        }
        syscall::SYS_DBG_PUTS => {
            let ptr = arg0 as *const u8;
            let len = arg1;

            let buf = unsafe { core::slice::from_raw_parts(ptr, len) };
            let tid = sched.cur_task_id();

            let _ = write!(console::DebugConsole, "#{tid} ");
            for b in buf {
                console::DebugConsole::putchar(*b);
            }
            console::DebugConsole::putchar(b'\n');

            Ok(len)
        }
        syscall::SYS_SHUTDOWN => {
            let code = arg0 as u16;

            utils::shutdown(code)
        }
        syscall::SYS_FUTEX_WAIT => {
            let futex_addr = arg0;
            let expected_val = arg1;
            let ticks = (arg2 as u64) | ((arg3 as u64) << 32);

            let v = unsafe {
                let p = futex_addr as *const AtomicUsize;

                (*p).load(Acquire)
            };

            if v == expected_val {
                if ticks > 0 {
                    sched.sleep_current_task(futex_addr, ticks);
                }
                Ok(0)
            } else {
                Err(SysError::Again)
            }
        }
        syscall::SYS_FUTEX_WAKE => {
            let futex_addr = arg0;
            let nwakeup = arg1;

            sched.futex_wake(futex_addr, nwakeup)
        }
        syscall::SYS_DO_DMA => {
            let src = arg0;
            let dst = arg1;
            let len_words = arg2;

            let mut dma = dma::get();

            dma.start(src, dst, len_words).map(|_| {
                sched.current_task_set_state(scheduler::TaskState::WaitingForDma);
                0
            })
        }
        _ => Err(SysError::NoSys),
    };

    let caller_state = if sched.cur_task_id() == caller_task {
        // Still running the same task
        CallerState::NoChange
    } else {
        // We're switching to a different task
        CallerState::Preempted
    };

    ECallRet::from_result_cstate(result, caller_state)
}

fn system_init() {
    let stack_start = unsafe { &__sstack as *const u8 as usize };
    let stack_end = unsafe { &__estack as *const u8 as usize };
    let stack_size = stack_end - stack_start;
    let heap_start = unsafe { &__sheap as *const u8 as usize };
    let heap_end = unsafe { &__eheap as *const u8 as usize };
    let heap_size = heap_end - heap_start;

    log::set_logger(&console::LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Trace);

    info!("BOOTING v{}", env!("CARGO_PKG_VERSION"));

    info!(
        "System stack: 0x{:x?} - 0x{:x?} [{:x}KiB]",
        stack_start,
        stack_end,
        stack_size / 1024
    );
    info!(
        "System heap:  0x{:x?} - 0x{:x?} [{}KiB]",
        heap_start,
        heap_end,
        heap_size / 1024
    );

    unsafe { ALLOCATOR.system_heap().init(heap_start, heap_size) };

    unsafe {
        // ACK everything just in case
        IRQ_PENDING.write_volatile(!0);
        let mut irq_en = 0;
        // Activate VSYNC IRQ (for tasks that block on VSync, we could only enable it when needed but
        // it's a minor load)
        irq_en |= 1;
        // Input dev IRQ
        irq_en |= 1 << 1;
        // DMA IRQ
        irq_en |= 1 << 2;
        IRQ_ENABLED.write_volatile(irq_en);
        riscv::register::mie::set_mext();
    }
}

#[global_allocator]
static ALLOCATOR: allocator::Allocator = allocator::Allocator::empty();

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
const MTIME_HZ: u32 = 44_100 * 16;

/// External Interrupt Controller: IRQ pending register
const IRQ_PENDING: *mut usize = 0xffff_fff0 as *mut usize;
/// External Interrupt Controller: IRQ enabled register
const IRQ_ENABLED: *mut usize = 0xffff_fff4 as *mut usize;
