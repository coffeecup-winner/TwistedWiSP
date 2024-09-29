mod audio;
mod compiler;
mod context;
mod midi;
mod runtime;
mod server;

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use clap::Parser;
use context::{WispContext, WispExecutionContext};
use midi::WispMidiIn;
use runtime::WispRuntime;

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

fn main() -> Result<(), Box<dyn Error>> {
    let mut log_builder = env_logger::Builder::from_default_env();
    log_builder.target(env_logger::Target::Stderr);
    log_builder.init();

    let args = Args::parse();

    if args.list_audio_devices {
        ConfiguredAudioDevice::list_all_devices()?;
        return Ok(());
    }

    let device = ConfiguredAudioDevice::open(
        args.audio_host,
        args.audio_device,
        args.audio_output_channels,
        args.audio_buffer_size,
        args.audio_sample_rate,
    )?;
    let midi_in = WispMidiIn::open(args.midi_in_port.as_deref())?;
    let wisp = WispContext::new(device.num_output_channels(), device.sample_rate());

    if args.server {
        crate::server::main(wisp, device, midi_in)
    } else {
        run_file(
            args.core_lib_path
                .as_ref()
                .expect("No core library path provided"),
            args.file_name.as_ref().expect("No file name provided"),
            wisp,
            device,
            midi_in,
        )
    }
}

fn run_file(
    core_lib_path: &Path,
    file_path: &Path,
    mut wisp: WispContext,
    device: ConfiguredAudioDevice,
    midi_in: WispMidiIn,
) -> Result<(), Box<dyn Error>> {
    let mut core_context = twisted_wisp::WispContext::new(wisp.num_outputs());
    core_context.add_builtin_functions();
    core_context.load_core_functions(core_lib_path)?;
    let flow_name = core_context.load_function(file_path)?;

    for f in core_context.functions_iter() {
        for ir_func in f.get_ir_functions(&core_context) {
            wisp.add_function(ir_func);
        }
    }

    let execution_context = WispExecutionContext::init();
    let mut runtime = WispRuntime::init(device, midi_in);

    runtime.switch_to_signal_processor(&execution_context, &wisp, &flow_name)?;
    runtime.start_dsp();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
