use crate::error::{SysError, SysResult};

/// Suspend task for [a1:a0] MTIME ticks
///
/// Always returns SysError::Timeout
pub const SYS_SLEEP: u32 = 0x01;

/// Put task to sleep until VSYNC
pub const SYS_WAIT_FOR_VSYNC: u32 = 0x02;

/// Spawn a thread
///
/// - a0: thread entry point
/// - a1: thread data
/// - a2: priority
/// - a3: stack pointer
/// - a4: global pointer
/// - a5: 4 byte ASCII identifier (little-endian)
pub const SYS_SPAWN_TASK: u32 = 0x03;

/// Kills the current thread
pub const SYS_EXIT: u32 = 0x04;

/// Allocate memory
///
/// - a0: size to allocate
/// - a1: alignment (must be power of 2)
pub const SYS_ALLOC: u32 = 0x05;

/// Free memory
///
/// - a0: pointer to free
/// - a1: block size
/// - a2: alignment (must be power of 2)
pub const SYS_FREE: u32 = 0x06;

/// Input port data exchange. Suspends task until transfer has completed.
///
/// - a0: port to select
/// - a1: pointer to the read/write buffer containing the data to be sent and filled with the reply
/// - a2: how many bytes to read/write (max 16)
pub const SYS_INPUT_DEV: u32 = 0x07;

/// Send a string to the debug console. The string is assumed to be UTF-8, other formats may be
/// mangled (but won't crash)
///
/// - a0: pointer to the start of the string
/// - a1: length of the string in bytes (NOT unicode characters)
pub const SYS_DBG_PUTS: u32 = 0x08;

/// Shutdown the emulator
///
/// - a0: exit code (truncated to 16bits)
pub const SYS_SHUTDOWN: u32 = 0x09;

/// Futex wait
///
/// - a0: address of an AtomicUsize
/// - a1: expected value of the AtomicIsize in a0 (if the values differ, the function returns).
/// - [a3:a2]: wait timeout in MTIME ticks (0 for infinite)
///
/// If the values differ, the call returns immediately with EAGAIN
///
/// The function can return spuriously for any reason
pub const SYS_FUTEX_WAIT: u32 = 0x0a;

/// Futex wake
///
/// - a0: address of an AtomicUsize
/// - a1: number of waiting threads to wake up
///
/// Returns the number of threads successfully awoken
pub const SYS_FUTEX_WAKE: u32 = 0x0b;

/// DMA transfer
///
/// - a0: source address
/// - a1: target address
/// - a2: length in words
pub const SYS_DO_DMA: u32 = 0x0c;

/// Representation of a DMA source/dest address
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DmaAddr(pub u32);

impl DmaAddr {
    /// The DMA reads from or writes to the GPU
    pub const GPU: DmaAddr = DmaAddr(2);

    pub fn target(self) -> SysResult<DmaTarget> {
        match self.0 & 3 {
            0 => Ok(DmaTarget::Memory),
            2 => {
                // Device
                match (self.0 >> 4) & 0xff {
                    0 => Ok(DmaTarget::Gpu),
                    _ => Err(SysError::Invalid),
                }
            }
            _ => Err(SysError::Invalid),
        }
    }

    /// Try to build a DmaAddr from a system bus address. Returns an error if the address is not
    /// word-aligned
    pub fn from_memory(addr: usize) -> SysResult<DmaAddr> {
        let a = DmaAddr(addr as u32);

        if a.target()? == DmaTarget::Memory {
            Ok(a)
        } else {
            Err(SysError::Invalid)
        }
    }

    pub fn src_from_raw(v: u32) -> SysResult<DmaAddr> {
        let a = DmaAddr(v);

        match a.target()? {
            DmaTarget::Memory => (),
            _ => return Err(SysError::Invalid),
        }

        Ok(a)
    }

    pub fn dst_from_raw(v: u32) -> SysResult<DmaAddr> {
        let a = DmaAddr(v);

        match a.target()? {
            DmaTarget::Memory => (),
            DmaTarget::Gpu => (),
        }

        Ok(a)
    }

    pub fn raw(&self) -> u32 {
        self.0
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DmaTarget {
    /// System bus (incrementing addresses, RAM/ROM)
    Memory,
    /// GPU port
    Gpu
}
