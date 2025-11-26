use anyhow::Result;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::path::Path;

#[derive(Clone)]
pub struct Score {
    samples: Vec<Vec<u8>>,
    ops: Vec<Op>,
}

impl Score {
    pub fn from_nras_path<P: AsRef<Path>>(nras_path: P) -> Result<Score> {
        let file = File::open(nras_path)?;

        Score::from_nras_reader(file)
    }

    pub fn from_nras_reader<R: Read>(mut nras: R) -> Result<Score> {
        let magic = nras.read_u32::<LittleEndian>()?;
        let mut sram = vec![0u8; SPU_RAM_SIZE_BYTE];

        if magic != 0x5341524e {
            bail!("Invalid NRAS magic");
        }

        let _flags = nras.read_u32::<LittleEndian>()?;

        let mut ops = Vec::with_capacity(1000);

        let mut samples: Vec<Vec<u8>> = Vec::new();

        let mut voice_start = [0u16; 24];

        loop {
            let b = match nras.read_u8() {
                Ok(b) => b,
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                r => r?,
            };

            let voice = || -> Result<u8> {
                let v = b & 0x1f;

                if v >= 24 {
                    bail!("Invalid NRAS voice number {}", v);
                }

                Ok(v)
            };

            debug!("{:x}", b >> 5);

            match b >> 5 {
                // Wait
                0 => {
                    let lo = nras.read_u8()?;

                    let delay_4410hz = u16::from_le_bytes([lo, b]);

                    ops.push(Op::Delay { delay_4410hz });
                }
                // Step
                1 => {
                    let step = nras.read_u16::<LittleEndian>()?;

                    ops.push(Op::Step {
                        voice: voice()?,
                        step,
                    });
                }
                // Volume
                2 => {
                    let l = nras.read_u8()?;
                    let r = nras.read_u8()?;

                    ops.push(Op::Volume {
                        voice: voice()?,
                        l,
                        r,
                    });
                }
                // ADSR
                3 => {
                    let envelope = nras.read_u32::<LittleEndian>()?;

                    ops.push(Op::Adsr {
                        voice: voice()?,
                        envelope,
                    });
                }
                // Sample
                4 => {
                    let voice = voice()?;
                    let baddr = nras.read_u16::<LittleEndian>()?;
                    voice_start[usize::from(voice)] = baddr;

                    let mut addr = usize::from(baddr) << 3;

                    let mut s = Vec::new();

                    loop {
                        for _ in 0..16 {
                            let w = sram[addr];
                            s.push(w);

                            addr = (addr + 1) % SPU_RAM_SIZE_BYTE;
                        }

                        // Check for end marker
                        if s[s.len() - 15] & 1 != 0 {
                            break;
                        }

                        if s.len() >= SPU_RAM_SIZE_BYTE {
                            break;
                        }
                    }

                    let index = match samples.iter().position(|os| *os == s) {
                        Some(p) => p,
                        None => {
                            samples.push(s);

                            samples.len() - 1
                        }
                    };

                    debug!("NRAS [{}] SAMPLE {} {}", voice, index, baddr);
                    ops.push(Op::Sample { voice, index });
                }
                // Loop
                5 => {
                    let voice = voice()?;
                    let loopaddr = nras.read_u16::<LittleEndian>()?;
                    let offset = loopaddr.wrapping_sub(voice_start[usize::from(voice)]);

                    ops.push(Op::Loop { voice, offset });
                }
                7 => match b & 0x1f {
                    // Release
                    0 => {
                        let b0 = nras.read_u8()?;
                        let b1 = nras.read_u8()?;
                        let b2 = nras.read_u8()?;

                        let mask = u32::from_le_bytes([b0, b1, b2, 0]);
                        ops.push(Op::Release { mask });
                    }
                    // Trigger
                    1 => {
                        let b0 = nras.read_u8()?;
                        let b1 = nras.read_u8()?;
                        let b2 = nras.read_u8()?;

                        let mask = u32::from_le_bytes([b0, b1, b2, 0]);
                        ops.push(Op::Trigger { mask });
                    }
                    // SPU RAM load
                    2 => {
                        // start and len are in 16B blocks
                        let bstart = nras.read_u16::<LittleEndian>()?;
                        let blen = nras.read_u16::<LittleEndian>()?;

                        let mut p = usize::from(bstart) << 3;
                        let len = (usize::from(blen) + 1) << 3;

                        debug!("NRAS RAM load {}B @0x{:x}", len, p);

                        for _ in 0..len {
                            sram[p % (SPU_RAM_SIZE_BYTE)] = nras.read_u8()?;
                            p = p.wrapping_add(1);
                        }
                    }
                    sub => bail!("Unhandled NRAS subcode {}", sub),
                },
                op => bail!("Unhandled NRAS opcode {}", op),
            }
        }

        info!("{:#?}", ops);

        Ok(Score { samples, ops })
    }

    pub fn dump_nras<W: Write>(&self, w: &mut W, offset: usize) -> Result<()> {
        // Samples are always 8-byte aligned due to addressing constraints
        fn sram_align(len: usize) -> usize {
            (len + 7) & (!7)
        }

        let offset = sram_align(offset);

        // Dump all samples at the start
        let total_sample_len: usize = self.samples.iter().map(|s| sram_align(s.len())).sum();

        info!(
            "Score uses {total_sample_len}B ({}%) of SPU RAM ({} samples total)",
            ((total_sample_len + 50) * 100) / SPU_RAM_SIZE_BYTE,
            self.samples.len()
        );

        if total_sample_len + offset > SPU_RAM_SIZE_BYTE {
            bail!("Not enough memory to store score at offset {offset}");
        }

        let mut sstart = Vec::with_capacity(self.samples.len());

        let mut p = offset;
        for s in self.samples.iter() {
            sstart.push((p >> 3) as u16);
            p += sram_align(s.len());
        }

        w.write_all(b"NRAS")?;
        w.write_all(&[0u8; 4])?;

        if total_sample_len >= 8 {
            // Dump all samples at once
            w.write_u8(0xe2)?;
            w.write_u16::<LittleEndian>((offset >> 3) as u16)?;
            w.write_u16::<LittleEndian>(((total_sample_len >> 3) - 1) as u16)?;

            for s in self.samples.iter() {
                w.write_all(s)?;

                let align = s.len() & 7;

                if align > 0 {
                    for _ in align..8 {
                        w.write_u8(0)?;
                    }
                }
            }
        }

        let mut voice_start = [0u16; 24];

        for op in self.ops.iter() {
            match *op {
                Op::Delay { delay_4410hz } => w.write_u16::<BigEndian>(delay_4410hz)?,
                Op::Step { voice, step } => {
                    w.write_u8((1u8 << 5) | voice)?;
                    w.write_u16::<LittleEndian>(step)?;
                }
                Op::Volume { voice, l, r } => {
                    let b = (2u8 << 5) | voice;

                    w.write_all(&[b, l, r])?;
                }
                Op::Adsr { voice, envelope } => {
                    w.write_u8((3u8 << 5) | voice)?;
                    w.write_u32::<LittleEndian>(envelope)?;
                }
                Op::Sample { voice, index } => {
                    w.write_u8((4u8 << 5) | voice)?;
                    let s = sstart[index as usize];
                    voice_start[usize::from(voice)] = s;
                    w.write_u16::<LittleEndian>(s)?;
                }
                Op::Loop { voice, offset } => {
                    w.write_u8((5u8 << 5) | voice)?;
                    let l = voice_start[voice as usize].wrapping_add(offset);
                    w.write_u16::<LittleEndian>(l)?;
                }
                Op::Release { mask } => {
                    w.write_u8(7u8 << 5)?;
                    w.write_u32::<LittleEndian>(mask)?;
                }
                Op::Trigger { mask } => {
                    w.write_u8((7u8 << 5) | 1)?;
                    w.write_u32::<LittleEndian>(mask)?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
enum Op {
    Delay { delay_4410hz: u16 },
    Step { voice: u8, step: u16 },
    Volume { voice: u8, l: u8, r: u8 },
    Adsr { voice: u8, envelope: u32 },
    Sample { voice: u8, index: usize },
    Loop { voice: u8, offset: u16 },
    Release { mask: u32 },
    Trigger { mask: u32 },
}

/// SPU RAM size
const SPU_RAM_SIZE_BYTE: usize = 512 * 1024;
