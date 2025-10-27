use super::{CPU_FREQ, CycleCounter, NoRa32, fifo::Fifo, sync};
use std::ops::{Index, IndexMut};

mod fir;

/// Offset into the SPU internal ram
type RamIndex = u32;

pub struct Spu {
    /// SPU internal RAM
    ram: Vec<u16>,
    /// Pointer in SPU RAM
    ram_ptr: RamIndex,
    /// The 24 voices
    voices: [Voice; 24],
    /// One bit per voice, set if voice is producing samples
    voice_on: u32,
    /// Main volume left
    volume_left: i16,
    /// Main volume right
    volume_right: i16,
    /// Output buffer containing samples @44.1kHz. The left/right stereo samples are interleaved.
    samples: Vec<i16>,
}

impl Spu {
    pub fn new() -> Spu {
        Spu {
            ram: vec![0; SPU_RAM_SIZE],
            ram_ptr: 0,
            voices: [
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
                Voice::new(),
            ],
            voice_on: 0,
            volume_left: 0,
            volume_right: 0,
            samples: Vec::new(),
        }
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    pub fn clear_samples(&mut self) {
        self.samples.clear();
    }

    pub fn ram_store(&mut self, v: u16) {
        let idx = self.ram_ptr as usize;
        self.ram[idx] = v;

        self.ram_ptr = self.ram_ptr.wrapping_add(1) % (SPU_RAM_SIZE as u32);
    }
}

impl Index<usize> for Spu {
    type Output = Voice;

    fn index(&self, port: usize) -> &Self::Output {
        &self.voices[port as usize]
    }
}

impl IndexMut<usize> for Spu {
    fn index_mut(&mut self, port: usize) -> &mut Self::Output {
        &mut self.voices[port as usize]
    }
}

pub fn run(m: &mut NoRa32) {
    let elapsed = sync::resync(m, SPUSYNC);

    let cycles = elapsed / AUDIO_DIVIDER;
    let rem = elapsed % AUDIO_DIVIDER;

    // If we have some leftover cycles that we can return to the sync module for next time
    sync::rewind(m, SPUSYNC, rem);

    for _ in 0..cycles {
        run_audio_cycle(m);
    }

    // We don't have async events in the SPU since we don't have IRQs at this point.
    sync::next_event(m, SPUSYNC, CPU_FREQ);
}

/// Called at 44.1kHz, must generate two new samples (left/right)
pub fn run_audio_cycle(m: &mut NoRa32) {
    let mut left = 0i32;
    let mut right = 0i32;

    for voice in 0..24 {
        if m.spu.voice_on & (1 << voice) != 0 {
            let [l, r] = run_voice_cycle(m, voice);

            left += l;
            right += r;
        }
    }

    m.spu
        .samples
        .push(left.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
    m.spu
        .samples
        .push(right.clamp(i16::MIN as i32, i16::MAX as i32) as i16);
}

pub fn run_voice_cycle(m: &mut NoRa32, voice: usize) -> [i32; 2] {
    run_voice_decoder(m, voice);

    let v = &mut m.spu[voice];

    let raw_sample = v.next_raw_sample();

    // XXX do ADSR

    let sample = raw_sample;

    let left = ((v.volume_left as i32) * sample) >> 15;
    let right = ((v.volume_right as i32) * sample) >> 15;

    // XXX run envelope

    v.step();

    [left, right]
}

/// ADPCM decoder implementation
pub fn run_voice_decoder(m: &mut NoRa32, voice: usize) {
    let v = &mut m.spu.voices[voice];

    while v.decoder_fifo.len() < 11 {
        if v.cur_index & 7 == 0 {
            // New block
            if v.block_header.end() {
                if v.block_header.is_loop() {
                    v.cur_index = v.loop_index;
                } else {
                    // Disable voice
                    m.spu.voice_on &= !(1 << voice);
                    return;
                }
            }

            let header = m.spu.ram[v.cur_index as usize];
            v.block_header = AdpcmHeader(header);
            if v.block_header.loop_start() {
                v.loop_index = v.cur_index;
            }
            v.inc_index();
        }

        let encoded = m.spu.ram[v.cur_index as usize];
        v.inc_index();
        v.decode(encoded);
    }
}

pub fn store_word(m: &mut NoRa32, addr: u32, val: u32) {
    run(m);

    match addr >> 2 {
        0 => {
            m.spu.volume_left = (val >> 16) as i16;
            m.spu.volume_right = val as i16;
        }
        1 => {
            for voice in 0..24 {
                if val & (1 << voice) != 0 {
                    m.spu[voice].start();
                }
            }

            m.spu.voice_on |= val;
        }
        4 => {
            m.spu.ram_ptr = (val >> 1) & !1;
        }

        5 => {
            m.spu.ram_store(val as u16);
            m.spu.ram_store((val >> 16) as u16);
        }
        0x40.. => {
            let voice = (((addr - 0x100) >> 5) & 0x1f) as usize;
            if voice >= 24 {
                panic!("Unknown voice {voice}");
            }

            let v = &mut m.spu[voice];

            match (addr >> 2) & 7 {
                0 => {
                    v.step_length = (val & 0x3fff) as u16;
                }
                1 => {
                    v.start_index = (val << 3) % SPU_RAM_SIZE as u32;
                }
                2 => {
                    v.volume_left = (val >> 16) as i16;
                    v.volume_right = val as i16;
                }
                n => panic!("Unknown SPU register {voice}.{n}"),
            }
        }
        n => panic!("Unknown SPU register {n:x}"),
    }
}

pub struct Voice {
    /// Voice volume left. Negative volume inverts the phase.
    volume_left: i16,
    /// Voice volume right. Negative volume inverts the phase.
    volume_right: i16,
    /// This value configures how fast the samples are played on this voice, which effectively
    /// changes the frequency of the output audio.
    ///
    /// The value is a 14 bit fixed point integer with 12 fractional bits
    step_length: u16,
    /// Remaining fractional steps carried between cycles, giving up the effective phase of the
    /// voice. 12 fractional bits.
    phase: u16,
    /// Value `cur_index` will take upon voice start
    start_index: RamIndex,
    /// Current index in SPU RAM for this voice
    cur_index: RamIndex,
    /// Target address for `cur_index` when an ADPCM block requests looping
    loop_index: RamIndex,
    /// Header for the current ADPCM block
    block_header: AdpcmHeader,
    /// Last two ADPCM-decoded samples, used to extrapolate the next one
    last_samples: [i16; 2],
    /// FIFO containing the last decoded samples for this voice
    decoder_fifo: Fifo<16, i16>,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            volume_left: 0,
            volume_right: 0,
            step_length: 0,
            phase: 0,
            start_index: 0,
            cur_index: 0,
            loop_index: 0,
            block_header: AdpcmHeader(0),
            last_samples: [0; 2],
            decoder_fifo: Fifo::new(),
        }
    }

    fn start(&mut self) {
        self.cur_index = self.start_index;
        self.phase = 0;
        self.block_header = AdpcmHeader(0);
    }

    fn inc_index(&mut self) {
        self.cur_index = self.cur_index.wrapping_add(1) % (SPU_RAM_SIZE as u32);
    }

    fn step(&mut self) {
        let step = self.phase + self.step_length;

        self.phase = step & 0xfff;

        let consumed = step >> 12;

        self.decoder_fifo.discard(consumed as usize);
    }

    /// Decode 4 samples from an ADPCM block
    fn decode(&mut self, encoded: u16) {
        let (wp, wn) = self.block_header.weights();
        let shift = self.block_header.shift().min(12);

        // Decode the four 4bit samples
        for i in 0..4 {
            // Extract the 4 bits and convert to signed to get proper sign extension when shifting
            let mut sample = (encoded << (12 - i * 4) & 0xf000) as i16;

            sample >>= shift;

            let mut sample = i32::from(sample);

            // Previous sample
            let sample_1 = i32::from(self.last_samples[0]);
            // Antepenultimate sample
            let sample_2 = i32::from(self.last_samples[1]);

            // Extrapolate with sample -1 using the positive weight
            sample += (sample_1 * wp) >> 6;
            // Extrapolate with sample -2 using the negative weight
            sample += (sample_2 * wn) >> 6;

            let sample = sample.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            self.decoder_fifo.push(sample);

            // Shift `last_samples` for the next sample
            self.last_samples[1] = self.last_samples[0];
            self.last_samples[0] = sample;
        }
    }

    /// Returns the next "raw" decoded sample for this voice, meaning the post-ADPCM decode and
    /// resampling but pre-ADSR.
    fn next_raw_sample(&self) -> i32 {
        let phase = (self.phase >> 4) as u8;
        let samples = [
            self.decoder_fifo[0],
            self.decoder_fifo[1],
            self.decoder_fifo[2],
            self.decoder_fifo[3],
        ];

        fir::filter(phase, samples)
    }
}

/// The first two bytes of a 16-byte ADPCM block
#[derive(Copy, Clone)]
struct AdpcmHeader(u16);

impl AdpcmHeader {
    /// If true the current block is the last one of the sample
    fn end(self) -> bool {
        self.0 & (1 << 8) != 0
    }

    /// True if the "loop" bit is set
    fn is_loop(self) -> bool {
        self.0 & (1 << 9) != 0
    }

    /// If true the current block is the target for a subsequent loop_end block.
    fn loop_start(self) -> bool {
        self.0 & (1 << 10) != 0
    }

    /// Returns the pair of positive and negative weights described in the header
    fn weights(self) -> (i32, i32) {
        // Weights taken from No$, Mednafen use the same values.
        let w: [(i32, i32); 16] = [
            (0, 0),
            (60, 0),
            (115, -52),
            (98, -55),
            (122, -60),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
            (0, 0),
        ];

        let off = (self.0 >> 4) & 0xf;

        w[off as usize]
    }

    /// Right shift value to apply to extended encoded samples
    fn shift(self) -> u8 {
        (self.0 & 0xf) as u8
    }
}

/// Divider used to bring CPU_FREQ to 44.1kHz
const AUDIO_DIVIDER: CycleCounter = (CPU_FREQ + 44_100 / 2) / 44_100;

const SPUSYNC: sync::SyncToken = sync::SyncToken::Spu;

/// SPU RAM size in multiple of 16bit words
const SPU_RAM_SIZE: usize = 256 * 1024;
