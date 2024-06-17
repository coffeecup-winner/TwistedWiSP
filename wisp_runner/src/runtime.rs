use std::{
    borrow::BorrowMut,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use cpal::Stream;
use inkwell::execution_engine::ExecutionEngine;
use log::{error, info};
use midir::MidiInputConnection;
use midly::{
    live::LiveEvent,
    num::{u4, u7},
    MidiMessage,
};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{DataIndex, WatchIndex, WatchedDataValues};

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::{DataArray, SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    context::{WispContext, WispExecutionContext},
    midi::WispMidiIn,
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
struct MidiCC {
    pub channel: u4,
    pub controller: u7,
}

struct MidiState {
    pub mappings: HashMap<MidiCC, (String, CallId, DataIndex)>,
    pub processor_mutex: Arc<Mutex<Option<SignalProcessor>>>,
    pub learn: Option<(String, CallId, DataIndex)>,
}

impl MidiState {
    pub fn new(processor_mutex: Arc<Mutex<Option<SignalProcessor>>>) -> Self {
        MidiState {
            mappings: HashMap::new(),
            processor_mutex,
            learn: None,
        }
    }
}

pub struct WispRuntime<'ectx> {
    _device: ConfiguredAudioDevice,
    _stream: Stream,
    _midi_in_connection: MidiInputConnection<Arc<Mutex<MidiState>>>,
    ee_ref: Option<ExecutionEngine<'ectx>>,
    builder: SignalProcessorBuilder,
    midi_state_mutex: Arc<Mutex<MidiState>>,
    processor_mutex: Arc<Mutex<Option<SignalProcessor>>>,
    state: RuntimeState<'ectx>,
}

enum RuntimeState<'ectx> {
    Running,
    Stopped(Option<Box<(SignalProcessor, ExecutionEngine<'ectx>)>>),
}

impl<'ectx> WispRuntime<'ectx> {
    pub fn init(device: ConfiguredAudioDevice, midi_in: WispMidiIn) -> Self {
        let processor_mutex: Arc<Mutex<Option<SignalProcessor>>> = Arc::new(Mutex::new(None));
        let midi_in_mutex = Arc::new(Mutex::new(MidiState::new(processor_mutex.clone())));

        let midi_in_connection = midi_in
            .midi_in
            .connect(
                &midi_in.port,
                "wisp-midi-in",
                move |_, message, state| match LiveEvent::parse(message) {
                    #[allow(clippy::single_match)]
                    Ok(LiveEvent::Midi { channel, message }) => match message {
                        MidiMessage::Controller { controller, value } => {
                            let mut state = state.lock().unwrap();
                            if let Some((name, id, idx)) = state.learn.take() {
                                info!(
                                    "Learned MIDI CC {} on channel {} => ({}, {}, {})",
                                    controller, channel, name, id.0, idx.0
                                );
                                state.mappings.insert(
                                    MidiCC {
                                        channel,
                                        controller,
                                    },
                                    (name, id, idx),
                                );
                            }
                            if let Some((name, id, idx)) = state.mappings.get(&MidiCC {
                                channel,
                                controller,
                            }) {
                                info!("MIDI CC {} on channel {} = {}", controller, channel, value);
                                if let Some(sp) = state.processor_mutex.lock().unwrap().as_mut() {
                                    sp.set_data_value(
                                        name,
                                        *id,
                                        *idx,
                                        value.as_int() as f32 / 127.0,
                                    );
                                };
                            }
                        }
                        _ => {}
                    },
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to parse MIDI message: {:?}", e);
                    }
                },
                midi_in_mutex.clone(),
            )
            .expect("Failed to connect to MIDI port");

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
            _midi_in_connection: midi_in_connection,
            ee_ref: None,
            builder: SignalProcessorBuilder::new(),
            midi_state_mutex: midi_in_mutex,
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

    pub fn set_data_value(&mut self, name: &str, id: CallId, idx: DataIndex, value: f32) {
        self.with_processor_mut_do(|sp| sp.set_data_value(name, id, idx, value));
    }

    pub fn set_data_array(
        &mut self,
        name: &str,
        id: CallId,
        idx: DataIndex,
        array: *mut DataArray,
    ) {
        self.with_processor_mut_do(|sp| sp.set_data_array(name, id, idx, array));
    }

    pub fn learn_midi_cc(&mut self, name: &str, id: CallId, idx: DataIndex) {
        self.midi_state_mutex.lock().unwrap().learn = Some((name.to_owned(), id, idx));
    }

    pub fn watch_data_value(
        &mut self,
        name: &str,
        id: CallId,
        idx: DataIndex,
        only_last_value: bool,
    ) -> Option<WatchIndex> {
        self.with_processor_mut_do(|sp| sp.watch_data_value(name, id, idx, only_last_value))
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        self.with_processor_mut_do(|sp| sp.unwatch_data_value(idx));
    }

    pub fn query_watched_data_values(&mut self) -> WatchedDataValues {
        self.with_processor_mut_do(|sp| sp.query_watched_data_value())
    }
}
