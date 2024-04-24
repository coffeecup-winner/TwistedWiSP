use std::{
    borrow::BorrowMut,
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::audio::device::ConfiguredAudioDevice;

pub fn main(
    wisp: crate::wisp::WispContext,
    device: ConfiguredAudioDevice,
) -> Result<(), Box<dyn Error>> {
    let mut processor_mutex = Arc::new(Mutex::new(wisp.create_empty_signal_processor()));
    let _stream = device
        .build_output_audio_stream(move |_num_outputs: u32, buffer: &mut [f32]| {
            processor_mutex.borrow_mut().lock().unwrap().process(buffer);
            // Clip the output to the safe levels
            for b in buffer.iter_mut() {
                *b = b.clamp(-1.0, 1.0);
            }
        })
        .expect("msg");

    loop {
        std::thread::sleep(Duration::from_millis(50));
        // Wait until Ctrl+C
    }
}
