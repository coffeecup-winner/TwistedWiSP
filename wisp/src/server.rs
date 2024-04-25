use std::{
    borrow::BorrowMut,
    error::Error,
    io::Write,
    sync::{Arc, Mutex},
};

use twisted_wisp_protocol::{self, WispCommand, WispCommandResponse};

use crate::{audio::device::ConfiguredAudioDevice, wisp::SignalProcessor};

pub fn main(
    wisp: crate::wisp::WispContext,
    device: ConfiguredAudioDevice,
) -> Result<(), Box<dyn Error>> {
    let mut processor_mutex = Arc::new(Mutex::new(wisp.create_empty_signal_processor()));
    let mut processor_mutex_audio_thread = processor_mutex.clone();
    let _stream = device
        .build_output_audio_stream(move |_num_outputs: u32, buffer: &mut [f32]| {
            processor_mutex_audio_thread
                .borrow_mut()
                .lock()
                .unwrap()
                .process(buffer);
            // Clip the output to the safe levels
            for b in buffer.iter_mut() {
                *b = b.clamp(-1.0, 1.0);
            }
        })
        .expect("msg");

    // TODO: Temp code - add (and then remove)
    // let (sp, _ee) = wisp.create_signal_processor("example")?;
    let mut paused_processor: Option<SignalProcessor> = None;

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
                if let Some(processor) = paused_processor {
                    *processor_mutex.borrow_mut().lock().unwrap() = processor;
                    paused_processor = None;
                }
                WispCommandResponse::Ok
            }
            WispCommand::StopDsp => {
                if paused_processor.is_none() {
                    let mut temp = wisp.create_empty_signal_processor();
                    std::mem::swap(&mut temp, &mut processor_mutex.borrow_mut().lock().unwrap());
                    paused_processor = Some(temp);
                }
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
