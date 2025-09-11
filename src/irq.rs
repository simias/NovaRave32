//! A very simple interrupt controller

use crate::{NoRa32, cpu};

/// All interrupts supported by the system (minus the MTI interrupt that's directly handled by the
/// CPU
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Interrupt {
    /// Triggered by the GPU every time a frame has completed
    VSync = 0,
    /// Triggered when the input device interface's IRQ line has a rising edge
    InputDev = 1,
    /// Triggered when a DMA transfer is complete
    DmaDone = 2,
}

pub struct Controller {
    pending: u32,
    enabled: u32,
}

impl Controller {
    pub fn new() -> Controller {
        Controller {
            pending: 0,
            enabled: 0,
        }
    }
}

fn refresh_cpu_irq(m: &mut NoRa32) {
    cpu::set_meip(m, (m.irq.pending & m.irq.enabled) != 0);
}

/// Trigger the given `irq`. All interrupts are edge-driven, so this should only be called when the
/// device's IRQ line goes from 0 to 1.
pub fn trigger(m: &mut NoRa32, irq: Interrupt) {
    m.irq.pending |= 1 << (irq as u32);
    refresh_cpu_irq(m);
}

pub fn store_word(m: &mut NoRa32, off: u32, v: u32) {
    match off {
        // Acknowledge
        0x0 => m.irq.pending &= !v,
        // Enable
        0x4 => m.irq.enabled = v,
        _ => (),
    }

    refresh_cpu_irq(m);
}

pub fn load_word(m: &mut NoRa32, off: u32) -> u32 {
    match off {
        0x0 => m.irq.pending,
        // Enable
        0x4 => m.irq.enabled,
        _ => !0,
    }
}
