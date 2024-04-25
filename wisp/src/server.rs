use std::{error::Error, io::Write};

use twisted_wisp_protocol::{self, WispCommand, WispCommandResponse};

use crate::{
    audio::device::ConfiguredAudioDevice,
    wisp::{WispContext, WispExecutionContext, WispRuntime},
};

pub fn main(wisp: WispContext, device: ConfiguredAudioDevice) -> Result<(), Box<dyn Error>> {
    let execution_context = WispExecutionContext::init();
    let mut runtime = WispRuntime::init(device);

    eprintln!("Switching to server mode - waiting for commands now");
    let input = std::io::stdin();
    let output = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        input.read_line(&mut line)?;
        if line.is_empty() {
            eprintln!("Client disconnected - exiting");
            return Ok(());
        }
        eprint!("< {}", line);
        let command = WispCommand::from_json(&line);
        let response = match command {
            WispCommand::StartDsp => {
                // TODO: Remove this
                runtime.switch_to_signal_processor(&execution_context, &wisp, "example")?;
                runtime.start_dsp();
                WispCommandResponse::Ok
            }
            WispCommand::StopDsp => {
                runtime.stop_dsp();
                WispCommandResponse::Ok
            }
            WispCommand::Exit => {
                eprintln!("Exiting");
                return Ok(());
            }
        };
        let mut resp = response.to_json();
        resp.push('\n');
        eprint!("> {}", resp);
        output.lock().write_all(resp.as_bytes())?;
    }
}
