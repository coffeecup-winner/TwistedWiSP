use std::collections::HashMap;

use inkwell::{
    builder::{Builder, BuilderError},
    module::Module,
};

use crate::wisp::{function::Function, runtime::Runtime};

use super::error::SignalProcessCreationError;

#[derive(Debug)]
pub(super) struct ModuleContext<'ctx, 'temp> {
    pub runtime: &'temp Runtime,
    pub module: &'temp Module<'ctx>,
    pub builder: &'temp Builder<'ctx>,
    pub data_indices: HashMap<String, u32>,
}

impl<'ctx, 'temp> ModuleContext<'ctx, 'temp> {
    pub fn new(
        runtime: &'temp Runtime,
        module: &'temp Module<'ctx>,
        builder: &'temp Builder<'ctx>,
        data_indices: HashMap<String, u32>,
    ) -> Self {
        ModuleContext {
            runtime,
            module,
            builder,
            data_indices,
        }
    }

    pub fn get_function(&self, name: &str) -> Result<&'temp Function, SignalProcessCreationError> {
        self.runtime
            .get_function(name)
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.into()))
    }

    pub fn build<F, R>(&self, name: &str, func: F) -> Result<R, SignalProcessCreationError>
    where
        F: FnOnce(&Builder<'ctx>, &str) -> Result<R, BuilderError>,
    {
        func(self.builder, &format!("tmp_{}", name))
            .map_err(|_| SignalProcessCreationError::BuildInstruction(name.into()))
    }
}
