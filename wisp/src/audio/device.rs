use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device, Stream, StreamConfig,
};

pub struct ConfiguredAudioDevice {
    device: Device,
    config: StreamConfig,
}

impl ConfiguredAudioDevice {
    pub fn open(
        preferred_host: Option<String>,
        preferred_device: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let device = Self::select_output_audio_device(preferred_host, preferred_device)?;
        let config = Self::select_output_audio_device_config(&device);
        Ok(ConfiguredAudioDevice { device, config })
    }

    pub fn list_all_devices() -> Result<(), Box<dyn std::error::Error>> {
        eprintln!("Available audio devices:");
        let default_host_id = cpal::default_host().id();
        for host_id in cpal::available_hosts() {
            eprintln!(
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
                eprintln!(
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
            |err| eprintln!("Output audio stream error: {err}"),
            None,
        )?;

        Ok(output_stream)
    }

    fn select_output_audio_device(
        preferred_host: Option<String>,
        preferred_device: Option<String>,
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
                    eprintln!("Failed to find the preferred audio host '{host_name}', falling back to default");
                    cpal::default_host()
                }
            }
        } else {
            cpal::default_host()
        };
        eprintln!("Selected audio host: {}", host.id().name());

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
                    eprintln!("Failed to find the preferred audio device '{device_name}', falling back to default");
                    host.default_output_device()
                }
            }
        } else {
            host.default_output_device()
        }
        .expect("Failed to find any output audio device");
        eprintln!("Selected audio device: {}", device.name()?);

        Ok(device)
    }

    fn select_output_audio_device_config(device: &Device) -> StreamConfig {
        let supported_config = device
            .default_output_config()
            .expect("No default config for the selected audio device");
        let config: StreamConfig = supported_config.into();
        // Add buffer size selection here
        eprintln!("Selected audio device config: ");
        eprintln!("  - channels: {}", config.channels);
        eprintln!("  - bufsize: {:?}", config.buffer_size);
        eprintln!("  - sample rate: {}", config.sample_rate.0);
        config
    }
}
