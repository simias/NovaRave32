//! RISC-V system timer, running at 48kHz
use crate::{cpu, sync, CycleCounter, NoRa32, CPU_FREQ};

pub struct Timer {
    /// Counter incrementing every MTIME tick. Considered to never overflow.
    mtime: u64,
    /// Trigger IRQ if MTIME >= MTIMECMP
    mtimecmp: u64,
}

impl Timer {
    pub fn new() -> Timer {
        Timer {
            mtime: 0,
            mtimecmp: !0,
        }
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, TIMERSYNC);

    let ticks = elapsed / MTIME_CPU_CLK_DIV;
    let rem = elapsed % MTIME_CPU_CLK_DIV;

    m.systimer.mtime += ticks as u64;

    // If we have some leftover cycles that we can return to the sync module for next time
    sync::rewind(m, TIMERSYNC, rem);

    check_for_irq(m);
}

fn check_for_irq(m: &mut NoRa32) {
    let irq_active = m.systimer.mtime >= m.systimer.mtimecmp;

    cpu::set_mtip(m, irq_active);

    if irq_active {
        sync::no_next_event(m, TIMERSYNC);
    } else {
        let to_irq = m.systimer.mtimecmp - m.systimer.mtime;

        // Force a resync when the IRQ will occur or in one second, whichever comes first
        let to_irq = to_irq.min(MTIME_HZ as u64);
        let to_irq = (to_irq as CycleCounter) * MTIME_CPU_CLK_DIV;

        sync::next_event(m, TIMERSYNC, to_irq);
    }
}

pub fn load_word(m: &mut NoRa32, off: u32) -> u32 {
    run(m);

    let t = &m.systimer;

    match off {
        0x0 => t.mtime as u32,
        0x4 => (t.mtime >> 32) as u32,
        0x8 => t.mtimecmp as u32,
        0xc => (t.mtimecmp >> 32) as u32,
        _ => !0,
    }
}

pub fn store_word(m: &mut NoRa32, off: u32, v: u32) {
    run(m);

    let t = &mut m.systimer;

    match off {
        0x0 => {
            t.mtime &= !0xffff_ffffu64;
            t.mtime |= u64::from(v);
        }
        0x4 => {
            t.mtime &= !(0xffff_ffffu64 << 32);
            t.mtime |= u64::from(v) << 32;
        }
        0x8 => {
            t.mtimecmp &= !0xffff_ffffu64;
            t.mtimecmp |= u64::from(v);
        }
        0xc => {
            t.mtimecmp &= !(0xffff_ffffu64 << 32);
            t.mtimecmp |= u64::from(v) << 32;
        }
        _ => (),
    }

    check_for_irq(m);
}

const TIMERSYNC: sync::SyncToken = sync::SyncToken::SysTimer;

/// Clock divider used for MTIME increments
pub const MTIME_CPU_CLK_DIV: CycleCounter = 32;

/// Number of MTIME ticks per second
///
/// With a CPU_FREQ at 44.1kHz * 512 and an MTIME_CPU_CLK_DIV at 32 this will be slightly above
/// 700kHz giving us ~10µs precision.
pub const MTIME_HZ: CycleCounter = (CPU_FREQ + MTIME_CPU_CLK_DIV / 2) / MTIME_CPU_CLK_DIV;
