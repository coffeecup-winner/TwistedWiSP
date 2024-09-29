use std::error::Error;

use log::info;

use crate::{
    protocol::{CommandResponse, SystemInfo, WispCommand, WispCommandResponse},
    runner::{
        audio::device::ConfiguredAudioDevice,
        context::{WispContext, WispExecutionContext},
        midi::WispMidiIn,
        runtime::WispRuntime,
    },
};

pub fn main(
    mut wisp: WispContext,
    device: ConfiguredAudioDevice,
    midi_in: WispMidiIn,
    command_receiver: crossbeam::channel::Receiver<WispCommand>,
    mut response_sender: crossbeam::channel::Sender<WispCommandResponse>,
) -> Result<(), Box<dyn Error>> {
    let execution_context = WispExecutionContext::init();
    let mut runtime = WispRuntime::init(device, midi_in);

    info!("Switching to server mode - waiting for commands now");
    loop {
        let command = command_receiver.recv()?;
        info!("< {:?}", command);
        match command {
            WispCommand::GetSystemInfo => reply(
                &mut response_sender,
                WispCommandResponse::Ok(CommandResponse::SystemInfo(SystemInfo {
                    num_channels: wisp.num_outputs(),
                })),
            ),
            WispCommand::DspStart => {
                runtime.start_dsp();
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::DspStop => {
                runtime.stop_dsp();
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::Exit => {
                info!("Exiting");
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )?;
                return Ok(());
            }
            WispCommand::ContextReset => {
                wisp.reset();
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextAddOrUpdateFunctions(functions) => {
                for func in functions {
                    wisp.add_function(func);
                }
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextRemoveFunction(name) => {
                wisp.remove_function(&name);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextSetMainFunction(name) => {
                wisp.set_main_function(&name);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextSetDataValue(name, id, idx, value) => {
                runtime.set_data_value(&name, id, idx, value);
                // TODO: Async update
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextSetDataArray(name, id, idx, array_name) => {
                let resp = match wisp.get_data_array(&name, &array_name) {
                    Some(array) => {
                        runtime.set_data_array(&name, id, idx, array);
                        WispCommandResponse::Ok(CommandResponse::Ack)
                    }
                    None => WispCommandResponse::NonFatalFailure,
                };
                // TODO: Async update
                reply(&mut response_sender, resp)
            }
            WispCommand::ContextLearnMidiCC(name, id, idx) => {
                runtime.learn_midi_cc(&name, id, idx);
                let idx = runtime.watch_data_value(&name, id, idx, true);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::WatchIndex(idx)),
                )
            }
            WispCommand::ContextWatchDataValue(name, id, idx) => {
                let idx = runtime.watch_data_value(&name, id, idx, false);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::WatchIndex(idx)),
                )
            }
            WispCommand::ContextUnwatchDataValue(idx) => {
                runtime.unwatch_data_value(idx);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextQueryWatchedDataValues => {
                let values = runtime.query_watched_data_values();
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::WatchedDataValues(values)),
                )
            }
            WispCommand::ContextLoadWaveFile(name, buffer_name, filepath) => {
                let resp = match wisp.load_wave_file(&name, &buffer_name, &filepath) {
                    Ok(()) => WispCommandResponse::Ok(CommandResponse::Ack),
                    Err(e) => {
                        log::error!("Failed to load wave file: {}", e);
                        WispCommandResponse::NonFatalFailure
                    }
                };
                reply(&mut response_sender, resp)
            }
            WispCommand::ContextUnloadWaveFile(name, buffer_name) => {
                wisp.unload_wave_file(&name, &buffer_name);
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
            WispCommand::ContextUpdate => {
                runtime.switch_to_signal_processor(
                    &execution_context,
                    &wisp,
                    wisp.main_function(),
                )?;
                reply(
                    &mut response_sender,
                    WispCommandResponse::Ok(CommandResponse::Ack),
                )
            }
        }?;
    }
}

fn reply(
    output: &mut crossbeam::channel::Sender<WispCommandResponse>,
    response: WispCommandResponse,
) -> Result<(), Box<dyn Error>> {
    info!("> {:?}", response);
    output.send(response)?;
    Ok(())
}
