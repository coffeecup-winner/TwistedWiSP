use std::path::PathBuf;

use clap::Parser;
use twisted_wisp::{core::WispFunction, TwistedWispEngine, TwistedWispEngineConfig};

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
        audio_host: args.audio_host.as_ref().map(|x| x.as_str()),
        audio_device: args.audio_device.as_ref().map(|s| s.as_str()),
        audio_output_channels: args.audio_output_channels,
        audio_buffer_size: args.audio_buffer_size,
        audio_sample_rate: args.audio_sample_rate,
        midi_in_port: args.midi_in_port.as_ref().map(|s| s.as_str()),
    };
    let mut wisp = TwistedWispEngine::create(config)?;

    let mut ctx = twisted_wisp::core::WispContext::new(wisp.get_system_info().num_channels);

    ctx.add_builtin_functions();
    ctx.load_core_functions(args.core_lib_path.as_ref().unwrap())
        .expect("Failed to add core functions");

    let flow_name = ctx.load_function(args.file_name.as_ref().unwrap())?;

    for f in ctx.functions_iter() {
        wisp.context_add_or_update_functions(f.get_ir_functions(&ctx));
    }

    wisp.context_set_main_function(flow_name);

    wisp.dsp_start();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
