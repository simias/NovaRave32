/// Suspend task for [a1:a0] MTIME ticks
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
/// - a0: return value
pub const SYS_SHUTDOWN: usize = 0x09;

/// Futex wait
///
/// - a0: address of an AtomicIsize
/// - a1: expected value of the AtomicIsize in a0 (if the values differ, the function returns).
/// - [a2:a3]: wait timeout in MTIME ticks (0 for infinite)
///
/// If the values differ, the call returns immediately with EAGAIN
///
/// The function can return spuriously for any reason
pub const SYS_FUTEX_WAIT: usize = 0x0a;

/// Futex wake
///
/// - a0: address of an AtomicIsize
/// - a1: number of waiting threads to wake up
pub const SYS_FUTEX_WAKE: usize = 0x0b;
