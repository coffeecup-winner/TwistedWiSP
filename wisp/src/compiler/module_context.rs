use std::cell::Ref;

use inkwell::{
    builder::{Builder, BuilderError},
    context::Context,
    module::Module,
};

use crate::{core::WispContext, ir::IRFunction, runner::context::WispRuntimeContext};

use super::error::SignalProcessCreationError;

#[derive(Debug)]
pub(super) struct ModuleContext<'ectx, 'temp> {
    pub ctx: &'temp WispContext,
    pub rctx: &'temp WispRuntimeContext,
    pub module: &'temp Module<'ectx>,
    pub builder: Builder<'ectx>,
}

impl<'ectx, 'temp> ModuleContext<'ectx, 'temp> {
    pub fn new(
        context: &'ectx Context,
        ctx: &'temp WispContext,
        rctx: &'temp WispRuntimeContext,
        module: &'temp Module<'ectx>,
    ) -> Self {
        ModuleContext {
            ctx,
            rctx,
            module,
            builder: context.create_builder(),
        }
    }

    pub fn get_function(
        &self,
        name: &str,
    ) -> Result<Ref<'temp, IRFunction>, SignalProcessCreationError> {
        self.rctx
            .get_function(name)
            .map(|f| f.ir_function().get_untracked())
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.into()))
    }

    pub fn build<F, R>(&self, name: &str, func: F) -> Result<R, SignalProcessCreationError>
    where
        F: FnOnce(&Builder<'ectx>, &str) -> Result<R, BuilderError>,
    {
        func(&self.builder, &format!("tmp_{}", name))
            .map_err(|_| SignalProcessCreationError::BuildInstruction(name.into()))
    }
}
