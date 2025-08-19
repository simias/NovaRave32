use core::fmt::{self, Write};
use log::{Log, Metadata, Record};

pub struct DebugConsole;

impl DebugConsole {
    pub fn putchar(c: u8) {
        unsafe {
            DEBUG_OUT.write_volatile(c);
        }
    }
}

// Implement `Write` trait for DebugConsole
impl Write for DebugConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            Self::putchar(c);
        }
        Ok(())
    }
}

// Implement a global logger
pub struct ConsoleLogger;

impl Log for ConsoleLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // Log everything
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let _ = writeln!(DebugConsole, "[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

pub static LOGGER: ConsoleLogger = ConsoleLogger;

const DEBUG_OUT: *mut u8 = 0x4000_0010 as *mut u8;
