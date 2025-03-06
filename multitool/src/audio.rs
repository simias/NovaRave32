use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler};
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;
use symphonia::core::audio::AudioBuffer as SAudioBuffer;
use symphonia::core::audio::Signal;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::io::MediaSourceStream;
use symphonia::default::get_probe;

#[derive(Clone)]
pub struct AudioBuffer {
    sample_rate: u32,
    samples: Vec<i16>,
}

impl AudioBuffer {
    pub fn from_path<P: AsRef<Path>>(audio_path: P, channel: Option<usize>) -> Result<AudioBuffer> {
        let p = audio_path.as_ref();

        let mut is_nrad = false;

        if let Some(ext) = p.extension().and_then(|ext| ext.to_str()) {
            if ext.to_lowercase() == "nrad" {
                is_nrad = true;
            }
        }

        let file = File::open(audio_path)?;

        if is_nrad {
            AudioBuffer::from_nrad_file(file)
        } else {
            AudioBuffer::from_file(file, channel)
        }
    }

    pub fn from_nrad_file(mut audio_file: File) -> Result<AudioBuffer> {
        let magic = audio_file.read_u32::<LittleEndian>()?;

        if magic != 0x4441524e {
            bail!("Invalid NRAD magic");
        }

        let _pad = audio_file.read_u16::<LittleEndian>()?;
        let spu_step = audio_file.read_u16::<LittleEndian>()?;

        let spu_base: u32 = 48_000;

        let sample_rate = (u32::from(spu_step) * spu_base + (1 << 11)) >> 12;

        let mut samples = Vec::new();

        let mut prev_samples = [0, 0];

        loop {
            let header = match audio_file.read_u16::<LittleEndian>() {
                Ok(h) => h,
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
                e => e?,
            };

            let filter = ((header >> 4) & 0xf) as usize;
            let shift = (header & 0xf) as u8;

            let mut encoded = Vec::with_capacity(14);

            for _ in 0..7 {
                encoded.push(audio_file.read_u16::<LittleEndian>()?);
            }

            let decoded = adpcm_decode_block(&encoded, prev_samples, filter, shift);

            prev_samples[0] = decoded[26];
            prev_samples[1] = decoded[27];

            samples.extend_from_slice(&decoded);
        }

        Ok(AudioBuffer {
            sample_rate,
            samples,
        })
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

    pub fn resample(&self, sample_rate: u32) -> Result<AudioBuffer> {
        if self.sample_rate == sample_rate {
            return Ok(self.clone());
        }

        let samples: Vec<f32> = self.samples.iter().map(|&s| (s as f32) / 32768.).collect();

        let mut resampler = FftFixedIn::<f32>::new(
            self.sample_rate as usize,
            sample_rate as usize,
            samples.len(),
            1024,
            1,
        )?;

        let resampled = resampler.process(&[&samples], None)?;

        let resampled = resampled[0]
            .iter()
            .map(|&s| (s * 32768.).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16)
            .collect();

        Ok(AudioBuffer {
            sample_rate,
            samples: resampled,
        })
    }

    /// Play the audio on the system's default output device
    pub fn playback(&self) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No output device available"))?;
        let config = device.default_output_config()?.config();

        info!(
            "Initiating playback on `{}` (sample rate: {}Hz)",
            device.name()?,
            config.sample_rate.0
        );

        let buf = self.resample(config.sample_rate.0)?;

        let channels = config.channels as usize;

        let finished = Arc::new(AtomicBool::new(false));
        let sample_index = Arc::new(Mutex::new(0));

        let finished_clone = Arc::clone(&finished);

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                let mut index = sample_index.lock().unwrap();

                for frame in data.chunks_mut(channels) {
                    if *index >= buf.samples.len() {
                        finished_clone.store(true, Ordering::SeqCst);
                        return;
                    }
                    let sample = buf.samples[*index];
                    for sample_out in frame.iter_mut() {
                        *sample_out = sample;
                    }
                    *index += 1;
                }
            },
            |err| eprintln!("Stream error: {}", err),
            None,
        )?;

        stream.play()?;

        while !finished.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
        }

        Ok(())
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

        // Padding that could be used for flags later
        w.write_u16::<LittleEndian>(0)?;
        w.write_u16::<LittleEndian>(spu_step)?;

        // We encode blocks of 28 samples. Each encoded sample will be 4 bits, plus 2B header for a
        // total of 16B per block
        let block_len = 28;

        let nblocks = self.samples.len().div_ceil(block_len);

        let mut start = true;

        let mut prev_samples = [0, 0];

        let mut total_error = 0;

        for (i, block) in self.samples.chunks(block_len).enumerate() {
            let stop = (i + 1) == nblocks;

            let _filter = 0;

            let mut samples: Vec<i16> = block.to_vec();

            // Make sure the last block is full by copying the last sample as padding
            samples.resize(block_len, *samples.last().unwrap());

            let mut filter = 0;

            let (mut encoded, mut shift) = adpcm_encode_block(&samples, prev_samples, filter);

            let mut decoded = adpcm_decode_block(&encoded, prev_samples, filter, shift);

            // Try the other filters to see if we get a better match.
            let mut error = adpcm_error(&samples, &decoded);

            // If we're (re)starting, we don't know what's in prev_samples, therefore we cannot use
            // any filter besides 0 (that ignores the previous samples)
            if start {
                start = false;
            } else if error > 0 {
                for f in 1..FILTER_WEIGHTS.len() {
                    let (fencoded, fshift) = adpcm_encode_block(&samples, prev_samples, f);

                    let fdecoded = adpcm_decode_block(&fencoded, prev_samples, f, fshift);

                    let ferror = adpcm_error(&samples, &fdecoded);

                    if ferror < error {
                        filter = f;
                        error = ferror;
                        shift = fshift;
                        encoded = fencoded;
                        decoded = fdecoded;

                        if error == 0 {
                            break;
                        }
                    }
                }
            }

            let header = ((stop as u16) << 8) | ((filter as u16) << 4) | (shift as u16);
            w.write_u16::<LittleEndian>(header)?;

            for e in encoded {
                w.write_u16::<LittleEndian>(e)?;
            }

            total_error += error;

            prev_samples[0] = decoded[block_len - 2];
            prev_samples[1] = decoded[block_len - 1];
        }

        info!(
            "Average absolute error per 16bit sample: {:.03}",
            (total_error as f32) / (self.samples.len() as f32)
        );

        Ok(())
    }
}

/// Encodes `samples` with the given `filter`. Returns the encoded buffer and the shift value used.
///
/// If filter is not 0 then `prev_samples` should be the last two *decoded* samples from the
/// previous block.
///
/// The number of samples should be a multiple of 4 since we encode 4 bits at a time into u16
fn adpcm_encode_block(samples: &[i16], prev_samples: [i16; 2], filter: usize) -> (Vec<u16>, u8) {
    assert_eq!(samples.len() % 4, 0);

    let (wp, wn) = FILTER_WEIGHTS[filter];
    let wp = wp as i32;
    let wn = wn as i32;

    let mut diff_max = 0;
    let mut diff_min = -1;

    // First pass where we look for the magnitude of the encoded difference to chose the shift
    // value.
    let mut ps = [i32::from(prev_samples[0]), i32::from(prev_samples[1])];

    for &s in samples {
        let s = i32::from(s);

        let mut predicted = 0;

        predicted += (ps[0] * wn) >> 6;
        predicted += (ps[1] * wp) >> 6;

        let diff = s - predicted;

        if diff > diff_max {
            diff_max = diff;
        }

        diff_max = diff_max.max(diff);
        diff_min = diff_min.min(diff);

        ps[0] = ps[1];
        ps[1] = s;
    }

    // Note that this code is somewhat sub-optimal because the choice of shift value will change
    // the precision of the diff encoding and therefore the intermediate sample values, so in edge
    // cases we may end up with increased diff values that no longer fit post-shift (or shift
    // values unnecessarily large in other cases). In this case we'll just saturate the diff and
    // hope that it'll converge after a few samples.
    //
    // In order to handle this edge case we could see if `shift + 1` and `shift - 1` result in
    // smaller errors.

    // The +1 is because we also need the sign bit
    let significant_bit_pos = (32 - diff_max.leading_zeros() + 1) as i32;
    let significant_bit_neg = (32 - diff_min.leading_ones() + 1) as i32;

    // We encode 4 bits per sample so we need to scale to fit the MSB
    let shift_pos = significant_bit_pos - 4;
    let shift_neg = significant_bit_neg - 4;

    let shift = shift_pos.max(shift_neg).clamp(0, 12) as u8;

    // Now that we have the shift, we can encode properly
    let mut ps = [i32::from(prev_samples[0]), i32::from(prev_samples[1])];

    let mut e = 0u16;
    let mut res = Vec::with_capacity(samples.len() / 4);

    for (i, &s) in samples.iter().enumerate() {
        let s = i32::from(s);

        let mut predicted = 0;

        predicted += (ps[0] * wn) >> 6;
        predicted += (ps[1] * wp) >> 6;

        let diff = (s - predicted) >> shift;

        let diff = diff.clamp(-8, 7);

        predicted += diff << shift;
        predicted = predicted.clamp(i32::from(i16::MIN), i32::from(i16::MAX));

        ps[0] = ps[1];
        ps[1] = predicted;

        let encoded = (diff as u16) & 0xf;

        let bpos = (i & 3) * 4;
        e |= encoded << bpos;
        if bpos == 12 {
            res.push(e);
            e = 0;
        }
    }

    (res, shift)
}

fn adpcm_decode_block(
    encoded: &[u16],
    prev_samples: [i16; 2],
    filter: usize,
    shift: u8,
) -> Vec<i16> {
    let (wp, wn) = FILTER_WEIGHTS[filter];
    let wp = wp as i32;
    let wn = wn as i32;

    let mut res = Vec::with_capacity(encoded.len() * 4);

    let mut ps = [i32::from(prev_samples[0]), i32::from(prev_samples[1])];
    for e in encoded {
        for i in 0..4 {
            // Sign-extend
            let e = ((e >> (i * 4)) << 12) as i16;
            let diff = e >> (12 - shift);

            let mut predicted = 0;

            predicted += (ps[0] * wn) >> 6;
            predicted += (ps[1] * wp) >> 6;
            predicted += i32::from(diff);

            let sample = predicted.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;

            res.push(sample);

            ps[0] = ps[1];
            ps[1] = sample as i32;
        }
    }

    res
}

/// Quantify the error between source and decoded
///
/// Returns the sum of absolute differences between `source` and `decoded`
fn adpcm_error(source: &[i16], decoded: &[i16]) -> u32 {
    assert_eq!(source.len(), decoded.len());

    source
        .iter()
        .zip(decoded.iter())
        .map(|(&s, &d)| ((s as i32) - (d as i32)).unsigned_abs())
        .sum()
}

/// Weights used for ADPCM encoding. The first weight is applied to the previous sample, the 2nd to
/// the penultimate
const FILTER_WEIGHTS: [(i8, i8); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (112, -60)];
