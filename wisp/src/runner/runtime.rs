use std::collections::HashMap;

use cpal::Stream;
use crossbeam::channel::{Receiver, Sender};
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

use crate::runner::{
    audio::device::ConfiguredAudioDevice,
    compiler::{
        DataArrayHandle, SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder,
    },
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
    pub runtime_tx: Sender<RuntimeStateMessage>,
    pub learn: Option<(String, CallId, DataIndex)>,
}

impl MidiState {
    pub fn new(runtime_tx: Sender<RuntimeStateMessage>) -> Self {
        MidiState {
            mappings: HashMap::new(),
            runtime_tx,
            learn: None,
        }
    }
}

enum MidiStateMessage {
    LearnCC(String, CallId, DataIndex),
}

enum RuntimeStateMessage {
    StartDsp,
    StopDsp,
    SetProcessor(SignalProcessor),
    SetDataValue(String, CallId, DataIndex, f32),
    SetDataArray(String, CallId, DataIndex, DataArrayHandle),
    WatchDataValue(String, CallId, DataIndex, bool),
    UnwatchDataValue(WatchIndex),
    QueryWatchedDataValues,
}

enum SignalProcessorResponse {
    Watch(Option<WatchIndex>),
    WatchData(WatchedDataValues),
}

pub struct WispRuntime<'ectx> {
    _device: ConfiguredAudioDevice,
    _stream: Stream,
    _midi_in_connection: MidiInputConnection<(MidiState, Receiver<MidiStateMessage>)>,
    ee_ref: Option<ExecutionEngine<'ectx>>,
    builder: SignalProcessorBuilder,
    midi_state_tx: Sender<MidiStateMessage>,
    runtime_tx: Sender<RuntimeStateMessage>,
    runtime_result_rx: Receiver<SignalProcessorResponse>,
}

struct RuntimeState {
    processor: Option<SignalProcessor>,
    is_running: bool,
}

impl<'ectx> WispRuntime<'ectx> {
    pub fn init(device: ConfiguredAudioDevice, midi_in: WispMidiIn) -> Self {
        let (runtime_tx, runtime_rx) = crossbeam::channel::bounded(0);
        let (runtime_result_tx, runtime_result_rx) = crossbeam::channel::bounded(0);
        let (midi_state_tx, midi_state_rx) = crossbeam::channel::bounded(0);

        let midi_state = MidiState::new(runtime_tx.clone());
        let midi_in_connection = midi_in
            .midi_in
            .connect(
                &midi_in.port,
                "wisp-midi-in",
                move |_, message, (state, rx)| {
                    if let Ok(message) = rx.try_recv() {
                        match message {
                            MidiStateMessage::LearnCC(name, id, idx) => {
                                state.learn = Some((name, id, idx));
                            }
                        }
                    }

                    match LiveEvent::parse(message) {
                        #[allow(clippy::single_match)]
                        Ok(LiveEvent::Midi { channel, message }) => match message {
                            MidiMessage::Controller { controller, value } => {
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
                                    info!(
                                        "MIDI CC {} on channel {} = {}",
                                        controller, channel, value
                                    );
                                    state
                                        .runtime_tx
                                        .send(RuntimeStateMessage::SetDataValue(
                                            name.to_owned(),
                                            *id,
                                            *idx,
                                            value.as_int() as f32 / 127.0,
                                        ))
                                        .expect("The processor channel is disconnected");
                                }
                            }
                            _ => {}
                        },
                        Ok(_) => {}
                        Err(e) => {
                            error!("Failed to parse MIDI message: {:?}", e);
                        }
                    }
                },
                (midi_state, midi_state_rx),
            )
            .expect("Failed to connect to MIDI port");

        let mut runtime_state = RuntimeState {
            processor: None,
            is_running: false,
        };
        let stream = device
            .build_output_audio_stream(move |_num_outputs: u32, buffer: &mut [f32]| {
                if let Ok(message) = runtime_rx.try_recv() {
                    match message {
                        RuntimeStateMessage::StartDsp => {
                            runtime_state.is_running = true;
                        }
                        RuntimeStateMessage::StopDsp => {
                            runtime_state.is_running = false;
                        }
                        RuntimeStateMessage::SetProcessor(mut sp) => {
                            if let Some(current_sp) = runtime_state.processor.take() {
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
                            }
                            runtime_state.processor = Some(sp);
                        }
                        RuntimeStateMessage::SetDataValue(name, id, idx, value) => {
                            if let Some(sp) = runtime_state.processor.as_mut() {
                                sp.set_data_value(&name, id, idx, value);
                            }
                        }
                        RuntimeStateMessage::SetDataArray(name, id, idx, array) => {
                            if let Some(sp) = runtime_state.processor.as_mut() {
                                sp.set_data_array(&name, id, idx, array);
                            }
                        }
                        RuntimeStateMessage::WatchDataValue(name, id, idx, only_last_value) => {
                            let idx = if let Some(sp) = runtime_state.processor.as_mut() {
                                sp.watch_data_value(&name, id, idx, only_last_value)
                            } else {
                                None
                            };
                            runtime_result_tx.send(SignalProcessorResponse::Watch(idx)).unwrap();
                        }
                        RuntimeStateMessage::UnwatchDataValue(index) => {
                            if let Some(sp) = runtime_state.processor.as_mut() {
                                sp.unwatch_data_value(index);
                            }
                        }
                        RuntimeStateMessage::QueryWatchedDataValues => {
                            let values = if let Some(sp) = runtime_state.processor.as_mut() {
                                sp.query_watched_data_value()
                            } else {
                                WatchedDataValues::default()
                            };
                            runtime_result_tx.send(SignalProcessorResponse::WatchData(values)).unwrap();
                        }
                    }
                }

                if runtime_state.is_running {
                    if let Some(sp) = runtime_state.processor.as_mut() {
                        sp.process(buffer);
                        // Clip the output to safe levels
                        for b in buffer.iter_mut() {
                            if b.is_nan() {
                                *b = 0.0;
                            } else {
                                *b = b.clamp(-1.0, 1.0);
                            }
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
            midi_state_tx,
            runtime_tx,
            runtime_result_rx,
        }
    }

    pub fn start_dsp(&mut self) {
        self.runtime_tx
            .send(RuntimeStateMessage::StartDsp)
            .expect("The processor channel is disconnected");
    }

    pub fn stop_dsp(&mut self) {
        self.runtime_tx
            .send(RuntimeStateMessage::StopDsp)
            .expect("The processor channel is disconnected");
    }

    pub fn switch_to_signal_processor(
        &mut self,
        ectx: &'ectx WispExecutionContext,
        ctx: &WispContext,
        top_level: &str,
    ) -> Result<(), SignalProcessCreationError> {
        let (sp, ee) = self.builder.create_signal_processor(ectx, ctx, top_level)?;
        self.runtime_tx
            .send(RuntimeStateMessage::SetProcessor(sp))
            .expect("The processor channel is disconnected");
        self.ee_ref = Some(ee);
        Ok(())
    }

    pub fn set_data_value(&mut self, name: &str, id: CallId, idx: DataIndex, value: f32) {
        self.runtime_tx
            .send(RuntimeStateMessage::SetDataValue(
                name.to_owned(),
                id,
                idx,
                value,
            ))
            .expect("The processor channel is disconnected");
    }

    pub fn set_data_array(
        &mut self,
        name: &str,
        id: CallId,
        idx: DataIndex,
        array: DataArrayHandle,
    ) {
        self.runtime_tx
            .send(RuntimeStateMessage::SetDataArray(
                name.to_owned(),
                id,
                idx,
                array,
            ))
            .expect("The processor channel is disconnected");
    }

    pub fn learn_midi_cc(&mut self, name: &str, id: CallId, idx: DataIndex) {
        self.midi_state_tx
            .send(MidiStateMessage::LearnCC(name.to_owned(), id, idx))
            .expect("The MIDI channel is disconnected");
    }

    pub fn watch_data_value(
        &mut self,
        name: &str,
        id: CallId,
        idx: DataIndex,
        only_last_value: bool,
    ) -> Option<WatchIndex> {
        self.runtime_tx
            .send(RuntimeStateMessage::WatchDataValue(
                name.to_owned(),
                id,
                idx,
                only_last_value,
            ))
            .expect("The processor channel is disconnected");
        match self.runtime_result_rx.recv().unwrap() {
            SignalProcessorResponse::Watch(idx) => idx,
            _ => unreachable!(),
        }
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        self.runtime_tx
            .send(RuntimeStateMessage::UnwatchDataValue(idx))
            .expect("The processor channel is disconnected");
    }

    pub fn query_watched_data_values(&mut self) -> WatchedDataValues {
        self.runtime_tx
            .send(RuntimeStateMessage::QueryWatchedDataValues)
            .expect("The processor channel is disconnected");
        match self.runtime_result_rx.recv().unwrap() {
            SignalProcessorResponse::WatchData(values) => values,
            _ => unreachable!(),
        }
    }
}
