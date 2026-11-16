use std::path::PathBuf;

use clap::Parser;
use twisted_wisp::{TwistedWispEngine, TwistedWispEngineConfig};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    list_audio_devices: bool,
    #[arg(short, long)]
    audio_host: Option<String>,
    #[arg(short = 'd', long)]
    audio_device: Option<String>,
    #[arg(short = 'o', long)]
    audio_output_channels: Option<u16>,
    #[arg(short = 'b', long)]
    audio_buffer_size: Option<u32>,
    #[arg(short = 'r', long)]
    audio_sample_rate: Option<u32>,
    #[arg(short = 'm', long)]
    midi_in_port: Option<String>,
    #[arg(short, long)]
    server: bool,

    // Non-server mode
    #[arg(short, long)]
    core_lib_path: Option<PathBuf>,
    #[arg()]
    file_name: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let config = TwistedWispEngineConfig {
        audio_host: args.audio_host.as_deref(),
        audio_device: args.audio_device.as_deref(),
        audio_output_channels: args.audio_output_channels,
        audio_buffer_size: args.audio_buffer_size,
        audio_sample_rate: args.audio_sample_rate,
        midi_in_port: args.midi_in_port.as_deref(),
        core_path: args.core_lib_path.as_deref(),
    };
    let mut wisp = TwistedWispEngine::create(config)?;

    wisp.context_set_main_function("phasor".to_string());
    wisp.context_update().expect("Failed to update context");

    wisp.dsp_start();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
