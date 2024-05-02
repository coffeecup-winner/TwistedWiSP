use std::{
    borrow::BorrowMut,
    sync::{Arc, Mutex},
};

use cpal::Stream;
use inkwell::execution_engine::ExecutionEngine;
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{WatchIndex, WatchedDataValues};

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::{SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    context::{WispContext, WispExecutionContext},
};

pub struct WispRuntime<'ectx> {
    _device: ConfiguredAudioDevice,
    _stream: Stream,
    ee_ref: Option<ExecutionEngine<'ectx>>,
    builder: SignalProcessorBuilder,
    processor_mutex: Arc<Mutex<Option<SignalProcessor>>>,
    paused_processor: Option<(SignalProcessor, ExecutionEngine<'ectx>)>,
}

impl<'ectx> WispRuntime<'ectx> {
    pub fn init(device: ConfiguredAudioDevice) -> Self {
        let processor_mutex: Arc<Mutex<Option<SignalProcessor>>> = Arc::new(Mutex::new(None));
        let mut processor_mutex_audio_thread = processor_mutex.clone();
        let stream = device
            .build_output_audio_stream(move |_num_outputs: u32, buffer: &mut [f32]| {
                if let Some(sp) = processor_mutex_audio_thread
                    .borrow_mut()
                    .lock()
                    .unwrap()
                    .as_mut()
                {
                    sp.process(buffer);
                    // Clip the output to safe levels
                    for b in buffer.iter_mut() {
                        if b.is_nan() {
                            *b = 0.0;
                        } else {
                            *b = b.clamp(-1.0, 1.0);
                        }
                    }
                } else {
                    // Silence if no signal processor
                    for b in buffer.iter_mut() {
                        *b = 0.0;
                    }
                }
            })
            .expect("Failed to create an audio stream");

        WispRuntime {
            _device: device,
            _stream: stream,
            ee_ref: None,
            builder: SignalProcessorBuilder::new(),
            processor_mutex,
            paused_processor: None,
        }
    }

    pub fn start_dsp(&mut self) {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_none() {
            let mut temp = None;
            std::mem::swap(&mut self.paused_processor, &mut temp);
            if let Some((sp, ee)) = temp {
                *running_processor = Some(sp);
                self.ee_ref = Some(ee);
            }
        }
    }

    pub fn stop_dsp(&mut self) {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            self.paused_processor = None;
            let mut temp = None;
            std::mem::swap(&mut *running_processor, &mut temp);
            self.paused_processor = Some((temp.unwrap(), self.ee_ref.take().unwrap()));
        }
    }

    pub fn switch_to_signal_processor(
        &mut self,
        ectx: &'ectx WispExecutionContext,
        ctx: &WispContext,
        top_level: &str,
    ) -> Result<(), SignalProcessCreationError> {
        let (sp, ee) = self.builder.create_signal_processor(ectx, ctx, top_level)?;
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            *running_processor = Some(sp);
            self.ee_ref = Some(ee);
        } else {
            self.paused_processor = Some((sp, ee));
        }
        Ok(())
    }

    pub fn set_data_value(&mut self, name: String, id: CallId, idx: u32, value: f32) {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            running_processor
                .as_mut()
                .unwrap()
                .set_data_value(name, id, idx, value);
        } else if let Some(paused_processor) = self.paused_processor.as_mut() {
            paused_processor.0.set_data_value(name, id, idx, value);
        }
    }

    pub fn watch_data_value(&mut self, name: String, id: CallId, idx: u32) -> Option<WatchIndex> {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            running_processor
                .as_mut()
                .unwrap()
                .watch_data_value(name, id, idx)
        } else if let Some(paused_processor) = self.paused_processor.as_mut() {
            paused_processor.0.watch_data_value(name, id, idx)
        } else {
            None
        }
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            running_processor.as_mut().unwrap().unwatch_data_value(idx);
        } else if let Some(paused_processor) = self.paused_processor.as_mut() {
            paused_processor.0.unwatch_data_value(idx);
        }
    }

    pub fn query_watched_data_values(&mut self) -> WatchedDataValues {
        let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
        if running_processor.is_some() {
            running_processor
                .as_mut()
                .unwrap()
                .query_watched_data_value()
        } else if let Some(paused_processor) = self.paused_processor.as_mut() {
            paused_processor.0.query_watched_data_value()
        } else {
            WatchedDataValues::default()
        }
    }
}
