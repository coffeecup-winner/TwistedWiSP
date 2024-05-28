use std::{error::Error, io::Write};

use log::info;
use twisted_wisp_protocol::{self, CommandResponse, SystemInfo, WispCommand, WispCommandResponse};

use crate::{
    audio::device::ConfiguredAudioDevice,
    context::{WispContext, WispExecutionContext},
    runtime::WispRuntime,
};

pub fn main(mut wisp: WispContext, device: ConfiguredAudioDevice) -> Result<(), Box<dyn Error>> {
    let execution_context = WispExecutionContext::init();
    let mut runtime = WispRuntime::init(device);

    info!("Switching to server mode - waiting for commands now");
    let input = std::io::stdin();
    let output = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        input.read_line(&mut line)?;
        if line.is_empty() {
            info!("Client disconnected - exiting");
            return Ok(());
        }
        info!("< {}", line.trim_end());
        let command = WispCommand::from_json(&line);
        match command {
            WispCommand::GetSystemInfo => reply(
                &output,
                WispCommandResponse::Ok(SystemInfo {
                    num_channels: wisp.num_outputs(),
                }),
            ),
            WispCommand::DspStart => {
                runtime.start_dsp();
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::DspStop => {
                runtime.stop_dsp();
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::Exit => {
                info!("Exiting");
                reply(&output, WispCommandResponse::Ok(()))?;
                return Ok(());
            }
            WispCommand::ContextReset => {
                wisp.reset();
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextAddOrUpdateFunctions(functions) => {
                for func in functions {
                    wisp.add_function(func);
                }
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextRemoveFunction(name) => {
                wisp.remove_function(&name);
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextSetMainFunction(name) => {
                wisp.set_main_function(&name);
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextSetDataValue(name, id, idx, value) => {
                runtime.set_data_value(name, id, idx.0, value);
                // TODO: Async update
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextSetDataArray(name, id, idx, array_name) => {
                let resp = match wisp.get_data_array(&array_name) {
                    Some(array) => {
                        runtime.set_data_array(name, id, idx.0, array);
                        WispCommandResponse::Ok(())
                    }
                    None => WispCommandResponse::<()>::NonFatalFailure,
                };
                // TODO: Async update
                reply(&output, resp)
            }
            WispCommand::ContextWatchDataValue(name, id, idx) => {
                let idx = runtime.watch_data_value(name, id, idx.0);
                reply(&output, WispCommandResponse::Ok(idx))
            }
            WispCommand::ContextUnwatchDataValue(idx) => {
                runtime.unwatch_data_value(idx);
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextQueryWatchedDataValues => {
                let values = runtime.query_watched_data_values();
                reply(&output, WispCommandResponse::Ok(values))
            }
            WispCommand::ContextLoadWaveFile(name, filepath) => {
                let resp = match wisp.load_wave_file(&name, &filepath) {
                    Ok(()) => WispCommandResponse::Ok(()),
                    Err(e) => {
                        log::error!("Failed to load wave file: {}", e);
                        WispCommandResponse::<()>::NonFatalFailure
                    }
                };
                reply(&output, resp)
            }
            WispCommand::ContextUnloadWaveFile(name) => {
                wisp.unload_wave_file(&name);
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::ContextUpdate => {
                runtime.switch_to_signal_processor(
                    &execution_context,
                    &wisp,
                    wisp.main_function(),
                )?;
                reply(&output, WispCommandResponse::Ok(()))
            }
        }?;
    }
}

fn reply<T>(
    output: &std::io::Stdout,
    response: WispCommandResponse<T>,
) -> Result<(), Box<dyn Error>>
where
    T: CommandResponse,
{
    let mut resp = response.to_json();
    info!("> {}", resp);
    resp.push('\n');
    output.lock().write_all(resp.as_bytes())?;
    Ok(())
}
