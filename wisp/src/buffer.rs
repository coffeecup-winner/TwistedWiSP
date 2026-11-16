//! Immutable decoded sources; each DSP binding receives private mutable storage.
use std::{path::Path, sync::Arc};
#[derive(Clone, Debug)]
pub struct DecodedAudio {
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub channels: u16,
    pub preview: Arc<[(f32, f32)]>,
}
impl DecodedAudio {
    pub const MAX_SAMPLES: usize = 16 * 1024 * 1024;
    pub fn load(path: &Path) -> Result<Self, String> {
        let reader =
            hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::decode(reader)
    }
    pub fn decode<R: std::io::Read>(reader: hound::WavReader<R>) -> Result<Self, String> {
        let spec = reader.spec();
        let channels = spec.channels as usize;
        if channels == 0 || spec.sample_rate == 0 {
            return Err("WAV channels/rate must be positive".into());
        }
        if reader.len() as usize > Self::MAX_SAMPLES {
            return Err("WAV exceeds 16M interleaved sample limit".into());
        }
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float if spec.bits_per_sample == 32 => reader
                .into_samples::<f32>()
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?,
            hound::SampleFormat::Int if [8, 16, 24, 32].contains(&spec.bits_per_sample) => {
                let divisor = 2.0_f64.powi(spec.bits_per_sample as i32 - 1);
                reader
                    .into_samples::<i32>()
                    .map(|s| s.map(|v| (v as f64 / divisor) as f32))
                    .collect::<Result<_, _>>()
                    .map_err(|e| e.to_string())?
            }
            _ => return Err("Unsupported WAV sample format/depth".into()),
        };
        if !samples.len().is_multiple_of(channels) {
            return Err("Truncated WAV frame".into());
        }
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("Nonfinite WAV sample".into());
        }
        let mono = samples
            .chunks_exact(channels)
            .map(|frame| (frame.iter().map(|v| *v as f64).sum::<f64>() / channels as f64) as f32)
            .collect::<Vec<_>>();
        Ok(Self::new(mono, spec.sample_rate, spec.channels))
    }
    pub fn builtin(name: &str) -> Result<Self, String> {
        if name != "sine" {
            return Err(format!("Unknown builtin buffer: {name}"));
        }
        // A normalized single cycle, not a recording at the device sample rate.
        Ok(Self::new(
            (0..1024)
                .map(|i| (i as f32 * std::f32::consts::TAU / 1024.).sin())
                .collect(),
            0,
            1,
        ))
    }
    fn new(samples: Vec<f32>, sample_rate: u32, channels: u16) -> Self {
        let chunk = samples.len().div_ceil(1024).max(1);
        let preview = samples
            .chunks(chunk)
            .map(|c| {
                c.iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(*v), hi.max(*v))
                    })
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            samples: samples.into(),
            sample_rate,
            channels,
            preview,
        }
    }
}

// JIT-only packed ABI: length bits followed by samples, always processor-owned
// or the immutable empty sentinel. These entrypoints are never exposed to C/UI.
// All float validation happens before integer conversion or address arithmetic.
pub(crate) unsafe extern "C" fn array_read(array: *mut f32, index: f32) -> f32 {
    let length = unsafe { array.cast::<u32>().read() } as usize;
    if !index.is_finite() || index < 0. || index as f64 >= length as f64 {
        return 0.;
    }
    unsafe { array.add(index as usize + 1).read() }
}
pub(crate) unsafe extern "C" fn array_write(array: *mut f32, index: f32, value: f32) {
    let length = unsafe { array.cast::<u32>().read() } as usize;
    if !index.is_finite() || index < 0. || index as f64 >= length as f64 {
        return;
    }
    unsafe {
        array.add(index as usize + 1).write(value);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn wav_depth_channels_rates_and_errors() {
        for bits in [8, 16, 24, 32] {
            for channels in [1, 2, 3] {
                let mut bytes = Cursor::new(Vec::new());
                let spec = hound::WavSpec {
                    channels,
                    sample_rate: 22050,
                    bits_per_sample: bits,
                    sample_format: hound::SampleFormat::Int,
                };
                {
                    let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
                    let amplitude = (1i64 << (bits - 2)) as i32;
                    for _ in 0..channels {
                        writer.write_sample(amplitude).unwrap();
                    }
                    for _ in 0..channels {
                        writer.write_sample(-amplitude).unwrap();
                    }
                    writer.finalize().unwrap();
                }
                bytes.set_position(0);
                let audio = DecodedAudio::decode(hound::WavReader::new(bytes).unwrap()).unwrap();
                assert_eq!(&*audio.samples, &[0.5, -0.5]);
                assert_eq!(audio.sample_rate, 22050);
                assert_eq!(audio.channels, channels);
            }
        }
        for value in [2.5, f32::NAN] {
            let mut bytes = Cursor::new(Vec::new());
            {
                let mut w = hound::WavWriter::new(
                    &mut bytes,
                    hound::WavSpec {
                        channels: 1,
                        sample_rate: 48000,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )
                .unwrap();
                w.write_sample(value).unwrap();
                w.finalize().unwrap();
            }
            bytes.set_position(0);
            let result = DecodedAudio::decode(hound::WavReader::new(bytes).unwrap());
            if value.is_finite() {
                assert_eq!(result.unwrap().samples[0], 2.5);
            } else {
                assert!(result.is_err());
            }
        }
        assert!(hound::WavReader::new(Cursor::new(b"bad wav")).is_err());
    }
}
