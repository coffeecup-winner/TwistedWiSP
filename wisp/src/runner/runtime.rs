use super::engine::{DataIndex, WatchIndex, WatchedDataValues};
use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::{
        DataArrayHandle, SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder,
    },
    core::{WispContext, WispFunction},
    midi::WispMidiIn,
    runner::context::WispRuntimeContext,
    CallIndex,
};
use cpal::{traits::StreamTrait, Stream};
use crossbeam::channel::{bounded, Receiver, Sender};
use inkwell::{
    context::Context,
    types::{FloatType, IntType, PointerType, VoidType},
    AddressSpace,
};
use midir::MidiInputConnection;
use midly::{live::LiveEvent, MidiMessage};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

enum RuntimeStateMessage {
    StartDsp,
    StopDsp,
    SetProcessor(SignalProcessor),
    SetDataValue(String, CallIndex, DataIndex, f32),
    SetDataArray(String, CallIndex, DataIndex, DataArrayHandle),
    WatchDataValue(String, CallIndex, DataIndex, bool),
    UnwatchDataValue(WatchIndex),
    QueryWatchedDataValues,
}

enum SignalProcessorResponse {
    Ack,
    Installed(Option<SignalProcessor>),
    Watch(Option<WatchIndex>),
    WatchData(WatchedDataValues),
}

struct RuntimeRequest {
    message: RuntimeStateMessage,
    reply: Option<Sender<Result<SignalProcessorResponse, String>>>,
}

struct RuntimeTransport {
    tx: Sender<RuntimeRequest>,
    failed: Arc<AtomicBool>,
}

impl RuntimeTransport {
    fn request(&self, message: RuntimeStateMessage) -> Result<SignalProcessorResponse, String> {
        self.request_timeout(message, Duration::from_millis(250))
    }

    fn request_timeout(
        &self,
        message: RuntimeStateMessage,
        timeout: Duration,
    ) -> Result<SignalProcessorResponse, String> {
        if self.failed.load(Ordering::Acquire) {
            return Err("Audio device failed; stop and retry audio initialization".into());
        }
        let (reply, rx) = bounded(1);
        self.tx
            .send_timeout(
                RuntimeRequest {
                    message,
                    reply: Some(reply),
                },
                timeout,
            )
            .map_err(|_| "Audio command queue unavailable or full".to_string())?;
        rx.recv_timeout(timeout)
            .map_err(|_| "Audio callback did not acknowledge the request".to_string())?
    }
}

struct RuntimeState {
    processor: Option<SignalProcessor>,
    is_running: bool,
}

impl RuntimeState {
    fn apply(&mut self, message: RuntimeStateMessage) -> Result<SignalProcessorResponse, String> {
        match message {
            RuntimeStateMessage::StartDsp => {
                if self.processor.is_none() {
                    return Err("No processor installed".into());
                }
                self.is_running = true;
            }
            RuntimeStateMessage::StopDsp => self.is_running = false,
            RuntimeStateMessage::SetProcessor(sp) => {
                return Ok(SignalProcessorResponse::Installed(
                    self.processor.replace(sp),
                ))
            }
            RuntimeStateMessage::SetDataValue(name, call, data, value) => {
                if !value.is_finite() {
                    return Err("Nonfinite control value".into());
                }
                self.processor
                    .as_mut()
                    .and_then(|p| p.set_data_value(&name, call, data, value))
                    .ok_or("Invalid scalar address or no processor")?;
            }
            RuntimeStateMessage::SetDataArray(name, call, data, array) => {
                self.processor
                    .as_mut()
                    .and_then(|p| p.set_data_array(&name, call, data, array))
                    .ok_or("Invalid array address or no processor")?;
            }
            RuntimeStateMessage::WatchDataValue(name, call, data, last) => {
                return Ok(SignalProcessorResponse::Watch(
                    self.processor
                        .as_mut()
                        .and_then(|p| p.watch_data_value(&name, call, data, last)),
                ))
            }
            RuntimeStateMessage::UnwatchDataValue(id) => {
                if let Some(p) = &mut self.processor {
                    p.unwatch_data_value(id);
                }
            }
            RuntimeStateMessage::QueryWatchedDataValues => {
                return Ok(SignalProcessorResponse::WatchData(
                    self.processor
                        .as_mut()
                        .map(|p| p.query_watched_data_value())
                        .unwrap_or_default(),
                ))
            }
        }
        Ok(SignalProcessorResponse::Ack)
    }

    fn drain(&mut self, rx: &Receiver<RuntimeRequest>) {
        for _ in 0..64 {
            let Ok(request) = rx.try_recv() else {
                break;
            };
            let response = self.apply(request.message);
            if let Some(reply) = request.reply {
                let _ = reply.try_send(response);
            }
        }
    }

    fn render(&mut self, buffer: &mut [f32]) {
        buffer.fill(0.0);
        if self.is_running {
            if let Some(p) = &mut self.processor {
                p.process_all(buffer);
                for value in buffer {
                    *value = if value.is_finite() {
                        value.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    };
                }
            }
        }
    }
}

pub struct WispExecutionContext {
    context: Context,
}

impl WispExecutionContext {
    pub fn init() -> Self {
        WispExecutionContext {
            context: Context::create(),
        }
    }

    pub fn llvm(&self) -> &Context {
        &self.context
    }

    pub fn void_t(&self) -> VoidType<'_> {
        self.context.void_type()
    }

    pub fn i32_t(&self) -> IntType<'_> {
        self.context.i32_type()
    }

    pub fn f32_t(&self) -> FloatType<'_> {
        self.context.f32_type()
    }

    pub fn pf32_t(&self) -> PointerType<'_> {
        self.context.ptr_type(AddressSpace::default())
    }

    // Data-wide type
    pub fn data_t(&self) -> IntType<'_> {
        self.context.i64_type()
    }
}

/// A fresh LLVM context is transferred into each compiled artifact's owner.
/// No context is shared between compilation and another processor's destruction.
pub(crate) fn compile_context(
    ctx: &WispContext,
    rctx: &mut WispRuntimeContext,
    root: &str,
) -> Result<SignalProcessor, SignalProcessCreationError> {
    ctx.validate_function(root)
        .map_err(SignalProcessCreationError::CustomLogicalError)?;
    *rctx = WispRuntimeContext::new();
    let mut pending = vec![root.to_owned()];
    let mut seen = HashSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let function = ctx
            .get_function(&name)
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.clone()))?;
        for ir in function.borrow().get_ir_functions(ctx) {
            pending.extend(crate::compiler::dependencies(&ir));
            rctx.add_function(ir);
        }
    }
    // Inkwell 0.5 reinitializes MCJIT's global native target on every build.
    // Serialize code generation across independent offline/live hosts, never audio processing.
    static COMPILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _compile_guard = COMPILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ectx = WispExecutionContext::init();
    let mut processor =
        SignalProcessorBuilder::new().build_signal_processor(ctx, &ectx, rctx, root)?;
    processor.retain_execution_context(ectx);
    Ok(processor)
}

pub struct WispRuntime {
    // Field order is deliberate: stop/drop callback before any other resources.
    _stream: Stream,
    _midi: Option<MidiInputConnection<()>>,
    _device: ConfiguredAudioDevice,
    midi: Arc<crate::MidiMailbox>,
    legacy_midi: HashMap<u64, (String, CallIndex, DataIndex)>,
    legacy_pending: HashMap<u64, f32>,
    transport: RuntimeTransport,
}

impl WispRuntime {
    pub fn init(
        device: ConfiguredAudioDevice,
        midi: Option<WispMidiIn>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = bounded(64);
        let mailbox = Arc::new(crate::MidiMailbox::default());
        let failed = Arc::new(AtomicBool::new(false));
        let stream_failed = failed.clone();
        let mut state = RuntimeState {
            processor: None,
            is_running: false,
        };
        let stream = device.build_output_audio_stream(
            move |_, buffer| {
                state.drain(&rx);
                state.render(buffer);
            },
            move |_| {
                stream_failed.store(true, Ordering::Release);
            },
        )?;
        // The callback must consume commands even when DSP is stopped.
        stream.play()?;
        let midi_connection = midi.and_then(|midi| {
            let mailbox = mailbox.clone();
            match midi.midi_in.connect(
                &midi.port,
                "wisp-midi-in",
                move |_, bytes, _| {
                    if let Ok(LiveEvent::Midi {
                        channel,
                        message: MidiMessage::Controller { controller, value },
                    }) = LiveEvent::parse(bytes)
                    {
                        mailbox.cc(channel.as_int(), controller.as_int(), value.as_int());
                    }
                },
                (),
            ) {
                Ok(connection) => Some(connection),
                Err(e) => {
                    log::warn!("MIDI unavailable: {e}");
                    None
                }
            }
        });
        Ok(Self {
            _stream: stream,
            _midi: midi_connection,
            _device: device,
            midi: mailbox,
            legacy_midi: HashMap::new(),
            legacy_pending: HashMap::new(),
            transport: RuntimeTransport { tx, failed },
        })
    }

    pub fn healthy(&self) -> bool {
        !self.transport.failed.load(Ordering::Acquire)
    }

    pub fn start_checked(&self) -> Result<(), String> {
        self.transport
            .request(RuntimeStateMessage::StartDsp)
            .map(|_| ())
    }

    pub fn stop_checked(&self) -> Result<(), String> {
        self.transport
            .request(RuntimeStateMessage::StopDsp)
            .map(|_| ())
    }

    pub fn install_checked(&self, sp: SignalProcessor) -> Result<(), String> {
        match self
            .transport
            .request(RuntimeStateMessage::SetProcessor(sp))?
        {
            // Destruction of the retired processor occurs on the service thread.
            SignalProcessorResponse::Installed(old) => {
                drop(old);
                Ok(())
            }
            _ => Err("Unexpected install reply".into()),
        }
    }

    pub fn scalar_checked(
        &self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        value: f32,
    ) -> Result<(), String> {
        self.transport
            .request(RuntimeStateMessage::SetDataValue(
                name.into(),
                call,
                data,
                value,
            ))
            .map(|_| ())
    }

    pub fn start_dsp(&mut self) {
        if let Err(e) = self.start_checked() {
            log::error!("{e}");
        }
    }

    pub fn stop_dsp(&mut self) {
        if let Err(e) = self.stop_checked() {
            log::error!("{e}");
        }
    }

    pub fn switch_to_signal_processor(&mut self, sp: SignalProcessor) {
        if let Err(e) = self.install_checked(sp) {
            log::error!("{e}");
        }
    }

    pub fn compile(
        &mut self,
        ctx: &WispContext,
        rctx: &mut WispRuntimeContext,
        root: &str,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        compile_context(ctx, rctx, root)
    }

    pub fn set_data_value(&mut self, name: &str, call: CallIndex, data: DataIndex, value: f32) {
        if let Err(e) = self.scalar_checked(name, call, data, value) {
            log::error!("{e}");
        }
    }

    pub fn set_data_array(
        &mut self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        array: DataArrayHandle,
    ) {
        if let Err(e) = self.transport.request(RuntimeStateMessage::SetDataArray(
            name.into(),
            call,
            data,
            array,
        )) {
            log::error!("{e}");
        }
    }

    pub fn midi_available(&self) -> bool {
        self._midi.is_some()
    }
    pub fn midi_learn(&self, target: u64) -> Result<(), String> {
        if !self.midi_available() {
            return Err("MIDI unavailable".into());
        }
        if self.midi.learn(target) {
            Ok(())
        } else {
            Err("MIDI target limit (128) or invalid token".into())
        }
    }
    pub fn midi_targets(&self) -> Vec<u64> {
        self.midi.targets()
    }
    pub fn midi_cancel(&self) {
        self.midi.cancel();
    }
    pub fn midi_remove(&self, target: u64) {
        self.midi.remove(target);
    }
    pub fn midi_poll(&self) -> Vec<crate::MidiObservation> {
        self.midi.poll()
    }
    pub fn scalar_try(
        &self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        value: f32,
    ) -> Result<(), String> {
        if !self.healthy() {
            return Err("Audio device failed".into());
        }
        self.transport
            .tx
            .try_send(RuntimeRequest {
                message: RuntimeStateMessage::SetDataValue(name.into(), call, data, value),
                reply: None,
            })
            .map_err(|_| "Audio queue full".into())
    }
    pub fn learn_midi_cc(&mut self, name: &str, call: CallIndex, data: DataIndex) {
        // Legacy callers already poll watches; use the same mailbox, with reserved tokens.
        if self.legacy_midi.len() >= 128 {
            return;
        }
        let token = (1u64 << 55) | (self.legacy_midi.len() as u64 + 1);
        self.legacy_midi.insert(token, (name.into(), call, data));
        let _ = self.midi_learn(token);
    }

    pub fn watch_checked(
        &self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        last: bool,
    ) -> Result<WatchIndex, String> {
        match self.transport.request(RuntimeStateMessage::WatchDataValue(
            name.into(),
            call,
            data,
            last,
        ))? {
            SignalProcessorResponse::Watch(Some(id)) => Ok(id),
            _ => Err("Invalid watch or watch limit (128)".into()),
        }
    }
    pub fn query_checked(&self) -> Result<WatchedDataValues, String> {
        match self
            .transport
            .request(RuntimeStateMessage::QueryWatchedDataValues)?
        {
            SignalProcessorResponse::WatchData(data) => Ok(data),
            _ => Err("Unexpected watch reply".into()),
        }
    }
    pub fn watch_data_value(
        &mut self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        last: bool,
    ) -> Option<WatchIndex> {
        match self
            .transport
            .request(RuntimeStateMessage::WatchDataValue(
                name.into(),
                call,
                data,
                last,
            ))
            .ok()?
        {
            SignalProcessorResponse::Watch(id) => id,
            _ => None,
        }
    }

    pub fn unwatch_data_value(&mut self, id: WatchIndex) {
        let _ = self
            .transport
            .request(RuntimeStateMessage::UnwatchDataValue(id));
    }

    pub fn query_watched_data_values(&mut self) -> WatchedDataValues {
        for v in self.midi_poll() {
            if self.legacy_midi.contains_key(&v.target) {
                self.legacy_pending.insert(v.target, v.value);
            }
        }
        self.legacy_pending.retain(|token, value| {
            let Some((name, call, data)) = self.legacy_midi.get(token) else {
                return false;
            };
            self.transport
                .tx
                .try_send(RuntimeRequest {
                    message: RuntimeStateMessage::SetDataValue(name.clone(), *call, *data, *value),
                    reply: None,
                })
                .is_err()
        });
        match self
            .transport
            .request(RuntimeStateMessage::QueryWatchedDataValues)
        {
            Ok(SignalProcessorResponse::WatchData(data)) => data,
            _ => WatchedDataValues::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_and_disconnected_transports_fail_within_deadline() {
        let (tx, rx) = bounded(1);
        let transport = RuntimeTransport {
            tx,
            failed: Arc::new(AtomicBool::new(false)),
        };
        transport
            .tx
            .try_send(RuntimeRequest {
                message: RuntimeStateMessage::StopDsp,
                reply: None,
            })
            .unwrap();
        assert!(
            matches!(transport.request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2)), Err(e) if e.contains("queue"))
        );
        drop(rx);
        assert!(transport
            .request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2))
            .is_err());
    }

    #[test]
    fn transport_times_out_without_callback_and_replies_do_not_cross() {
        let (tx, rx) = bounded(1);
        let transport = RuntimeTransport {
            tx,
            failed: Arc::new(AtomicBool::new(false)),
        };
        assert!(transport
            .request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2))
            .is_err());
        let mut state = RuntimeState {
            processor: None,
            is_running: false,
        };
        state.drain(&rx); // abandoned response must not wait
        let (reply, response) = bounded(1);
        transport
            .tx
            .try_send(RuntimeRequest {
                message: RuntimeStateMessage::StartDsp,
                reply: Some(reply),
            })
            .unwrap();
        state.drain(&rx);
        assert!(response.try_recv().unwrap().is_err());
        transport.failed.store(true, Ordering::Release);
        assert!(transport.request(RuntimeStateMessage::StopDsp).is_err());
    }

    #[test]
    fn stopped_callback_consumes_commands_and_silences_output() {
        let (tx, rx) = bounded(1);
        let (reply, response) = bounded(1);
        tx.try_send(RuntimeRequest {
            message: RuntimeStateMessage::StopDsp,
            reply: Some(reply),
        })
        .unwrap();
        let mut state = RuntimeState {
            processor: None,
            is_running: false,
        };
        state.drain(&rx);
        assert!(matches!(
            response.try_recv().unwrap(),
            Ok(SignalProcessorResponse::Ack)
        ));
        let mut buffer = [1.0; 4];
        state.render(&mut buffer);
        assert_eq!(buffer, [0.0; 4]);
        state.is_running = true;
        state.render(&mut buffer);
        assert_eq!(buffer, [0.0; 4]);
    }
}
