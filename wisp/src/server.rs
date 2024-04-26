use std::{error::Error, io::Write};

use log::info;
use twisted_wisp_protocol::{
    self, CommandResponse, FlowNodeIndex, WispCommand, WispCommandResponse,
};

use crate::{
    audio::device::ConfiguredAudioDevice,
    wisp::{
        flow::{Flow, FlowNodeIndex as CoreFlowNodeIndex},
        function::Function,
        WispContext, WispExecutionContext, WispRuntime,
    },
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
            WispCommand::StartDsp => {
                // TODO: Remove this
                wisp.update_all_function_instructions();
                runtime.switch_to_signal_processor(&execution_context, &wisp, "example")?;
                runtime.start_dsp();
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::StopDsp => {
                runtime.stop_dsp();
                reply(&output, WispCommandResponse::Ok(()))
            }
            WispCommand::Exit => {
                info!("Exiting");
                reply(&output, WispCommandResponse::Ok(()))?;
                return Ok(());
            }
            WispCommand::CreateFunction => {
                let mut name;
                let mut idx = 0;
                loop {
                    name = format!("flow_{}", idx);
                    if wisp.get_function(&name).is_some() {
                        break;
                    }
                    idx += 1;
                }
                let func = Function::new_flow(name.clone(), Flow::new());
                wisp.add_function(func);
                reply(&output, WispCommandResponse::Ok(name))
            }
            WispCommand::RemoveFunction(name) => {
                let func = wisp.remove_function(&name);
                let resp = if func.is_some() {
                    WispCommandResponse::Ok(())
                } else {
                    WispCommandResponse::NonFatalFailure
                };
                reply(&output, resp)
            }
            WispCommand::FlowAddNode(flow_name, func_name) => {
                let resp = match wisp.get_flow_mut(&flow_name) {
                    Some(flow) => {
                        let idx = flow.add_node(func_name);
                        WispCommandResponse::Ok(FlowNodeIndex(idx.0.index() as u32))
                    }
                    None => WispCommandResponse::NonFatalFailure,
                };
                reply(&output, resp)
            }
            WispCommand::FlowConnect(flow_name, node_out, node_outlet, node_in, node_inlet) => {
                let resp = match wisp.get_flow_mut(&flow_name) {
                    Some(flow) => {
                        flow.connect(
                            CoreFlowNodeIndex(node_out.0.into()),
                            node_outlet.0,
                            CoreFlowNodeIndex(node_in.0.into()),
                            node_inlet.0,
                        );
                        WispCommandResponse::Ok(())
                    }
                    None => WispCommandResponse::NonFatalFailure,
                };
                reply(&output, resp)
            }
            WispCommand::FlowDisconnect(flow_name, node_out, node_outlet, node_in, node_inlet) => {
                let resp = match wisp.get_flow_mut(&flow_name) {
                    Some(flow) => {
                        flow.disconnect(
                            CoreFlowNodeIndex(node_out.0.into()),
                            node_outlet.0,
                            CoreFlowNodeIndex(node_in.0.into()),
                            node_inlet.0,
                        );
                        WispCommandResponse::Ok(())
                    }
                    None => WispCommandResponse::NonFatalFailure,
                };
                reply(&output, resp)
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
