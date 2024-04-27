mod audio;
mod compiler;
mod context;
mod runtime;
mod server;

use std::error::Error;

use clap::Parser;
use stderrlog::LogLevelNum;

use crate::audio::device::ConfiguredAudioDevice;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    list_audio_devices: bool,
    #[arg(short, long)]
    audio_host: Option<String>,
    #[arg(short = 'd', long)]
    audio_device: Option<String>,
    #[arg(short, long)]
    server: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    stderrlog::new()
        .module(module_path!())
        .verbosity(LogLevelNum::Debug)
        .timestamp(stderrlog::Timestamp::Microsecond)
        .init()
        .expect("Failed to initialize the logger");

    let args = Args::parse();

    if args.list_audio_devices {
        ConfiguredAudioDevice::list_all_devices()?;
        return Ok(());
    }

    let device = ConfiguredAudioDevice::open(args.audio_host, args.audio_device)?;
    let wisp = context::WispContext::new(device.num_output_channels(), device.sample_rate());

    if args.server {
        return crate::server::main(wisp, device);
    }

    // let (mut processor, _ee) = wisp.create_signal_processor("example")?;
    // let mut v = vec![0.0; 64];
    // let start = std::time::Instant::now();
    // processor.process(&mut v);
    // let end = std::time::Instant::now();
    // let duration_ns = (end - start).as_nanos();
    // info!("Result: {:?}", v);
    // let time_limit_ns =
    //     1_000_000_000 / device.sample_rate() * v.len() as u32 / device.num_output_channels();
    // info!(
    //     "Took {}.{}µs (CPU usage: {:.2}%)",
    //     duration_ns / 1000,
    //     duration_ns % 1000,
    //     (duration_ns as f32 / time_limit_ns as f32 * 100.0)
    // );

    Ok(())
}
