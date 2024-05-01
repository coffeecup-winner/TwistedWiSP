mod audio;
mod compiler;
mod context;
mod runtime;
mod server;

use std::error::Error;

use clap::Parser;
use context::{WispContext, WispExecutionContext};
use runtime::WispRuntime;
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

    // Non-server mode
    #[arg(short, long)]
    core_lib_path: Option<String>,
    #[arg()]
    file_name: Option<String>,
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
    let wisp = WispContext::new(device.num_output_channels(), device.sample_rate());

    if args.server {
        crate::server::main(wisp, device)
    } else {
        run_file(
            args.core_lib_path.expect("No core library path provided"),
            args.file_name.expect("No file name provided"),
            wisp,
            device,
        )
    }
}

fn run_file(
    core_lib_path: String,
    file_path: String,
    mut wisp: WispContext,
    device: ConfiguredAudioDevice,
) -> Result<(), Box<dyn Error>> {
    let mut core_context = twisted_wisp::WispContext::new(wisp.num_outputs());
    core_context.add_builtin_functions();
    core_context.load_core_functions(&core_lib_path)?;
    let result = core_context.load_function(&file_path)?;

    for f in core_context.functions_iter() {
        wisp.add_function(f.get_ir_function(&core_context));
    }

    let execution_context = WispExecutionContext::init();
    let mut runtime = WispRuntime::init(device);

    runtime.switch_to_signal_processor(&execution_context, &wisp, &result.name)?;
    runtime.start_dsp();

    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
