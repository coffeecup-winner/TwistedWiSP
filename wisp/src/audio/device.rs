use cpal::{
    traits::{DeviceTrait, HostTrait},
    BufferSize, Device, SampleFormat, Stream, StreamConfig, SupportedBufferSize,
};
use log::{error, info};

pub struct ConfiguredAudioDevice {
    device: Device,
    config: StreamConfig,
}

impl ConfiguredAudioDevice {
    pub fn open(
        preferred_host: Option<&str>,
        preferred_device: Option<&str>,
        preferred_output_channels: Option<u16>,
        preferred_buffer_size: Option<u32>,
        preferred_sample_rate: Option<u32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = Self::select_output_audio_device(preferred_host, preferred_device)?;
        let config = Self::select_output_audio_device_config(
            &device,
            preferred_output_channels,
            preferred_buffer_size,
            preferred_sample_rate,
        )?;
        Ok(ConfiguredAudioDevice { device, config })
    }

    pub fn num_output_channels(&self) -> u32 {
        self.config.channels as u32
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate
    }

    pub fn build_output_audio_stream<
        F: FnMut(u32, &mut [f32]) + Send + 'static,
        E: FnMut(cpal::Error) + Send + 'static,
    >(
        &self,
        mut process_audio: F,
        on_error: E,
    ) -> Result<Stream, Box<dyn std::error::Error>> {
        let num_output_channels = self.num_output_channels();
        let output_stream = self.device.build_output_stream(
            self.config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                process_audio(num_output_channels, data);
            },
            on_error,
            None,
        )?;

        Ok(output_stream)
    }

    fn select_output_audio_device(
        preferred_host: Option<&str>,
        preferred_device: Option<&str>,
    ) -> Result<Device, Box<dyn std::error::Error>> {
        let host = if let Some(host_name) = preferred_host {
            let mut host = None;
            for host_id in cpal::available_hosts() {
                if host_id.name() == host_name {
                    host = Some(cpal::host_from_id(host_id)?);
                    break;
                }
            }
            match host {
                Some(host) => host,
                None => {
                    error!("Failed to find the preferred audio host '{host_name}', falling back to default");
                    cpal::default_host()
                }
            }
        } else {
            cpal::default_host()
        };
        info!("Selected audio host: {}", host.id().name());

        let device = if let Some(device_name) = preferred_device {
            let mut device = None;
            for dev in host.output_devices()? {
                if dev.description()?.name() == device_name {
                    device = Some(dev);
                    break;
                }
            }
            match device {
                Some(dev) => Some(dev),
                None => {
                    error!("Failed to find the preferred audio device '{device_name}', falling back to default");
                    host.default_output_device()
                }
            }
        } else {
            host.default_output_device()
        }
        .ok_or("No output audio device is available")?;
        info!("Selected audio device: {}", device.description()?.name());

        Ok(device)
    }

    fn select_output_audio_device_config(
        device: &Device,
        channels: Option<u16>,
        buffer: Option<u32>,
        rate: Option<u32>,
    ) -> Result<StreamConfig, Box<dyn std::error::Error>> {
        let ranges: Vec<_> = device.supported_output_configs()?.collect();
        let config = choose_config(&ranges, channels, buffer, rate)?;
        info!(
            "Audio: {} channels, {} Hz, buffer {:?}",
            config.channels, config.sample_rate, config.buffer_size
        );
        Ok(config)
    }
}

fn choose_config(
    ranges: &[cpal::SupportedStreamConfigRange],
    channels: Option<u16>,
    buffer: Option<u32>,
    rate: Option<u32>,
) -> Result<StreamConfig, Box<dyn std::error::Error>> {
    if channels == Some(0) || rate == Some(0) || buffer == Some(0) {
        return Err("Audio channels/rate/buffer must be positive".into());
    }
    let wanted_channels = channels.unwrap_or(2);
    let wanted_rate = rate.unwrap_or(44_100);
    let best = ranges
        .iter()
        .filter(|r| r.sample_format() == SampleFormat::F32 && r.channels() > 0)
        .min_by_key(|r| {
            (
                r.channels().abs_diff(wanted_channels),
                if wanted_rate < r.min_sample_rate() {
                    r.min_sample_rate() - wanted_rate
                } else {
                    wanted_rate.saturating_sub(r.max_sample_rate())
                },
            )
        })
        .ok_or("No F32 audio output configuration is supported")?;
    let rate = wanted_rate.clamp(best.min_sample_rate(), best.max_sample_rate());
    if best.channels() != wanted_channels || rate != wanted_rate {
        info!(
            "Requested audio configuration unsupported; using {} channels at {} Hz",
            best.channels(),
            rate
        );
    }
    let mut config: StreamConfig = (*best).with_sample_rate(rate).into();
    config.buffer_size = match (buffer, best.buffer_size()) {
        (Some(n), SupportedBufferSize::Range { min, max }) => {
            BufferSize::Fixed(n.clamp(*min, *max))
        }
        _ => BufferSize::Default,
    };
    Ok(config)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
    ) -> cpal::SupportedStreamConfigRange {
        cpal::SupportedStreamConfigRange::new(
            channels,
            min,
            max,
            SupportedBufferSize::Range { min: 64, max: 1024 },
            format,
        )
    }
    #[test]
    fn requires_float_and_clamps_unsupported_settings() {
        let ranges = [
            range(2, 44100, 48000, SampleFormat::I16),
            range(1, 48000, 96000, SampleFormat::F32),
        ];
        let c = choose_config(&ranges, Some(2), Some(1), Some(44100)).unwrap();
        assert_eq!(c.channels, 1);
        assert_eq!(c.sample_rate, 48000);
        assert_eq!(c.buffer_size, BufferSize::Fixed(64));
        assert!(choose_config(&ranges[..1], None, None, None).is_err());
        assert!(choose_config(&ranges, Some(0), None, None).is_err());
    }
}
