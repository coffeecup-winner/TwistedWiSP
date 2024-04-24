use inkwell::execution_engine::ExecutionEngine;

use super::{
    compiler::{SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    flow::Flow,
    function::Function,
    runtime::Runtime,
};

pub struct WispContext {
    builder: SignalProcessorBuilder,
    runtime: Runtime,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
            builder: SignalProcessorBuilder::new(),
            runtime: Runtime::init(num_outputs, sample_rate),
        }
    }

    pub fn create_signal_processor<'ctx>(
        &'ctx mut self,
        top_level: &str,
    ) -> Result<(SignalProcessor, ExecutionEngine<'ctx>), SignalProcessCreationError> {
        self.builder
            .create_signal_processor(top_level, &self.runtime)
    }

    pub fn add_function(&mut self, func: Function) {
        self.runtime.add_function(func)
    }

    pub fn compile_flow(&mut self, flow: &Flow) {
        let func = flow.compile_function(&self.runtime);
        self.runtime.add_function(func);
    }
}
