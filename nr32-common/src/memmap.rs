//! NR32 memory map

pub struct Range {
    pub base: u32,
    pub len: u32,
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

pub const RAM: Range = Range {
    base: 0x0000_0000,
    len: 2 * 1024 * 1024,
};

pub const ROM: Range = Range {
    base: 0x2000_0000,
    len: 64 * 1024 * 1024,
};

pub const DEBUG: Range = Range {
    base: 0x4000_0000,
    len: 1024,
};

pub const GPU: Range = Range {
    base: 0x4001_0000,
    len: 1024,
};

pub const SPU: Range = Range {
    base: 0x4002_0000,
    len: 1024,
};

pub const INPUT_DEV: Range = Range {
    base: 0x4003_0000,
    len: 1024,
};

pub const DMA: Range = Range {
    base: 0x4004_0000,
    len: 1024,
};

pub const SYS_TIMER: Range = Range {
    base: 0xffff_ffe0,
    len: 16,
};

pub const IRQ_CONTROLLER: Range = Range {
    base: 0xffff_fff0,
    len: 8,
};

