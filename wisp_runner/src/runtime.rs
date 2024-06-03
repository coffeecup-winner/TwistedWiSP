use std::{
    borrow::BorrowMut,
    sync::{Arc, Mutex},
};

use cpal::Stream;
use inkwell::execution_engine::ExecutionEngine;
use log::info;
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{WatchIndex, WatchedDataValues};

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::{DataArray, SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    context::{WispContext, WispExecutionContext},
};

pub struct WispRuntime<'ectx> {
    _device: ConfiguredAudioDevice,
    _stream: Stream,
    ee_ref: Option<ExecutionEngine<'ectx>>,
    builder: SignalProcessorBuilder,
    processor_mutex: Arc<Mutex<Option<SignalProcessor>>>,
    state: RuntimeState<'ectx>,
}

enum RuntimeState<'ectx> {
    Running,
    Stopped(Option<Box<(SignalProcessor, ExecutionEngine<'ectx>)>>),
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
            state: RuntimeState::Stopped(None),
        }
    }

    pub fn start_dsp(&mut self) {
        match self.state {
            RuntimeState::Running => (),
            RuntimeState::Stopped(ref mut stopped_sp) => {
                if let Some(pair) = stopped_sp.take() {
                    *self.processor_mutex.borrow_mut().lock().unwrap() = Some(pair.0);
                    self.ee_ref = Some(pair.1);
                }
                self.state = RuntimeState::Running;
            }
        }
    }

    pub fn stop_dsp(&mut self) {
        match self.state {
            RuntimeState::Running => {
                let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
                let stopped_processor = running_processor
                    .take()
                    .map(|sp| Box::new((sp, self.ee_ref.take().unwrap())));
                self.state = RuntimeState::Stopped(stopped_processor);
            }
            RuntimeState::Stopped(_) => (),
        }
    }

    fn with_processor_take_do<T>(&mut self, f: impl FnOnce(SignalProcessor) -> T) -> T
    where
        T: Default,
    {
        match self.state {
            RuntimeState::Running => {
                let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
                if let Some(current_sp) = running_processor.take() {
                    f(current_sp)
                } else {
                    T::default()
                }
            }
            RuntimeState::Stopped(ref mut stopped_sp) => {
                if let Some(stopped_sp) = stopped_sp.take() {
                    f(stopped_sp.0)
                } else {
                    T::default()
                }
            }
        }
    }

    fn with_processor_mut_do<T>(&mut self, f: impl FnOnce(&mut SignalProcessor) -> T) -> T
    where
        T: Default,
    {
        match self.state {
            RuntimeState::Running => {
                let mut running_processor = self.processor_mutex.borrow_mut().lock().unwrap();
                if let Some(current_sp) = running_processor.as_mut() {
                    f(current_sp)
                } else {
                    T::default()
                }
            }
            RuntimeState::Stopped(ref mut stopped_sp) => {
                if let Some(stopped_sp) = stopped_sp.as_mut() {
                    f(&mut stopped_sp.0)
                } else {
                    T::default()
                }
            }
        }
    }

    pub fn switch_to_signal_processor(
        &mut self,
        ectx: &'ectx WispExecutionContext,
        ctx: &WispContext,
        top_level: &str,
    ) -> Result<(), SignalProcessCreationError> {
        let (mut sp, ee) = self.builder.create_signal_processor(ectx, ctx, top_level)?;
        self.with_processor_take_do(|current_sp| {
            if sp.name() == current_sp.name() {
                info!("Copying data from the current signal processor");
                sp.copy_from(current_sp);
            } else {
                info!(
                    "Not copying data from the current signal processor ({} != {})",
                    sp.name(),
                    current_sp.name()
                );
            }
        });
        match self.state {
            RuntimeState::Running => {
                *self.processor_mutex.borrow_mut().lock().unwrap() = Some(sp);
                self.ee_ref = Some(ee);
            }
            RuntimeState::Stopped(ref mut stopped_sp) => {
                *stopped_sp = Some(Box::new((sp, ee)));
            }
        }
        Ok(())
    }

    pub fn set_data_value(&mut self, name: String, id: CallId, idx: u32, value: f32) {
        self.with_processor_mut_do(|sp| sp.set_data_value(name, id, idx, value));
    }

    pub fn set_data_array(&mut self, name: String, id: CallId, idx: u32, array: *mut DataArray) {
        self.with_processor_mut_do(|sp| sp.set_data_array(name, id, idx, array));
    }

    pub fn watch_data_value(&mut self, name: String, id: CallId, idx: u32) -> Option<WatchIndex> {
        self.with_processor_mut_do(|sp| sp.watch_data_value(name, id, idx))
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        self.with_processor_mut_do(|sp| sp.unwatch_data_value(idx));
    }

    pub fn query_watched_data_values(&mut self) -> WatchedDataValues {
        self.with_processor_mut_do(|sp| sp.query_watched_data_value())
    }
}
