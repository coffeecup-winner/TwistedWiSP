use std::collections::HashMap;

use inkwell::{
    builder::{Builder, BuilderError},
    context::Context,
    module::Module,
    types::{FloatType, IntType, PointerType, VoidType},
    AddressSpace,
};

use crate::wisp::{function::Function, WispContext};

use super::{data_layout::FunctionDataLayout, error::SignalProcessCreationError};

#[derive(Debug)]
pub(super) struct ModuleTypes<'ectx> {
    pub void: VoidType<'ectx>,
    pub i32: IntType<'ectx>,
    pub f32: FloatType<'ectx>,
    pub pf32: PointerType<'ectx>,
}

impl<'ectx> ModuleTypes<'ectx> {
    pub fn new(context: &'ectx Context) -> Self {
        ModuleTypes {
            void: context.void_type(),
            i32: context.i32_type(),
            f32: context.f32_type(),
            pf32: context.f32_type().ptr_type(AddressSpace::default()),
        }
    }
}

#[derive(Debug)]
pub(super) struct ModuleContext<'ectx, 'temp> {
    pub wctx: &'temp WispContext,
    pub types: ModuleTypes<'ectx>,
    pub module: &'temp Module<'ectx>,
    pub builder: Builder<'ectx>,
    pub data_layout: HashMap<String, FunctionDataLayout>,
}

impl<'ectx, 'temp> ModuleContext<'ectx, 'temp> {
    pub fn new(
        context: &'ectx Context,
        wctx: &'temp WispContext,
        module: &'temp Module<'ectx>,
        data_layout: HashMap<String, FunctionDataLayout>,
    ) -> Self {
        ModuleContext {
            wctx,
            types: ModuleTypes::new(context),
            module,
            builder: context.create_builder(),
            data_layout,
        }
    }

    pub fn get_function(&self, name: &str) -> Result<&'temp Function, SignalProcessCreationError> {
        self.wctx
            .get_function(name)
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
