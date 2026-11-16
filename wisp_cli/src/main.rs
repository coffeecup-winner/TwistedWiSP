use std::path::PathBuf;

use clap::Parser;
use twisted_wisp::{TwistedWispEngine, TwistedWispEngineConfig};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
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
    core_lib_path: Option<PathBuf>,
    #[arg()]
    file_name: PathBuf,
    #[arg(short, long)]
    process_one: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    TwistedWispEngine::enable_logging();

    let config = TwistedWispEngineConfig {
        audio_host: args.audio_host,
        audio_device: args.audio_device,
        audio_output_channels: args.audio_output_channels,
        audio_buffer_size: args.audio_buffer_size,
        audio_sample_rate: args.audio_sample_rate,
        midi_in_port: args.midi_in_port,
        core_path: args.core_lib_path,
    };
    let mut wisp = TwistedWispEngine::create(&config)?;

    let name = wisp.ctx_load_flow_from_file(args.file_name.to_str().unwrap())?;

    let mut sp = wisp.runtime_compile_signal_processor(name)?;

    if args.process_one {
        let mut data = vec![0.0; sp.output_channels()];
        sp.process_one_checked(&mut data)?;
        println!("{:?}", data);
        return Ok(());
    }

    wisp.runtime_install_checked(sp)?;
    wisp.runtime_start_checked()?;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
