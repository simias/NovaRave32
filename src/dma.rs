use crate::fifo::Fifo;
use crate::irq;
use crate::{CPU_FREQ, CycleCounter, NoRa32, cpu, gpu, sync};
use nr32_common::memmap::{RAM, ROM};
use nr32_common::syscall::{DmaAddr, DmaTarget};

pub struct Dma {
    /// Where the DMA is reading from
    src: DmaAddr,
    /// Where the DMA is writing to
    dst: DmaAddr,
    /// How many words left to copy. Note that this counter is ahead of the actual words written
    /// because of the internal buffer.
    rem_words: u32,
    /// Copy buffer
    buf: Fifo<32, u32>,
}

impl Dma {
    pub fn new() -> Dma {
        Dma {
            src: DmaAddr(0),
            dst: DmaAddr(0),
            rem_words: 0,
            buf: Fifo::new(),
        }
    }

    pub fn dst(&self) -> DmaAddr {
        self.dst
    }

    pub fn rem_words(&self) -> u32 {
        self.rem_words
    }

    pub fn is_running(&self) -> bool {
        self.rem_words > 0 || !self.buf.is_empty()
    }
}

fn sync_for_dma(m: &mut NoRa32, target: DmaTarget) {
    match target {
        DmaTarget::Memory => (),
        DmaTarget::Gpu => gpu::run(m),
    }
}

fn dma_refill_buf(m: &mut NoRa32, cycles: &mut CycleCounter) {
    let src_target = m.dma.src.target().unwrap();

    while m.dma.rem_words > 0 && !m.dma.buf.is_full() {
        match src_target {
            DmaTarget::Memory => {
                if let Some(off) = RAM.contains(m.dma.src.raw()) {
                    if *cycles >= 1 {
                        *cycles -= 1;
                        let v = m.ram[(off >> 2) as usize];
                        m.dma.buf.push(v);
                        m.dma.rem_words -= 1;
                        m.dma.src.0 = m.dma.src.0.wrapping_add(4);
                    } else {
                        return;
                    }
                } else if let Some(off) = ROM.contains(m.dma.src.raw()) {
                    if *cycles >= 20 {
                        *cycles -= 20;
                        let v = m.rom[(off >> 2) as usize];
                        m.dma.buf.push(v);
                        m.dma.rem_words -= 1;
                        m.dma.src.0 = m.dma.src.0.wrapping_add(4);
                    } else {
                        return;
                    }
                } else {
                    todo!()
                }
            }
            _ => todo!(),
        }
    }
}

fn run_dma_cycles(m: &mut NoRa32, mut cycles: CycleCounter) -> CycleCounter {
    let src_target = m.dma.src.target().unwrap();
    let dst_target = m.dma.dst.target().unwrap();

    sync_for_dma(m, src_target);
    sync_for_dma(m, dst_target);

    loop {
        dma_refill_buf(m, &mut cycles);

        if m.dma.buf.is_empty() {
            // We're stalling on input (or we're done).
            return match src_target {
                // Arbitrary value that should be short enough to avoid introducing too much
                // latency but long enough to avoid a big performance impact.
                DmaTarget::Memory => {
                    sync::rewind(m, DMASYNC, cycles);
                    128
                }
                _ => todo!(),
            };
        }

        while let Some(v) = m.dma.buf.front() {
            let res = match dst_target {
                DmaTarget::Memory => {
                    if let Some(off) = RAM.contains(m.dma.dst.raw()) {
                        m.ram[(off >> 2) as usize] = v;
                        m.dma.dst.0 = m.dma.dst.0.wrapping_add(4);
                        DmaResult::Ok
                    } else {
                        todo!()
                    }
                }
                DmaTarget::Gpu => gpu::dma_store(m, v),
            };

            match res {
                DmaResult::Ok => {
                    m.dma.buf.pop();
                }
                DmaResult::Stall(duration) => {
                    // We're stalling on output
                    assert!(duration > 0);
                    // Make sure we don't waste cycles if we could read more
                    dma_refill_buf(m, &mut cycles);

                    return duration;
                }
            }
        }
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, DMASYNC);

    sync::next_event(m, DMASYNC, CPU_FREQ);

    let was_running = m.dma.is_running();

    let next_sync = if was_running {
        run_dma_cycles(m, elapsed)
    } else {
        CPU_FREQ
    };

    let is_running = m.dma.is_running();

    if was_running && !is_running {
        // End of transfer
        irq::trigger(m, irq::Interrupt::DmaDone);
    }

    let next_sync = if is_running {
        next_sync
    } else {
        // Idle
        CPU_FREQ
    };

    sync::next_event(m, DMASYNC, next_sync);
}

pub fn store_word(m: &mut NoRa32, addr: u32, val: u32) {
    run(m);

    match addr >> 2 {
        // SRC
        0 => m.dma.src = DmaAddr(val),
        // DST
        1 => m.dma.dst = DmaAddr(val),
        // LEN
        2 => {
            m.dma.rem_words = val;
            m.dma.buf.clear();

            let n_e = if val == 0 {
                // Idle
                CPU_FREQ
            } else {
                // Start
                cpu::check_dma_reservation(m);
                32
            };

            sync::next_event(m, DMASYNC, n_e);
        }
        n => panic!("Unknown DMA register {n:x}"),
    }
}

pub enum DmaResult {
    Ok,
    Stall(CycleCounter),
}

const DMASYNC: sync::SyncToken = sync::SyncToken::Dma;
