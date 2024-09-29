use cpal::{
    traits::{DeviceTrait, HostTrait},
    BufferSize, Device, SampleFormat, SampleRate, Stream, StreamConfig, SupportedBufferSize,
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
        );
        Ok(ConfiguredAudioDevice { device, config })
    }

    #[allow(dead_code)]
    pub fn list_all_devices() -> Result<(), Box<dyn std::error::Error>> {
        info!("Available audio devices:");
        let default_host_id = cpal::default_host().id();
        for host_id in cpal::available_hosts() {
            info!(
                " {} {}:",
                if default_host_id == host_id { "*" } else { "-" },
                host_id.name()
            );
            let host = cpal::host_from_id(host_id)?;
            let default_device_name = host
                .default_output_device()
                .map(|d| d.name().unwrap())
                .unwrap_or_else(|| "".to_owned());
            for device in host.output_devices()? {
                let name = device.name()?;
                info!(
                    "    {} {}",
                    if default_device_name == name {
                        "*"
                    } else {
                        "-"
                    },
                    name
                );
            }
        }

        Ok(())
    }

    pub fn num_output_channels(&self) -> u32 {
        self.config.channels as u32
    }

    pub fn sample_rate(&self) -> u32 {
        self.config.sample_rate.0
    }

    pub fn build_output_audio_stream<F: FnMut(u32, &mut [f32]) + Send + 'static>(
        &self,
        mut process_audio: F,
    ) -> Result<Stream, Box<dyn std::error::Error>> {
        let num_output_channels = self.num_output_channels();
        let output_stream = self.device.build_output_stream(
            &self.config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                process_audio(num_output_channels, data);
            },
            |err| error!("Output audio stream error: {err}"),
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
                if dev.name()? == device_name {
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
        .expect("Failed to find any output audio device");
        info!("Selected audio device: {}", device.name()?);

        Ok(device)
    }

    fn select_output_audio_device_config(
        device: &Device,
        preferred_output_channels: Option<u16>,
        preferred_buffer_size: Option<u32>,
        preferred_sample_rate: Option<u32>,
    ) -> StreamConfig {
        let supported_config_range = device
            .supported_output_configs()
            .expect("No supported configs for the selected audio device")
            .find(|c| {
                c.channels() == preferred_output_channels.unwrap_or(2)
                    || c.sample_format() == SampleFormat::F32
            })
            .expect("No supported configs with the preferred number of channels");
        let supported_config = supported_config_range
            .with_sample_rate(SampleRate(preferred_sample_rate.unwrap_or(44100)));
        let buffer_size = if let Some(mut buffer_size) = preferred_buffer_size {
            if let SupportedBufferSize::Range { min, max } = supported_config.buffer_size() {
                let size = buffer_size.clamp(*min, *max);
                if size != buffer_size {
                    error!(
                        "Preferred buffer size {} is out of range, using {} instead",
                        buffer_size, size
                    );
                    buffer_size = size;
                }
            }
            BufferSize::Fixed(buffer_size)
        } else {
            BufferSize::Default
        };
        let mut config: StreamConfig = supported_config.into();
        config.buffer_size = buffer_size;
        info!("Selected audio device config: ");
        info!("  - channels: {}", config.channels);
        info!("  - bufsize: {:?}", config.buffer_size);
        info!("  - sample rate: {}", config.sample_rate.0);
        config
    }
}
