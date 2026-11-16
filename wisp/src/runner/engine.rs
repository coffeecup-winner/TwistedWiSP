use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::{
    audio::device::ConfiguredAudioDevice,
    compiler::SignalProcessCreationError,
    core::{FlowFunction, WispContext, WispFunction},
    ir::IRFunction,
    midi::WispMidiIn,
    runner::{
        context::{WispEngineContext, WispExecutionContext},
        runtime::WispRuntime,
    },
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

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FunctionRef(*const dyn WispFunction);

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FlowFunctionRef(*const FlowFunction);

#[derive(Debug, Default)]
pub struct TwistedWispEngineConfig<'a> {
    pub audio_host: Option<&'a str>,
    pub audio_device: Option<&'a str>,
    pub audio_output_channels: Option<u16>,
    pub audio_buffer_size: Option<u32>,
    pub audio_sample_rate: Option<u32>,
    pub midi_in_port: Option<&'a str>,
    pub core_path: Option<&'a Path>,
}

pub struct TwistedWispEngine {
    ctx: WispContext,
    wisp: WispEngineContext,
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
        let mut ctx = WispContext::new(device.num_output_channels(), device.sample_rate());
        let mut wisp = WispEngineContext::new();

        ctx.add_builtin_functions();
        if let Some(core_path) = config.core_path {
            ctx.load_core_functions(core_path)?;
        }

        let midi_in = WispMidiIn::open(config.midi_in_port)?;
        let execution_context = WispExecutionContext::init();
        let runtime = WispRuntime::init(device, midi_in);

        for f in ctx.functions_iter() {
            for func in f.get_ir_functions(&ctx) {
                wisp.add_function(func);
            }
        }

        Ok(TwistedWispEngine {
            ctx,
            wisp,
            execution_context,
            runtime,
        })
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

    pub fn ctx_list_functions(&self) -> Vec<String> {
        self.ctx
            .functions_iter()
            .map(|f| f.name().to_owned())
            .collect()
    }

    pub fn ctx_load_flow_from_file(
        &mut self,
        path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.ctx.load_function(&PathBuf::from(path))
    }

    pub fn ctx_get_flow(&self, name: &str) -> Option<FlowFunctionRef> {
        self.ctx
            .get_function(name)?
            .as_flow()
            .map(|f| FlowFunctionRef(f))
    }

    pub fn ctx_remove_function(&mut self, name: &str) {
        self.ctx.remove_function(name);
    }

    pub fn ctx_get_function_metadata(&self, _name: &str) -> Option<FunctionRef> {
        // let f = self.ctx.get_function(name)?;
        // Some(FunctionRef(&*f as *const _))
        todo!()
    }

    pub fn flow_get_ir_functions(&self, flow: FlowFunctionRef) -> Vec<IRFunction> {
        unsafe { (*flow.0).get_ir_functions(&self.ctx) }
    }

    pub fn flow_get_buffers(&self, flow: FlowFunctionRef) -> &HashMap<String, Option<PathBuf>> {
        unsafe { (*flow.0).buffers() }
    }

    pub fn flow_get_node_indices(
        &self,
        flow: FlowFunctionRef,
    ) -> petgraph::stable_graph::NodeIndices<'_, crate::core::FlowNode> {
        unsafe { (*flow.0).node_indices() }
    }

    pub fn flow_get_node(
        &self,
        flow: FlowFunctionRef,
        idx: petgraph::graph::NodeIndex,
    ) -> Option<&crate::core::FlowNode> {
        unsafe { (*flow.0).get_node(idx) }
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
        self.ctx.set_main_function(&name);
    }

    pub fn context_set_data_value(
        &mut self,
        name: String,
        id: CallIndex,
        idx: DataIndex,
        value: f32,
    ) {
        self.runtime.set_data_value(&name, id, idx, value);
    }

    pub fn context_set_data_array(
        &mut self,
        name: String,
        call_idx: CallIndex,
        data_idx: DataIndex,
        array_name: String,
    ) -> Option<()> {
        match self.wisp.get_data_array(&name, &array_name) {
            Some(array) => {
                self.runtime
                    .set_data_array(&name, call_idx, data_idx, array);
                Some(())
            }
            None => None,
        }
    }

    pub fn context_learn_midi_cc(
        &mut self,
        name: String,
        call_idx: CallIndex,
        data_idx: DataIndex,
    ) -> Option<WatchIndex> {
        self.runtime.learn_midi_cc(&name, call_idx, data_idx);
        self.runtime
            .watch_data_value(&name, call_idx, data_idx, true)
    }

    pub fn context_watch_data_value(
        &mut self,
        name: String,
        call_idx: CallIndex,
        data_idx: DataIndex,
    ) -> Option<WatchIndex> {
        self.runtime
            .watch_data_value(&name, call_idx, data_idx, false)
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
        if let Some(main_function) = self.ctx.main_function() {
            self.runtime.switch_to_signal_processor(
                &self.ctx,
                &self.execution_context,
                &self.wisp,
                main_function,
            )
        } else {
            Err(SignalProcessCreationError::NoMainFunction)
        }
    }
}
