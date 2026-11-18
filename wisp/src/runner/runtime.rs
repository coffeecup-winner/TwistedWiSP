use super::engine::{DataIndex, WatchIndex, WatchedDataValues};
use crate::compiler::{Watch, WatchBatch};
use crate::{
    CallIndex,
    audio::device::ConfiguredAudioDevice,
    compiler::{SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    core::{WispContext, WispFunction},
    midi::WispMidiIn,
    runner::context::WispRuntimeContext,
};
use cpal::{Stream, traits::StreamTrait};
use crossbeam::channel::{Receiver, Sender, bounded};
use inkwell::{
    AddressSpace,
    context::Context,
    types::{FloatType, IntType, PointerType, VoidType},
};
use midir::MidiInputConnection;
use midly::{MidiMessage, live::LiveEvent};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

enum RuntimeStateMessage {
    StartDsp,
    StopDsp,
    SetProcessor(Option<SignalProcessor>),
    SetDataValue(String, CallIndex, DataIndex, f32),
    WatchDataValue(String, CallIndex, DataIndex, Option<Watch>),
    QueryWatchedDataValues(Option<Box<WatchBatch>>),
}

enum SignalProcessorResponse {
    Ack,
    Installed,
    Watch(Option<WatchIndex>),
    WatchData(Box<WatchBatch>),
}

struct RuntimeRequest {
    message: RuntimeStateMessage,
    reply: Option<Sender<Result<SignalProcessorResponse, &'static str>>>,
    deferred: Option<Result<SignalProcessorResponse, &'static str>>,
}

struct RuntimeTransport {
    retired: Arc<crossbeam::queue::ArrayQueue<RuntimeRequest>>,
    tx: Sender<RuntimeRequest>,
    failed: Arc<AtomicBool>,
}

impl RuntimeTransport {
    fn collect(&self) {
        while let Some(request) = self.retired.pop() {
            drop(request);
        }
    }

    fn request(&self, message: RuntimeStateMessage) -> Result<SignalProcessorResponse, String> {
        self.request_timeout(message, Duration::from_millis(250))
    }

    fn request_timeout(
        &self,
        message: RuntimeStateMessage,
        timeout: Duration,
    ) -> Result<SignalProcessorResponse, String> {
        self.collect();
        if self.failed.load(Ordering::Acquire) {
            return Err("Audio device failed; stop and retry audio initialization".into());
        }
        let (reply, rx) = bounded(1);
        self.tx
            .send_timeout(
                RuntimeRequest {
                    message,
                    reply: Some(reply),
                    deferred: None,
                },
                timeout,
            )
            .map_err(|_| "Audio command queue unavailable or full".to_string())?;
        let result = rx
            .recv_timeout(timeout)
            .map_err(|_| "Audio callback did not acknowledge the request".to_string())?
            .map_err(str::to_owned);
        self.collect();
        result
    }
}

struct RuntimeState {
    retired: Arc<crossbeam::queue::ArrayQueue<RuntimeRequest>>,
    processor: Option<SignalProcessor>,
    is_running: bool,
}

impl RuntimeState {
    fn apply(
        &mut self,
        message: &mut RuntimeStateMessage,
    ) -> Result<SignalProcessorResponse, &'static str> {
        match message {
            RuntimeStateMessage::StartDsp => {
                if self.processor.is_none() {
                    return Err("No processor installed");
                }
                self.is_running = true;
            }
            RuntimeStateMessage::StopDsp => self.is_running = false,
            RuntimeStateMessage::SetProcessor(sp) => {
                std::mem::swap(&mut self.processor, sp);
                return Ok(SignalProcessorResponse::Installed);
            }
            RuntimeStateMessage::SetDataValue(name, call, data, value) => {
                if !value.is_finite() {
                    return Err("Nonfinite control value");
                }
                self.processor
                    .as_mut()
                    .and_then(|p| p.set_data_value(name, *call, *data, *value))
                    .ok_or("Invalid scalar address or no processor")?;
            }
            RuntimeStateMessage::WatchDataValue(name, call, data, last) => {
                return Ok(SignalProcessorResponse::Watch(
                    self.processor
                        .as_mut()
                        .and_then(|p| p.watch_prepared(name, *call, *data, last)),
                ));
            }
            RuntimeStateMessage::QueryWatchedDataValues(batch) => {
                let mut batch = batch.take().ok_or("Missing watch storage")?;
                if let Some(processor) = &mut self.processor {
                    processor.query_into(&mut batch);
                }
                return Ok(SignalProcessorResponse::WatchData(batch));
            }
        }
        Ok(SignalProcessorResponse::Ack)
    }

    fn drain(&mut self, rx: &Receiver<RuntimeRequest>) {
        for _ in 0..64 {
            // One producer, control-thread consumer. Never accept work without
            // space to retain its payload, retired processor and abandoned reply.
            if self.retired.is_full() {
                break;
            }
            let Ok(mut request) = rx.try_recv() else {
                break;
            };
            let response = self.apply(&mut request.message);
            if let Some(reply) = &request.reply {
                if let Err(error) = reply.try_send(response) {
                    request.deferred = Some(error.into_inner());
                }
            } else {
                request.deferred = Some(response);
            }
            // Space was reserved before receiving; this cannot fail.
            if self.retired.push(request).is_err() {
                unreachable!("Retirement capacity invariant");
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

// CPAL may destroy its callback on the backend thread. Return the entire
// closure state rather than destroying LLVM/resources on that thread.
struct CallbackOwner {
    state: Option<RuntimeState>,
    requests: Receiver<RuntimeRequest>,
    returned: Sender<(RuntimeState, Receiver<RuntimeRequest>)>,
}
impl CallbackOwner {
    fn process(&mut self, buffer: &mut [f32]) {
        let state = self.state.as_mut().unwrap();
        state.drain(&self.requests);
        state.render(buffer);
    }
}
impl Drop for CallbackOwner {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            // Exactly one return, receiver owned until stream destruction completes.
            if self
                .returned
                .try_send((state, self.requests.clone()))
                .is_err()
            {
                unreachable!("Callback return invariant");
            }
        }
    }
}

pub struct WispRuntime {
    // Field order is deliberate: stop/drop callback before any other resources.
    _stream: Option<Stream>,
    state_rx: Receiver<(RuntimeState, Receiver<RuntimeRequest>)>,
    _midi: Option<MidiInputConnection<()>>,
    _device: ConfiguredAudioDevice,
    midi: Arc<crate::MidiMailbox>,
    watch_batch: std::cell::RefCell<Option<Box<WatchBatch>>>,
    transport: RuntimeTransport,
}

impl Drop for WispRuntime {
    fn drop(&mut self) {
        drop(self._stream.take());
        // Stream destruction joins the backend callback on supported Linux CPAL.
        // Closure destruction returns its state; processor destruction is here.
        if let Ok(state) = self.state_rx.recv() {
            drop(state);
        }
        self.transport.collect();
    }
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
        let retired = Arc::new(crossbeam::queue::ArrayQueue::new(128));
        let (state_tx, state_rx) = bounded(1);
        let mut owner = CallbackOwner {
            state: Some(RuntimeState {
                retired: retired.clone(),
                processor: None,
                is_running: false,
            }),
            requests: rx,
            returned: state_tx,
        };
        let stream = device.build_output_audio_stream(
            move |_, buffer| {
                owner.process(buffer);
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
            _stream: Some(stream),
            state_rx,
            _midi: midi_connection,
            _device: device,
            midi: mailbox,
            watch_batch: std::cell::RefCell::new(Some(Box::new(WatchBatch::new()))),
            transport: RuntimeTransport {
                tx,
                failed,
                retired,
            },
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
            .request(RuntimeStateMessage::SetProcessor(Some(sp)))?
        {
            // Destruction of the retired processor occurs on the service thread.
            SignalProcessorResponse::Installed => Ok(()),
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

    pub fn compile(
        &mut self,
        ctx: &WispContext,
        rctx: &mut WispRuntimeContext,
        root: &str,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        compile_context(ctx, rctx, root)
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
        self.transport.collect();
        self.transport
            .tx
            .try_send(RuntimeRequest {
                message: RuntimeStateMessage::SetDataValue(name.into(), call, data, value),
                reply: None,
                deferred: None,
            })
            .map_err(|_| "Audio queue full".into())
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
            Some(Watch::prepare(last)),
        ))? {
            SignalProcessorResponse::Watch(Some(id)) => Ok(id),
            _ => Err("Invalid watch or watch limit (128)".into()),
        }
    }
    pub fn query_checked(&self) -> Result<WatchedDataValues, String> {
        let batch = self
            .watch_batch
            .borrow_mut()
            .take()
            .unwrap_or_else(|| Box::new(WatchBatch::new()));
        match self
            .transport
            .request(RuntimeStateMessage::QueryWatchedDataValues(Some(batch)))?
        {
            SignalProcessorResponse::WatchData(batch) => {
                let values = batch.values();
                *self.watch_batch.borrow_mut() = Some(batch);
                Ok(values)
            }
            _ => Err("Unexpected watch reply".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataArrayHandle;

    #[test]
    fn abandoned_installs_and_callback_destruction_return_owned_state() {
        let mut session = crate::OfflineSession::new(
            44100,
            1,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
        )
        .unwrap();
        let name = session
            .load_flow_text("[flow]\nname='retirement'\n[[flow.nodes]]\ntext='buffer'\n")
            .unwrap();
        let samples: Arc<[f32]> = vec![1., 2.].into();
        let weak = Arc::downgrade(&samples);
        let mut old = session.compile(&name).unwrap();
        old.set_data_array(
            &name,
            CallIndex(0),
            DataIndex(0),
            DataArrayHandle::from_shared(samples),
        )
        .unwrap();
        let new = session.compile(&name).unwrap();
        drop(session);
        let retired = Arc::new(crossbeam::queue::ArrayQueue::new(1));
        let (tx, requests) = bounded(2);
        let (returned, states) = bounded(1);
        // Full retirement storage must leave the replacement queued, not drop
        // either LLVM owner or switch processors on the callback.
        retired
            .push(RuntimeRequest {
                message: RuntimeStateMessage::StopDsp,
                reply: None,
                deferred: None,
            })
            .ok()
            .unwrap();
        let (reply, abandoned) = bounded(1);
        drop(abandoned);
        tx.try_send(RuntimeRequest {
            message: RuntimeStateMessage::SetProcessor(Some(new)),
            reply: Some(reply),
            deferred: None,
        })
        .unwrap();
        let mut owner = CallbackOwner {
            state: Some(RuntimeState {
                retired: retired.clone(),
                processor: Some(old),
                is_running: false,
            }),
            requests,
            returned,
        };
        owner.process(&mut [0.]);
        assert_eq!(owner.requests.len(), 1);
        drop(retired.pop());
        // Simulate a backend that destroys the callback closure on its worker.
        std::thread::spawn(move || {
            owner.process(&mut [0.]);
            drop(owner);
        })
        .join()
        .unwrap();
        assert!(weak.upgrade().is_some());
        let packet = retired.pop().unwrap();
        assert!(packet.deferred.is_some());
        drop(packet); // Old processor's source is released on this control thread.
        assert!(weak.upgrade().is_none());
        drop(states.recv().unwrap());
    }

    #[test]
    fn prepared_callback_operations_do_not_allocate_or_destroy_payloads() {
        let mut session = crate::OfflineSession::new(
            44100,
            2,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
        )
        .unwrap();
        let name = session.load_flow_text("[flow]\nname='probe'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='noise'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=0\n").unwrap();
        let processor = session.compile(&name).unwrap();
        let retired = Arc::new(crossbeam::queue::ArrayQueue::new(128));
        let mut state = RuntimeState {
            retired: retired.clone(),
            processor: None,
            is_running: false,
        };
        let (tx, rx) = bounded(64);
        let messages = vec![
            RuntimeStateMessage::SetProcessor(Some(processor)),
            RuntimeStateMessage::SetDataValue(name.clone(), CallIndex(0), DataIndex(0), 0.5),
            RuntimeStateMessage::WatchDataValue(
                name,
                CallIndex(0),
                DataIndex(0),
                Some(Watch::prepare(false)),
            ),
            RuntimeStateMessage::StartDsp,
            RuntimeStateMessage::QueryWatchedDataValues(Some(Box::new(WatchBatch::new()))),
            RuntimeStateMessage::StopDsp,
        ];
        let mut buffer = [0.; 1024];
        for message in messages {
            tx.try_send(RuntimeRequest {
                message,
                reply: None,
                deferred: None,
            })
            .unwrap();
            let counts = crate::allocation_probe::measure(|| {
                state.drain(&rx);
                state.render(&mut buffer);
            });
            assert_eq!(counts, (0, 0), "allocations/destructions in callback");
            while let Some(packet) = retired.pop() {
                assert!(packet.deferred.as_ref().unwrap().is_ok());
                drop(packet);
            }
        }
    }

    #[test]
    fn full_and_disconnected_transports_fail_within_deadline() {
        let (tx, rx) = bounded(1);
        let retired = Arc::new(crossbeam::queue::ArrayQueue::new(128));
        let transport = RuntimeTransport {
            retired: retired.clone(),
            tx,
            failed: Arc::new(AtomicBool::new(false)),
        };
        transport
            .tx
            .try_send(RuntimeRequest {
                message: RuntimeStateMessage::StopDsp,
                reply: None,
                deferred: None,
            })
            .unwrap();
        assert!(
            matches!(transport.request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2)), Err(e) if e.contains("queue"))
        );
        drop(rx);
        assert!(
            transport
                .request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2))
                .is_err()
        );
    }

    #[test]
    fn transport_times_out_without_callback_and_replies_do_not_cross() {
        let (tx, rx) = bounded(1);
        let retired = Arc::new(crossbeam::queue::ArrayQueue::new(128));
        let transport = RuntimeTransport {
            retired: retired.clone(),
            tx,
            failed: Arc::new(AtomicBool::new(false)),
        };
        assert!(
            transport
                .request_timeout(RuntimeStateMessage::StopDsp, Duration::from_millis(2))
                .is_err()
        );
        let mut state = RuntimeState {
            retired: Arc::new(crossbeam::queue::ArrayQueue::new(128)),
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
                deferred: None,
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
            deferred: None,
        })
        .unwrap();
        let mut state = RuntimeState {
            retired: Arc::new(crossbeam::queue::ArrayQueue::new(128)),
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
