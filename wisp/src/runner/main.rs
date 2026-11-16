use std::error::Error;

use crate::protocol::{WispCommand, WispCommandResponse};

use super::{audio::device::ConfiguredAudioDevice, context::WispContext, midi::WispMidiIn};

pub struct Args {
    pub list_audio_devices: bool,
    pub audio_host: Option<String>,
    pub audio_device: Option<String>,
    pub audio_output_channels: Option<u16>,
    pub audio_buffer_size: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub midi_in_port: Option<String>,
    pub server: bool,
    // pub core_lib_path: Option<PathBuf>,
    // pub file_name: Option<PathBuf>,
}

pub fn main(
    args: Args,
    command_receiver: crossbeam::channel::Receiver<WispCommand>,
    response_sender: crossbeam::channel::Sender<WispCommandResponse>,
) -> Result<(), Box<dyn Error>> {
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
        super::server::main(wisp, device, midi_in, command_receiver, response_sender)
    } else {
        // TODO
        Ok(())

        // run_file(
        //     args.core_lib_path
        //         .as_ref()
        //         .expect("No core library path provided"),
        //     args.file_name.as_ref().expect("No file name provided"),
        //     wisp,
        //     device,
        //     midi_in,
        // )
    }
}

// fn run_file(
//     core_lib_path: &Path,
//     file_path: &Path,
//     mut wisp: WispContext,
//     device: ConfiguredAudioDevice,
//     midi_in: WispMidiIn,
// ) -> Result<(), Box<dyn Error>> {
//     let mut core_context = twisted_wisp::core::WispContext::new(wisp.num_outputs());
//     core_context.add_builtin_functions();
//     core_context.load_core_functions(core_lib_path)?;
//     let flow_name = core_context.load_function(file_path)?;

//     for f in core_context.functions_iter() {
//         for ir_func in f.get_ir_functions(&core_context) {
//             wisp.add_function(ir_func);
//         }
//     }

//     let execution_context = WispExecutionContext::init();
//     let mut runtime = WispRuntime::init(device, midi_in);

//     runtime.switch_to_signal_processor(&execution_context, &wisp, &flow_name)?;
//     runtime.start_dsp();

//     loop {
//         std::thread::sleep(std::time::Duration::from_millis(50));
//         // Wait until Ctrl+C
//     }
// }
