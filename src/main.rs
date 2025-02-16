#[macro_use]
extern crate static_assertions;

#[macro_use]
extern crate log;

mod cpu;

use env_logger::Env;
use std::env;
use std::fs;
use std::process;

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <binary file>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    info!("Loading ROM from {}", filename);

    let mut bin = match fs::read(filename) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("Failed to read {}: {}", filename, err);
            process::exit(1);
        }
    };

    if bin.len() > ROM.len as usize {
        warn!("ROM file is too large! Truncating");
    }

    bin.resize(ROM.len as usize, 0);

    let mut machine = Machine {
        cpu: cpu::Cpu::new(),
        rom: Box::new([0; (ROM.len >> 2) as usize]),
        ram: Box::new([0; (RAM.len >> 2) as usize]),
        dbg_out: Vec::new(),
        run: true,
    };

    // Copy ROM
    for (off, &b) in bin.iter().enumerate() {
        let rpos = off >> 2;
        let roff = off & 3;

        machine.rom[rpos] |= u32::from(b) << (roff * 8);
    }

    while machine.run {
        cpu::step(&mut machine);
    }
}

struct Machine {
    cpu: cpu::Cpu,
    rom: Box<[u32; (ROM.len >> 2) as _]>,
    ram: Box<[u32; (RAM.len >> 2) as _]>,
    /// Buffer containing messages written to the debug console before they're flushed to stdout
    dbg_out: Vec<u8>,
    run: bool,
}

impl Machine {
    /// Store word `v` at `addr`. `addr` is assumed to be correctly aligned
    fn store_word(&mut self, addr: u32, v: u32) {
        debug_assert!(addr & 3 == 0);

        if let Some(off) = RAM.contains(addr) {
            self.ram[(off >> 2) as usize] = v;
            return;
        }

        if let Some(off) = DEBUG.contains(addr) {
            if off == 0x20 {
                // Shutdown
                if v >> 16 == 0xd1e {
                    info!("Shutdown requested with code {}", v & 0xffff);
                    self.run = false;
                }
            }
            return;
        }

        panic!("Can't sw at {:x} {:?}", addr, self.cpu);
    }

    /// Store halfword `v` at `addr`.
    fn store_halfword(&mut self, addr: u32, v: u16) {
        debug_assert!(addr & 1 == 0);

        if let Some(off) = RAM.contains(addr) {
            let wo = (off >> 2) as usize;
            let bitpos = (off & 3) << 3;

            let mut word = self.ram[wo];
            word &= !(0xffff << bitpos);
            word |= u32::from(v) << bitpos;
            self.ram[wo] = word;
            return;
        }

        panic!("Can't sb at {:x} {:?}", addr, self.cpu);
    }

    /// Store byte `v` at `addr`.
    fn store_byte(&mut self, addr: u32, v: u8) {
        if let Some(off) = RAM.contains(addr) {
            let wo = (off >> 2) as usize;
            let bitpos = (off & 3) << 3;

            let mut word = self.ram[wo];
            word &= !(0xff << bitpos);
            word |= u32::from(v) << bitpos;
            self.ram[wo] = word;
            return;
        }

        if let Some(off) = DEBUG.contains(addr) {
            if off == 0x10 {
                // Debug console
                if v == b'\n' {
                    self.flush_debug_console();
                } else {
                    self.dbg_out.push(v);

                    if self.dbg_out.len() > 1024 {
                        self.flush_debug_console();
                    }
                }
            }
            return;
        }

        panic!("Can't sb at {:x} {:?}", addr, self.cpu);
    }

    /// Load 32bit value from `addr`. `addr` is assumed to be correctly aligned.
    fn load_word(&mut self, addr: u32) -> u32 {
        debug_assert!(addr & 3 == 0);

        if let Some(off) = RAM.contains(addr) {
            return self.ram[(off >> 2) as usize];
        }

        if let Some(off) = ROM.contains(addr) {
            return self.rom[(off >> 2) as usize];
        }

        panic!("Can't load from {:x} {:?}", addr, self.cpu);
    }

    /// Load bite from `addr`. `addr` is assumed to be correctly aligned.
    fn load_byte(&mut self, addr: u32) -> u8 {
        if let Some(off) = RAM.contains(addr) {
            let word = self.ram[(off >> 2) as usize];
            return (word >> ((off & 3) << 3)) as u8;
        }

        if let Some(off) = ROM.contains(addr) {
            let word = self.rom[(off >> 2) as usize];
            return (word >> ((off & 3) << 3)) as u8;
        }

        panic!("Can't load from {:x} {:?}", addr, self.cpu);
    }

    // Print any message in the debug console to stdout and reset the buffer
    fn flush_debug_console(&mut self) {
        if self.dbg_out.is_empty() {
            return;
        }

        info!("DBG: {}", String::from_utf8_lossy(&self.dbg_out));

        self.dbg_out.clear();
    }
}

pub struct Range {
    base: u32,
    len: u32,
}

impl Range {
    /// Return `Some(offset)` if addr is contained in `self`
    pub fn contains(self, addr: u32) -> Option<u32> {
        if addr >= self.base && addr < self.base + self.len {
            Some(addr - self.base)
        } else {
            None
        }
    }
}

const ROM: Range = Range {
    base: 0x2000_0000,
    len: 2 * 1024 * 1024,
};

const RAM: Range = Range {
    base: 0x4000_0000,
    len: 2 * 1024 * 1024,
};

const DEBUG: Range = Range {
    base: 0x1000_0000,
    len: 1024,
};
