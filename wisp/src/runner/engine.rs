use std::{collections::HashMap, path::PathBuf};

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::{SignalProcessCreationError, SignalProcessor},
    core::WispContext,
    midi::WispMidiIn,
    runner::{context::WispRuntimeContext, runtime::WispRuntime},
};

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct CallIndex(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct DataIndex(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct WatchIndex(pub u32);

#[derive(Debug, Default)]
pub struct WatchedDataValues {
    pub values: HashMap<WatchIndex, Vec<f32>>,
}

#[derive(Debug, Default, Clone)]
pub struct TwistedWispEngineConfig {
    pub audio_host: Option<String>,
    pub audio_device: Option<String>,
    pub audio_output_channels: Option<u16>,
    pub audio_buffer_size: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub midi_in_port: Option<String>,
    pub core_path: Option<PathBuf>,
}

pub struct TwistedWispEngine {
    // Stop callback before context-owned buffers/functions are destroyed.
    runtime: WispRuntime,
    ctx: WispContext,
    rctx: WispRuntimeContext,
}

impl TwistedWispEngine {
    pub fn create(config: &TwistedWispEngineConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let device = ConfiguredAudioDevice::open(
            config.audio_host.as_deref(),
            config.audio_device.as_deref(),
            config.audio_output_channels,
            config.audio_buffer_size,
            config.audio_sample_rate,
        )?;
        let mut ctx = WispContext::new(device.num_output_channels(), device.sample_rate());
        let rctx = WispRuntimeContext::new();

        ctx.add_builtin_functions();
        if let Some(core_path) = config.core_path.as_deref() {
            ctx.load_core_functions(core_path)?;
        }

        let midi_in = match WispMidiIn::open(config.midi_in_port.as_deref()) {
            Ok(midi) => Some(midi),
            Err(error) => {
                log::warn!("MIDI unavailable (audio remains available): {error}");
                None
            }
        };
        let runtime = WispRuntime::init(device, midi_in)?;

        Ok(TwistedWispEngine { ctx, rctx, runtime })
    }

    /// Safe Rust editor facade. No engine pointers or borrowed function handles escape.
    pub fn ctx_load_flow_from_text(
        &mut self,
        text: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.ctx.load_flow_text(text)
    }
    pub fn audio_configuration(&self) -> (u32, u32) {
        (self.ctx.sample_rate(), self.ctx.num_outputs())
    }
    pub fn runtime_healthy(&self) -> bool {
        self.runtime.healthy()
    }
    pub fn runtime_start_checked(&self) -> Result<(), String> {
        self.runtime.start_checked()
    }
    pub fn runtime_stop_checked(&self) -> Result<(), String> {
        self.runtime.stop_checked()
    }
    pub fn runtime_install_checked(&self, processor: SignalProcessor) -> Result<(), String> {
        self.runtime.install_checked(processor)
    }
    pub fn runtime_scalar_checked(
        &self,
        name: &str,
        call: CallIndex,
        data: DataIndex,
        value: f32,
    ) -> Result<(), String> {
        self.runtime.scalar_checked(name, call, data, value)
    }

    pub fn midi_available(&self) -> bool {
        self.runtime.midi_available()
    }
    pub fn midi_learn(&self, target: u64) -> Result<(), String> {
        self.runtime.midi_learn(target)
    }
    pub fn midi_targets(&self) -> Vec<u64> {
        self.runtime.midi_targets()
    }
    pub fn midi_cancel(&self) {
        self.runtime.midi_cancel();
    }
    pub fn midi_remove(&self, target: u64) {
        self.runtime.midi_remove(target);
    }
    pub fn midi_poll(&self) -> Vec<crate::MidiObservation> {
        self.runtime.midi_poll()
    }
    pub fn runtime_scalar_try(
        &self,
        root: &str,
        call: CallIndex,
        value: f32,
    ) -> Result<(), String> {
        self.runtime.scalar_try(root, call, DataIndex(0), value)
    }
    pub fn runtime_watch_checked(
        &self,
        root: &str,
        call: CallIndex,
        last: bool,
    ) -> Result<WatchIndex, String> {
        self.runtime.watch_checked(root, call, DataIndex(0), last)
    }
    pub fn runtime_query_checked(&self) -> Result<WatchedDataValues, String> {
        self.runtime.query_checked()
    }
    pub fn enable_logging() -> bool {
        let mut builder = env_logger::Builder::new();
        let result = builder
            .filter_level(log::LevelFilter::Info)
            .parse_env("TWISTED_WISP_LOG")
            .try_init()
            .is_ok();

        if result {
            log::info!("Logging enabled");
        } else {
            log::warn!("Failed to enable logging");
        }
        result
    }

    // ===== Context API =====

    pub fn ctx_load_flow_from_file(
        &mut self,
        path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.ctx.load_function(&PathBuf::from(path))
    }

    // ===== Runtime API =====

    pub fn runtime_compile_signal_processor(
        &mut self,
        main_function: String,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        self.runtime
            .compile(&self.ctx, &mut self.rctx, &main_function)
    }
}
