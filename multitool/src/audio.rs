use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use symphonia::core::audio::AudioBuffer as SAudioBuffer;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::io::MediaSourceStream;
use symphonia::default::get_probe;

pub struct AudioBuffer {
    sample_rate: u32,
    samples: Vec<i16>,
}

impl AudioBuffer {
    pub fn from_path<P: AsRef<Path>>(audio_path: P, channel: Option<usize>) -> Result<AudioBuffer> {
        let file = File::open(audio_path)?;

        AudioBuffer::from_file(file, channel)
    }

    pub fn from_file(audio_file: File, channel: Option<usize>) -> Result<AudioBuffer> {
        let mss = MediaSourceStream::new(Box::new(audio_file), Default::default());

        let probed = get_probe().format(
            &Default::default(),
            mss,
            &Default::default(),
            &Default::default(),
        )?;

        let mut format_reader = probed.format;

        // Use the first valid audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("No valid audio track found"))?;

        let num_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

        debug!("Track has {} audio channels", num_channels);

        if let Some(c) = channel {
            if c >= num_channels {
                bail!("Track does not have an audio channel {}", c);
            }
        }

        // Get the sample rate from the track metadata
        let sample_rate = track
            .codec_params
            .sample_rate
            .ok_or_else(|| anyhow!("No sample rate found"))?;
        debug!("Track sample rate: {}", sample_rate);

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())?;

        let mut samples = Vec::new();

        let track_id = track.id;

        while let Ok(packet) = format_reader.next_packet() {
            if packet.track_id() != track_id {
                continue;
            }

            let raw_abuf = decoder.decode(&packet)?;

            let mut abuf: SAudioBuffer<i16> = raw_abuf.make_equivalent();

            raw_abuf.convert(&mut abuf);

            match channel {
                Some(c) => samples.extend_from_slice(abuf.chan(c)),
                None => {
                    if num_channels == 1 {
                        samples.extend_from_slice(abuf.chan(0));
                    } else {
                        for p in 0..abuf.frames() {
                            let mut sum = 0i32;

                            for c in 0..num_channels {
                                sum += i32::from(abuf.chan(c)[p]);
                            }
                            samples.push((sum / num_channels as i32) as i16);
                        }
                    }
                }
            }
        }

        Ok(AudioBuffer {
            sample_rate,
            samples,
        })
    }

    pub fn samples(&self) -> &[i16] {
        &self.samples
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn dump_nrad<W: Write>(&self, w: &mut W) -> Result<()> {
        w.write_all(b"NRAD")?;

        // The NovaRave SPU runs at 48kHz and uses 12 fractional bits when stepping.
        let spu_base: u32 = 48_000;

        // Divider to reach the sample rate
        let spu_step = ((self.sample_rate << 12) + spu_base / 2) / spu_base;

        let spu_step = if spu_step > 0x3fff {
            0x3fff
        } else {
            spu_step as u16
        };

        info!(
            "SPU_STEP will be 0x{:x} ({:.03}) resulting in a true sample rate of {}Hz",
            spu_step,
            (spu_step as f32) / ((1 << 12) as f32),
            (u32::from(spu_step) * spu_base + (1 << 11)) >> 12
        );

        w.write_u16::<LittleEndian>(spu_step)?;

        // We encode blocks of 33 samples. The first one is stored in full, the rest is encoded as
        // 4-bit deltas.
        //
        // The total block size is therefore 2B header + 2B sample0 + 16B payload = 20B
        let block_len = 33;

        let nblocks = (self.samples.len() + block_len - 1) / block_len;

        // We carry the step index from block to block
        let mut index = 0i8;

        for (i, block) in self.samples.chunks(block_len).enumerate() {
            let stop = (i + 1) == nblocks;

            let mut prev = block[0] as i32;

            w.write_u8(stop as u8)?;
            w.write_u8(index as u8)?;
            w.write_i16::<LittleEndian>(prev as i16)?;

            let mut b: u8 = 0;

            for si in 1..block_len {
                let s = match block.get(si) {
                    Some(&s) => s as i32,
                    None => prev as i32,
                };

                let step_size = STEP_SIZE_LUT[index as usize] as u32;

                let diff = s - prev;

                let abs_diff = diff.unsigned_abs();

                let mut encoded = (abs_diff << 2) / step_size;

                if encoded > 0b111 {
                    encoded = 0b111
                }

                if diff < 0 {
                    encoded |= 8;
                }

                if si & 1 == 1 {
                    b = (encoded as u8) << 4;
                } else {
                    b |= encoded as u8;
                    w.write_u8(b)?;
                }

                // Now we need to update `prev` for the next sample by effectively decoding the
                // current sample
                let encoded_abs = (encoded & 7) as u32;
                let encoded_diff = (step_size >> 3) + ((encoded_abs * step_size) >> 2);

                let mut encoded_diff = encoded_diff as i32;

                if encoded & 8 != 0 {
                    encoded_diff = -encoded_diff;
                }

                prev += encoded_diff;

                prev = prev.min(i32::from(i16::MAX));
                prev = prev.max(i32::from(i16::MIN));

                index += ADPCM_INDEX_OFF[encoded_abs as usize];
                index = index.max(0);
                index = index.min((STEP_SIZE_LUT.len() - 1) as i8);
            }
        }

        Ok(())
    }
}

const ADPCM_INDEX_OFF: [i8; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

const STEP_SIZE_LUT: [i16; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
