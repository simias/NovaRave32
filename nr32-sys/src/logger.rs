use crate::syscall;
use alloc::format;
use log::{Log, Metadata, Record};

// Implement a global logger
pub struct SysLogger;

impl Log for SysLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // Log everything
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let s = format!("[{}] {}", record.level(), record.args());

            syscall::dbg_puts(&s);
        }
    }

    fn flush(&self) {}
}

pub static LOGGER: SysLogger = SysLogger;
