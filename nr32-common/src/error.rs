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

pub type SysResult<T> = Result<T, SysError>;
