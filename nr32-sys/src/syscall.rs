use alloc::boxed::Box;
use core::alloc::Layout;
use core::arch::asm;
use core::ptr::NonNull;
use core::time::Duration;
use core::sync::atomic::AtomicUsize;

/// Frequency of the MTIME timer tick
const MTIME_HZ: u32 = 44_100 * 16;

fn duration_to_ticks(duration: Duration) -> u64 {
    if duration.as_secs() < 0xffff_ffff {
        let micros = duration.as_micros() as u64;
        let f = u64::from(MTIME_HZ);

        (micros * f + 1_000_000 / 2) / 1_000_000
    } else {
        // Duration so large it may as well be infinite
        !0
    }
}

pub fn sleep(duration: Duration) {
    let ticks = duration_to_ticks(duration);

    let r = unsafe { syscall_2(SYS_SLEEP, ticks as usize, (ticks >> 32) as usize) };

    assert_eq!(r, Err(SysError::TimeOut))
}

pub fn wait_for_vsync() {
    unsafe { syscall_0(SYS_WAIT_FOR_VSYNC).unwrap() };
}

pub fn spawn_task(f: fn(), prio: i32) -> SysResult<usize> {
    unsafe { syscall_2(SYS_SPAWN_TASK, f as usize, prio as usize) }
}

pub fn exit() -> ! {
    unsafe { syscall_0(SYS_EXIT).unwrap() };

    unreachable!()
}

pub fn shutdown(code: u16) -> ! {
    unsafe { syscall_1(SYS_SHUTDOWN, code as _).unwrap() };

    unreachable!()
}

pub fn alloc(layout: Layout) -> SysResult<NonNull<u8>> {
    unsafe { syscall_2(SYS_ALLOC, layout.size(), layout.align()) }
        .and_then(|p| NonNull::new(p as *mut _).ok_or(SysError::NoMem))
}

pub fn free(ptr: *mut u8) -> SysResult<()> {
    unsafe { syscall_1(SYS_FREE, ptr as usize) }.map(|_| ())
}

pub fn input_device(port: u8, data_in_out: &mut [u8]) -> SysResult<()> {
    let len = data_in_out.len();
    let ptr = data_in_out.as_mut_ptr();

    unsafe { syscall_3(SYS_INPUT_DEV, port as usize, ptr as usize, len) }.map(|_| ())
}

pub fn dbg_puts(s: &str) {
    let s = s.as_bytes();

    let len = s.len();
    let ptr = s.as_ptr();

    unsafe { syscall_2(SYS_DBG_PUTS, ptr as usize, len).unwrap() };
}

pub fn futex_wait(atomic: &AtomicUsize, val: usize, timeout: Option<Duration>) -> SysResult<()> {
    let ticks = match timeout {
        Some(d) => duration_to_ticks(d),
        None => !0,
    };

    unsafe {
        syscall_4(
            SYS_FUTEX_WAIT,
            atomic as *const _ as usize,
            val,
            ticks as usize,
            (ticks >> 32) as usize,
        )
    }
    .map(|_| ())
}

pub fn futex_wake(atomic: &AtomicUsize, nwake: usize) -> SysResult<usize> {
    unsafe { syscall_2(SYS_FUTEX_WAKE, atomic as *const _ as usize, nwake) }
}

#[derive(Copy, Clone)]
pub struct ThreadBuilder {
    priority: i32,
    stack_size: usize,
}

impl ThreadBuilder {
    pub fn new() -> ThreadBuilder {
        ThreadBuilder {
            priority: 0,
            stack_size: 4096,
        }
    }

    pub fn stack_size(mut self, stack_size: usize) -> ThreadBuilder {
        self.stack_size = stack_size;

        self
    }

    pub fn priority(mut self, priority: i32) -> ThreadBuilder {
        self.priority = priority;

        self
    }

    pub fn spawn<F>(self, f: F) -> SysResult<usize>
    where
        F: FnOnce() + Send + 'static,
    {
        // Box the closure before sending it to the kernel
        let closure: Box<dyn FnOnce()> = Box::new(f);
        let closure: *mut dyn FnOnce() = Box::into_raw(closure);

        let trampoline = Self::trampoline::<F>;

        unsafe {
            syscall_4(
                SYS_SPAWN_TASK,
                trampoline as usize,
                closure as *mut u8 as usize,
                self.priority as usize,
                self.stack_size,
            )
        }
    }

    unsafe extern "C" fn trampoline<F>(closure: *mut F)
    where
        F: FnOnce(),
    {
        unsafe {
            let closure: Box<F> = Box::from_raw(closure);

            (*closure)()
        }
    }
}

impl Default for ThreadBuilder {
    fn default() -> Self {
        Self::new()
    }
}

unsafe fn syscall_0(code: usize) -> SysResult<usize> {
    let mut arg0;
    let mut arg1;

    unsafe {
        asm!("ecall",
            in("a7") code,
            out("a0") arg0,
            out("a1") arg1,
            clobber_abi("C"),
        );
    }

    check_syscall_return(arg0, arg1)
}

unsafe fn syscall_1(code: usize, mut arg0: usize) -> SysResult<usize> {
    let mut arg1;

    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            out("a1") arg1,
            clobber_abi("C"),
        );
    }

    check_syscall_return(arg0, arg1)
}

unsafe fn syscall_2(code: usize, mut arg0: usize, mut arg1: usize) -> SysResult<usize> {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            inout("a1") arg1,
            clobber_abi("C"),
        );
    }

    check_syscall_return(arg0, arg1)
}

unsafe fn syscall_3(
    code: usize,
    mut arg0: usize,
    mut arg1: usize,
    arg2: usize,
) -> SysResult<usize> {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            inout("a1") arg1,
            in("a2") arg2,
            clobber_abi("C"),
        );
    }

    check_syscall_return(arg0, arg1)
}

unsafe fn syscall_4(
    code: usize,
    mut arg0: usize,
    mut arg1: usize,
    arg2: usize,
    arg3: usize,
) -> SysResult<usize> {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            inout("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            clobber_abi("C"),
        );
    }

    check_syscall_return(arg0, arg1)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SysError {
    /// Device or resource is busy
    Busy = 1,
    /// Resource temporarily unavailable
    Again = 2,
    /// Cannot allocate memory
    NoMem = 3,
    /// Invalid argument
    Invalid = 4,
    /// Message is too long
    TooLong = 5,
    /// Function not implemented
    NoSys = 6,
    /// Timeout
    TimeOut = 7,
}

type SysResult<T> = Result<T, SysError>;

fn check_syscall_return(result: usize, val: usize) -> SysResult<usize> {
    use SysError::*;

    let err = match result {
        0 => return Ok(val),
        1 => Busy,
        2 => Again,
        3 => NoMem,
        4 => Invalid,
        5 => TooLong,
        6 => NoSys,
        7 => TimeOut,
        e => {
            warn!("Unexpected syscall error: {e}");
            Invalid
        }
    };

    Err(err)
}

/// Suspend task for [a1:a0] MTIME ticks
///
/// Always returns SysError::TimeOut
pub const SYS_SLEEP: usize = 0x01;

/// Put task to sleep until VSYNC
pub const SYS_WAIT_FOR_VSYNC: usize = 0x02;

/// Spawn a thread
///
/// - a0: thread entry point
/// - a1: thread data
/// - a2: priority
/// - a3: stack size
pub const SYS_SPAWN_TASK: usize = 0x03;

/// Kills the current thread
pub const SYS_EXIT: usize = 0x04;

/// Allocate memory
///
/// - a0: size to allocate
/// - a1: alignment (must be power of 2)
pub const SYS_ALLOC: usize = 0x05;

/// Free memory
///
/// - a0: pointer to free
/// - a1: block size
/// - a2: alignment (must be power of 2)
pub const SYS_FREE: usize = 0x06;

/// Input port data exchange. Suspends task until transfer has completed.
///
/// - a0: port to select
/// - a1: pointer to the read/write buffer containing the data to be sent and filled with the reply
/// - a2: how many bytes to read/write (max 16)
pub const SYS_INPUT_DEV: usize = 0x07;

/// Send a string to the debug console. The string is assumed to be UTF-8, other formats may be
/// mangled (but won't crash)
///
/// - a0: pointer to the start of the string
/// - a1: length of the string in bytes (NOT unicode characters)
pub const SYS_DBG_PUTS: usize = 0x08;

/// Shutdown the emulator
///
/// - a0: exit code (truncated to 16bits)
pub const SYS_SHUTDOWN: usize = 0x09;

/// Futex wait
///
/// - a0: address of an AtomicUsize
/// - a1: expected value of the AtomicIsize in a0 (if the values differ, the function returns).
/// - [a3:a2]: wait timeout in MTIME ticks (0 for infinite)
///
/// If the values differ, the call returns immediately with EAGAIN
///
/// The function can return spuriously for any reason
pub const SYS_FUTEX_WAIT: usize = 0x0a;

/// Futex wake
///
/// - a0: address of an AtomicUsize
/// - a1: number of waiting threads to wake up
///
/// Returns the number of threads successfully awoken
pub const SYS_FUTEX_WAKE: usize = 0x0b;
