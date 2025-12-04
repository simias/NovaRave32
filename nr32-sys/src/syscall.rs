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

unsafe fn syscall_0(code: u32) -> SysResult<usize> {
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

unsafe fn syscall_1(code: u32, mut arg0: usize) -> SysResult<usize> {
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

unsafe fn syscall_2(code: u32, mut arg0: usize, mut arg1: usize) -> SysResult<usize> {
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

pub(crate) unsafe fn syscall_3(
    code: u32,
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
    code: u32,
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

pub(crate) unsafe fn syscall_6(
    code: u32,
    mut arg0: usize,
    mut arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> SysResult<usize> {
    unsafe {
        asm!("ecall",
            in("a7") code,
            inout("a0") arg0,
            inout("a1") arg1,
            in("a2") arg2,
            in("a3") arg3,
            in("a4") arg4,
            in("a5") arg5,
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
    /// No such file or directory
    NoEnt = 8,
}

pub type SysResult<T> = Result<T, SysError>;

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
