use inkwell::execution_engine::ExecutionEngine;

use super::{
    compiler::{SignalProcessCreationError, SignalProcessor, SignalProcessorBuilder},
    flow::Flow,
    runtime::Runtime,
};

pub struct WispContext {
    builder: SignalProcessorBuilder,
}

impl WispContext {
    pub fn new() -> Self {
        WispContext {
            builder: SignalProcessorBuilder::new(),
        }
    }

    pub fn create_signal_processor<'ctx>(
        &'ctx mut self,
        flow: &Flow,
        runtime: &Runtime,
    ) -> Result<(SignalProcessor, ExecutionEngine<'ctx>), SignalProcessCreationError> {
        self.builder.create_signal_processor(flow, runtime)
    }
}
