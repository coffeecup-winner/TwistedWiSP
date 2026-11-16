use std::collections::HashMap;

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::SignalProcessCreationError,
    ir::{CallId, IRFunction},
    midi::WispMidiIn,
    runner::{
        context::{WispContext, WispExecutionContext},
        runtime::WispRuntime,
    },
};

#[derive(Debug)]
pub struct SystemInfo {
    pub num_channels: u32,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct DataIndex(pub u32);

#[derive(Debug, Hash, PartialEq, Eq, Clone, Copy)]
pub struct WatchIndex(pub u32);

#[derive(Debug, Default)]
pub struct WatchedDataValues {
    pub values: HashMap<WatchIndex, Vec<f32>>,
}

pub struct TwistedWispEngineConfig<'a> {
    pub audio_host: Option<&'a str>,
    pub audio_device: Option<&'a str>,
    pub audio_output_channels: Option<u16>,
    pub audio_buffer_size: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub midi_in_port: Option<&'a str>,
}

pub struct TwistedWispEngine {
    wisp: WispContext,
    execution_context: WispExecutionContext,
    runtime: WispRuntime,
}

impl TwistedWispEngine {
    pub fn create(config: TwistedWispEngineConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let device = ConfiguredAudioDevice::open(
            config.audio_host,
            config.audio_device,
            config.audio_output_channels,
            config.audio_buffer_size,
            config.audio_sample_rate,
        )?;
        let midi_in = WispMidiIn::open(config.midi_in_port)?;
        let wisp = WispContext::new(device.num_output_channels(), device.sample_rate());

        let execution_context = WispExecutionContext::init();
        let runtime = WispRuntime::init(device, midi_in);

        Ok(TwistedWispEngine {
            wisp,
            execution_context,
            runtime,
        })
    }

    pub fn get_system_info(&mut self) -> SystemInfo {
        SystemInfo {
            num_channels: self.wisp.num_outputs(),
        }
    }

    pub fn dsp_start(&mut self) {
        self.runtime.start_dsp();
    }

    pub fn dsp_stop(&mut self) {
        self.runtime.stop_dsp();
    }

    pub fn context_reset(&mut self) {
        self.wisp.reset();
    }

    pub fn context_add_or_update_functions(&mut self, functions: Vec<IRFunction>) {
        for func in functions {
            self.wisp.add_function(func);
        }
    }

    pub fn context_remove_function(&mut self, name: String) {
        self.wisp.remove_function(&name);
    }

    pub fn context_set_main_function(&mut self, name: String) {
        self.wisp.set_main_function(&name);
    }

    pub fn context_set_data_value(&mut self, name: String, id: CallId, idx: DataIndex, value: f32) {
        self.runtime.set_data_value(&name, id, idx, value);
    }

    pub fn context_set_data_array(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
        array_name: String,
    ) -> Option<()> {
        match self.wisp.get_data_array(&name, &array_name) {
            Some(array) => {
                self.runtime.set_data_array(&name, id, idx, array);
                Some(())
            }
            None => None,
        }
    }

    pub fn context_learn_midi_cc(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
    ) -> Option<WatchIndex> {
        self.runtime.learn_midi_cc(&name, id, idx);
        self.runtime.watch_data_value(&name, id, idx, true)
    }

    pub fn context_watch_data_value(
        &mut self,
        name: String,
        id: CallId,
        idx: DataIndex,
    ) -> Option<WatchIndex> {
        self.runtime.watch_data_value(&name, id, idx, false)
    }

    pub fn context_unwatch_data_value(&mut self, idx: WatchIndex) {
        self.runtime.unwatch_data_value(idx);
    }

    pub fn context_query_watched_data_values(&mut self) -> WatchedDataValues {
        self.runtime.query_watched_data_values()
    }

    pub fn context_load_wave_file(
        &mut self,
        name: String,
        buffer_name: String,
        path: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.wisp.load_wave_file(&name, &buffer_name, &path)
    }

    pub fn context_unload_wave_file(&mut self, name: String, buffer_name: String) {
        self.wisp.unload_wave_file(&name, &buffer_name);
    }

    pub fn context_update(&mut self) -> Result<(), SignalProcessCreationError> {
        self.runtime.switch_to_signal_processor(
            &self.execution_context,
            &self.wisp,
            self.wisp.main_function(),
        )
    }
}
