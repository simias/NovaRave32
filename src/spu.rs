use super::{sync, CycleCounter, NoRa32, CPU_FREQ};

pub struct Spu {
    /// Output buffer containing samples @44.1kHz. The left/right stereo samples are interleaved.
    samples: Vec<i16>,
    v: i16,
}

impl Spu {
    pub fn new() -> Spu {
        Spu {
            samples: Vec::new(),
            v: 0,
        }
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, SPUSYNC);

    let cycles = elapsed / AUDIO_DIVIDER;
    let rem = elapsed % AUDIO_DIVIDER;

    for _ in 0..cycles {
        m.spu.samples.push(m.spu.v);
        m.spu.samples.push(m.spu.v);
        m.spu.v = m.spu.v.wrapping_add(64);
    }

    // If we have some leftover cycles that we can return to the sync module for next time
    sync::rewind(m, SPUSYNC, rem);

    // We don't really have any deadline, so just schedule a refresh
    sync::next_event(m, SPUSYNC, CPU_FREQ);
}

/// Divider used to bring CPU_FREQ to 44.1kHz
const AUDIO_DIVIDER: CycleCounter = (CPU_FREQ + 44_100 / 2) / 44_100;

const SPUSYNC: sync::SyncToken = sync::SyncToken::Spu;
