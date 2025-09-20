use alloc::boxed::Box;
use core::alloc::Layout;
use core::arch::asm;
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;
use core::time::Duration;
use nr32_common::syscall::*;

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

    assert_eq!(r, Err(SysError::Timeout))
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
    /// Global pointer. Defaults to the builder thread's current GP.
    gp: usize,
}

impl ThreadBuilder {
    #[inline(never)]
    pub fn new() -> ThreadBuilder {
        let gp: usize;
        unsafe {
            asm!("mv {0}, gp", out(reg) gp);
        }

        ThreadBuilder {
            priority: 0,
            gp,
            stack_size: 4096,
        }
    }

    pub fn stack_size(mut self, stack_size: usize) -> ThreadBuilder {
        self.stack_size = stack_size;

        self
    }

    pub fn gp(mut self, gp: usize) -> ThreadBuilder {
        self.gp = gp;

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
            syscall_5(
                SYS_SPAWN_TASK,
                trampoline as usize,
                closure as *mut u8 as usize,
                self.priority as usize,
                self.stack_size,
                self.gp,
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

unsafe fn syscall_5(
    code: usize,
    mut arg0: usize,
    mut arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
) -> SysResult<usize> {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            inout("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            in("a4") arg4,
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
    Timeout = 7,
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
        7 => Timeout,
        e => {
            warn!("Unexpected syscall error: {e}");
            Invalid
        }
    };

    Err(err)
}
