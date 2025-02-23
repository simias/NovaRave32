#[macro_use]
extern crate static_assertions;
extern crate console_error_panic_hook;
#[macro_use]
extern crate log;

mod cpu;
mod gpu;
mod sync;
mod systimer;

use cfg_if::cfg_if;
use std::panic;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn main() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    init_log();
}

#[wasm_bindgen]
extern "C" {
    /// Function used to draw 3D primitives
    fn drawTriangles3D(f32_ptr: *const f32, u8_ptr: *const u8, count: usize);
}

#[wasm_bindgen]
pub struct NoRa32 {
    cpu: cpu::Cpu,
    sync: sync::Synchronizer,
    rom: Box<[u32; (ROM.len >> 2) as _]>,
    ram: Box<[u32; (RAM.len >> 2) as _]>,
    gpu: gpu::Gpu,
    systimer: systimer::Timer,
    /// Buffer containing messages written to the debug console before they're flushed to stdout
    dbg_out: Vec<u8>,
    /// Sets to false if the emulator should shutdown
    run: bool,
    /// Incremented by the CPU as it runs
    cycle_counter: CycleCounter,
}

#[wasm_bindgen]
impl NoRa32 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> NoRa32 {
        NoRa32 {
            cpu: cpu::Cpu::new(),
            sync: sync::Synchronizer::new(),
            rom: Box::new([0; (ROM.len >> 2) as usize]),
            ram: Box::new([0; (RAM.len >> 2) as usize]),
            gpu: gpu::Gpu::new(),
            systimer: systimer::Timer::new(),
            dbg_out: Vec::new(),
            run: true,
            cycle_counter: 0,
        }
    }

    #[wasm_bindgen]
    pub fn load_rom(&mut self, rom: &[u8]) {
        if (rom.len() >> 2) >= self.rom.len() {
            error!(
                "Loaded ROM is too large: {}B (max {}B)",
                rom.len(),
                self.rom.len() << 2
            );
            return;
        }

        for (off, &b) in rom.iter().enumerate() {
            let rpos = off >> 2;
            let roff = off & 3;

            self.rom[rpos] |= u32::from(b) << (roff * 8);
        }

        info!("Loaded {}B to ROM", rom.len());
    }

    #[wasm_bindgen]
    pub fn run_frame(&mut self) {
        let frame_budget = self.cycle_counter + (CPU_FREQ / 30);

        while self.run && self.cycle_counter < frame_budget {
            if self.cpu.wfi() {
                sync::fast_forward_to_next_event(self);
            } else {
                while !sync::is_event_pending(self) {
                    cpu::step(self);
                }
            }
            sync::handle_events(self);
        }

        sync::rebase_counters(self);
    }

    fn tick(&mut self, cycles: CycleCounter) {
        self.cycle_counter += cycles;
    }

    /// Store word `v` at `addr`. `addr` is assumed to be correctly aligned
    fn store_word(&mut self, addr: u32, v: u32) {
        debug_assert!(addr & 3 == 0);

        if let Some(off) = RAM.contains(addr) {
            self.ram[(off >> 2) as usize] = v;
            return;
        }

        if let Some(off) = GPU.contains(addr) {
            gpu::store_word(self, off, v);
            return;
        }

        if let Some(off) = SYS_TIMER.contains(addr) {
            return systimer::store_word(self, off, v);
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

        self.tick(1);

        if let Some(off) = RAM.contains(addr) {
            return self.ram[(off >> 2) as usize];
        }

        if let Some(off) = ROM.contains(addr) {
            return self.rom[(off >> 2) as usize];
        }

        if let Some(off) = SYS_TIMER.contains(addr) {
            return systimer::load_word(self, off);
        }

        if let Some(off) = GPU.contains(addr) {
            return gpu::load_word(self, off);
        }

        panic!("Can't load from {:x} {:?}", addr, self.cpu);
    }

    /// Load bite from `addr`. `addr` is assumed to be correctly aligned.
    fn load_byte(&mut self, addr: u32) -> u8 {
        self.tick(1);

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

        info!("SYS {}", String::from_utf8_lossy(&self.dbg_out));

        self.dbg_out.clear();
    }
}

impl Default for NoRa32 {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Range {
    base: u32,
    len: u32,
}

impl Range {
    /// Return `Some(offset)` if addr is contained in `self`
    pub fn contains(self, addr: u32) -> Option<u32> {
        if addr >= self.base && addr <= self.base + (self.len - 1) {
            Some(addr - self.base)
        } else {
            None
        }
    }
}

cfg_if! {
    if #[cfg(feature = "console_log")] {
        fn init_log() {
            use log::Level;
            console_log::init_with_level(Level::Trace).expect("error initializing log");
        }
    } else {
        fn init_log() {}
    }
}

const DEBUG: Range = Range {
    base: 0x1000_0000,
    len: 1024,
};

const GPU: Range = Range {
    base: 0x1001_0000,
    len: 1024,
};

const ROM: Range = Range {
    base: 0x2000_0000,
    len: 2 * 1024 * 1024,
};

const RAM: Range = Range {
    base: 0x4000_0000,
    len: 2 * 1024 * 1024,
};

const SYS_TIMER: Range = Range {
    base: 0xffff_fff0,
    len: 16,
};

type CycleCounter = i32;

/// The CPU runs at precisely 24.576MHz.
///
/// The frequency is chosen to be a multiple of the audio frequency (48kHz).
const CPU_FREQ: CycleCounter = 48_000 * 512;
